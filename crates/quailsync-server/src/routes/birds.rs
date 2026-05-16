use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use quailsync_common::*;
use rusqlite::params;

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_bird(
    State(state): State<AppState>,
    Json(body): Json<CreateBird>,
) -> impl IntoResponse {
    if body.lineage_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "lineage_ids must contain at least one lineage",
        )
            .into_response();
    }
    let conn = acquire_db(&state);
    // Re-tagging guard: NFC tags get re-used across batches. If this tag is
    // already on another bird, clear it from that bird first so our INSERT
    // doesn't trip the UNIQUE(nfc_tag_id) constraint and surface as a 500.
    // The cleared bird's tag goes to NULL — they'll get a fresh tag the
    // next time someone bands them.
    if let Some(ref tag) = body.nfc_tag_id {
        clear_nfc_tag_from_others(&conn, tag, None);
    }
    if let Err(e) = conn.execute(
        "INSERT INTO birds (band_color, sex, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id, chick_group_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            body.band_color, sex_to_str(&body.sex),
            body.hatch_date.to_string(), body.mother_id, body.father_id,
            body.generation, bird_status_to_str(&body.status), body.notes, body.nfc_tag_id,
            // Issue #14: stamp the chick group back-link when caller supplies
            // it. Android's batch-band flow uses this to preserve the
            // relationship that the /graduate handler maintains internally.
            body.chick_group_id,
        ],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    if let Err(e) = replace_bird_lineages(&conn, id, &body.lineage_ids) {
        return db_error(e);
    }
    let lineages = fetch_bird_lineages(&conn, id);
    (
        StatusCode::CREATED,
        Json(Bird {
            id,
            band_color: body.band_color,
            sex: body.sex,
            hatch_date: body.hatch_date,
            mother_id: body.mother_id,
            father_id: body.father_id,
            generation: body.generation,
            status: body.status,
            notes: body.notes,
            nfc_tag_id: body.nfc_tag_id,
            current_brooder_id: None,
            photo_path: None,
            housing_id: None,
            chick_group_id: body.chick_group_id,
            lineages,
        }),
    )
        .into_response()
}

const BIRD_SELECT: &str = "SELECT id, band_color, sex, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id, current_brooder_id, photo_path, housing_id, chick_group_id FROM birds";

pub(crate) async fn list_birds(State(state): State<AppState>) -> Json<Vec<Bird>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(&format!("{BIRD_SELECT} ORDER BY id"))
        .expect("prepare failed");
    // TODO: filter_map silently drops row-mapping errors
    let mut rows: Vec<Bird> = stmt
        .query_map([], row_to_bird)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    populate_bird_lineages(&conn, &mut rows);
    Json(rows)
}

