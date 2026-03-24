use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use quailsync_common::*;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::helpers::*;
use crate::state::{acquire_db, db_error, AppState};

// --- Breeding Pairs ---

pub(crate) async fn create_breeding_pair(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingPair>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO breeding_pairs (male_id, female_id, start_date, end_date, notes) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![body.male_id, body.female_id, body.start_date.to_string(), body.end_date.map(|d| d.to_string()), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(BreedingPair {
            id,
            male_id: body.male_id,
            female_id: body.female_id,
            start_date: body.start_date,
            end_date: body.end_date,
            notes: body.notes,
        }),
    )
        .into_response()
}

pub(crate) async fn list_breeding_pairs(State(state): State<AppState>) -> Json<Vec<BreedingPair>> {
    let conn = acquire_db(&state);
    let mut stmt = conn.prepare("SELECT id, male_id, female_id, start_date, end_date, notes FROM breeding_pairs ORDER BY id").expect("prepare failed");
    let rows: Vec<BreedingPair> = stmt
        .query_map([], row_to_breeding_pair)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// --- Breeding Groups ---

pub(crate) async fn create_breeding_group(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingGroup>,
) -> impl IntoResponse {
    let count = body.female_ids.len();
    let warning = if !(MIN_FEMALES_PER_MALE..=MAX_FEMALES_PER_MALE).contains(&count) {
        Some(format!("Warning: {count} females per male is outside the recommended {MIN_FEMALES_PER_MALE}-{MAX_FEMALES_PER_MALE} range"))
    } else {
        None
    };

    let conn = acquire_db(&state);
    if let Err(e) = conn.execute(
        "INSERT INTO breeding_groups (name, male_id, start_date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![
            body.name,
            body.male_id,
            body.start_date.to_string(),
            body.notes
        ],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();

    for fid in &body.female_ids {
        if let Err(e) = conn.execute(
            "INSERT INTO breeding_group_members (group_id, female_id) VALUES (?1, ?2)",
            params![id, fid],
        ) {
            return db_error(e);
        }
    }

    #[derive(Serialize)]
    struct Resp {
        #[serde(flatten)]
        group: BreedingGroup,
        warning: Option<String>,
    }
    (
        StatusCode::CREATED,
        Json(Resp {
            group: BreedingGroup {
                id,
                name: body.name,
                male_id: body.male_id,
                female_ids: body.female_ids,
                start_date: body.start_date,
                notes: body.notes,
            },
            warning,
        }),
    )
        .into_response()
}

pub(crate) async fn list_breeding_groups(
    State(state): State<AppState>,
) -> Json<Vec<BreedingGroup>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, male_id, start_date, notes FROM breeding_groups ORDER BY id")
        .expect("prepare failed");
    let groups: Vec<(i64, String, i64, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, String>(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for (id, name, male_id, start_str, notes) in groups {
        let mut fstmt = conn
            .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
            .expect("prepare failed");
        let female_ids: Vec<i64> = fstmt
            .query_map(params![id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        result.push(BreedingGroup {
            id,
            name,
            male_id,
            female_ids,
            start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d").unwrap_or_default(),
            notes,
        });
    }
    Json(result)
}

pub(crate) async fn get_breeding_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let group = conn.query_row(
        "SELECT id, name, male_id, start_date, notes FROM breeding_groups WHERE id = ?1",
        params![id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    );
    match group {
        Ok((gid, name, male_id, start_str, notes)) => {
            let mut fstmt = conn
                .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
                .expect("prepare failed");
            let female_ids: Vec<i64> = fstmt
                .query_map(params![gid], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            (
                StatusCode::OK,
                Json(Some(BreedingGroup {
                    id: gid,
                    name,
                    male_id,
                    female_ids,
                    start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d")
                        .unwrap_or_default(),
                    notes,
                })),
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<BreedingGroup>)).into_response(),
    }
}

// --- Breeding Suggestions ---

pub(crate) struct BirdRecord {
    id: i64,
    sex: Sex,
    bloodline_id: i64,
    mother_id: Option<i64>,
    father_id: Option<i64>,
}

pub(crate) fn compute_relatedness(m: &BirdRecord, f: &BirdRecord) -> f64 {
    let share_mother = matches!((m.mother_id, f.mother_id), (Some(a), Some(b)) if a == b);
    let share_father = matches!((m.father_id, f.father_id), (Some(a), Some(b)) if a == b);
    if share_mother && share_father {
        return 0.5;
    }
    if share_mother || share_father {
        return 0.25;
    }
    if m.bloodline_id == f.bloodline_id {
        return 0.25;
    }
    0.0
}

fn load_bird_records(conn: &std::sync::MutexGuard<'_, rusqlite::Connection>) -> Vec<BirdRecord> {
    let mut stmt = conn
        .prepare(
            "SELECT id, sex, bloodline_id, mother_id, father_id FROM birds WHERE status = 'Active'",
        )
        .expect("prepare failed");
    stmt.query_map([], |row| {
        let sex_str: String = row.get(1)?;
        Ok(BirdRecord {
            id: row.get(0)?,
            sex: str_to_sex(&sex_str),
            bloodline_id: row.get(2)?,
            mother_id: row.get(3)?,
            father_id: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

pub(crate) async fn breeding_suggest(
    State(state): State<AppState>,
) -> Json<Vec<InbreedingCoefficient>> {
    let conn = acquire_db(&state);
    let birds = load_bird_records(&conn);
    let males: Vec<&BirdRecord> = birds.iter().filter(|b| b.sex == Sex::Male).collect();
    let females: Vec<&BirdRecord> = birds.iter().filter(|b| b.sex == Sex::Female).collect();

    let mut results: Vec<InbreedingCoefficient> = Vec::new();
    for m in &males {
        for f in &females {
            let coefficient = compute_relatedness(m, f);
            results.push(InbreedingCoefficient {
                male_id: m.id,
                female_id: f.id,
                coefficient,
                safe: coefficient < 0.0625,
            });
        }
    }
    results.sort_by(|a, b| {
        a.coefficient
            .partial_cmp(&b.coefficient)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Json(results)
}

#[derive(Deserialize)]
pub(crate) struct InbreedingCheckQuery {
    pub(crate) male_id: i64,
    pub(crate) female_id: i64,
}

pub(crate) async fn inbreeding_check(
    State(state): State<AppState>,
    Query(q): Query<InbreedingCheckQuery>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let get_bird = |id: i64| -> Option<BirdRecord> {
        conn.query_row(
            "SELECT id, sex, bloodline_id, mother_id, father_id FROM birds WHERE id = ?1",
            params![id],
            |row| {
                let sex_str: String = row.get(1)?;
                Ok(BirdRecord {
                    id: row.get(0)?,
                    sex: str_to_sex(&sex_str),
                    bloodline_id: row.get(2)?,
                    mother_id: row.get(3)?,
                    father_id: row.get(4)?,
                })
            },
        )
        .ok()
    };

    match (get_bird(q.male_id), get_bird(q.female_id)) {
        (Some(m), Some(f)) => {
            let coefficient = compute_relatedness(&m, &f);
            Json(serde_json::json!({
                "male_id": q.male_id, "female_id": q.female_id, "coefficient": coefficient,
                "safe": coefficient < 0.0625,
                "warning": if coefficient >= 0.0625 { format!("High inbreeding risk: {:.1}%", coefficient * 100.0) } else { String::new() }
            })).into_response()
        }
        _ => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "One or both birds not found"})),
        )
            .into_response(),
    }
}

// --- Cull Recommendations ---

pub(crate) async fn cull_recommendations(
    State(state): State<AppState>,
) -> Json<Vec<CullRecommendation>> {
    let conn = acquire_db(&state);
    let mut recs: Vec<CullRecommendation> = Vec::new();

    let active_females: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Female' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let ideal_males = if active_females > 0 {
        ((active_females as f64) / (MAX_FEMALES_PER_MALE as f64)).ceil() as i64
    } else {
        0
    };

    let mut male_stmt = conn
        .prepare("SELECT id FROM birds WHERE sex = 'Male' AND status = 'Active' ORDER BY id DESC")
        .expect("prepare failed");
    let active_male_ids: Vec<i64> = male_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let surplus = (active_male_ids.len() as i64) - ideal_males;
    if surplus > 0 {
        for &mid in active_male_ids.iter().take(surplus as usize) {
            recs.push(CullRecommendation {
                bird_id: mid,
                reason: CullReason::ExcessMale,
            });
        }
    }

    let mut fw_stmt = conn.prepare(
        "SELECT b.id, w.weight_grams FROM birds b JOIN weight_records w ON w.bird_id = b.id
         WHERE b.sex = 'Female' AND b.status = 'Active'
           AND w.id = (SELECT w2.id FROM weight_records w2 WHERE w2.bird_id = b.id ORDER BY w2.date DESC LIMIT 1)"
    ).expect("prepare failed");
    let low_weight: Vec<(i64, f64)> = fw_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .filter(|(_, w)| *w < COTURNIX_MIN_BREEDING_WEIGHT_GRAMS)
        .collect();
    for (bid, w) in low_weight {
        recs.push(CullRecommendation {
            bird_id: bid,
            reason: CullReason::LowWeight { weight_grams: w },
        });
    }

    let all_birds = load_bird_records(&conn);
    let males: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Male).collect();
    let females: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Female).collect();

    for m in &males {
        let has_safe = females.iter().any(|f| compute_relatedness(m, f) < 0.0625);
        if !has_safe && !females.is_empty() {
            let worst = females
                .iter()
                .map(|f| compute_relatedness(m, f))
                .fold(0.0_f64, f64::max);
            if !recs.iter().any(|r| r.bird_id == m.id) {
                recs.push(CullRecommendation {
                    bird_id: m.id,
                    reason: CullReason::HighInbreeding { coefficient: worst },
                });
            }
        }
    }
    for f in &females {
        let has_safe = males.iter().any(|m| compute_relatedness(m, f) < 0.0625);
        if !has_safe && !males.is_empty() {
            let worst = males
                .iter()
                .map(|m| compute_relatedness(m, f))
                .fold(0.0_f64, f64::max);
            if !recs.iter().any(|r| r.bird_id == f.id) {
                recs.push(CullRecommendation {
                    bird_id: f.id,
                    reason: CullReason::HighInbreeding { coefficient: worst },
                });
            }
        }
    }

    Json(recs)
}

// --- Flock Summary ---

#[derive(Serialize)]
pub(crate) struct FlockSummary {
    total_birds: i64,
    active_birds: i64,
    males: i64,
    females: i64,
    bloodlines: Vec<BloodlineCount>,
}

#[derive(Serialize)]
pub(crate) struct BloodlineCount {
    name: String,
    count: i64,
}

pub(crate) async fn flock_summary(State(state): State<AppState>) -> Json<FlockSummary> {
    let conn = acquire_db(&state);
    let total_birds: i64 = conn
        .query_row("SELECT COUNT(*) FROM birds", [], |row| row.get(0))
        .unwrap_or(0);
    let active_birds: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let males: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Male' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let females: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Female' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn.prepare("SELECT b.name, COUNT(*) FROM birds bi JOIN bloodlines b ON bi.bloodline_id = b.id WHERE bi.status = 'Active' GROUP BY b.name ORDER BY COUNT(*) DESC").expect("prepare failed");
    let bloodlines: Vec<BloodlineCount> = stmt
        .query_map([], |row| {
            Ok(BloodlineCount {
                name: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(FlockSummary {
        total_birds,
        active_birds,
        males,
        females,
        bloodlines,
    })
}
