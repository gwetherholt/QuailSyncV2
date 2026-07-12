//! Indoor-camera assignment (a flat mode field for the single indoor camera).
//!
//! The one indoor Tapo camera is assigned to an "incubator" or a "brooder"; the
//! assignment is stored in `camera_assignments` and selects which vision model
//! stage 3 will eventually run. `active_model` is DERIVED from `assignment` via
//! [`quailsync_common::active_model_for`] (the single source of truth stage 3
//! reuses) and is never stored. This is storage + API only — the vision pipeline
//! does NOT consume the assignment yet.
//!
//! Distinct from the housing-unit assignment in `routes/indoor_cameras.rs`
//! (which attaches a camera to a specific brooder/incubator row): this is a flat
//! mode field for the single indoor camera, not a location system.
//!
//! Reads/writes are single short auto-committed statements (the DB is shared with
//! the incubator sidecar under WAL).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use quailsync_common::{active_model_for, CameraAssignmentDto, SetCameraAssignmentRequest};

use crate::state::{acquire_db, db_error, internal_error_response, AppState};

/// JSON error body, matching the other route modules.
fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

/// Load a camera's row and build the DTO with the derived `active_model`.
/// Returns `None` when `camera_id` is unknown.
fn fetch_assignment(conn: &Connection, camera_id: &str) -> Option<CameraAssignmentDto> {
    conn.query_row(
        "SELECT camera_id, assignment, updated_at
         FROM camera_mode_assignments WHERE camera_id = ?1",
        params![camera_id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
    .ok()
    .flatten()
    .map(|(camera_id, assignment, updated_at)| {
        // Writes are validated, so `assignment` is always valid here; fall back
        // defensively for a hand-edited row rather than panicking.
        let active_model = active_model_for(&assignment)
            .unwrap_or("incubation")
            .to_string();
        CameraAssignmentDto {
            camera_id,
            assignment,
            active_model,
            updated_at,
        }
    })
}

/// `GET /api/cameras/{id}/assignment` — current assignment + derived model.
/// 404 when `camera_id` is unknown.
pub(crate) async fn get_assignment(
    State(state): State<AppState>,
    Path(camera_id): Path<String>,
) -> Response {
    let conn = acquire_db(&state);
    match fetch_assignment(&conn, &camera_id) {
        Some(dto) => (StatusCode::OK, Json(dto)).into_response(),
        None => err(StatusCode::NOT_FOUND, "not_found", "camera not found"),
    }
}

/// `PUT /api/cameras/{id}/assignment` — set the assignment (`incubator` |
/// `brooder`), upserting `assignment` + `updated_at`, and return the updated DTO.
/// 400 on any other value.
pub(crate) async fn set_assignment(
    State(state): State<AppState>,
    Path(camera_id): Path<String>,
    Json(body): Json<SetCameraAssignmentRequest>,
) -> Response {
    // Validate via the same mapping that derives the model — one source of truth
    // for what a valid assignment is.
    if active_model_for(&body.assignment).is_none() {
        return err(
            StatusCode::BAD_REQUEST,
            "invalid_assignment",
            "assignment must be 'incubator' or 'brooder'",
        );
    }

    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO camera_mode_assignments (camera_id, assignment, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ','now'))
         ON CONFLICT(camera_id) DO UPDATE SET
             assignment = excluded.assignment,
             updated_at = excluded.updated_at",
        params![camera_id, body.assignment],
    ) {
        return db_error(e);
    }

    match fetch_assignment(&conn, &camera_id) {
        Some(dto) => (StatusCode::OK, Json(dto)).into_response(),
        None => internal_error_response(),
    }
}
