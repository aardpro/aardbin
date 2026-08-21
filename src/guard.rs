//! HTTP middleware: session auth (SPEC §29) and POST origin guard (SPEC §7.5).

use crate::session::{extract_valid_session, SessionManager};
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;

#[derive(Clone)]
pub struct AuthState {
    pub sessions: Arc<SessionManager>,
}

/// Requires a valid session cookie. HTMX requests get a client-side redirect
/// via HX-Redirect; normal navigations get 303 → /login.
pub async fn require_auth(
    State(auth): State<AuthState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let authed = extract_valid_session(
        req.headers()
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok()),
        &auth.sessions,
    );

    if authed {
        return next.run(req).await;
    }

    let is_htmx = req
        .headers()
        .get("HX-Request")
        .and_then(|v| v.to_str().ok())
        == Some("true");

    if is_htmx {
        (
            StatusCode::UNAUTHORIZED,
            [("HX-Redirect", "/login")],
            "session expired",
        )
            .into_response()
    } else {
        (StatusCode::SEE_OTHER, [(header::LOCATION, "/login")]).into_response()
    }
}

/// Rejects cross-site POSTs (defense in depth on top of SameSite=Lax).
pub async fn origin_guard(req: Request<Body>, next: Next) -> Response {
    if req.method() == Method::POST {
        // Sec-Fetch-Site (modern browsers)
        if let Some(sfs) = req.headers().get("sec-fetch-site") {
            let ok = sfs
                .to_str()
                .map(|v| matches!(v, "same-origin" | "same-site" | "none"))
                .unwrap_or(false);
            if !ok {
                return (StatusCode::FORBIDDEN, "cross-site request rejected").into_response();
            }
        }
        // Origin host must match Host header
        if let Some(origin) = req.headers().get(header::ORIGIN) {
            let origin_host = origin
                .to_str()
                .ok()
                .and_then(|o| o.split("://").nth(1))
                .map(|h| h.trim_end_matches('/').to_ascii_lowercase());
            let host = req
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .map(|h| h.to_ascii_lowercase());
            match (origin_host, host) {
                (Some(o), Some(h)) if o == h => {}
                _ => return (StatusCode::FORBIDDEN, "origin mismatch").into_response(),
            }
        }
    }
    next.run(req).await
}
