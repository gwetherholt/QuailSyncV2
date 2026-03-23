use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::params;

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_clutch(
    State(state): State<AppState>,
    Json(body): Json<CreateClutch>,
) -> impl IntoResponse {
    let expected = body.set_date + chrono::Duration::days(17);
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO clutches (breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![body.breeding_pair_id, body.bloodline_id, body.eggs_set, body.eggs_fertile, body.eggs_hatched,
            body.set_date.to_string(), expected.to_string(), clutch_status_to_str(&body.status), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (StatusCode::CREATED, Json(Clutch {
        id, breeding_pair_id: body.breeding_pair_id, bloodline_id: body.bloodline_id,
        eggs_set: body.eggs_set, eggs_fertile: body.eggs_fertile, eggs_hatched: body.eggs_hatched,
        set_date: body.set_date, expected_hatch_date: expected, status: body.status, notes: body.notes,
        eggs_stillborn: None, eggs_quit: None, eggs_infertile: None, eggs_damaged: None, hatch_notes: None,
    })).into_response()
}

const CLUTCH_SELECT: &str = "SELECT id, breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes, eggs_stillborn, eggs_quit, eggs_infertile, eggs_damaged, hatch_notes FROM clutches";

pub(crate) async fn list_clutches(State(state): State<AppState>) -> Json<Vec<Clutch>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare(&format!("{CLUTCH_SELECT} ORDER BY id")).expect("prepare failed");
    let rows: Vec<Clutch> = stmt.query_map([], row_to_clutch).unwrap().filter_map(|r| r.ok()).collect();
    Json(rows)
}

pub(crate) async fn update_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateClutch>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let exists: bool = conn.query_row("SELECT COUNT(*) FROM clutches WHERE id = ?1", params![id], |row| row.get::<_, i64>(0)).map(|c| c > 0).unwrap_or(false);
    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<Clutch>)).into_response();
    }

    macro_rules! update_field {
        ($field:expr, $sql:expr) => {
            if let Some(val) = $field {
                if let Err(e) = conn.execute($sql, params![val, id]) { return db_error(e); }
            }
        };
    }

    update_field!(body.eggs_fertile, "UPDATE clutches SET eggs_fertile = ?1 WHERE id = ?2");
    update_field!(body.eggs_hatched, "UPDATE clutches SET eggs_hatched = ?1 WHERE id = ?2");
    if let Some(ref status) = body.status {
        if let Err(e) = conn.execute("UPDATE clutches SET status = ?1 WHERE id = ?2", params![clutch_status_to_str(status), id]) { return db_error(e); }
    }
    if let Some(ref notes) = body.notes {
        if let Err(e) = conn.execute("UPDATE clutches SET notes = ?1 WHERE id = ?2", params![notes, id]) { return db_error(e); }
    }
    if let Some(ref set_date) = body.set_date {
        let expected = *set_date + chrono::Duration::days(17);
        if let Err(e) = conn.execute("UPDATE clutches SET set_date = ?1, expected_hatch_date = ?2 WHERE id = ?3", params![set_date.to_string(), expected.to_string(), id]) { return db_error(e); }
    }
    update_field!(body.eggs_stillborn, "UPDATE clutches SET eggs_stillborn = ?1 WHERE id = ?2");
    update_field!(body.eggs_quit, "UPDATE clutches SET eggs_quit = ?1 WHERE id = ?2");
    update_field!(body.eggs_infertile, "UPDATE clutches SET eggs_infertile = ?1 WHERE id = ?2");
    update_field!(body.eggs_damaged, "UPDATE clutches SET eggs_damaged = ?1 WHERE id = ?2");
    if let Some(ref hatch_notes) = body.hatch_notes {
        if let Err(e) = conn.execute("UPDATE clutches SET hatch_notes = ?1 WHERE id = ?2", params![hatch_notes, id]) { return db_error(e); }
    }

    match conn.query_row(&format!("{CLUTCH_SELECT} WHERE id = ?1"), params![id], row_to_clutch) {
        Ok(clutch) => (StatusCode::OK, Json(Some(clutch))).into_response(),
        Err(e) => db_error(e),
    }
}

pub(crate) async fn delete_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let affected = conn.execute("DELETE FROM clutches WHERE id = ?1", params![id]).unwrap_or(0);
    if affected > 0 { StatusCode::NO_CONTENT } else { StatusCode::NOT_FOUND }
}
