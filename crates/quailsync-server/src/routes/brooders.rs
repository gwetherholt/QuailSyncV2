use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use quailsync_common::*;
use rusqlite::params;
use serde::Serialize;

use crate::alerts::youngest_chick_age_in_brooder;
use crate::db::helpers::*;
use crate::routes::telemetry::HistoryParams;
use crate::state::{acquire_db, db_error, is_brooder_online, AppState};

pub(crate) async fn create_brooder(
    State(state): State<AppState>,
    Json(body): Json<CreateBrooder>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Some(bl_id) = body.lineage_id {
        let exists = conn
            .query_row(
                "SELECT COUNT(*) FROM lineages WHERE id = ?1",
                params![bl_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return (
                StatusCode::BAD_REQUEST,
                format!("Lineage #{bl_id} does not exist"),
            )
                .into_response();
        }
    }
    // Default housing_type to Brooder when the caller doesn't specify one —
    // keeps older clients (pre-issue-#11) writing valid rows.
    let housing = body.housing_type.unwrap_or_default();
    match conn.execute(
        "INSERT INTO brooders (name, lineage_id, life_stage, qr_code, notes, camera_url, housing_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            body.name, body.lineage_id, life_stage_to_str(&body.life_stage),
            body.qr_code, body.notes, body.camera_url, housing_type_to_str(&housing),
        ],
    ) {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            (StatusCode::CREATED, Json(Brooder {
                id, name: body.name, lineage_id: body.lineage_id, life_stage: body.life_stage,
                qr_code: body.qr_code, notes: body.notes, camera_url: body.camera_url,
                housing_type: housing,
            })).into_response()
        }
        Err(e) => {
            eprintln!("[create_brooder] {e}");
            crate::state::internal_error_response()
        }
    }
}

