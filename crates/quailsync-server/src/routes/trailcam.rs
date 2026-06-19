//! Trail-cam observation endpoints (SQLite-backed).
//!
//! The trail-cam pipeline (separate process, see `trailcam/`) POSTs one
//! observation per processed photo to `POST /api/trailcam/observation`; the
//! bridge keeps a `processed/observations.jsonl` write-ahead log only as a
//! fallback for when this API is unreachable. Observations live in the
//! `trail_cam_observations` table; the JPEGs are served from
//! `processed/{camera_id}/`.
//!
//! Posting an observation also auto-registers the camera in `trail_cameras`
//! (the same way the Govee ingest auto-registers sensors).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::params;
use serde_json::{json, Value};

use crate::state::{acquire_db, db_error, AppState};

/// JSON error body shared by these handlers.
fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

/// Strip any directory part from a client-supplied filename so we only ever
/// store/serve a basename (the image-serve handler also rejects separators).
fn basename(name: &str) -> String {
    name.rsplit(['/', '\\']).next().unwrap_or(name).to_string()
}

/// `/api/trailcam/image/{camera_id}/{filename}` URL for a stored filename, or
/// `Null` when absent/empty.
fn image_url_for(camera_id: &str, filename: Option<&str>) -> Value {
    filename
        .filter(|f| !f.is_empty())
        .map(|f| json!(format!("/api/trailcam/image/{camera_id}/{f}")))
        .unwrap_or(Value::Null)
}

/// Like [`image_url_for`] but only when the annotated file is actually on disk —
/// so the client can fall back to the raw image when there's no overlay copy.
fn annotated_url_for(
    processed_dir: &std::path::Path,
    camera_id: &str,
    filename: Option<&str>,
) -> Value {
    filename
        .filter(|f| !f.is_empty())
        .filter(|f| processed_dir.join(camera_id).join(f).is_file())
        .map(|f| json!(format!("/api/trailcam/image/{camera_id}/{f}")))
        .unwrap_or(Value::Null)
}

/// Parse the stored detections JSON text back into a value (defaulting to `[]`).
fn parse_detections(s: Option<&str>) -> Value {
    s.and_then(|s| serde_json::from_str::<Value>(s).ok())
        .unwrap_or_else(|| json!([]))
}

// ---------------------------------------------------------------------------
// POST /api/trailcam/observation — ingest one observation from the pipeline.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub(crate) struct ObservationRequest {
    camera_id: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    bird_count: i64,
    #[serde(default)]
    average_confidence: Option<f64>,
    #[serde(default)]
    min_confidence: Option<f64>,
    #[serde(default)]
    detections: Value,
    #[serde(default)]
    inference_time_ms: f64,
    #[serde(default)]
    image_filename: Option<String>,
    #[serde(default)]
    annotated_image_filename: Option<String>,
}

/// Insert one observation. Auto-registers the camera, then stores the row.
/// Returns 201 with the new id.
pub(crate) async fn trailcam_observation(
    State(state): State<AppState>,
    Json(body): Json<ObservationRequest>,
) -> Response {
    if body.camera_id.trim().is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "camera_id is required",
        );
    }
    let conn = acquire_db(&state);

    // Auto-register (or bump last_seen on) the camera — mirrors Govee ingest.
    super::trail_cameras::ensure_trail_camera(&conn, &body.camera_id);

    let detections_json =
        serde_json::to_string(&body.detections).unwrap_or_else(|_| "[]".to_string());
    // Defensive: never store a host path, only a basename.
    let image_filename = body.image_filename.as_deref().map(basename);
    let annotated_image_filename = body.annotated_image_filename.as_deref().map(basename);

    match conn.execute(
        "INSERT INTO trail_cam_observations
            (camera_id, timestamp, bird_count, average_confidence, min_confidence,
             detections, inference_time_ms, image_filename, annotated_image_filename)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            body.camera_id,
            body.timestamp,
            body.bird_count,
            body.average_confidence,
            body.min_confidence,
            detections_json,
            body.inference_time_ms,
            image_filename,
            annotated_image_filename,
        ],
    ) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(json!({ "stored": 1, "id": conn.last_insert_rowid() })),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

// ---------------------------------------------------------------------------
// GET /api/trailcam/cameras — distinct cameras with observations.
// ---------------------------------------------------------------------------

/// Returns `[{ "camera_id", "label" }]` with labels "Outdoor Cam 1", … numbered
/// by order of first appearance (lowest row id). Empty when no observations.
pub(crate) async fn trailcam_cameras(State(state): State<AppState>) -> Response {
    let conn = acquire_db(&state);
    let ids: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT camera_id FROM trail_cam_observations
                 GROUP BY camera_id ORDER BY MIN(id)",
            )
            .expect("prepare failed");
        stmt.query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    let cameras: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, cid)| json!({ "camera_id": cid, "label": format!("Outdoor Cam {}", i + 1) }))
        .collect();
    Json(cameras).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/trailcam/latest/{camera_id} — most recent observation for a camera.