pub(crate) async fn update_bird(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBird>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<Bird>)).into_response();
    }

    if let Some(ref status) = body.status {
        if let Err(e) = conn.execute(
            "UPDATE birds SET status = ?1 WHERE id = ?2",
            params![bird_status_to_str(status), id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref notes) = body.notes {
        if let Err(e) = conn.execute(
            "UPDATE birds SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref nfc) = body.nfc_tag_id {
        // Same re-tagging guard as create_bird — clear the tag from any
        // OTHER bird that currently holds it, then set it on this bird.
        // `Some(id)` excludes the bird we're about to update so writing the
        // same tag back to the same bird is a clean no-op, not a clear.
        clear_nfc_tag_from_others(&conn, nfc, Some(id));
        if let Err(e) = conn.execute(
            "UPDATE birds SET nfc_tag_id = ?1 WHERE id = ?2",
            params![nfc, id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref bc) = body.band_color {
        if let Err(e) = conn.execute(
            "UPDATE birds SET band_color = ?1 WHERE id = ?2",
            params![bc, id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref sex) = body.sex {
        if let Err(e) = conn.execute(
            "UPDATE birds SET sex = ?1 WHERE id = ?2",
            params![sex_to_str(sex), id],
        ) {
            return db_error(e);
        }
    }
    if let Some(hd) = body.hatch_date {
        if let Err(e) = conn.execute(
            "UPDATE birds SET hatch_date = ?1 WHERE id = ?2",
            params![hd.to_string(), id],
        ) {
            return db_error(e);
        }
    }
    if let Some(hid) = body.housing_id {
        // Issue #13: set housing assignment. Clearing goes through
        // POST /api/brooders/{id}/unassign-birds — keep this one as a pure
        // "set" so we don't need the Option<Option<i64>> serde dance.
        if let Err(e) = conn.execute(
            "UPDATE birds SET housing_id = ?1 WHERE id = ?2",
            params![hid, id],
        ) {
            return db_error(e);
        }
    }

    match conn.query_row(
        &format!("{BIRD_SELECT} WHERE id = ?1"),
        params![id],
        row_to_bird,
    ) {
        Ok(mut bird) => {
            bird.lineages = fetch_bird_lineages(&conn, bird.id);
            (StatusCode::OK, Json(Some(bird))).into_response()
        }
        Err(e) => db_error(e),
    }
}

pub(crate) async fn delete_bird(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    // Cascade-delete related records (section 8)
    conn.execute("DELETE FROM weight_records WHERE bird_id = ?1", params![id])
        .ok();
    conn.execute(
        "DELETE FROM breeding_pairs WHERE male_id = ?1 OR female_id = ?1",
        params![id],
    )
    .ok();
    conn.execute(
        "DELETE FROM breeding_group_members WHERE female_id = ?1",
        params![id],
    )
    .ok();
    conn.execute(
        "DELETE FROM breeding_groups WHERE male_id = ?1",
        params![id],
    )
    .ok();
    conn.execute(
        "DELETE FROM processing_records WHERE bird_id = ?1",
        params![id],
    )
    .ok();
    let affected = conn
        .execute("DELETE FROM birds WHERE id = ?1", params![id])
        .unwrap_or(0);
    if affected > 0 {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

pub(crate) async fn get_bird_by_nfc(
    State(state): State<AppState>,
    Path(tag_id): Path<String>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    match conn.query_row(
        &format!("{BIRD_SELECT} WHERE nfc_tag_id = ?1"),
        params![tag_id],
        row_to_bird,
    ) {
        Ok(mut b) => {
            b.lineages = fetch_bird_lineages(&conn, b.id);
            (StatusCode::OK, Json(Some(b))).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<Bird>)).into_response(),
    }
}

pub(crate) async fn move_bird(
    State(state): State<AppState>,
    Path(bird_id): Path<i64>,
    Json(body): Json<MoveBirdRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "UPDATE birds SET current_brooder_id = ?1 WHERE id = ?2",
        params![body.target_brooder_id, bird_id],
    ) {
        return db_error(e);
    }
    match conn.query_row(
        &format!("{BIRD_SELECT} WHERE id = ?1"),
        params![bird_id],
        row_to_bird,
    ) {
        Ok(mut bird) => {
            bird.lineages = fetch_bird_lineages(&conn, bird.id);
            Json(bird).into_response()
        }
        Err(e) => db_error(e),
    }
}

// --- Weight tracking ---

pub(crate) async fn create_weight(
    State(state): State<AppState>,
    Path(bird_id): Path<i64>,
    Json(body): Json<CreateWeightRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO weight_records (bird_id, weight_grams, date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![
            bird_id,
            body.weight_grams,
            body.date.to_string(),
            body.notes
        ],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(WeightRecord {
            id,
            bird_id,
            weight_grams: body.weight_grams,
            date: body.date,
            notes: body.notes,
        }),
    )
        .into_response()
}

pub(crate) async fn list_weights(
    State(state): State<AppState>,
    Path(bird_id): Path<i64>,
) -> Json<Vec<WeightRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare(
        "SELECT id, bird_id, weight_grams, date, notes FROM weight_records WHERE bird_id = ?1 ORDER BY date DESC",
    ).expect("prepare failed");
    let rows: Vec<WeightRecord> = stmt
        .query_map(params![bird_id], |row| {
            let date_str: String = row.get(3)?;
            Ok(WeightRecord {
                id: row.get(0)?,
                bird_id: row.get(1)?,
                weight_grams: row.get(2)?,
                date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_default(),
                notes: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

pub(crate) async fn delete_weight(
    State(state): State<AppState>,
    Path((_bird_id, weight_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let affected = conn
        .execute(
            "DELETE FROM weight_records WHERE id = ?1",
            params![weight_id],
        )
        .unwrap_or(0);
    if affected > 0 {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

// --- Lineages ---

pub(crate) async fn create_lineage(
    State(state): State<AppState>,
    Json(body): Json<CreateLineage>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO lineages (name, source, notes) VALUES (?1, ?2, ?3)",
        params![body.name, body.source, body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(Lineage {
            id,
            name: body.name,
            source: body.source,
            notes: body.notes,
        }),
    )
        .into_response()
}

pub(crate) async fn list_lineages(State(state): State<AppState>) -> Json<Vec<Lineage>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, source, notes FROM lineages ORDER BY id")
        .expect("prepare failed");
    let rows: Vec<Lineage> = stmt
        .query_map([], |row| {
            Ok(Lineage {
                id: row.get(0)?,
                name: row.get(1)?,
                source: row.get(2)?,
                notes: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

/// PUT /api/birds/{id}/lineages — replace a bird's lineage set atomically.
pub(crate) async fn replace_bird_lineages_handler(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<ReplaceLineagesRequest>,
) -> impl IntoResponse {
    if body.lineage_ids.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "lineage_ids must contain at least one lineage",
        )
            .into_response();
    }
    let conn = acquire_db(&state);
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !exists {
        return (StatusCode::NOT_FOUND, "bird not found").into_response();
    }
    if let Err(e) = replace_bird_lineages(&conn, id, &body.lineage_ids) {
        return db_error(e);
    }
    match conn.query_row(
        &format!("{BIRD_SELECT} WHERE id = ?1"),
        params![id],
        row_to_bird,
    ) {
        Ok(mut bird) => {
            bird.lineages = fetch_bird_lineages(&conn, bird.id);
            (StatusCode::OK, Json(bird)).into_response()
        }
        Err(e) => db_error(e),
    }
}
