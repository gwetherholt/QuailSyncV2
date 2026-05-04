use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::params;

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_chick_group(
    State(state): State<AppState>,
    Json(body): Json<CreateChickGroup>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO chick_groups (clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'Active', ?7)",
        params![body.clutch_id, body.bloodline_id, body.brooder_id, body.initial_count, body.initial_count, body.hatch_date.to_string(), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    let mut group = ChickGroup {
        id,
        clutch_id: body.clutch_id,
        bloodline_id: body.bloodline_id,
        brooder_id: body.brooder_id,
        initial_count: body.initial_count,
        current_count: body.initial_count,
        hatch_date: body.hatch_date,
        status: ChickGroupStatus::Active,
        notes: body.notes,
        is_ready_to_transition: false,
    };
    group.is_ready_to_transition = group.compute_is_ready_to_transition();
    (StatusCode::CREATED, Json(group)).into_response()
}

const GROUP_SELECT: &str = "SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes FROM chick_groups";

pub(crate) async fn list_chick_groups(State(state): State<AppState>) -> Json<Vec<ChickGroup>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(&format!(
            "{GROUP_SELECT} ORDER BY status='Active' DESC, id DESC"
        ))
        .expect("prepare failed");
    let rows: Vec<ChickGroup> = stmt
        .query_map([], row_to_chick_group)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

pub(crate) async fn get_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    match conn.query_row(
        &format!("{GROUP_SELECT} WHERE id = ?1"),
        params![id],
        row_to_chick_group,
    ) {
        Ok(g) => (StatusCode::OK, Json(Some(g))).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<ChickGroup>)).into_response(),
    }
}

