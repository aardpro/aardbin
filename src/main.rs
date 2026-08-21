//! aardbin — self-hosted single-user personal bin (see docs/SPEC.md).

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

use aardbin::*;
use aardbin::config::Config;
use aardbin::crypto::Crypto;
use aardbin::db::Db;
use aardbin::files::FileStore;
use aardbin::guard::AuthState;
use aardbin::ratelimit::LoginRateLimiter;
use aardbin::render::Renderer;
use aardbin::session::SessionManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("aardbin=info")),
        )
        .init();

    let cfg = match Config::from_env() {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("configuration error: {e}");
            std::process::exit(2);
        }
    };
    std::fs::create_dir_all(&cfg.data_dir)?;

    let db = Db::open(&cfg.data_dir.join("aardbin.db"))?;
    let files = FileStore::new(&cfg.data_dir)?;

    // Startup orphan scan (SPEC §33): warn only, never auto-delete.
    let known_ids = db.all_attachment_ids().await?;
    files.orphan_scan(&known_ids).await?;

    let (events, _) = broadcast::channel::<()>(64);

    let state = AppState {
        sessions: Arc::new(SessionManager::new(
            &cfg.access_key,
            cfg.session_ttl,
            cfg.cookie_secure,
        )),
        crypto: Crypto::new(&cfg.crypto_key),
        limiter: Arc::new(LoginRateLimiter::new()),
        renderer: Renderer::new(&cfg.templates_dir),
        events,
        db,
        files,
        cfg: cfg.clone(),
    };

    // Authenticated application routes (SPEC §30).
    let protected = Router::new()
        .route("/", get(routes::index))
        .route(
            "/records",
            get(routes::records_partial).post(routes::create_record),
        )
        .route("/records/new", get(routes::new_form))
        .route("/records/{id}/edit", get(routes::edit_form))
        .route("/records/{id}", post(routes::update_record))
        .route("/records/{id}/delete", post(routes::delete_record))
        .route("/records/{id}/copy", get(routes::copy_record))
        .route(
            "/records/{id}/attachments/{aid}/delete",
            post(routes::delete_attachment),
        )
        .route("/attachments/{id}", get(routes::download_attachment))
        .route("/events", get(routes::events))
        .route_layer(axum::middleware::from_fn_with_state(
            AuthState {
                sessions: state.sessions.clone(),
            },
            guard::require_auth,
        ));

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/login", get(routes::get_login).post(routes::post_login))
        .route("/logout", post(routes::post_logout))
        .route("/lang", post(routes::post_lang))
        // JSON API (E1)
        .route("/api/records", get(api::api_list_records).post(api::api_create_record))
        .route("/api/records/{id}", get(api::api_get_record).delete(api::api_delete_record))
        .route("/api/attachments/{id}", get(api::api_download_attachment))
        .nest_service("/static", ServeDir::new(cfg.static_dir.clone()))
        .merge(protected)
        .layer(DefaultBodyLimit::max(cfg.max_request_bytes as usize))
        .layer(axum::middleware::from_fn(guard::origin_guard))
        .with_state(state);

    let listener = TcpListener::bind(&cfg.listen_addr).await?;
    tracing::info!(addr = %cfg.listen_addr, "aardbin listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    tracing::info!("aardbin stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::SignalKind;
        if let Ok(mut s) = tokio::signal::unix::signal(SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
