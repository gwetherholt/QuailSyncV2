use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc, Mutex, RwLock};
use std::time::Instant;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;
use quailsync_common::Settings;
use rusqlite::Connection;
use tokio::sync::broadcast;

/// How long before a brooder sensor is considered offline (no telemetry received).
pub const SENSOR_STALE_SECS: u64 = 15;

/// Where uploaded bird photos are stored and how anomalous-rejection alerts
/// are pushed. Held in `AppState` (rather than read from the environment at
/// the point of use) so the composition root — `main.rs` — owns all env
/// parsing, and so tests can inject a temp dir + a mock ntfy endpoint.
#[derive(Clone)]
pub struct PhotoConfig {
    /// Directory the upload handler writes photo files into. Created on first
    /// use if absent. In production this is relative ("bird_photos") and so
    /// resolves under the container's `/data` workdir — i.e. the host's
    /// `…/data/bird_photos/`, the same path the backup script globs.
    pub dir: Arc<PathBuf>,
    /// ntfy base server (e.g. "https://ntfy.sh"). Empty disables alerts.
    pub ntfy_server: String,
    /// ntfy topic. `None` disables alerts. The topic is a secret and lives in
    /// the out-of-repo env file — never hardcoded here.
    pub ntfy_topic: Option<String>,
}

impl PhotoConfig {
    /// Production configuration, read entirely from the environment. Mirrors
    /// the backup script's variables (`NTFY_SERVER`, `NTFY_TOPIC`) so a single
    /// env file configures both. A blank or placeholder topic disables alerts.
    pub fn from_env() -> Self {
        let dir =
            std::env::var("QUAILSYNC_PHOTO_DIR").unwrap_or_else(|_| "bird_photos".to_string());
        let ntfy_topic = std::env::var("NTFY_TOPIC")
            .ok()
            .filter(|t| !t.trim().is_empty() && t != "quailsync-REPLACE-ME");
        Self {
            dir: Arc::new(PathBuf::from(dir)),
            ntfy_server: std::env::var("NTFY_SERVER")
                .unwrap_or_else(|_| "https://ntfy.sh".to_string()),
            ntfy_topic,
        }
    }

    /// Test/default configuration: photos under `dir`, ntfy alerts disabled.
    pub fn for_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Arc::new(dir.into()),
            ntfy_server: String::new(),
            ntfy_topic: None,
        }
    }

    /// Whether ntfy alerting is configured (server + topic both present).
    pub fn ntfy_enabled(&self) -> bool {
        !self.ntfy_server.is_empty() && self.ntfy_topic.is_some()
    }
}

/// Location of the trail-cam pipeline's output. The pipeline writes
/// `observations.jsonl` to `processed_dir` and the per-camera JPEGs to
/// `processed_dir/{camera_id}/`. The server reads both to surface the latest
/// observation and serve images.
#[derive(Clone)]
pub struct TrailcamConfig {
    pub processed_dir: Arc<PathBuf>,
}

impl TrailcamConfig {
    /// From the environment: `TRAILCAM_PROCESSED_DIR` wins; otherwise
    /// `{TRAILCAM_BASE_DIR or ~/trailcam}/processed`.
    pub fn from_env() -> Self {
        let dir = if let Ok(p) = std::env::var("TRAILCAM_PROCESSED_DIR") {
            PathBuf::from(p)
        } else {
            let base = std::env::var("TRAILCAM_BASE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join("trailcam")
                });
            base.join("processed")
        };
        Self {
            processed_dir: Arc::new(dir),
        }
    }

    /// Test/explicit config pointing at a specific processed dir.
    pub fn for_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            processed_dir: Arc::new(dir.into()),
        }
    }

    /// Path to the appended observations log.
    pub fn observations_path(&self) -> PathBuf {
        self.processed_dir.join("observations.jsonl")
    }
}

/// Location of the indoor-cam pipeline's output (the RTSP poller in
/// `indoor-cam/`). Mirrors [`TrailcamConfig`]: the poller writes per-camera
/// JPEGs to `processed_dir/{camera_id}/` and the server reads them to serve
/// observation images.
#[derive(Clone)]
pub struct IndoorcamConfig {
    pub processed_dir: Arc<PathBuf>,
}

impl IndoorcamConfig {
    /// From the environment: `INDOORCAM_PROCESSED_DIR` wins; otherwise
    /// `{INDOORCAM_BASE_DIR or ~/indoor-cam}/processed`.
    pub fn from_env() -> Self {
        let dir = if let Ok(p) = std::env::var("INDOORCAM_PROCESSED_DIR") {
            PathBuf::from(p)
        } else {
            let base = std::env::var("INDOORCAM_BASE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    let home = std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".to_string());
                    PathBuf::from(home).join("indoor-cam")
                });
            base.join("processed")
        };
        Self {
            processed_dir: Arc::new(dir),
        }
    }

    /// Test/explicit config pointing at a specific processed dir.
    pub fn for_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            processed_dir: Arc::new(dir.into()),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub agent_connected: Arc<AtomicBool>,
    /// Server-owned lifecycle + alert settings, loaded from `system_settings`
    /// at startup. Behind a lock so `PUT /api/system-settings` can refresh the
    /// live copy the alert engine reads. See `routes/system_settings.rs`.
    pub settings: Arc<RwLock<Settings>>,
    pub live_tx: broadcast::Sender<String>,
    /// Tracks the last time telemetry was received for each brooder_id.
    pub last_seen: Arc<RwLock<HashMap<i64, Instant>>>,
    /// Prometheus metrics handle for rendering /metrics output.
    pub metrics_handle: PrometheusHandle,
    /// Bird-photo upload storage + alert configuration.
    pub photos: PhotoConfig,
    /// Trail-cam pipeline output location (observations + images).
    pub trailcam: TrailcamConfig,
    /// Indoor-cam (RTSP) pipeline output location (observation images).
    pub indoorcam: IndoorcamConfig,
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
