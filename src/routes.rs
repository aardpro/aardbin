//! HTTP handlers (PRD §30 route table).

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{ConnectInfo, Multipart, Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response, Sse};
use axum::response::sse::{Event, KeepAlive};
use axum::Form;
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tokio_stream::StreamExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::db::AttachmentRow;
use crate::files::{content_disposition, human_size, INLINE_WHITELIST};
use crate::render::{display_title, format_ts_utc, snippet, truncate_chars};
use crate::AppState;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

// ---------------------------------------------------------------------------
// View models
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct LoginView {
    error: Option<String>,
}

#[derive(Serialize)]
struct RecordView {
    id: String,
    title: String,
    untitled: bool,
    snippet: String,
    updated_ts: i64,
    updated_text: String,
    attachment_count: i64,
    first_attachment_name: String,
    extra_attachments: i64,
    undecryptable: bool,
}

#[derive(Serialize)]
struct ListView {
    records: Vec<RecordView>,
    page: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
    total: i64,
}

#[derive(Serialize)]
struct AttachmentView {
    id: String,
    record_id: String,
    original_filename: String,
    size_text: String,
}

#[derive(Serialize)]
struct FormView {
    mode: String,
    action: String,
    id: String,
    title: String,
    content: String,
    attachments: Vec<AttachmentView>,
    max_attachment_human: String,
    max_attachment_bytes: u64,
}

fn attachment_view(a: &AttachmentRow) -> AttachmentView {
    AttachmentView {
        id: a.id.clone(),
        record_id: a.record_id.clone(),
        original_filename: a.original_filename.clone(),
        size_text: human_size(a.size_bytes),
    }
}

// ---------------------------------------------------------------------------
// Errors → responses
// ---------------------------------------------------------------------------

fn err_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, msg.into()).into_response()
}

fn internal(e: anyhow::Error) -> Response {
    tracing::error!(error = %e, "internal error");
    err_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
}

// ---------------------------------------------------------------------------
// Auth (PRD §7, §29)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct LoginQuery {
    error: Option<String>,
}

pub async fn get_login(State(s): State<AppState>, Query(q): Query<LoginQuery>) -> Response {
    let view = LoginView {
        error: q.error.map(|_| "Invalid access key".to_string()),
    };
    match s.renderer.render("login.html", &view) {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e.into()),
    }
}

#[derive(Deserialize)]
pub struct LoginForm {
    access_key: String,
}

pub async fn post_login(
    State(s): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Form(form): Form<LoginForm>,
) -> Response {
    let ip = addr.ip();

    if let Err(remaining) = s.limiter.check(ip) {
        let minutes = remaining.as_secs().div_ceil(60);
        let view = LoginView {
            error: Some(format!(
                "Too many attempts. Try again in {minutes} minute{}.",
                if minutes == 1 { "" } else { "s" }
            )),
        };
        let html = s
            .renderer
            .render("login.html", &view)
            .unwrap_or_else(|_| "Too many attempts".into());
        return (StatusCode::TOO_MANY_REQUESTS, Html(html)).into_response();
    }

    let ok: bool = form
        .access_key
        .as_bytes()
        .ct_eq(s.cfg.access_key.as_bytes())
        .into();

    if !ok {
        s.limiter.record_failure(ip);
        return (
            StatusCode::SEE_OTHER,
            [(header::LOCATION, "/login?error=1")],
        )
            .into_response();
    }

    s.limiter.record_success(ip);
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/".to_string()),
            (header::SET_COOKIE, s.sessions.issue_set_cookie()),
        ],
    )
        .into_response()
}

pub async fn post_logout(State(s): State<AppState>) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, "/login".to_string()),
            (header::SET_COOKIE, s.sessions.clear_set_cookie()),
        ],
    )
        .into_response()
}

// ---------------------------------------------------------------------------
// Records list (PRD §10–§13)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PageQuery {
    page: Option<i64>,
}

