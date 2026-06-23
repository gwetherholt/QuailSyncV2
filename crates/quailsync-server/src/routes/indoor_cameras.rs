//! Indoor-camera (RTSP) registry, CRUD + assignment.
//!
//! Mirrors the SPYPOINT trail-camera registry (`routes/trail_cameras.rs`) and
//! the Govee sensor system: cameras auto-register when the poller's first
//! observation is seen (keyed by `camera_id`) or are created/managed via the
//! `/api/indoor-cameras` CRUD endpoints, carry an `rtsp_url` for the management
//! UI, and are assignable to a housing unit — at most one active assignment per
//! camera (enforced by a partial unique index on
//! `indoor_camera_assignments(indoor_camera_id) WHERE unassigned_at IS NULL`).
//!
//! Scope: unlike trail cameras (which can go on brooders OR hutches), indoor
//! cameras only watch **brooders or incubators** — [`assign_camera`] rejects a
//! target whose `housing_type` is `hutch`.
//!
//! Note on paths: the registry/observation split mirrors trail cams. CRUD +
//! assignment live under `/api/indoor-cameras`; the observation ingest/read +
//! image serving live under `/api/indoorcam/*` (see `routes/indoorcam.rs`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::{params, Connection};

use crate::state::{acquire_db, db_error, internal_error_response, AppState};

// ---------------------------------------------------------------------------
// Read helpers — assemble an IndoorCamera (with current assignment) from DB.
// ---------------------------------------------------------------------------

/// The camera's current (open) assignment, or `None` if it's unassigned. Joins
/// the housing unit so the response carries its name + housing_type.
fn fetch_active_assignment(conn: &Connection, camera_id: i64) -> Option<IndoorCameraAssignment> {
    conn.query_row(
        "SELECT ica.brooder_id, b.name, b.housing_type, ica.assigned_at
         FROM indoor_camera_assignments ica
         JOIN brooders b ON b.id = ica.brooder_id
         WHERE ica.indoor_camera_id = ?1 AND ica.unassigned_at IS NULL",
        params![camera_id],
        |row| {
            Ok(IndoorCameraAssignment {
                brooder_id: row.get(0)?,
                brooder_name: row.get(1)?,
                housing_type: row.get(2)?,
                assigned_at: row.get(3)?,
            })
        },
    )
    .ok()
}