// ---------------------------------------------------------------------------

pub(crate) async fn trailcam_latest(
    State(state): State<AppState>,
    Path(camera_id): Path<String>,
) -> Response {
    let conn = acquire_db(&state);
    let row = conn.query_row(
        "SELECT timestamp, bird_count, average_confidence, detections,
                image_filename, annotated_image_filename
         FROM trail_cam_observations
         WHERE camera_id = ?1
         ORDER BY timestamp DESC, id DESC
         LIMIT 1",
        params![camera_id],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<f64>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        },
    );

    let (timestamp, bird_count, avg_conf, detections_str, image_filename, annotated_filename) =
        match row {
            Ok(r) => r,
            Err(_) => {
                return err(
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "No observation for that camera.",
                )
            }
        };

    let body = json!({
        "camera_id": camera_id,
        "bird_count": bird_count,
        "timestamp": timestamp,
        "confidence_avg": avg_conf,
        "detections": parse_detections(detections_str.as_deref()),
        "image_url": image_url_for(&camera_id, image_filename.as_deref()),
        "annotated_image_url": annotated_url_for(
            &state.trailcam.processed_dir,
            &camera_id,
            annotated_filename.as_deref(),
        ),
    });
    (StatusCode::OK, Json(body)).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/trailcam/history/{camera_id}?hours=24 — observations in a window.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
pub(crate) struct HistoryQuery {
    hours: Option<i64>,
}

/// All observations for a camera within the last `hours` (default 24), oldest
/// first — for trend graphs. Window is by `created_at` (server insertion time,
/// reliable UTC) so it doesn't depend on the camera's own clock format.
pub(crate) async fn trailcam_history(
    State(state): State<AppState>,
    Path(camera_id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> Response {
    let hours = q.hours.unwrap_or(24).clamp(1, 24 * 365);
    let cutoff = format!("-{hours} hours");
    let conn = acquire_db(&state);

    type Row = (
        Option<String>, // timestamp
        i64,            // bird_count
        Option<f64>,    // average_confidence
        Option<f64>,    // min_confidence
        Option<String>, // detections
        Option<f64>,    // inference_time_ms
        Option<String>, // image_filename
        Option<String>, // annotated_image_filename
        String,         // created_at
    );
    let rows: Vec<Row> = {
        let mut stmt = conn
            .prepare(
                "SELECT timestamp, bird_count, average_confidence, min_confidence,
                        detections, inference_time_ms, image_filename,
                        annotated_image_filename, created_at
                 FROM trail_cam_observations
                 WHERE camera_id = ?1 AND created_at >= datetime('now', ?2)
                 ORDER BY created_at ASC, id ASC",
            )
            .expect("prepare failed");
        stmt.query_map(params![camera_id, cutoff], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    let out: Vec<Value> = rows
        .into_iter()
        .map(|(ts, bird_count, avg, min, det, inf, img, ann, created)| {
            json!({
                "camera_id": camera_id,
                "timestamp": ts,
                "bird_count": bird_count,
                "confidence_avg": avg,
                "min_confidence": min,
                "detections": parse_detections(det.as_deref()),
                "inference_time_ms": inf,
                "image_url": image_url_for(&camera_id, img.as_deref()),
                "annotated_image_url": annotated_url_for(
                    &state.trailcam.processed_dir, &camera_id, ann.as_deref(),
                ),
                "created_at": created,
            })
        })
        .collect();
    Json(out).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/trailcam/image/{camera_id}/{filename} — serve a processed JPEG.
// ---------------------------------------------------------------------------

/// Both path segments are validated (no separators, no `..`, `.jpg` only), so
/// the join can't escape the processed directory.
pub(crate) async fn trailcam_image(
    State(state): State<AppState>,
    Path((camera_id, filename)): Path<(String, String)>,
) -> Response {
    fn safe_segment(s: &str) -> bool {
        !s.is_empty() && !s.contains('/') && !s.contains('\\') && !s.contains("..")
    }
    if !safe_segment(&camera_id)
        || !safe_segment(&filename)
        || !filename.to_ascii_lowercase().ends_with(".jpg")
    {
        return err(StatusCode::NOT_FOUND, "not_found", "No such image.");
    }

    let path = state
        .trailcam
        .processed_dir
        .join(&camera_id)
        .join(&filename);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => return err(StatusCode::NOT_FOUND, "not_found", "No such image."),
    };

    let mime = mime_guess::from_path(&filename).first_or_octet_stream();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
        bytes,
    )
        .into_response()
}