async fn build_list_view(s: &AppState, page: i64) -> anyhow::Result<ListView> {
    let page_size = s.cfg.page_size;
    let (rows, total) = s.db.list_records(page.max(1), page_size).await?;
    let total_pages = ((total + page_size - 1) / page_size).max(1);
    let page = page.clamp(1, total_pages);
    // requested page was beyond the end → re-fetch the clamped page
    let rows = if rows.is_empty() && page > 1 {
        s.db.list_records(page, page_size).await?.0
    } else {
        rows
    };

    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let attachments = s.db.attachments_for_records(&ids).await?;
    let mut by_record: HashMap<String, Vec<AttachmentRow>> = HashMap::new();
    for a in attachments {
        by_record.entry(a.record_id.clone()).or_default().push(a);
    }

    let records = rows
        .into_iter()
        .map(|r| {
            let atts = by_record.remove(&r.id).unwrap_or_default();
            let attachment_count = atts.len() as i64;
            let first_attachment_name = atts
                .first()
                .map(|a| a.original_filename.clone())
                .unwrap_or_default();

            match s.crypto.decrypt(&r.blob) {
                Ok((title, content)) => {
                    let (title, untitled) = display_title(&title, &content);
                    RecordView {
                        id: r.id,
                        title,
                        untitled,
                        snippet: snippet(&content),
                        updated_ts: r.updated_at,
                        updated_text: format_ts_utc(r.updated_at),
                        attachment_count,
                        first_attachment_name,
                        extra_attachments: (attachment_count - 1).max(0),
                        undecryptable: false,
                    }
                }
                Err(_) => RecordView {
                    id: r.id,
                    title: String::new(),
                    untitled: false,
                    snippet: String::new(),
                    updated_ts: r.updated_at,
                    updated_text: format_ts_utc(r.updated_at),
                    attachment_count,
                    first_attachment_name,
                    extra_attachments: (attachment_count - 1).max(0),
                    undecryptable: true,
                },
            }
        })
        .collect();

    Ok(ListView {
        records,
        page,
        total_pages,
        has_prev: page > 1,
        has_next: page < total_pages,
        total,
    })
}

/// GET / — full page.
pub async fn index(State(s): State<AppState>) -> Response {
    match build_list_view(&s, 1).await {
        Ok(view) => match s.renderer.render("list.html", &view) {
            Ok(html) => Html(html).into_response(),
            Err(e) => internal(e.into()),
        },
        Err(e) => internal(e),
    }
}

/// GET /records?page=N — HTMX partial for the list region.
pub async fn records_partial(State(s): State<AppState>, Query(q): Query<PageQuery>) -> Response {
    match build_list_view(&s, q.page.unwrap_or(1)).await {
        Ok(view) => match s.renderer.render("partials/records.html", &view) {
            Ok(html) => Html(html).into_response(),
            Err(e) => internal(e.into()),
        },
        Err(e) => internal(e),
    }
}

// ---------------------------------------------------------------------------
// Record create / edit / delete / copy (PRD §14–§16)
// ---------------------------------------------------------------------------

pub async fn new_form(State(s): State<AppState>) -> Response {
    let view = FormView {
        mode: "new".into(),
        action: "/records".into(),
        id: String::new(),
        title: String::new(),
        content: String::new(),
        attachments: vec![],
        max_attachment_human: human_size(s.cfg.max_attachment_bytes as i64),
        max_attachment_bytes: s.cfg.max_attachment_bytes,
    };
    match s.renderer.render("form.html", &view) {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e.into()),
    }
}

struct PendingUpload {
    id: String,
    original_filename: String,
    mime_type: String,
    size: u64,
}

struct ParsedRecordForm {
    title: String,
    content: String,
    uploads: Vec<PendingUpload>,
}