/// Load a single camera with its current assignment.
fn fetch_camera(conn: &Connection, camera_id: i64) -> Option<IndoorCamera> {
    let (id, camera_id_str, name, rtsp_url, model, first_seen, last_seen, created_at) = conn
        .query_row(
            "SELECT id, camera_id, name, rtsp_url, model, first_seen, last_seen, created_at
             FROM indoor_cameras WHERE id = ?1",
            params![camera_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .ok()?;
    Some(IndoorCamera {
        id,
        camera_id: camera_id_str,
        name,
        rtsp_url,
        model,
        first_seen,
        last_seen,
        created_at,
        assignment: fetch_active_assignment(conn, id),
    })
}

/// Build `IndoorCamera`s for a list of ids, preserving order and skipping any
/// that vanished (shouldn't happen under the mutex, but stay tolerant).
fn fetch_cameras(conn: &Connection, ids: &[i64]) -> Vec<IndoorCamera> {
    ids.iter()
        .filter_map(|id| fetch_camera(conn, *id))
        .collect()
}

/// Auto-register an indoor camera the first time the poller's observations
/// surface it, or bump its `last_seen` if already known. Best-effort: errors
/// are swallowed so a read path never breaks on a write failure. Called from
/// the observation ingest handler (see `routes/indoorcam.rs`).
pub(crate) fn ensure_indoor_camera(conn: &Connection, camera_id: &str) {
    if camera_id.is_empty() {
        return;
    }
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM indoor_cameras WHERE camera_id = ?1",
            params![camera_id],
            |row| row.get(0),
        )
        .ok();
    match existing {
        Some(id) => {
            let _ = conn.execute(
                "UPDATE indoor_cameras SET last_seen = CURRENT_TIMESTAMP WHERE id = ?1",
                params![id],
            );
        }
        None => {
            let _ = conn.execute(
                "INSERT INTO indoor_cameras (camera_id) VALUES (?1)",
                params![camera_id],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// POST /api/indoor-cameras — create (or upsert metadata on) a camera.
// ---------------------------------------------------------------------------

/// Create the camera if new (201), or update its `name`/`rtsp_url`/`model` when
/// provided (200). Idempotent upsert keyed on `camera_id`.
pub(crate) async fn create_camera(
    State(state): State<AppState>,
    Json(body): Json<RegisterIndoorCameraRequest>,
) -> impl IntoResponse {
    if body.camera_id.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "camera_id is required").into_response();
    }
    let conn = acquire_db(&state);

    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM indoor_cameras WHERE camera_id = ?1",
            params![body.camera_id],
            |row| row.get(0),
        )
        .ok();

    let (id, created) = match existing {
        Some(id) => {
            // Update only the fields the caller supplied.
            if body.name.is_some() {
                if let Err(e) = conn.execute(
                    "UPDATE indoor_cameras SET name = ?1 WHERE id = ?2",
                    params![body.name, id],
                ) {
                    return db_error(e);
                }
            }
            if body.rtsp_url.is_some() {
                if let Err(e) = conn.execute(
                    "UPDATE indoor_cameras SET rtsp_url = ?1 WHERE id = ?2",
                    params![body.rtsp_url, id],
                ) {
                    return db_error(e);
                }
            }
            if body.model.is_some() {
                if let Err(e) = conn.execute(
                    "UPDATE indoor_cameras SET model = ?1 WHERE id = ?2",
                    params![body.model, id],
                ) {
                    return db_error(e);
                }
            }
            (id, false)
        }
        None => {
            if let Err(e) = conn.execute(
                "INSERT INTO indoor_cameras (camera_id, name, rtsp_url, model)
                 VALUES (?1, ?2, ?3, ?4)",
                params![body.camera_id, body.name, body.rtsp_url, body.model],
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
// GET /api/indoor-cameras — all registered cameras + current assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn list_cameras(State(state): State<AppState>) -> Json<Vec<IndoorCamera>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM indoor_cameras ORDER BY id")
            .expect("prepare failed");
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_cameras(&conn, &ids))
}

// ---------------------------------------------------------------------------
// GET /api/indoor-cameras/{id} — one camera + current assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn get_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    match fetch_camera(&conn, camera_id) {
        Some(camera) => (StatusCode::OK, Json(camera)).into_response(),
        None => (StatusCode::NOT_FOUND, "camera not found").into_response(),
    }
}

// ---------------------------------------------------------------------------
// PUT /api/indoor-cameras/{id} — update metadata (name / rtsp_url / model).
// ---------------------------------------------------------------------------

pub(crate) async fn update_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
    Json(body): Json<UpdateIndoorCameraRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM indoor_cameras WHERE id = ?1",
            params![camera_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return (StatusCode::NOT_FOUND, "camera not found").into_response();
    }

    // Each present field overwrites; omitted fields are left unchanged.
    if body.name.is_some() {
        if let Err(e) = conn.execute(
            "UPDATE indoor_cameras SET name = ?1 WHERE id = ?2",
            params![body.name, camera_id],
        ) {
            return db_error(e);
        }
    }
    if body.rtsp_url.is_some() {
        if let Err(e) = conn.execute(
            "UPDATE indoor_cameras SET rtsp_url = ?1 WHERE id = ?2",
            params![body.rtsp_url, camera_id],
        ) {
            return db_error(e);
        }
    }
    if body.model.is_some() {
        if let Err(e) = conn.execute(
            "UPDATE indoor_cameras SET model = ?1 WHERE id = ?2",
            params![body.model, camera_id],
        ) {
            return db_error(e);
        }
    }

    match fetch_camera(&conn, camera_id) {
        Some(camera) => (StatusCode::OK, Json(camera)).into_response(),
        None => internal_error_response(),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/indoor-cameras/{id} — remove the camera + its assignment rows.
// ---------------------------------------------------------------------------

/// Deletes the camera and any assignment history. Observations are keyed by the
/// `camera_id` string (not a FK) and are intentionally left in place so the
/// historical record survives a camera being decommissioned. Idempotent: a
/// missing camera is a no-op 204.
pub(crate) async fn delete_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "DELETE FROM indoor_camera_assignments WHERE indoor_camera_id = ?1",
        params![camera_id],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "DELETE FROM indoor_cameras WHERE id = ?1",
        params![camera_id],
    ) {
        return db_error(e);
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// PUT /api/indoor-cameras/{id}/assign — (re)assign to a brooder/incubator.
// ---------------------------------------------------------------------------

pub(crate) async fn assign_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
    Json(body): Json<AssignIndoorCameraRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let camera_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM indoor_cameras WHERE id = ?1",
            params![camera_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if camera_exists == 0 {
        return (StatusCode::NOT_FOUND, "camera not found").into_response();
    }

    // The target must exist AND be a brooder or incubator — indoor cameras
    // never watch hutches.
    let housing_type: Option<String> = conn
        .query_row(
            "SELECT housing_type FROM brooders WHERE id = ?1",
            params![body.brooder_id],
            |row| row.get(0),
        )
        .ok();
    match housing_type.as_deref() {
        None => return (StatusCode::BAD_REQUEST, "brooder not found").into_response(),
        Some("hutch") => {
            return (
                StatusCode::BAD_REQUEST,
                "indoor cameras can only be assigned to a brooder or incubator, not a hutch",
            )
                .into_response()
        }
        Some(_) => {}
    }

    // Close any existing active assignment FIRST so the partial unique index
    // (one open assignment per camera) doesn't reject the new row.
    if let Err(e) = conn.execute(
        "UPDATE indoor_camera_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE indoor_camera_id = ?1 AND unassigned_at IS NULL",
        params![camera_id],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "INSERT INTO indoor_camera_assignments (indoor_camera_id, brooder_id) VALUES (?1, ?2)",
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
// DELETE /api/indoor-cameras/{id}/assign — clear the active assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn unassign_camera(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    // Idempotent: closing a non-existent active assignment is a no-op 204.
    let _ = conn.execute(
        "UPDATE indoor_camera_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE indoor_camera_id = ?1 AND unassigned_at IS NULL",
        params![camera_id],
    );
    StatusCode::NO_CONTENT
}

// ---------------------------------------------------------------------------
// GET /api/brooders/{id}/indoor-cameras — cameras assigned to this unit.
// ---------------------------------------------------------------------------

pub(crate) async fn brooder_indoor_cameras(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
) -> Json<Vec<IndoorCamera>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare(
                "SELECT indoor_camera_id FROM indoor_camera_assignments
                 WHERE brooder_id = ?1 AND unassigned_at IS NULL
                 ORDER BY indoor_camera_id",
            )
            .expect("prepare failed");
        stmt.query_map(params![brooder_id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_cameras(&conn, &ids))
}
