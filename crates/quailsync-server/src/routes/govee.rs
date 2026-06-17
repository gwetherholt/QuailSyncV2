//! Govee H5179 WiFi temp/humidity sensor endpoints.
//!
//! A separate Python poller hits the Govee cloud API and POSTs batches of
//! readings to `POST /api/govee/readings`. Sensors auto-register on first sight
//! (keyed by `govee_device_id`) and are dynamically assignable to housing units
//! — a sensor can move between brooders/hutches, with at most one active
//! assignment at a time (enforced by a partial unique index on
//! `sensor_assignments(govee_sensor_id) WHERE unassigned_at IS NULL`).

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::{params, Connection};

use crate::state::{acquire_db, db_error, internal_error_response, AppState};

// ---------------------------------------------------------------------------
// Read helpers — assemble a GoveeSensor (assignment + latest reading) from DB.
// ---------------------------------------------------------------------------

/// The sensor's current (open) assignment, or `None` if it's unassigned.
fn fetch_active_assignment(conn: &Connection, sensor_id: i64) -> Option<GoveeAssignment> {
    conn.query_row(
        "SELECT sa.brooder_id, b.name, sa.assigned_at
         FROM sensor_assignments sa
         JOIN brooders b ON b.id = sa.brooder_id
         WHERE sa.govee_sensor_id = ?1 AND sa.unassigned_at IS NULL",
        params![sensor_id],
        |row| {
            Ok(GoveeAssignment {
                brooder_id: row.get(0)?,
                brooder_name: row.get(1)?,
                assigned_at: row.get(2)?,
            })
        },
    )
    .ok()
}

/// The sensor's most recent reading (by recorded_at), or `None` if it has none.
fn fetch_latest_reading(conn: &Connection, sensor_id: i64) -> Option<GoveeLatestReading> {
    conn.query_row(
        "SELECT temperature_f, humidity, recorded_at FROM govee_readings
         WHERE govee_sensor_id = ?1
         ORDER BY recorded_at DESC, id DESC LIMIT 1",
        params![sensor_id],
        |row| {
            Ok(GoveeLatestReading {
                temperature_f: row.get(0)?,
                humidity: row.get(1)?,
                recorded_at: row.get(2)?,
            })
        },
    )
    .ok()
}

/// Load a single sensor with its current assignment + latest reading.
fn fetch_sensor(conn: &Connection, sensor_id: i64) -> Option<GoveeSensor> {
    let (id, govee_device_id, name, model, first_seen, last_seen) = conn
        .query_row(
            "SELECT id, govee_device_id, name, model, first_seen, last_seen
             FROM govee_sensors WHERE id = ?1",
            params![sensor_id],
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
    Some(GoveeSensor {
        id,
        govee_device_id,
        name,
        model,
        first_seen,
        last_seen,
        assignment: fetch_active_assignment(conn, id),
        latest_reading: fetch_latest_reading(conn, id),
    })
}

/// Build `GoveeSensor`s for a list of sensor ids, preserving order and skipping
/// any that vanished (shouldn't happen under the mutex, but stay tolerant).
fn fetch_sensors(conn: &Connection, ids: &[i64]) -> Vec<GoveeSensor> {
    ids.iter()
        .filter_map(|id| fetch_sensor(conn, *id))
        .collect()
}

// ---------------------------------------------------------------------------
// POST /api/govee/readings — ingest a batch from the poller.
// ---------------------------------------------------------------------------

/// For each reading: auto-register the sensor (or bump its `last_seen`), then
/// store the reading. Returns 201 with the count stored.
pub(crate) async fn ingest_readings(
    State(state): State<AppState>,
    Json(body): Json<GoveeReadingsRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let mut stored: i64 = 0;

    for r in &body.readings {
        // 1. Auto-register the sensor if this device_id is new; otherwise just
        //    bump last_seen. govee_device_id is UNIQUE so it's the natural key.
        let existing: Option<i64> = conn
            .query_row(
                "SELECT id FROM govee_sensors WHERE govee_device_id = ?1",
                params![r.device_id],
                |row| row.get(0),
            )
            .ok();
        let sensor_id = match existing {
            Some(id) => {
                // 2. Update last_seen on the known sensor.
                if let Err(e) = conn.execute(
                    "UPDATE govee_sensors SET last_seen = CURRENT_TIMESTAMP WHERE id = ?1",
                    params![id],
                ) {
                    return db_error(e);
                }
                id
            }
            None => {
                if let Err(e) = conn.execute(
                    "INSERT INTO govee_sensors (govee_device_id, name, model) VALUES (?1, ?2, ?3)",
                    params![r.device_id, r.name, r.model],
                ) {
                    return db_error(e);
                }
                conn.last_insert_rowid()
            }
        };

        // 3. Store the reading.
        if let Err(e) = conn.execute(
            "INSERT INTO govee_readings (govee_sensor_id, temperature_f, humidity, recorded_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![sensor_id, r.temperature_f, r.humidity, r.recorded_at],
        ) {
            return db_error(e);
        }
        stored += 1;
    }

    (StatusCode::CREATED, Json(GoveeReadingsResponse { stored })).into_response()
}

// ---------------------------------------------------------------------------
// GET /api/govee/sensors — all registered sensors + current status.
// ---------------------------------------------------------------------------

pub(crate) async fn list_sensors(State(state): State<AppState>) -> Json<Vec<GoveeSensor>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM govee_sensors ORDER BY id")
            .expect("prepare failed");
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_sensors(&conn, &ids))
}

