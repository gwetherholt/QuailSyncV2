//! Trail-cam read endpoints.
//!
//! The trail-cam pipeline (separate process, see `trailcam/`) appends one JSON
//! object per line to `processed/observations.jsonl` and stores the JPEGs in
//! `processed/{camera_id}/`. Until a proper `POST /api/trailcam/observation`
//! endpoint + table exist, these handlers read that file directly to surface
//! the latest observation per camera and to serve the images.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

use crate::state::AppState;

/// JSON error body shared by these handlers.
fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

/// `GET /api/trailcam/cameras` — distinct cameras seen in `observations.jsonl`.
///
/// Returns `[{ "camera_id", "label" }]` with labels "Outdoor Cam 1", "Outdoor
/// Cam 2", … numbered by order of first appearance. A missing/empty log yields
/// `[]` (clients then show no outdoor cameras).
pub(crate) async fn trailcam_cameras(State(state): State<AppState>) -> Response {
    let content = match std::fs::read_to_string(state.trailcam.observations_path()) {
        Ok(c) => c,
        Err(_) => return Json(json!([])).into_response(),
    };

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut cameras: Vec<Value> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obs: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let camera_id = match obs.get("camera_id").and_then(Value::as_str) {
            Some(c) if !c.is_empty() => c,
            _ => continue,
        };
        if seen.insert(camera_id.to_string()) {
            let n = cameras.len() + 1;
            cameras.push(json!({ "camera_id": camera_id, "label": format!("Outdoor Cam {n}") }));
        }
    }

    Json(cameras).into_response()
}

/// `GET /api/trailcam/latest/{camera_id}` — most recent observation for a camera.
///
/// Scans `observations.jsonl` for the matching camera_id with the greatest
/// `timestamp` (ISO strings sort chronologically; ties resolve to the later
/// line) and returns `{ camera_id, bird_count, timestamp, confidence_avg,
/// detections, image_url }`. 404 if the log is missing or has no such camera.
pub(crate) async fn trailcam_latest(
    State(state): State<AppState>,
    Path(camera_id): Path<String>,
) -> Response {
    let obs_path = state.trailcam.observations_path();
    let content = match std::fs::read_to_string(&obs_path) {
        Ok(c) => c,
        Err(_) => {
            return err(
                StatusCode::NOT_FOUND,
                "no_observations",
                "No trail-cam observations recorded yet.",
            )
        }
    };

    // Pick the matching observation with the largest timestamp; on a tie or a
    // missing/empty timestamp, the later line wins.
    let mut latest: Option<Value> = None;
    let mut latest_ts = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let obs: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue, // skip a corrupt line rather than failing
        };
        if obs.get("camera_id").and_then(Value::as_str) != Some(camera_id.as_str()) {
            continue;
        }
        let ts = obs
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if latest.is_none() || ts >= latest_ts {
            latest_ts = ts;
            latest = Some(obs);
        }
    }

    let Some(obs) = latest else {
        return err(
            StatusCode::NOT_FOUND,
            "not_found",
            "No observation for that camera.",
        );
    };

    // Build the image URL from the stored image_path's filename (basename only,
    // so the server never echoes a host path).
    let image_url = obs
        .get("image_path")
        .and_then(Value::as_str)
        .and_then(|p| p.rsplit(['/', '\\']).next())
        .filter(|f| !f.is_empty())
        .map(|filename| json!(format!("/api/trailcam/image/{camera_id}/{filename}")))
        .unwrap_or(Value::Null);

    let body = json!({
        "camera_id": camera_id,
        "bird_count": obs.get("bird_count").cloned().unwrap_or(Value::Null),
        "timestamp": obs.get("timestamp").cloned().unwrap_or(Value::Null),
        "confidence_avg": obs.get("average_confidence").cloned().unwrap_or(Value::Null),
        "detections": obs.get("detections").cloned().unwrap_or_else(|| json!([])),
        "image_url": image_url,
    });
    (StatusCode::OK, Json(body)).into_response()
}

/// `GET /api/trailcam/image/{camera_id}/{filename}` — serve a processed JPEG.
///
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
