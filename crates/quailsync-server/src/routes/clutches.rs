use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::*;
use rusqlite::{params, Connection};
use std::collections::HashMap;

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

pub(crate) async fn create_clutch(
    State(state): State<AppState>,
    Json(body): Json<CreateClutch>,
) -> impl IntoResponse {
    let expected = body.set_date + chrono::Duration::days(17);
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO clutches (breeding_group_id, lineage_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![body.breeding_group_id, body.lineage_id, body.eggs_set, body.eggs_fertile, body.eggs_hatched,
            body.set_date.to_string(), expected.to_string(), clutch_status_to_str(&body.status), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();

    // Phase 2: freeze the breeding group's composition for this clutch so its
    // maternal/paternal lineage stays fixed even if birds move afterward.
    if let Some(group_id) = body.breeding_group_id {
        snapshot_group_composition(&conn, id, group_id);
    }

    // Re-read via the JOIN so the response carries breeding_group_name + snapshot.
    match conn.query_row(
        &format!("{CLUTCH_SELECT} WHERE c.id = ?1"),
        params![id],
        row_to_clutch,
    ) {
        Ok(clutch) => {
            let snapshot = read_clutch_snapshot(&conn, id);
            (StatusCode::CREATED, Json(ClutchDetail { clutch, snapshot })).into_response()
        }
        Err(e) => db_error(e),
    }
}

// ---------------------------------------------------------------------------
// Phase 2: group composition snapshots
// ---------------------------------------------------------------------------

/// Freeze a breeding group's current composition into `clutch_snapshots`: one
/// row per (bird, lineage) for every male and female in the group. Called once
/// at clutch creation and never updated. Best-effort per row — a bird with no
/// lineage tags is simply omitted (it can't contribute a probability).
fn snapshot_group_composition(conn: &Connection, clutch_id: i64, group_id: i64) {
    fn read_ids(conn: &Connection, sql: &str, group_id: i64) -> Vec<i64> {
        conn.prepare(sql)
            .and_then(|mut s| {
                let it = s.query_map(params![group_id], |r| r.get::<_, i64>(0))?;
                Ok(it.filter_map(|r| r.ok()).collect::<Vec<i64>>())
            })
            .unwrap_or_default()
    }
    let sides = [
        (
            "SELECT male_id FROM breeding_group_males WHERE group_id = ?1 ORDER BY rowid",
            "Male",
        ),
        (
            "SELECT female_id FROM breeding_group_members WHERE group_id = ?1",
            "Female",
        ),
    ];
    for (member_sql, sex) in sides {
        for bird_id in read_ids(conn, member_sql, group_id) {
            let lineage_ids = read_ids(
                conn,
                "SELECT lineage_id FROM bird_lineages WHERE bird_id = ?1",
                bird_id,
            );
            for lineage_id in lineage_ids {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO clutch_snapshots (clutch_id, bird_id, sex, lineage_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![clutch_id, bird_id, sex, lineage_id],
                );
            }
        }
    }
}

/// Read a clutch's frozen snapshot, grouping rows by bird and deriving the
/// maternal/paternal lineage distributions. `None` when there are no snapshot
/// rows (a lineage-only clutch with no recorded group).
fn read_clutch_snapshot(conn: &Connection, clutch_id: i64) -> Option<ClutchSnapshot> {
    let rows: Vec<(i64, String, i64)> = conn
        .prepare(
            "SELECT bird_id, sex, lineage_id FROM clutch_snapshots
             WHERE clutch_id = ?1 ORDER BY bird_id, lineage_id",
        )
        .ok()?
        .query_map(params![clutch_id], |r| {
            Ok((r.get(0)?, r.get::<_, String>(1)?, r.get(2)?))
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        return None;
    }
    let names = lineage_name_map(conn);

    let (mut males, mut females): (Vec<SnapshotBird>, Vec<SnapshotBird>) = (Vec::new(), Vec::new());
    for (bird_id, sex, lineage_id) in rows {
        let list = if sex == "Male" {
            &mut males
        } else {
            &mut females
        };
        match list.iter_mut().find(|b| b.bird_id == bird_id) {
            Some(b) => b.lineage_ids.push(lineage_id),
            None => list.push(SnapshotBird {
                bird_id,
                lineage_ids: vec![lineage_id],
            }),
        }
    }
    Some(ClutchSnapshot {
        paternal_distribution: distribution(&males, &names),
        maternal_distribution: distribution(&females, &names),
        males,
        females,
    })
}

fn lineage_name_map(conn: &Connection) -> HashMap<i64, String> {
    conn.prepare("SELECT id, name FROM lineages")
        .and_then(|mut s| {
            let it = s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            Ok(it.filter_map(|r| r.ok()).collect::<HashMap<i64, String>>())
        })
        .unwrap_or_default()
}

/// Distribution over lineages: each bird weighted equally (1) and split across
/// its lineage tags, normalized by the bird count so probabilities sum to 1.0.
/// Highest-probability first; ties broken by lineage id for determinism. The
/// underlying math is shared with bird genetic profiles via `crate::genetics`.
fn distribution(birds: &[SnapshotBird], names: &HashMap<i64, String>) -> Vec<LineageProbability> {
    let lists: Vec<Vec<i64>> = birds.iter().map(|b| b.lineage_ids.clone()).collect();
    crate::genetics::distribution(&lists)
        .into_iter()
        .map(|(lineage_id, probability)| LineageProbability {
            lineage_id,
            lineage_name: names.get(&lineage_id).cloned().unwrap_or_default(),
            probability,
        })
        .collect()
}

/// `GET /api/clutches/{id}` — the clutch plus its frozen group snapshot.
pub(crate) async fn get_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let clutch = match conn.query_row(
        &format!("{CLUTCH_SELECT} WHERE c.id = ?1"),
        params![id],
        row_to_clutch,
    ) {
        Ok(c) => c,
        Err(_) => return (StatusCode::NOT_FOUND, Json(None::<ClutchDetail>)).into_response(),
    };
    let snapshot = read_clutch_snapshot(&conn, id);
    (
        StatusCode::OK,
        Json(Some(ClutchDetail { clutch, snapshot })),
    )
        .into_response()
}

// Clutch read, with the breeding group's name LEFT-JOINed in (null when the
// clutch has no group). Column order matches `row_to_clutch`.
const CLUTCH_SELECT: &str = "SELECT c.id, c.breeding_group_id, g.name, c.lineage_id, c.eggs_set, c.eggs_fertile, c.eggs_hatched, c.set_date, c.expected_hatch_date, c.status, c.notes, c.eggs_stillborn, c.eggs_quit, c.eggs_infertile, c.eggs_damaged, c.hatch_notes FROM clutches c LEFT JOIN breeding_groups g ON g.id = c.breeding_group_id";

pub(crate) async fn list_clutches(State(state): State<AppState>) -> Json<Vec<Clutch>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(&format!("{CLUTCH_SELECT} ORDER BY c.id"))
        .expect("prepare failed");
    let rows: Vec<Clutch> = stmt
        .query_map([], row_to_clutch)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

pub(crate) async fn update_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateClutch>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM clutches WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);
    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<Clutch>)).into_response();
    }

    macro_rules! update_field {
        ($field:expr, $sql:expr) => {
            if let Some(val) = $field {
                if let Err(e) = conn.execute($sql, params![val, id]) {
                    return db_error(e);
                }
            }
        };
    }

    update_field!(
        body.eggs_fertile,
        "UPDATE clutches SET eggs_fertile = ?1 WHERE id = ?2"
    );
    update_field!(
        body.eggs_hatched,
        "UPDATE clutches SET eggs_hatched = ?1 WHERE id = ?2"
    );
    if let Some(ref status) = body.status {
        if let Err(e) = conn.execute(
            "UPDATE clutches SET status = ?1 WHERE id = ?2",
            params![clutch_status_to_str(status), id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref notes) = body.notes {
        if let Err(e) = conn.execute(
            "UPDATE clutches SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        ) {
            return db_error(e);
        }
    }
    if let Some(ref set_date) = body.set_date {
        let expected = *set_date + chrono::Duration::days(17);
        if let Err(e) = conn.execute(
            "UPDATE clutches SET set_date = ?1, expected_hatch_date = ?2 WHERE id = ?3",
            params![set_date.to_string(), expected.to_string(), id],
        ) {
            return db_error(e);
        }
    }
    update_field!(
        body.eggs_stillborn,
        "UPDATE clutches SET eggs_stillborn = ?1 WHERE id = ?2"
    );
    update_field!(
        body.eggs_quit,
        "UPDATE clutches SET eggs_quit = ?1 WHERE id = ?2"
    );
    update_field!(
        body.eggs_infertile,
        "UPDATE clutches SET eggs_infertile = ?1 WHERE id = ?2"
    );
    update_field!(
        body.eggs_damaged,
        "UPDATE clutches SET eggs_damaged = ?1 WHERE id = ?2"
    );
    if let Some(ref hatch_notes) = body.hatch_notes {
        if let Err(e) = conn.execute(
            "UPDATE clutches SET hatch_notes = ?1 WHERE id = ?2",
            params![hatch_notes, id],
        ) {
            return db_error(e);
        }
    }

    match conn.query_row(
        &format!("{CLUTCH_SELECT} WHERE c.id = ?1"),
        params![id],
        row_to_clutch,
    ) {
        Ok(clutch) => (StatusCode::OK, Json(Some(clutch))).into_response(),
        Err(e) => db_error(e),
    }
}

pub(crate) async fn delete_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let affected = conn
        .execute("DELETE FROM clutches WHERE id = ?1", params![id])
        .unwrap_or(0);
    if affected > 0 {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> HashMap<i64, String> {
        HashMap::from([(1, "NWQuail".to_string()), (2, "Fernbank".to_string())])
    }
    fn bird(id: i64, lineages: &[i64]) -> SnapshotBird {
        SnapshotBird {
            bird_id: id,
            lineage_ids: lineages.to_vec(),
        }
    }

    #[test]
    fn distribution_single_lineage_matches_spec_example() {
        // 4 NWQuail + 1 Fernbank hens -> 0.8 / 0.2, highest first.
        let females = [
            bird(1, &[1]),
            bird(2, &[1]),
            bird(3, &[1]),
            bird(4, &[1]),
            bird(5, &[2]),
        ];
        let d = distribution(&females, &names());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].lineage_id, 1);
        assert_eq!(d[0].lineage_name, "NWQuail");
        assert!((d[0].probability - 0.8).abs() < 1e-9);
        assert_eq!(d[1].lineage_id, 2);
        assert!((d[1].probability - 0.2).abs() < 1e-9);
        // Sums to 1.
        assert!((d.iter().map(|p| p.probability).sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distribution_splits_multi_lineage_birds_equally() {
        // Bird A has [1,2] (weight 0.5 each), bird B has [1]. Over 2 birds:
        // lineage 1 = (0.5 + 1.0)/2 = 0.75, lineage 2 = 0.5/2 = 0.25.
        let birds = [bird(1, &[1, 2]), bird(2, &[1])];
        let d = distribution(&birds, &names());
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].lineage_id, 1);
        assert!((d[0].probability - 0.75).abs() < 1e-9);
        assert!((d[1].probability - 0.25).abs() < 1e-9);
        assert!((d.iter().map(|p| p.probability).sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distribution_single_male_is_certain() {
        let d = distribution(&[bird(9, &[2])], &names());
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].lineage_id, 2);
        assert!((d[0].probability - 1.0).abs() < 1e-9);
    }

    #[test]
    fn distribution_empty_is_empty() {
        assert!(distribution(&[], &names()).is_empty());
    }
}
