//! JSON API endpoints (SPEC §30 + E1).
//!
//! Authentication: `Authorization: Bearer <ACCESS_KEY>` with constant-time
//! comparison.  Shares the login rate-limit bucket (E6): only wrong-key
//! attempts count; success does **not** clear the counter.
//!
//! All responses are JSON.  Errors: `{"error":"..."}` + appropriate status.
//! Locale-agnostic contract (E10): raw `updated_at`, `untitled` boolean,
//! decrypt failure → 422.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, FromRequestParts, Multipart, Path, Query, State};
use axum::http::{header, request::Parts, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum::RequestPartsExt;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::db::AttachmentRow;
use crate::files::{content_disposition, INLINE_WHITELIST};
use crate::render::{display_title, snippet};
use crate::routes::{cleanup_uploads, insert_uploads, parse_record_form};
use crate::AppState;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// ApiAuth extractor
// ---------------------------------------------------------------------------

pub struct ApiAuth;

impl FromRequestParts<AppState> for ApiAuth {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let ConnectInfo(addr) = parts
            .extract::<ConnectInfo<SocketAddr>>()
            .await
            .map_err(|_| json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error"))?;
        let ip = addr.ip();

        // Rate-limit check (same bucket as login)
        if let Err(remaining) = state.limiter.check(ip) {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                [(header::RETRY_AFTER, format!("{}", remaining.as_secs()))],
                serde_json::to_string(&ErrorResponse {
                    error: format!(
                        "Too many attempts. Try again in {} seconds.",
                        remaining.as_secs()
                    ),
                })
                .unwrap_or_default(),
            )
                .into_response());
        }

        // Extract Bearer token
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "missing Authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or_else(|| json_error(StatusCode::UNAUTHORIZED, "invalid Authorization format"))?;

        // Constant-time comparison
        let ok: bool = token
            .as_bytes()
            .ct_eq(state.cfg.access_key.as_bytes())
            .into();

        if !ok {
            state.limiter.record_failure(ip);
            return Err(json_error(StatusCode::UNAUTHORIZED, "invalid access key"));
        }

        // Success does NOT clear the counter (E6)
        Ok(ApiAuth)
    }
}

// ---------------------------------------------------------------------------
// JSON helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

fn json_error(status: StatusCode, msg: &str) -> Response {
    let body = serde_json::to_string(&ErrorResponse {
        error: msg.to_string(),
    })
    .unwrap_or_default();
    (
        status,
        [(header::CONTENT_TYPE, "application/json".to_string())],
        body,
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Query params
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ApiListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
}

#[derive(Deserialize)]
pub struct InlineQuery {
    inline: Option<String>,
}

// ---------------------------------------------------------------------------
// JSON response types (E10: locale-agnostic)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ApiRecord {
    pub id: String,
    pub title: String,
    pub content: String,
    pub untitled: bool,
    pub updated_at: i64,
    pub created_at: i64,
    pub attachments: Vec<ApiAttachmentMeta>,
}

#[derive(Serialize)]
pub struct ApiAttachmentMeta {
    pub id: String,
    pub original_filename: String,
    pub size_bytes: i64,
    pub mime_type: String,
}

#[derive(Serialize)]
pub struct ApiListResponse {
    pub records: Vec<ApiRecordSummary>,
    pub page: i64,
    pub page_size: i64,
    pub total: i64,
    pub total_pages: i64,
}

#[derive(Serialize)]
pub struct ApiRecordSummary {
    pub id: String,
    pub title: String,
    pub untitled: bool,
    pub snippet: String,
    pub updated_at: i64,
    pub attachment_count: usize,
}

#[derive(Serialize)]
struct ApiCreatedResponse {
    id: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[allow(clippy::result_large_err)]
fn decrypt_record(
    s: &AppState,
    row: &crate::db::RecordRow,
    attachments: Vec<AttachmentRow>,
) -> Result<ApiRecord, Response> {
    match s.crypto.decrypt(&row.blob) {
        Ok((title, content)) => {
            let (title, untitled) = display_title(&title, &content, crate::i18n::Lang::En);
            Ok(ApiRecord {
                id: row.id.clone(),
                title,
                content,
                untitled,
                updated_at: row.updated_at,
                created_at: row.created_at,
                attachments: attachments
                    .into_iter()
                    .map(|a| ApiAttachmentMeta {
                        id: a.id,
                        original_filename: a.original_filename,
                        size_bytes: a.size_bytes,
                        mime_type: a.mime_type,
                    })
                    .collect(),
            })
        }
        Err(_) => Err(json_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "undecryptable",
        )),
    }
}

// ---------------------------------------------------------------------------
// Endpoint handlers
// ---------------------------------------------------------------------------

