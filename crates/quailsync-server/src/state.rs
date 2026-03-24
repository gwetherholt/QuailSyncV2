use std::sync::{atomic::AtomicBool, Arc, Mutex};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use quailsync_common::AlertConfig;
use rusqlite::Connection;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub agent_connected: Arc<AtomicBool>,
    pub alert_config: AlertConfig,
    pub live_tx: broadcast::Sender<String>,
}

/// Acquire the database connection, recovering from a poisoned mutex.
pub fn acquire_db(state: &AppState) -> std::sync::MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|poisoned| {
        eprintln!("[WARN] Database mutex was poisoned — recovering");
        poisoned.into_inner()
    })
}

/// Convert a rusqlite error into a 500 response.
pub fn db_error(e: rusqlite::Error) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Database error: {e}"),
    )
        .into_response()
}