// ---------------------------------------------------------------------------
// PUT /api/govee/sensors/{id}/assign — (re)assign a sensor to a brooder.
// ---------------------------------------------------------------------------

pub(crate) async fn assign_sensor(
    State(state): State<AppState>,
    Path(sensor_id): Path<i64>,
    Json(body): Json<AssignSensorRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let sensor_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM govee_sensors WHERE id = ?1",
            params![sensor_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if sensor_exists == 0 {
        return (StatusCode::NOT_FOUND, "sensor not found").into_response();
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
    // (one open assignment per sensor) doesn't reject the new row.
    if let Err(e) = conn.execute(
        "UPDATE sensor_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE govee_sensor_id = ?1 AND unassigned_at IS NULL",
        params![sensor_id],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "INSERT INTO sensor_assignments (govee_sensor_id, brooder_id) VALUES (?1, ?2)",
        params![sensor_id, body.brooder_id],
    ) {
        return db_error(e);
    }

    match fetch_sensor(&conn, sensor_id) {
        Some(sensor) => (StatusCode::OK, Json(sensor)).into_response(),
        None => internal_error_response(),
    }
}

// ---------------------------------------------------------------------------
// DELETE /api/govee/sensors/{id}/assign — clear the active assignment.
// ---------------------------------------------------------------------------

pub(crate) async fn unassign_sensor(
    State(state): State<AppState>,
    Path(sensor_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    // Idempotent: closing a non-existent active assignment is a no-op 204.
    let _ = conn.execute(
        "UPDATE sensor_assignments SET unassigned_at = CURRENT_TIMESTAMP
         WHERE govee_sensor_id = ?1 AND unassigned_at IS NULL",
        params![sensor_id],
    );
    StatusCode::NO_CONTENT
}

// ---------------------------------------------------------------------------
// GET /api/brooders/{id}/sensors — sensors currently in this brooder.
// ---------------------------------------------------------------------------

pub(crate) async fn brooder_sensors(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
) -> Json<Vec<GoveeSensor>> {
    let conn = acquire_db(&state);
    let ids: Vec<i64> = {
        let mut stmt = conn
            .prepare(
                "SELECT govee_sensor_id FROM sensor_assignments
                 WHERE brooder_id = ?1 AND unassigned_at IS NULL
                 ORDER BY govee_sensor_id",
            )
            .expect("prepare failed");
        stmt.query_map(params![brooder_id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };
    Json(fetch_sensors(&conn, &ids))
}