/// Streams a multipart form: text fields to memory, files to temp files
/// (renamed to final UUID path on success). Enforces all PRD §17 limits.
async fn parse_record_form(
    mut multipart: Multipart,
    s: &AppState,
    content_length: Option<u64>,
) -> Result<ParsedRecordForm, Response> {
    if let Some(len) = content_length {
        if len > s.cfg.max_request_bytes {
            return Err(err_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("Request too large (max {} bytes)", s.cfg.max_request_bytes),
            ));
        }
    }

    let mut title = String::new();
    let mut content = String::new();
    let mut uploads: Vec<PendingUpload> = Vec::new();
    let mut total_bytes: u64 = 0;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                cleanup_uploads(s, &uploads).await;
                return Err(err_response(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format!("Invalid form data: {e}"),
                ));
            }
        };

        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "title" | "content" => {
                let text = match field.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        cleanup_uploads(s, &uploads).await;
                        return Err(err_response(
                            StatusCode::UNPROCESSABLE_ENTITY,
                            format!("Invalid field: {e}"),
                        ));
                    }
                };
                total_bytes += text.len() as u64;
                if total_bytes > s.cfg.max_request_bytes {
                    cleanup_uploads(s, &uploads).await;
                    return Err(err_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "Request too large",
                    ));
                }
                if name == "content" && text.len() > s.cfg.max_content_bytes {
                    cleanup_uploads(s, &uploads).await;
                    return Err(err_response(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!(
                            "Content too large (max {} bytes)",
                            s.cfg.max_content_bytes
                        ),
                    ));
                }
                if name == "title" {
                    title = truncate_chars(text.trim(), 200);
                } else {
                    content = text;
                }
            }
            "files" => {
                let Some(filename) = field.file_name().map(|f| f.to_string()) else {
                    continue;
                };
                if filename.is_empty() {
                    continue; // browsers send an empty file part when nothing selected
                }
                let declared = field.content_type().map(|c| c.to_string());
                let (id, _tmp_path, mut tmp_file) = match s.files.begin_upload().await {
                    Ok(v) => v,
                    Err(e) => {
                        cleanup_uploads(s, &uploads).await;
                        return Err(internal(e.into()));
                    }
                };

                let mut size: u64 = 0;
                let mut failed: Option<Response> = None;
                let mut field = field;
                loop {
                    match field.chunk().await {
                        Ok(Some(chunk)) => {
                            size += chunk.len() as u64;
                            total_bytes += chunk.len() as u64;
                            if size > s.cfg.max_attachment_bytes {
                                failed = Some(err_response(
                                    StatusCode::UNPROCESSABLE_ENTITY,
                                    format!(
                                        "\"{filename}\" exceeds max attachment size ({})",
                                        human_size(s.cfg.max_attachment_bytes as i64)
                                    ),
                                ));
                                break;
                            }
                            if total_bytes > s.cfg.max_request_bytes {
                                failed = Some(err_response(
                                    StatusCode::PAYLOAD_TOO_LARGE,
                                    "Request too large",
                                ));
                                break;
                            }
                            if let Err(e) =
                                tokio::io::AsyncWriteExt::write_all(&mut tmp_file, &chunk).await
                            {
                                failed = Some(internal(e.into()));
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            failed = Some(err_response(
                                StatusCode::UNPROCESSABLE_ENTITY,
                                format!("Upload failed: {e}"),
                            ));
                            break;
                        }
                    }
                }

                if let Some(resp) = failed {
                    drop(tmp_file);
                    s.files.abort_upload(&id).await;
                    cleanup_uploads(s, &uploads).await;
                    return Err(resp);
                }

                // fsync + close + rename to attachments/{uuid} (PRD §32)
                if let Err(e) = s.files.finalize_upload(&id, tmp_file).await {
                    s.files.abort_upload(&id).await;
                    cleanup_uploads(s, &uploads).await;
                    return Err(internal(e.into()));
                }

                uploads.push(PendingUpload {
                    id,
                    original_filename: filename.clone(),
                    mime_type: crate::files::guess_mime(&filename, declared.as_deref()),
                    size,
                });
            }
            _ => {
                // unknown field: drain & count
                if let Ok(text) = field.text().await {
                    total_bytes += text.len() as u64;
                }
            }
        }
    }

    Ok(ParsedRecordForm {
        title,
        content,
        uploads,
    })
}

async fn cleanup_uploads(s: &AppState, uploads: &[PendingUpload]) {
    for u in uploads {
        s.files.delete_attachment(&u.id).await;
    }
}

async fn insert_uploads(
    s: &AppState,
    record_id: &str,
    uploads: Vec<PendingUpload>,
    ts: i64,
) -> anyhow::Result<()> {
    for u in uploads {
        let row = AttachmentRow {
            id: u.id.clone(),
            record_id: record_id.to_string(),
            original_filename: u.original_filename,
            size_bytes: u.size as i64,
            mime_type: u.mime_type,
            created_at: ts,
        };
        if let Err(e) = s.db.insert_attachment(&row).await {
            // DB failed after rename → remove file to avoid orphans
            s.files.delete_attachment(&u.id).await;
            return Err(e);
        }
    }
    Ok(())
}

pub async fn create_record(
    State(s): State<AppState>,
    headers: HeaderMap,
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

    if let Err(e) = s.db.create_record(&id, &blob, ts).await {
        cleanup_uploads(&s, &form.uploads).await;
        return internal(e);
    }
    if let Err(e) = insert_uploads(&s, &id, form.uploads, ts).await {
        let _ = s.db.delete_record(&id).await;
        return internal(e);
    }

    let _ = s.events.send(());
    (
        StatusCode::NO_CONTENT,
        [("HX-Redirect", "/")],
    )
        .into_response()
}

pub async fn edit_form(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let row = match s.db.get_record(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => return internal(e),
    };
    let (title, content) = match s.crypto.decrypt(&row.blob) {
        Ok(v) => v,
        Err(_) => {
            // PRD §8.4: undecryptable records cannot be edited or overwritten
            return err_response(
                StatusCode::UNPROCESSABLE_ENTITY,
                "Unable to decrypt this record; editing is disabled.",
            );
        }
    };
    let attachments = match s.db.list_attachments(&id).await {
        Ok(a) => a,
        Err(e) => return internal(e),
    };
    let view = FormView {
        mode: "edit".into(),
        action: format!("/records/{id}"),
        id,
        title,
        content,
        attachments: attachments.iter().map(attachment_view).collect(),
        max_attachment_human: human_size(s.cfg.max_attachment_bytes as i64),
        max_attachment_bytes: s.cfg.max_attachment_bytes,
    };
    match s.renderer.render("form.html", &view) {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e.into()),
    }
}

