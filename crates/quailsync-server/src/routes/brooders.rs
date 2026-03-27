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
    if let Some(bl_id) = body.bloodline_id {
        let exists = conn
            .query_row(
                "SELECT COUNT(*) FROM bloodlines WHERE id = ?1",
                params![bl_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return (
                StatusCode::BAD_REQUEST,
                format!("Bloodline #{bl_id} does not exist"),
            )
                .into_response();
        }
    }
    match conn.execute(
        "INSERT INTO brooders (name, bloodline_id, life_stage, qr_code, notes, camera_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![body.name, body.bloodline_id, life_stage_to_str(&body.life_stage), body.qr_code, body.notes, body.camera_url],
    ) {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            (StatusCode::CREATED, Json(Brooder {
                id, name: body.name, bloodline_id: body.bloodline_id, life_stage: body.life_stage,
                qr_code: body.qr_code, notes: body.notes, camera_url: body.camera_url,
            })).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed to create brooder: {e}")).into_response(),
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
    if let Some(bl_id) = body.get("bloodline_id") {
        if bl_id.is_null() {
            conn.execute(
                "UPDATE brooders SET bloodline_id = NULL WHERE id = ?1",
                params![id],
            )
            .ok();
        } else if let Some(v) = bl_id.as_i64() {
            conn.execute(
                "UPDATE brooders SET bloodline_id = ?1 WHERE id = ?2",
                params![v, id],
            )
            .ok();
        }
    }
    StatusCode::OK.into_response()
}

pub(crate) async fn list_brooders(State(state): State<AppState>) -> Json<Vec<Brooder>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare("SELECT id, name, bloodline_id, life_stage, qr_code, notes, camera_url FROM brooders ORDER BY id").expect("prepare failed");
    let rows: Vec<Brooder> = stmt
        .query_map([], row_to_brooder)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
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
        "SELECT id, name, bloodline_id, life_stage, qr_code, notes, camera_url FROM brooders WHERE id = ?1",
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
            let config = state.alert_config.clone();
            let mut alert = false;
            let mut msg = None;
            if temp < config.brooder_temp_min || temp > config.brooder_temp_max {
                alert = true;
                msg = Some(format!(
                    "Temperature {:.1}\u{00b0}F out of range ({:.1}-{:.1})",
                    temp, config.brooder_temp_min, config.brooder_temp_max
                ));
            } else if hum < config.humidity_min || hum > config.humidity_max {
                alert = true;
                msg = Some(format!(
                    "Humidity {:.1}% out of range ({:.1}-{:.1})",
                    hum, config.humidity_min, config.humidity_max
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
        "SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes FROM chick_groups WHERE id = ?1",
        params![body.group_id], row_to_chick_group,
    ) {
        Ok(group) => (StatusCode::OK, Json(group)).into_response(),
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
    let mut stmt = conn.prepare(
        "SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes
         FROM chick_groups WHERE brooder_id = ?1 AND status = 'Active'"
    ).expect("prepare failed");
    let groups: Vec<ChickGroup> = stmt
        .query_map(params![brooder_id], row_to_chick_group)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let mut stmt = conn.prepare(
        "SELECT id, band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id, current_brooder_id
         FROM birds WHERE current_brooder_id = ?1 AND status = 'Active'"
    ).expect("prepare failed");
    let birds: Vec<Bird> = stmt
        .query_map(params![brooder_id], row_to_bird)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(BrooderResidentsResponse {
        brooder_id,
        chick_groups: groups,
        individual_birds: birds,
    })
}
