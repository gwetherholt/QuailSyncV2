//! SPYPOINT trail-camera registry + assignment.
//!
//! Mirrors the Govee sensor system (see `routes/govee.rs`): cameras auto-register
//! when the poller's photos are first seen (keyed by `spypoint_camera_id`) or are
//! seeded manually, and are dynamically assignable to housing units — at most one
//! active assignment per camera (enforced by a partial unique index on
//! `camera_assignments(trail_camera_id) WHERE unassigned_at IS NULL`).
//!
//! Note on paths: the registry lives under `/api/trail-cameras` rather than
//! `/api/cameras`, because `/api/cameras` is the pre-existing MJPEG/RTSP
//! `camera_feeds` resource (see `routes/cameras.rs`) and `/api/trailcam/cameras`
//! is the observation-derived list (see `routes/trailcam.rs`). The per-brooder
//! endpoint is `/api/brooders/{id}/cameras`.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::{params, Connection};

use crate::state::{acquire_db, db_error, internal_error_response, AppState};

// ---------------------------------------------------------------------------
// Read helpers — assemble a TrailCamera (with current assignment) from DB.
// ---------------------------------------------------------------------------

/// The camera's current (open) assignment, or `None` if it's unassigned.
fn fetch_active_assignment(conn: &Connection, camera_id: i64) -> Option<CameraAssignment> {
    conn.query_row(
        "SELECT ca.brooder_id, b.name, ca.assigned_at
         FROM camera_assignments ca
         JOIN brooders b ON b.id = ca.brooder_id
         WHERE ca.trail_camera_id = ?1 AND ca.unassigned_at IS NULL",
        params![camera_id],
        |row| {
            Ok(CameraAssignment {
                brooder_id: row.get(0)?,
                brooder_name: row.get(1)?,
                assigned_at: row.get(2)?,
            })
        },
    )
    .ok()
}

/// Load a single camera with its current assignment.
fn fetch_camera(conn: &Connection, camera_id: i64) -> Option<TrailCamera> {
    let (id, spypoint_camera_id, name, model, first_seen, last_seen) = conn
        .query_row(
            "SELECT id, spypoint_camera_id, name, model, first_seen, last_seen
             FROM trail_cameras WHERE id = ?1",
            params![camera_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .ok()?;
    Some(TrailCamera {
        id,
        spypoint_camera_id,
        name,
        model,
        first_seen,
        last_seen,
        assignment: fetch_active_assignment(conn, id),
    })
}

/// Build `TrailCamera`s for a list of ids, preserving order and skipping any
/// that vanished (shouldn't happen under the mutex, but stay tolerant).
fn fetch_cameras(conn: &Connection, ids: &[i64]) -> Vec<TrailCamera> {
    ids.iter()
        .filter_map(|id| fetch_camera(conn, *id))
        .collect()
}

/// Auto-register a trail camera the first time the poller's photos surface it,
/// or bump its `last_seen` if already known. Best-effort: errors are swallowed
/// so a read path (e.g. the observations list) never breaks on a write failure.
/// Called from the trail-cam observation reader (see `routes/trailcam.rs`).
pub(crate) fn ensure_trail_camera(conn: &Connection, spypoint_camera_id: &str) {
    if spypoint_camera_id.is_empty() {
        return;
    }
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM trail_cameras WHERE spypoint_camera_id = ?1",
            params![spypoint_camera_id],
            |row| row.get(0),
        )
        .ok();
    match existing {
        Some(id) => {
            let _ = conn.execute(
                "UPDATE trail_cameras SET last_seen = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id],
            );
        }
        None => {
            let _ = conn.execute(
                "INSERT INTO trail_cameras (spypoint_camera_id) VALUES (?1)",
                params![spypoint_camera_id],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/trail-cameras/register — manual seed / metadata upsert.
// ---------------------------------------------------------------------------

/// Create the camera if new (201), or update its `name`/`model` when provided
/// (200). Returns the camera record.
pub(crate) async fn register_camera(
    State(state): State<AppState>,
    Json(body): Json<RegisterCameraRequest>,
) -> impl IntoResponse {
    if body.spypoint_camera_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "spypoint_camera_id is required").into_response();
    }
    let conn = acquire_db(&state);

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM trail_cameras WHERE spypoint_camera_id = ?1",
            params![body.spypoint_camera_id],
            |row| row.get(0),
        )
        .ok();

    let (id, created) = match existing {
        Some(id) => {
            // Update name/model only when the caller supplied them.
            if body.name.is_some() {
                if let Err(e) = conn.execute(
                    "UPDATE trail_cameras SET name = ?1 WHERE id = ?2",
                    params![body.name, id],
                ) {
                    return db_error(e);
                }
            }
            if body.model.is_some() {
                if let Err(e) = conn.execute(
                    "UPDATE trail_cameras SET model = ?1 WHERE id = ?2",
                    params![body.model, id],
                ) {
                    return db_error(e);
                }
            }
            (id, false)
        }
        None => {
            if let Err(e) = conn.execute(
                "INSERT INTO trail_cameras (spypoint_camera_id, name, model) VALUES (?1, ?2, ?3)",
                params![body.spypoint_camera_id, body.name, body.model],
            ) {
                return db_error(e);
            }
            (conn.last_insert_rowid(), true)
        }
    };

    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    match fetch_camera(&conn, id) {
        Some(camera) => (status, Json(camera)).into_response(),
        None => internal_error_response(),
    }
}

// ---------------------------------------------------------------------------
// GET /api/trail-cameras — all registered cameras + current assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn list_cameras(State(state): State<AppState>) -> Json<Vec<TrailCamera>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM trail_cameras ORDER BY id")
            .expect("prepare failed");
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_cameras(&conn, &ids))
}