pub async fn update_record(
    State(s): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Response {
    let row = match s.db.get_record(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => return internal(e),
    };
    if s.crypto.decrypt(&row.blob).is_err() {
        return err_response(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Unable to decrypt this record; refusing to overwrite.",
        );
    }

    let content_length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let form = match parse_record_form(multipart, &s, content_length).await {
        Ok(f) => f,
        Err(resp) => return resp,
    };

    let ts = now();
    let blob = s.crypto.encrypt(&form.title, &form.content);
    match s.db.update_record(&id, &blob, ts).await {
        Ok(true) => {}
        Ok(false) => return err_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => {
            cleanup_uploads(&s, &form.uploads).await;
            return internal(e);
        }
    }
    if let Err(e) = insert_uploads(&s, &id, form.uploads, ts).await {
        return internal(e);
    }

    let _ = s.events.send(());
    (
        StatusCode::NO_CONTENT,
        [("HX-Redirect", "/")],
    )
        .into_response()
}

pub async fn delete_record(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_record(&id).await {
        Ok(Some(attachment_ids)) => {
            for aid in attachment_ids {
                s.files.delete_attachment(&aid).await;
            }
            let _ = s.events.send(());
            StatusCode::OK.into_response()
        }
        Ok(None) => err_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => internal(e),
    }
}

pub async fn copy_record(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let row = match s.db.get_record(&id).await {
        Ok(Some(r)) => r,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "record not found"),
        Err(e) => return internal(e),
    };
    match s.crypto.decrypt(&row.blob) {
        Ok((_title, content)) => (
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            content,
        )
            .into_response(),
        Err(_) => err_response(StatusCode::UNPROCESSABLE_ENTITY, "unable to decrypt"),
    }
}

// ---------------------------------------------------------------------------
// Attachments (PRD §9)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct InlineQuery {
    inline: Option<String>,
}

pub async fn download_attachment(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<InlineQuery>,
) -> Response {
    let meta = match s.db.get_attachment(&id).await {
        Ok(Some(m)) => m,
        Ok(None) => return err_response(StatusCode::NOT_FOUND, "attachment not found"),
        Err(e) => return internal(e),
    };

    let path = s.files.attachment_path(&id);
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!(attachment_id = %id, error = %e,
                "attachment row exists but file is missing");
            return err_response(StatusCode::NOT_FOUND, "attachment file missing");
        }
    };

    let inline_ok = q.inline.as_deref() == Some("1")
        && INLINE_WHITELIST.contains(&meta.mime_type.as_str());
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

/// POST /records/:id/attachments/:aid/delete — from the edit form.
pub async fn delete_attachment(
    State(s): State<AppState>,
    Path((record_id, attachment_id)): Path<(String, String)>,
) -> Response {
    let removed = match s.db.delete_attachment(&record_id, &attachment_id).await {
        Ok(v) => v,
        Err(e) => return internal(e),
    };
    if removed.is_none() {
        return err_response(StatusCode::NOT_FOUND, "attachment not found");
    }
    s.files.delete_attachment(&attachment_id).await;
    // attachment changes bump the record's updated_at (PRD §14.2)
    if let Err(e) = s.db.touch_record(&record_id, now()).await {
        return internal(e);
    }
    let _ = s.events.send(());

    let attachments = match s.db.list_attachments(&record_id).await {
        Ok(a) => a,
        Err(e) => return internal(e),
    };
    let views: Vec<AttachmentView> = attachments.iter().map(attachment_view).collect();
    #[derive(Serialize)]
    struct Ctx {
        attachments: Vec<AttachmentView>,
    }
    match s
        .renderer
        .render("partials/attachments.html", &Ctx { attachments: views })
    {
        Ok(html) => Html(html).into_response(),
        Err(e) => internal(e.into()),
    }
}

// ---------------------------------------------------------------------------
// SSE (PRD §18)
// ---------------------------------------------------------------------------

pub async fn events(State(s): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = s.events.subscribe();
    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).map(|_| {
        // Ok(()) or Err(Lagged) — either way, changes happened
        Ok(Event::default().event("data_changed").data("{}"))
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(25))
            .text("ping"),
    )
}
