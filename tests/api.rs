//! In-memory integration tests for the JSON API (E5).
//!
//! Uses tower::ServiceExt::oneshot to send requests without binding a port.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::extract::ConnectInfo;
use axum::http::{header, Method, Request, StatusCode};
use axum::routing::{get, post, delete};
use axum::Router;
use serde_json::Value;
use tower::ServiceExt;

// Re-use the server's modules.
use aardbin::config::Config;
use aardbin::crypto::Crypto;
use aardbin::db::Db;
use aardbin::files::FileStore;
use aardbin::guard::AuthState;
use aardbin::ratelimit::LoginRateLimiter;
use aardbin::render::Renderer;
use aardbin::session::SessionManager;
use aardbin::AppState;

fn test_config() -> Config {
    Config {
        access_key: "test-api-key-0123456789".into(),
        crypto_key: [0xaa; 32],
        max_attachment_bytes: 2 * 1024 * 1024,
        max_request_bytes: 8 * 1024 * 1024,
        max_content_bytes: 1024 * 1024,
        page_size: 20,
        session_ttl: std::time::Duration::from_secs(3600),
        cookie_secure: false,
        listen_addr: "127.0.0.1:0".into(),
        data_dir: std::env::temp_dir().join(format!("aardbin-api-test-{}", uuid::Uuid::new_v4())),
        templates_dir: "./templates".into(),
        static_dir: "./static".into(),
    }
}

async fn test_app() -> (Router, tempfile::TempDir) {
    let cfg = test_config();
    std::fs::create_dir_all(&cfg.data_dir).unwrap();
    let db = Db::open(&cfg.data_dir.join("test.db")).unwrap();
    let files = FileStore::new(&cfg.data_dir).unwrap();
    let (events, _) = tokio::sync::broadcast::channel::<()>(16);

    let state = AppState {
        sessions: Arc::new(SessionManager::new(&cfg.access_key, cfg.session_ttl, cfg.cookie_secure)),
        crypto: Crypto::new(&cfg.crypto_key),
        limiter: Arc::new(LoginRateLimiter::new()),
        renderer: Renderer::new(&cfg.templates_dir),
        events,
        db,
        files,
        cfg: Arc::new(cfg),
    };

    let app = Router::new()
        .route("/api/records", get(aardbin::api::api_list_records).post(aardbin::api::api_create_record))
        .route("/api/records/{id}", get(aardbin::api::api_get_record).delete(aardbin::api::api_delete_record))
        .route("/api/attachments/{id}", get(aardbin::api::api_download_attachment))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .with_state(state);

    let tmp = tempfile::tempdir().unwrap();
    (app, tmp)
}

fn test_addr() -> ConnectInfo<SocketAddr> {
    ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 9999)))
}

fn bearer(key: &str) -> String {
    format!("Bearer {key}")
}

#[tokio::test]
async fn api_missing_auth_returns_401() {
    let (app, _tmp) = test_app().await;
    let resp = app
        .oneshot(
            Request::get("/api/records")
                .extension(test_addr())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_wrong_key_returns_401() {
    let (app, _tmp) = test_app().await;
    let resp = app
        .oneshot(
            Request::get("/api/records")
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("wrong-key-wrong-key-wrong"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn api_list_empty() {
    let (app, _tmp) = test_app().await;
    let resp = app
        .oneshot(
            Request::get("/api/records")
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(body["total"], 0);
    assert!(body["records"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn api_create_and_get() {
    let (app, _tmp) = test_app().await;

    // Create a record via multipart form
    let form = reqwest::multipart::Form::new()
        .text("title", "API Test")
        .text("content", "hello from api test");
    // We can't use reqwest in oneshot, so build the multipart body manually
    let boundary = "----Boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nAPI Test\r\n\
         --{boundary}\r
Content-Disposition: form-data; name=\"content\"\r
\r
hello from api test\r
\
         --{boundary}--\r\n"
    );
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/records")
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created: Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap()).unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    assert!(!id.is_empty());

    // Get the record
    let resp = app
        .oneshot(
            Request::get(&format!("/api/records/{id}"))
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let record: Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(record["title"], "API Test");
    assert_eq!(record["content"], "hello from api test");
    assert_eq!(record["untitled"], false);
}

#[tokio::test]
async fn api_delete_record() {
    let (app, _tmp) = test_app().await;

    // Create
    let boundary = "----Boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nDel Me\r\n\
         --{boundary}\r
Content-Disposition: form-data; name=\"content\"\r
\r
bye\r
\
         --{boundary}--\r\n"
    );
    let resp = app
        .clone()
        .oneshot(
            Request::post("/api/records")
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let created: Value = serde_json::from_slice(&axum::body::to_bytes(resp.into_body(), 1_000_000).await.unwrap()).unwrap();
    let id = created["id"].as_str().unwrap();

    // Delete
    let resp = app
        .clone()
        .oneshot(
            Request::delete(&format!("/api/records/{id}"))
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify gone
    let resp = app
        .oneshot(
            Request::get(&format!("/api/records/{id}"))
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("test-api-key-0123456789"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_rate_limit_returns_429_with_retry_after() {
    let (app, _tmp) = test_app().await;

    // 5 wrong-key attempts → 6th should be 429
    for _ in 0..5 {
        let _ = app
            .clone()
            .oneshot(
                Request::get("/api/records")
                    .extension(test_addr())
                    .header(header::AUTHORIZATION, bearer("wrong-key-wrong-key-wrong-key"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
    }
    let resp = app
        .oneshot(
            Request::get("/api/records")
                .extension(test_addr())
                .header(header::AUTHORIZATION, bearer("wrong-key-wrong-key-wrong-key"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(resp.headers().contains_key(header::RETRY_AFTER));
}