// ---------------------------------------------------------------------------
// PUT /api/trail-cameras/{id}/assign — (re)assign a camera to a brooder.
// ---------------------------------------------------------------------------

pub(crate) async fn assign_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
    Json(body): Json<AssignCameraRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let camera_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM trail_cameras WHERE id = ?1",
            params![camera_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if camera_exists == 0 {
        return (StatusCode::NOT_FOUND, "camera not found").into_response();
    }

    let brooder_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM brooders WHERE id = ?1",
            params![body.brooder_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if brooder_exists == 0 {
        return (StatusCode::BAD_REQUEST, "brooder not found").into_response();
    }

    // Close any existing active assignment FIRST so the partial unique index
    // (one open assignment per camera) doesn't reject the new row.
    if let Err(e) = conn.execute(
        "UPDATE camera_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE trail_camera_id = ?1 AND unassigned_at IS NULL",
        params![camera_id],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "INSERT INTO camera_assignments (trail_camera_id, brooder_id) VALUES (?1, ?2)",
        params![camera_id, body.brooder_id],
    ) {
        return db_error(e);
    }

    match fetch_camera(&conn, camera_id) {
        Some(camera) => (StatusCode::OK, Json(camera)).into_response(),
        None => internal_error_response(),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/trail-cameras/{id}/assign — clear the active assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn unassign_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    // Idempotent: closing a non-existent active assignment is a no-op 204.
    let _ = conn.execute(
        "UPDATE camera_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE trail_camera_id = ?1 AND unassigned_at IS NULL",
        params![camera_id],
    );
    StatusCode::NO_CONTENT
}

// ---------------------------------------------------------------------------
// GET /api/brooders/{id}/cameras — cameras currently assigned to this brooder.
// ---------------------------------------------------------------------------

pub(crate) async fn brooder_cameras(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
) -> Json<Vec<TrailCamera>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare(
                "SELECT trail_camera_id FROM camera_assignments
                 WHERE brooder_id = ?1 AND unassigned_at IS NULL
                 ORDER BY trail_camera_id",
            )
            .expect("prepare failed");
        stmt.query_map(params![brooder_id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_cameras(&conn, &ids))
}