pub(crate) async fn update_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Some(count) = body.get("current_count").and_then(|v| v.as_u64()) {
        conn.execute(
            "UPDATE chick_groups SET current_count = ?1 WHERE id = ?2",
            params![count, id],
        )
        .ok();
    }
    if body.get("brooder_id").is_some() {
        let val: Option<i64> = body.get("brooder_id").and_then(|v| v.as_i64());
        conn.execute(
            "UPDATE chick_groups SET brooder_id = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }
    if let Some(notes) = body.get("notes") {
        let val: Option<String> = if notes.is_null() {
            None
        } else {
            notes.as_str().map(|s| s.to_string())
        };
        conn.execute(
            "UPDATE chick_groups SET notes = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }
    if let Some(status) = body.get("status").and_then(|v| v.as_str()) {
        conn.execute(
            "UPDATE chick_groups SET status = ?1 WHERE id = ?2",
            params![status, id],
        )
        .ok();
    }
    StatusCode::OK
}

pub(crate) async fn delete_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "DELETE FROM chick_mortality_log WHERE group_id = ?1",
        params![id],
    )
    .ok();
    let affected = conn
        .execute("DELETE FROM chick_groups WHERE id = ?1", params![id])
        .unwrap_or(0);
    if affected > 0 {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

pub(crate) async fn log_mortality(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<MortalityRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let current: u32 = match conn.query_row(
        "SELECT current_count FROM chick_groups WHERE id = ?1 AND status = 'Active'",
        params![id],
        |row| row.get(0),
    ) {
        Ok(c) => c,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "chick group not found or not active").into_response()
        }
    };
    if body.count > current {
        return (
            StatusCode::BAD_REQUEST,
            "mortality count exceeds current count",
        )
            .into_response();
    }

    let new_count = current - body.count;
    let today = chrono::Local::now().date_naive();

    if let Err(e) = conn.execute(
        "INSERT INTO chick_mortality_log (group_id, count, reason, date) VALUES (?1, ?2, ?3, ?4)",
        params![id, body.count, body.reason, today.to_string()],
    ) {
        return db_error(e);
    }
    if let Err(e) = conn.execute(
        "UPDATE chick_groups SET current_count = ?1 WHERE id = ?2",
        params![new_count, id],
    ) {
        return db_error(e);
    }
    if new_count == 0 {
        conn.execute(
            "UPDATE chick_groups SET status = 'Lost' WHERE id = ?1",
            params![id],
        )
        .ok();
    }

    let log_id = conn.last_insert_rowid();
    Json(ChickMortalityLog {
        id: log_id,
        group_id: id,
        count: body.count,
        reason: body.reason,
        date: today,
    })
    .into_response()
}

pub(crate) async fn graduate_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<GraduateRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let group = match conn.query_row(
        &format!("{GROUP_SELECT} WHERE id = ?1"),
        params![id],
        row_to_chick_group,
    ) {
        Ok(g) => g,
        Err(_) => return (StatusCode::NOT_FOUND, "chick group not found").into_response(),
    };
    if group.status != ChickGroupStatus::Active {
        return (StatusCode::BAD_REQUEST, "group is not active").into_response();
    }

    // Section 7: Look up parent generation from the clutch's breeding pair
    let parent_generation: u32 = group.clutch_id
        .and_then(|cid| {
            conn.query_row(
                "SELECT bp.male_id, bp.female_id FROM clutches c JOIN breeding_pairs bp ON bp.id = c.breeding_pair_id WHERE c.id = ?1",
                params![cid],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
            ).ok()
        })
        .map(|(male_id, female_id)| {
            let m_gen: u32 = conn.query_row("SELECT generation FROM birds WHERE id = ?1", params![male_id], |row| row.get(0)).unwrap_or(1);
            let f_gen: u32 = conn.query_row("SELECT generation FROM birds WHERE id = ?1", params![female_id], |row| row.get(0)).unwrap_or(1);
            m_gen.max(f_gen)
        })
        .unwrap_or(0);
    let generation = parent_generation + 1;

    // Look up parent IDs for the birds
    let (mother_id, father_id): (Option<i64>, Option<i64>) = group.clutch_id
        .and_then(|cid| {
            conn.query_row(
                "SELECT bp.female_id, bp.male_id FROM clutches c JOIN breeding_pairs bp ON bp.id = c.breeding_pair_id WHERE c.id = ?1",
                params![cid],
                |row| Ok((Some(row.get::<_, i64>(0)?), Some(row.get::<_, i64>(1)?))),
            ).ok()
        })
        .unwrap_or((None, None));

    let mut birds_created = Vec::new();
    for gb in &body.birds {
        if let Err(e) = conn.execute(
            "INSERT INTO birds (band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id, current_brooder_id, photo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'Active', ?8, ?9, ?10, ?11)",
            params![gb.band_color, sex_to_str(&gb.sex), group.bloodline_id, group.hatch_date.to_string(),
                mother_id, father_id, generation, gb.notes, gb.nfc_tag_id, group.brooder_id, gb.photo_path],
        ) {
            return db_error(e);
        }
        let bird_id = conn.last_insert_rowid();

        // Persist initial weight to weight_records so it shows in growth history.
        if let Some(grams) = gb.weight_grams {
            if let Err(e) = conn.execute(
                "INSERT INTO weight_records (bird_id, weight_grams, date, notes) VALUES (?1, ?2, ?3, ?4)",
                params![bird_id, grams, group.hatch_date.to_string(), Option::<&str>::None],
            ) {
                return db_error(e);
            }
        }

        birds_created.push(Bird {
            id: bird_id,
            band_color: gb.band_color.clone(),
            sex: gb.sex.clone(),
            bloodline_id: group.bloodline_id,
            hatch_date: group.hatch_date,
            mother_id,
            father_id,
            generation,
            status: BirdStatus::Active,
            notes: gb.notes.clone(),
            nfc_tag_id: gb.nfc_tag_id.clone(),
            current_brooder_id: group.brooder_id,
            photo_path: gb.photo_path.clone(),
        });
    }

    conn.execute(
        "UPDATE chick_groups SET status = 'Graduated' WHERE id = ?1",
        params![id],
    )
    .ok();

    Json(birds_created).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn group_with(hatch: NaiveDate, status: ChickGroupStatus) -> ChickGroup {
        ChickGroup {
            id: 1,
            clutch_id: None,
            bloodline_id: 1,
            brooder_id: None,
            initial_count: 12,
            current_count: 12,
            hatch_date: hatch,
            status,
            notes: None,
            is_ready_to_transition: false,
        }
    }

    #[test]
    fn not_ready_at_four_weeks() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let hatch = today - chrono::Duration::days(28);
        let g = group_with(hatch, ChickGroupStatus::Active);
        assert!(!g.compute_is_ready_to_transition_at(today));
    }

    #[test]
    fn ready_at_five_weeks_exactly() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let hatch = today - chrono::Duration::days(35);
        let g = group_with(hatch, ChickGroupStatus::Active);
        assert!(g.compute_is_ready_to_transition_at(today));
    }

    #[test]
    fn not_ready_when_graduated() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let hatch = today - chrono::Duration::weeks(8);
        let g = group_with(hatch, ChickGroupStatus::Graduated);
        assert!(!g.compute_is_ready_to_transition_at(today));
    }

    #[test]
    fn not_ready_at_day_34() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let hatch = today - chrono::Duration::days(34);
        let g = group_with(hatch, ChickGroupStatus::Active);
        assert!(!g.compute_is_ready_to_transition_at(today));
    }

    #[test]
    fn ready_at_day_35() {
        let today = NaiveDate::from_ymd_opt(2026, 5, 4).unwrap();
        let hatch = today - chrono::Duration::days(35);
        let g = group_with(hatch, ChickGroupStatus::Active);
        assert!(g.compute_is_ready_to_transition_at(today));
    }

    /// Regression: GraduateBird payload must round-trip through serde even when
    /// the new optional intake fields (weight_grams, photo_path) are omitted —
    /// CLI/API callers from before the per-bird intake feature must keep working.
    #[test]
    fn graduate_bird_deserializes_without_optional_fields() {
        let json = r#"{"sex":"Male","band_color":null,"nfc_tag_id":null,"notes":null}"#;
        let gb: GraduateBird = serde_json::from_str(json).unwrap();
        assert_eq!(gb.sex, Sex::Male);
        assert!(gb.weight_grams.is_none());
        assert!(gb.photo_path.is_none());
    }

    /// Regression: GraduateBird carries the new optional fields when present.
    #[test]
    fn graduate_bird_deserializes_with_optional_fields() {
        let json = r#"{"sex":"Female","band_color":"red","nfc_tag_id":"T1","notes":null,"weight_grams":140.5,"photo_path":"x.jpg"}"#;
        let gb: GraduateBird = serde_json::from_str(json).unwrap();
        assert_eq!(gb.weight_grams, Some(140.5));
        assert_eq!(gb.photo_path.as_deref(), Some("x.jpg"));
    }
}
