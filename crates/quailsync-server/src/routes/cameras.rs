use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use quailsync_common::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_camera(
    State(state): State<AppState>,
    Json(body): Json<CreateCameraFeed>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO camera_feeds (name, location, feed_url, status, brooder_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![body.name, body.location, body.feed_url, camera_status_to_str(&body.status), body.brooder_id],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (StatusCode::CREATED, Json(CameraFeed {
        id, name: body.name, location: body.location, feed_url: body.feed_url, status: body.status, brooder_id: body.brooder_id,
    })).into_response()
}

pub(crate) async fn list_cameras(State(state): State<AppState>) -> Json<Vec<CameraFeed>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare("SELECT id, name, location, feed_url, status, brooder_id FROM camera_feeds ORDER BY id").expect("prepare failed");
    let rows: Vec<CameraFeed> = stmt.query_map([], |row| {
        let status_str: String = row.get(4)?;
        Ok(CameraFeed { id: row.get(0)?, name: row.get(1)?, location: row.get(2)?, feed_url: row.get(3)?, status: str_to_camera_status(&status_str), brooder_id: row.get(5)? })
    }).unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}

pub(crate) async fn delete_camera(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let affected = conn.execute("DELETE FROM camera_feeds WHERE id = ?1", params![id]).unwrap_or(0);
    if affected > 0 { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
}

pub(crate) async fn update_camera_brooder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let brooder_id = body.get("brooder_id").and_then(|v| v.as_i64());
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute("UPDATE camera_feeds SET brooder_id = ?1 WHERE id = ?2", params![brooder_id, id]) {
        return db_error(e);
    }
    StatusCode::OK.into_response()
}

pub(crate) async fn create_frame(
    State(state): State<AppState>,
    Json(body): Json<CreateFrameCapture>,
) -> impl IntoResponse {
    let now = Utc::now();
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO frame_captures (camera_id, timestamp, image_path, life_stage) VALUES (?1, ?2, ?3, ?4)",
        params![body.camera_id, now.to_rfc3339(), body.image_path, life_stage_to_str(&body.life_stage)],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (StatusCode::CREATED, Json(FrameCapture { id, camera_id: body.camera_id, timestamp: now, image_path: body.image_path, life_stage: body.life_stage })).into_response()
}

pub(crate) async fn create_frame_detections(
    State(state): State<AppState>,
    Path(frame_id): Path<i64>,
    Json(body): Json<Vec<CreateDetectionResult>>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let mut results = Vec::new();
    for d in body {
        if let Err(e) = conn.execute(
            "INSERT INTO detection_results (frame_id, label, confidence, bounding_box_x, bounding_box_y, bounding_box_w, bounding_box_h, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![frame_id, d.label, d.confidence, d.bounding_box_x, d.bounding_box_y, d.bounding_box_w, d.bounding_box_h, d.notes],
        ) {
            return db_error(e);
        }
        let id = conn.last_insert_rowid();
        results.push(DetectionResult {
            id, frame_id, label: d.label, confidence: d.confidence,
            bounding_box_x: d.bounding_box_x, bounding_box_y: d.bounding_box_y,
            bounding_box_w: d.bounding_box_w, bounding_box_h: d.bounding_box_h, notes: d.notes,
        });
    }
    (StatusCode::CREATED, Json(results)).into_response()
}

#[derive(Deserialize)]
pub(crate) struct FrameQueryParams {
    camera_id: Option<i64>,
    minutes: Option<u64>,
}

pub(crate) async fn list_frames(
    State(state): State<AppState>,
    Query(params): Query<FrameQueryParams>,
) -> Json<Vec<FrameCapture>> {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);

    let (sql, binds): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match params.camera_id {
        Some(cid) => (
            "SELECT id, camera_id, timestamp, image_path, life_stage FROM frame_captures WHERE camera_id = ?1 AND timestamp >= datetime('now', ?2) ORDER BY id DESC".to_string(),
            vec![Box::new(cid) as Box<dyn rusqlite::types::ToSql>, Box::new(format!("-{minutes} minutes"))],
        ),
        None => (
            "SELECT id, camera_id, timestamp, image_path, life_stage FROM frame_captures WHERE timestamp >= datetime('now', ?1) ORDER BY id DESC".to_string(),
            vec![Box::new(format!("-{minutes} minutes")) as Box<dyn rusqlite::types::ToSql>],
        ),
    };

    let mut stmt = conn.prepare(&sql).expect("prepare failed");
    let rows: Vec<FrameCapture> = stmt
        .query_map(rusqlite::params_from_iter(binds.iter()), |row| {
            let ts_str: String = row.get(2)?;
            let stage_str: String = row.get(4)?;
            Ok(FrameCapture {
                id: row.get(0)?, camera_id: row.get(1)?,
                timestamp: ts_str.parse::<DateTime<Utc>>().unwrap_or_default(),
                image_path: row.get(3)?, life_stage: str_to_life_stage(&stage_str),
            })
        }).unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}

#[derive(Serialize)]
pub(crate) struct DetectionSummaryEntry {
    label: String,
    count: i64,
    avg_confidence: f64,
}

pub(crate) async fn camera_detection_summary(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> Json<Vec<DetectionSummaryEntry>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare(
        "SELECT dr.label, COUNT(*), AVG(dr.confidence) FROM detection_results dr
         JOIN frame_captures fc ON fc.id = dr.frame_id
         WHERE fc.camera_id = ?1 AND fc.timestamp >= datetime('now', '-60 minutes')
         GROUP BY dr.label ORDER BY COUNT(*) DESC"
    ).expect("prepare failed");
    let rows: Vec<DetectionSummaryEntry> = stmt
        .query_map(params![camera_id], |row| Ok(DetectionSummaryEntry { label: row.get(0)?, count: row.get(1)?, avg_confidence: row.get(2)? }))
        .unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}