/// POST /api/records — create a new record (multipart).
pub async fn api_create_record(
    State(s): State<AppState>,
    _auth: ApiAuth,
    headers: axum::http::HeaderMap,
    multipart: Multipart,
) -> Response {
    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let form = match parse_record_form(multipart, &s, content_length).await {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    let id = Uuid::new_v4().to_string();
    let ts = now();
    let blob = s.crypto.encrypt(&form.title, &form.content);

    if let Err(_e) = s.db.create_record(&id, &blob, ts).await {
        cleanup_uploads(&s, &form.uploads).await;
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
    }
    if let Err(_e) = insert_uploads(&s, &id, form.uploads, ts).await {
        let _ = s.db.delete_record(&id).await;
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
    }

    let _ = s.events.send(());
    (
        StatusCode::CREATED,
        Json(ApiCreatedResponse { id }),
    )
        .into_response()
}

/// GET /api/records — list records (JSON, paginated).
pub async fn api_list_records(
    State(s): State<AppState>,
    _auth: ApiAuth,
    Query(q): Query<ApiListQuery>,
) -> Response {
    let page_size = q.page_size.unwrap_or(s.cfg.page_size).max(1);
    let page = q.page.unwrap_or(1).max(1);
    let (rows, total) = match s.db.list_records(page, page_size).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "api list failed");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };
    let total_pages = ((total + page_size - 1) / page_size).max(1);
    let page = page.clamp(1, total_pages);

    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let attachments = match s.db.attachments_for_records(&ids).await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "api attachments fetch failed");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };
    let mut by_record: HashMap<String, Vec<AttachmentRow>> = HashMap::new();
    for a in attachments {
        by_record.entry(a.record_id.clone()).or_default().push(a);
    }

    let records = rows
        .into_iter()
        .map(|r| {
            let atts = by_record.remove(&r.id).unwrap_or_default();
            match s.crypto.decrypt(&r.blob) {
                Ok((title, content)) => {
                    let (title, untitled) =
                        display_title(&title, &content, crate::i18n::Lang::En);
                    ApiRecordSummary {
                        id: r.id,
                        title,
                        untitled,
                        snippet: snippet(&content),
                        updated_at: r.updated_at,
                        attachment_count: atts.len(),
                    }
                }
                Err(_) => ApiRecordSummary {
                    id: r.id,
                    title: String::new(),
                    untitled: false,
                    snippet: String::new(),
                    updated_at: r.updated_at,
                    attachment_count: atts.len(),
                },
            }
        })
        .collect();

    Json(ApiListResponse {
        records,
        page,
        page_size,
        total,
        total_pages,
    })
    .into_response()
}

/// GET /api/records/:id — single record detail (JSON).
pub async fn api_get_record(
    State(s): State<AppState>,
    _auth: ApiAuth,
    Path(id): Path<String>,
) -> Response {
    let row = match s.db.get_record(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => {
            tracing::error!(error = %e, "api get record failed");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };
    let attachments = match s.db.list_attachments(&id).await {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(error = %e, "api list attachments failed");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };
    match decrypt_record(&s, &row, attachments) {
        Ok(record) => Json(record).into_response(),
        Err(resp) => resp,
    }
}

/// DELETE /api/records/:id — delete a record.
pub async fn api_delete_record(
    State(s): State<AppState>,
    _auth: ApiAuth,
    Path(id): Path<String>,
) -> Response {
    match s.db.delete_record(&id).await {
        Ok(Some(attachment_ids)) => {
            for aid in attachment_ids {
                s.files.delete_attachment(&aid).await;
            }
            let _ = s.events.send(());
            Json(serde_json::json!({"deleted": true})).into_response()
        }
        Ok(None) => json_error(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => {
            tracing::error!(error = %e, "api delete failed");
            json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
        }
    }
}

/// GET /api/attachments/:id — download an attachment (raw bytes).
pub async fn api_download_attachment(
    State(s): State<AppState>,
    _auth: ApiAuth,
    Path(id): Path<String>,
    Query(q): Query<InlineQuery>,
) -> Response {
    let meta = match s.db.get_attachment(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "attachment not found"),
        Err(e) => {
            tracing::error!(error = %e, "api get attachment failed");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "internal error");
        }
    };

    let path = s.files.attachment_path(&id);
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(attachment_id = %id, error = %e,
                "attachment row exists but file is missing");
            return json_error(StatusCode::NOT_FOUND, "attachment file missing");
        }
    };

    let inline_ok =
        q.inline.as_deref() == Some("1") && INLINE_WHITELIST.contains(&meta.mime_type.as_str());
    let disposition = content_disposition(
        if inline_ok { "inline" } else { "attachment" },
        &meta.original_filename,
    );

    let stream = ReaderStream::new(file);
    (
        [
            (header::CONTENT_TYPE, meta.mime_type),
            (header::CONTENT_DISPOSITION, disposition),
            (
                header::HeaderName::from_static("x-content-type-options"),
                "nosniff".to_string(),
            ),
            (header::CONTENT_LENGTH, meta.size_bytes.to_string()),
            (header::CACHE_CONTROL, "private, no-store".to_string()),
        ],
        Body::from_stream(stream),
    )
        .into_response()
}
