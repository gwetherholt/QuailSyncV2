use std::collections::HashMap;
use std::sync::{atomic::AtomicBool, Arc, Mutex, RwLock};
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;
use quailsync_common::AlertConfig;
use rusqlite::Connection;
use tokio::sync::broadcast;

/// How long before a brooder sensor is considered offline (no telemetry received).
pub const SENSOR_STALE_SECS: u64 = 15;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub agent_connected: Arc<AtomicBool>,
    pub alert_config: AlertConfig,
    pub live_tx: broadcast::Sender<String>,
    /// Tracks the last time telemetry was received for each brooder_id.
    pub last_seen: Arc<RwLock<HashMap<i64, Instant>>>,
    /// Prometheus metrics handle for rendering /metrics output.
    pub metrics_handle: PrometheusHandle,
}

/// Record that we just received telemetry for a brooder.
pub fn touch_brooder(state: &AppState, brooder_id: i64) {
    if let Ok(mut map) = state.last_seen.write() {
        map.insert(brooder_id, Instant::now());
    }
}

/// Check whether a brooder's sensor is currently online.
pub fn is_brooder_online(state: &AppState, brooder_id: i64) -> bool {
    if let Ok(map) = state.last_seen.read() {
        if let Some(last) = map.get(&brooder_id) {
            return last.elapsed().as_secs() < SENSOR_STALE_SECS;
        }
    }
    false
}

/// Acquire the database connection, recovering from a poisoned mutex.
pub fn acquire_db(state: &AppState) -> std::sync::MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|poisoned| {
        eprintln!("[WARN] Database mutex was poisoned — recovering");
        poisoned.into_inner()
    })
}

/// Convert a rusqlite error into a 500 response.
///
/// The actual error is logged server-side for debugging; the client receives
/// a generic `{ "error": "internal_error", "message": "…" }` body so SQL
/// internals never leak to the UI. (See state.rs and routes/*.rs callers —
/// raw SQL error messages used to bubble all the way through to the Android
/// app, e.g. "NOT NULL constraint failed: birds.bloodline_id".)
pub fn db_error(e: rusqlite::Error) -> Response {
    eprintln!("[db_error] {e}");
    internal_error_response()
}

/// Generic 500 response shared by every internal-error path. Centralised so
/// callers can't accidentally leak implementation detail.
pub fn internal_error_response() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({
            "error": "internal_error",
            "message": "Something went wrong on our end. Please try again or contact support.",
        })),
    )
        .into_response()
}
