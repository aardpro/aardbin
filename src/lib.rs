//! aardbin — library root for integration tests.

pub mod api;
pub mod config;
pub mod crypto;
pub mod db;
pub mod files;
pub mod guard;
pub mod i18n;
pub mod ratelimit;
pub mod render;
pub mod routes;
pub mod session;

use std::sync::Arc;

use crate::config::Config;
use crate::crypto::Crypto;
use crate::db::Db;
use crate::files::FileStore;
use crate::ratelimit::LoginRateLimiter;
use crate::render::Renderer;
use crate::session::SessionManager;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: Db,
    pub files: FileStore,
    pub crypto: Crypto,
    pub sessions: Arc<SessionManager>,
    pub limiter: Arc<LoginRateLimiter>,
    pub events: tokio::sync::broadcast::Sender<()>,
    pub renderer: Renderer,
}
