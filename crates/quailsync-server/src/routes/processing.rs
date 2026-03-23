use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::params;
use serde::Deserialize;

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_processing(
    State(state): State<AppState>,
    Json(body): Json<CreateProcessingRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO processing_records (bird_id, reason, scheduled_date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![body.bird_id, processing_reason_to_str(&body.reason), body.scheduled_date.to_string(), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (StatusCode::CREATED, Json(ProcessingRecord {
        id, bird_id: body.bird_id, reason: body.reason, scheduled_date: body.scheduled_date,
        processed_date: None, final_weight_grams: None, status: ProcessingStatus::Scheduled, notes: body.notes,
    })).into_response()
}

pub(crate) async fn update_processing(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProcessingRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let exists: bool = conn.query_row("SELECT COUNT(*) FROM processing_records WHERE id = ?1", params![id], |row| row.get::<_, i64>(0)).map(|c| c > 0).unwrap_or(false);
    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<ProcessingRecord>)).into_response();
    }

    if let Some(ref d) = body.processed_date {
        if let Err(e) = conn.execute("UPDATE processing_records SET processed_date = ?1 WHERE id = ?2", params![d.to_string(), id]) { return db_error(e); }
    }
    if let Some(w) = body.final_weight_grams {
        if let Err(e) = conn.execute("UPDATE processing_records SET final_weight_grams = ?1 WHERE id = ?2", params![w, id]) { return db_error(e); }
    }
    if let Some(ref s) = body.status {
        if let Err(e) = conn.execute("UPDATE processing_records SET status = ?1 WHERE id = ?2", params![processing_status_to_str(s), id]) { return db_error(e); }
    }
    if let Some(ref n) = body.notes {
        if let Err(e) = conn.execute("UPDATE processing_records SET notes = ?1 WHERE id = ?2", params![n, id]) { return db_error(e); }
    }

    match conn.query_row(
        "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes FROM processing_records WHERE id = ?1",
        params![id], row_to_processing_record,
    ) {
        Ok(rec) => (StatusCode::OK, Json(Some(rec))).into_response(),
        Err(e) => db_error(e),
    }
}

pub(crate) async fn list_processing(State(state): State<AppState>) -> Json<Vec<ProcessingRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare(
        "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes FROM processing_records ORDER BY id"
    ).expect("prepare failed");
    let rows: Vec<ProcessingRecord> = stmt.query_map([], row_to_processing_record).unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}

pub(crate) async fn list_processing_queue(State(state): State<AppState>) -> Json<Vec<ProcessingRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare(
        "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes FROM processing_records WHERE status = 'Scheduled' ORDER BY scheduled_date"
    ).expect("prepare failed");
    let rows: Vec<ProcessingRecord> = stmt.query_map([], row_to_processing_record).unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}

// --- Batch Cull ---

#[derive(Deserialize)]
#[allow(dead_code)]
pub(crate) struct CullBatchRequest {
    bird_ids: Vec<i64>,
    reason: String,
    method: String,
    notes: Option<String>,
    processed_date: String,
}

pub(crate) async fn cull_batch(
    State(state): State<AppState>,
    Json(body): Json<CullBatchRequest>,
) -> impl IntoResponse {
    // Section 6: Validate status value
    let valid_statuses = ["Culled", "Deceased", "Sold"];
    if !valid_statuses.contains(&body.method.as_str()) {
        return (StatusCode::BAD_REQUEST, "Invalid status. Must be: Culled, Deceased, or Sold").into_response();
    }

    let conn = acquire_db(&state);
    let status = body.method.as_str();
    let mut count = 0i64;
    for bird_id in &body.bird_ids {
        let rows = conn.execute("UPDATE birds SET status = ?1 WHERE id = ?2 AND status = 'Active'", params![status, bird_id]).unwrap_or(0);
        count += rows as i64;
    }
    Json(serde_json::json!({"updated": count})).into_response()
}