pub(crate) async fn update_brooder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let exists = conn
        .query_row(
            "SELECT COUNT(*) FROM brooders WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return (StatusCode::NOT_FOUND, "brooder not found").into_response();
    }
    if let Some(url) = body.get("camera_url") {
        let val = if url.is_null() {
            None
        } else {
            url.as_str().map(|s| s.to_string())
        };
        conn.execute(
            "UPDATE brooders SET camera_url = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        conn.execute(
            "UPDATE brooders SET name = ?1 WHERE id = ?2",
            params![name, id],
        )
        .ok();
    }
    if let Some(notes) = body.get("notes") {
        let val = if notes.is_null() {
            None
        } else {
            notes.as_str().map(|s| s.to_string())
        };
        conn.execute(
            "UPDATE brooders SET notes = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }
    if let Some(qr) = body.get("qr_code").and_then(|v| v.as_str()) {
        conn.execute(
            "UPDATE brooders SET qr_code = ?1 WHERE id = ?2",
            params![qr, id],
        )
        .ok();
    }
    if let Some(bl_id) = body.get("lineage_id") {
        if bl_id.is_null() {
            conn.execute(
                "UPDATE brooders SET lineage_id = NULL WHERE id = ?1",
                params![id],
            )
            .ok();
        } else if let Some(v) = bl_id.as_i64() {
            conn.execute(
                "UPDATE brooders SET lineage_id = ?1 WHERE id = ?2",
                params![v, id],
            )
            .ok();
        }
    }
    if let Some(ht) = body.get("housing_type").and_then(|v| v.as_str()) {
        // Only accept the three recognised values; anything else is a 400.
        let normalized = ht.to_lowercase();
        if !matches!(normalized.as_str(), "incubator" | "brooder" | "hutch") {
            return (
                StatusCode::BAD_REQUEST,
                "housing_type must be one of: incubator, brooder, hutch",
            )
                .into_response();
        }
        conn.execute(
            "UPDATE brooders SET housing_type = ?1 WHERE id = ?2",
            params![normalized, id],
        )
        .ok();
    }
    StatusCode::OK.into_response()
}

pub(crate) async fn delete_brooder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let exists = conn
        .query_row(
            "SELECT COUNT(*) FROM brooders WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return (StatusCode::NOT_FOUND, "brooder not found").into_response();
    }
    // Delete all readings for this brooder
    conn.execute(
        "DELETE FROM brooder_readings WHERE brooder_id = ?1",
        params![id],
    )
    .ok();
    // Unassign chick groups from this brooder
    conn.execute(
        "UPDATE chick_groups SET brooder_id = NULL WHERE brooder_id = ?1",
        params![id],
    )
    .ok();
    // Clear bird references to this brooder
    conn.execute(
        "UPDATE birds SET current_brooder_id = NULL WHERE current_brooder_id = ?1",
        params![id],
    )
    .ok();
    // Unlink camera feeds from this brooder
    conn.execute(
        "UPDATE camera_feeds SET brooder_id = NULL WHERE brooder_id = ?1",
        params![id],
    )
    .ok();
    // Delete the brooder
    conn.execute("DELETE FROM brooders WHERE id = ?1", params![id])
        .ok();
    // Remove from last_seen tracking
    if let Ok(mut map) = state.last_seen.write() {
        map.remove(&id);
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Per-brooder alerts. The global `alerts` table doesn't track brooder_id,
/// so we return an empty array for now.  A future migration will add
/// brooder_id to the alerts table and populate this properly.
pub(crate) async fn brooder_alerts(
    _state: State<AppState>,
    Path(_id): Path<i64>,
) -> Json<Vec<serde_json::Value>> {
    Json(vec![])
}

#[derive(serde::Deserialize)]
pub(crate) struct HeadcountRequest {
    pub count: i64,
    pub timestamp: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct HeadcountResponse {
    pub brooder_id: i64,
    pub count: i64,
    pub timestamp: String,
}

pub(crate) async fn post_headcount(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<HeadcountRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let ts = body
        .timestamp
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
    match conn.execute(
        "INSERT INTO headcounts (brooder_id, count, timestamp) VALUES (?1, ?2, ?3)",
        params![id, body.count, ts],
    ) {
        Ok(_) => (
            StatusCode::CREATED,
            Json(HeadcountResponse {
                brooder_id: id,
                count: body.count,
                timestamp: ts,
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

pub(crate) async fn get_headcount_latest(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    match conn.query_row(
        "SELECT count, timestamp FROM headcounts WHERE brooder_id = ?1 ORDER BY received_at DESC LIMIT 1",
        params![id],
        |row| Ok(HeadcountResponse {
            brooder_id: id,
            count: row.get(0)?,
            timestamp: row.get(1)?,
        }),
    ) {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(_) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "brooder_id": id,
                "count": serde_json::Value::Null,
                "timestamp": serde_json::Value::Null
            })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub(crate) struct ListBroodersQuery {
    /// Optional housing-type filter — `incubator`, `brooder`, or `hutch`.
    /// Unknown values fall through and the filter is ignored (safer than 400
    /// for a list endpoint).
    pub(crate) r#type: Option<String>,
}

pub(crate) async fn list_brooders(
    State(state): State<AppState>,
    Query(q): Query<ListBroodersQuery>,
) -> Json<Vec<Brooder>> {
    let conn = acquire_db(&state);
    let filter = q
        .r#type
        .as_deref()
        .map(|s| s.to_lowercase())
        .filter(|s| matches!(s.as_str(), "incubator" | "brooder" | "hutch"));

    const COLS: &str = "id, name, lineage_id, life_stage, qr_code, notes, camera_url, housing_type";
    let rows: Vec<Brooder> = match filter {
        Some(ref t) => {
            let sql = format!("SELECT {COLS} FROM brooders WHERE housing_type = ?1 ORDER BY id");
            let mut stmt = conn.prepare(&sql).expect("prepare failed");
            stmt.query_map(params![t], row_to_brooder)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
        None => {
            let sql = format!("SELECT {COLS} FROM brooders ORDER BY id");
            let mut stmt = conn.prepare(&sql).expect("prepare failed");
            stmt.query_map([], row_to_brooder)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
    };
    Json(rows)
}

pub(crate) async fn brooder_readings(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<HistoryParams>,
) -> Json<Vec<BrooderReading>> {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
         WHERE brooder_id = ?1 AND received_at >= datetime('now', ?2) ORDER BY id DESC",
        )
        .expect("prepare failed");
    let cutoff = format!("-{minutes} minutes");
    let readings: Vec<BrooderReading> = stmt
        .query_map(params![id, cutoff], |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_f: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
                brooder_id: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(readings)
}

#[derive(Serialize)]
struct BrooderStatus {
    brooder: Brooder,
    latest_temp: Option<f64>,
    latest_humidity: Option<f64>,
    has_alert: bool,
    alert_message: Option<String>,
    sensor_status: String,
}

pub(crate) async fn brooder_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let brooder = match conn.query_row(
        "SELECT id, name, lineage_id, life_stage, qr_code, notes, camera_url, housing_type FROM brooders WHERE id = ?1",
        params![id], row_to_brooder,
    ) {
        Ok(b) => b,
        Err(_) => return (StatusCode::NOT_FOUND, "brooder not found").into_response(),
    };

    let latest = conn.query_row(
        "SELECT temperature, humidity FROM brooder_readings WHERE brooder_id = ?1 ORDER BY id DESC LIMIT 1",
        params![id], |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
    );

    let (latest_temp, latest_humidity, has_alert, alert_message) = match latest {
        Ok((temp, hum)) => {
            // Use age-based temperature range if a chick group is assigned
            let (temp_min, temp_max) =
                if let Some((_gid, age)) = youngest_chick_age_in_brooder(&conn, id) {
                    let (target, tolerance) = target_temp_for_age(age);
                    (target - tolerance, target + tolerance)
                } else {
                    (ADULT_TEMP_MIN, ADULT_TEMP_MAX)
                };
            let mut alert = false;
            let mut msg = None;
            if temp < temp_min || temp > temp_max {
                alert = true;
                msg = Some(format!(
                    "Temperature {:.1}\u{00b0}F out of range ({:.0}-{:.0})",
                    temp, temp_min, temp_max
                ));
            }
            (Some(temp), Some(hum), alert, msg)
        }
        Err(_) => (None, None, false, None),
    };

    let sensor_status = if is_brooder_online(&state, id) {
        "online".to_string()
    } else {
        "offline".to_string()
    };

    Json(BrooderStatus {
        brooder,
        latest_temp,
        latest_humidity,
        has_alert,
        alert_message,
        sensor_status,
    })
    .into_response()
}

pub(crate) async fn brooder_target_temp(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Some((group_id, age)) = youngest_chick_age_in_brooder(&conn, id) {
        let (target, tolerance) = target_temp_for_age(age);
        let week = (age / 7) + 1;
        let status = if age >= 35 {
            "ambient"
        } else if age >= 28 {
            "weaning"
        } else {
            "heat_required"
        };
        Json(TargetTempResponse {
            brooder_id: id,
            target_temp_f: target,
            min_temp_f: target - tolerance,
            max_temp_f: target + tolerance,
            week,
            age_days: Some(age),
            chick_group_id: Some(group_id),
            schedule_label: temp_schedule_label(age),
            status: status.to_string(),
        })
    } else {
        Json(TargetTempResponse {
            brooder_id: id,
            target_temp_f: (ADULT_TEMP_MIN + ADULT_TEMP_MAX) / 2.0,
            min_temp_f: ADULT_TEMP_MIN,
            max_temp_f: ADULT_TEMP_MAX,
            week: 0,
            age_days: None,
            chick_group_id: None,
            schedule_label: "Unassigned — adult range".to_string(),
            status: "unassigned".to_string(),
        })
    }
}

pub(crate) async fn assign_group_to_brooder(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
    Json(body): Json<AssignGroupRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "UPDATE chick_groups SET brooder_id = NULL WHERE brooder_id = ?1",
        params![brooder_id],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "UPDATE chick_groups SET brooder_id = ?1 WHERE id = ?2",
        params![brooder_id, body.group_id],
    ) {
        return db_error(e);
    }
    match conn.query_row(
        "SELECT id, clutch_id, brooder_id, initial_count, current_count, hatch_date, status, notes, housing_id FROM chick_groups WHERE id = ?1",
        params![body.group_id], row_to_chick_group,
    ) {
        Ok(mut group) => {
            group.lineages = fetch_chick_group_lineages(&conn, group.id);
            (StatusCode::OK, Json(group)).into_response()
        }
        Err(e) => db_error(e),
    }
}

pub(crate) async fn unassign_brooder_group(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "UPDATE chick_groups SET brooder_id = NULL WHERE brooder_id = ?1",
        params![brooder_id],
    ) {
        return db_error(e);
    }
    StatusCode::NO_CONTENT.into_response()
}

pub(crate) async fn brooder_residents(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    // Issue #14: a housing unit's group residents come from two sources —
    //   • brooder/nursery role: Active chick groups whose nursery brooder_id
    //     matches.
    //   • hutch role: Graduated groups that have been assigned via
    //     chick_groups.housing_id.
    // A single OR query covers both; the status field disambiguates on the
    // client and we don't need to know the unit's housing_type here.
    let mut stmt = conn.prepare(
        "SELECT id, clutch_id, brooder_id, initial_count, current_count, hatch_date, status, notes, housing_id
         FROM chick_groups
         WHERE (brooder_id = ?1 AND status = 'Active')
            OR (housing_id  = ?1 AND status = 'Graduated')"
    ).expect("prepare failed");
    let mut groups: Vec<ChickGroup> = stmt
        .query_map(params![brooder_id], row_to_chick_group)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    populate_chick_group_lineages(&conn, &mut groups);

    // Issue #13: residents are individual adult birds permanently assigned
    // via `birds.housing_id`. Chick-stage birds still come in through
    // `chick_groups` above. The two are intentionally disjoint — a bird in
    // a chick group has housing_id = NULL.
    let mut stmt = conn.prepare(
        "SELECT id, band_color, sex, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id, current_brooder_id, photo_path, photo_uploaded_at, housing_id, chick_group_id
         FROM birds WHERE housing_id = ?1 AND status = 'Active'"
    ).expect("prepare failed");
    let mut birds: Vec<Bird> = stmt
        .query_map(params![brooder_id], row_to_bird)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    populate_bird_lineages(&conn, &mut birds);

    // The headcount is purely "Active birds housed here right now" — the same
    // set as `individual_birds` above. Graduated groups are provenance only and
    // must not inflate this number with stale graduation-time counts.
    let active_bird_count = birds.len() as i64;

    Json(BrooderResidentsResponse {
        brooder_id,
        chick_groups: groups,
        individual_birds: birds,
        active_bird_count,
    })
}

// ---------------------------------------------------------------------------
// Issue #13 — assign / unassign individual birds to a housing unit
// ---------------------------------------------------------------------------

/// `POST /api/brooders/{id}/assign-birds` — body `{ "bird_ids": [...] }`.
/// Validates the housing unit and all bird ids exist, then sets
/// `birds.housing_id = {id}` for each. Atomic across the batch.
pub(crate) async fn assign_birds(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
    Json(body): Json<BirdAssignmentRequest>,
) -> impl IntoResponse {
    if body.bird_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "bird_ids must contain at least one id",
        )
            .into_response();
    }
    let conn = acquire_db(&state);

    // Housing unit must exist.
    let housing_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM brooders WHERE id = ?1",
            params![brooder_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if housing_exists == 0 {
        return (StatusCode::NOT_FOUND, "housing unit not found").into_response();
    }

    // All bird ids must exist before any writes — fail loudly on a typo so
    // we don't half-apply the batch.
    for bird_id in &body.bird_ids {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM birds WHERE id = ?1",
                params![bird_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return (
                StatusCode::BAD_REQUEST,
                format!("bird #{bird_id} does not exist"),
            )
                .into_response();
        }
    }

    let mut updated: i64 = 0;
    for bird_id in &body.bird_ids {
        match conn.execute(
            "UPDATE birds SET housing_id = ?1 WHERE id = ?2",
            params![brooder_id, bird_id],
        ) {
            Ok(n) => updated += n as i64,
            Err(e) => return db_error(e),
        }
    }
    (StatusCode::OK, Json(BirdAssignmentResponse { updated })).into_response()
}

/// `POST /api/brooders/{id}/unassign-birds` — clears `housing_id` for every
/// bird id in the body. Tolerant: ids that don't exist or that weren't
/// housed in this unit are simply no-ops (the row count returned reflects
/// only rows actually modified).
pub(crate) async fn unassign_birds(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
    Json(body): Json<BirdAssignmentRequest>,
) -> impl IntoResponse {
    if body.bird_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "bird_ids must contain at least one id",
        )
            .into_response();
    }
    let conn = acquire_db(&state);

    let mut updated: i64 = 0;
    for bird_id in &body.bird_ids {
        // Only clear when the bird is actually in this housing unit; avoids
        // accidentally unhousing a bird that was assigned elsewhere.
        match conn.execute(
            "UPDATE birds SET housing_id = NULL WHERE id = ?1 AND housing_id = ?2",
            params![bird_id, brooder_id],
        ) {
            Ok(n) => updated += n as i64,
            Err(e) => return db_error(e),
        }
    }
    (StatusCode::OK, Json(BirdAssignmentResponse { updated })).into_response()
}

// ---------------------------------------------------------------------------
// Issue #14 — move an already-graduated chick group (and the birds it
// produced) into a hutch.
// ---------------------------------------------------------------------------

/// `POST /api/brooders/{id}/assign-graduated-group` — body `{ "group_id": N }`.
/// Used from the hutch detail view to place a previously-graduated group
/// whose `housing_id` is NULL (or pointed somewhere else). Validates:
///   • target housing unit exists and is of type `hutch`
///   • group exists and has status = `Graduated`
/// Writes both the group row and every bird with `chick_group_id = group_id`
/// in a single pair of UPDATEs.
pub(crate) async fn assign_graduated_group(
    State(state): State<AppState>,
    Path(brooder_id): Path<i64>,
    Json(body): Json<AssignGraduatedGroupRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    // Target must exist and be a hutch.
    let housing_type: Option<String> = conn
        .query_row(
            "SELECT housing_type FROM brooders WHERE id = ?1",
            params![brooder_id],
            |row| row.get(0),
        )
        .ok();
    match housing_type.as_deref() {
        Some("hutch") => {}
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("housing unit #{brooder_id} is a {other}, not a hutch"),
            )
                .into_response();
        }
        None => return (StatusCode::NOT_FOUND, "housing unit not found").into_response(),
    }

    // Group must exist and be graduated.
    let group_status: Option<String> = conn
        .query_row(
            "SELECT status FROM chick_groups WHERE id = ?1",
            params![body.group_id],
            |row| row.get(0),
        )
        .ok();
    match group_status.as_deref() {
        Some("Graduated") => {}
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                format!("group #{} is {other}, not Graduated", body.group_id),
            )
                .into_response();
        }
        None => return (StatusCode::NOT_FOUND, "chick group not found").into_response(),
    }

    if let Err(e) = conn.execute(
        "UPDATE chick_groups SET housing_id = ?1 WHERE id = ?2",
        params![brooder_id, body.group_id],
    ) {
        return db_error(e);
    }
    let birds_updated = match conn.execute(
        "UPDATE birds SET housing_id = ?1 WHERE chick_group_id = ?2 AND status = 'Active'",
        params![brooder_id, body.group_id],
    ) {
        Ok(n) => n as i64,
        Err(e) => return db_error(e),
    };

    (
        StatusCode::OK,
        Json(AssignGraduatedGroupResponse {
            group_id: body.group_id,
            housing_id: brooder_id,
            birds_updated,
        }),
    )
        .into_response()
}
