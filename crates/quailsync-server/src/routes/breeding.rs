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

// --- Breeding Groups ---

/// Reads a group's full male list from the `breeding_group_males` junction —
/// the single source of truth. Ordered by insertion (rowid) so the first male
/// added stays first. May be empty (an `infertile` group has no males).
fn read_group_male_ids(conn: &rusqlite::Connection, group_id: i64) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT male_id FROM breeding_group_males WHERE group_id = ?1 ORDER BY rowid")
        .expect("prepare failed");
    stmt.query_map(params![group_id], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

fn read_group_female_ids(conn: &rusqlite::Connection, group_id: i64) -> Vec<i64> {
    let mut stmt = conn
        .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
        .expect("prepare failed");
    stmt.query_map(params![group_id], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

pub(crate) async fn create_breeding_group(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingGroup>,
) -> impl IntoResponse {
    let males = body.males();
    if males.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "missing_male",
                "message": "A breeding group needs at least one male.",
            })),
        )
            .into_response();
    }

    let count = body.female_ids.len();
    let warning = if !(MIN_FEMALES_PER_MALE..=MAX_FEMALES_PER_MALE).contains(&count) {
        Some(format!("Warning: {count} females per male is outside the recommended {MIN_FEMALES_PER_MALE}-{MAX_FEMALES_PER_MALE} range"))
    } else {
        None
    };

    let conn = acquire_db(&state);
    // A new group always has at least one male (checked above) → 'active'.
    if let Err(e) = conn.execute(
        "INSERT INTO breeding_groups (name, start_date, notes, status) VALUES (?1, ?2, ?3, 'active')",
        params![body.name, body.start_date.to_string(), body.notes],
    ) {
        return db_error(e);
    }
    let id = conn.last_insert_rowid();

    for m in &males {
        if let Err(e) = conn.execute(
            "INSERT INTO breeding_group_males (group_id, male_id) VALUES (?1, ?2)",
            params![id, m],
        ) {
            return db_error(e);
        }
    }

    for fid in &body.female_ids {
        // A female belongs to at most one group. Adding her here transfers
        // her out of any prior group (the female picker stages this move).
        if let Err(e) = conn.execute(
            "DELETE FROM breeding_group_members WHERE female_id = ?1",
            params![fid],
        ) {
            return db_error(e);
        }
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
                male_ids: males,
                female_ids: body.female_ids,
                start_date: body.start_date,
                notes: body.notes,
                status: "active".to_string(),
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
        .prepare("SELECT id, name, start_date, notes, status FROM breeding_groups ORDER BY id")
        .expect("prepare failed");
    let groups: Vec<(i64, String, String, Option<String>, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for (id, name, start_str, notes, status) in groups {
        let male_ids = read_group_male_ids(&conn, id);
        let female_ids = read_group_female_ids(&conn, id);
        result.push(BreedingGroup {
            id,
            name,
            male_ids,
            female_ids,
            start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d").unwrap_or_default(),
            notes,
            status,
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
        "SELECT id, name, start_date, notes, status FROM breeding_groups WHERE id = ?1",
        params![id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    );
    match group {
        Ok((gid, name, start_str, notes, status)) => {
            let male_ids = read_group_male_ids(&conn, gid);
            let female_ids = read_group_female_ids(&conn, gid);
            (
                StatusCode::OK,
                Json(Some(BreedingGroup {
                    id: gid,
                    name,
                    male_ids,
                    female_ids,
                    start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d")
                        .unwrap_or_default(),
                    notes,
                    status,
                })),
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<BreedingGroup>)).into_response(),
    }
}

/// Partial edit of a breeding group. Present fields replace the corresponding
/// state; `male_ids`/`female_ids`, when supplied, fully replace that
/// membership (females are transferred out of any other group, matching
/// create semantics). Returns the updated group.
pub(crate) async fn update_breeding_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBreedingGroup>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    // Confirm the group exists.
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM breeding_groups WHERE id = ?1",
            params![id],
            |_| Ok(()),
        )
        .is_ok();
    if !exists {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": "No breeding group with that id.",
            })),
        )
            .into_response();
    }

    if let Some(name) = &body.name {
        if let Err(e) = conn.execute(
            "UPDATE breeding_groups SET name = ?1 WHERE id = ?2",
            params![name, id],
        ) {
            return db_error(e);
        }
    }
    if let Some(notes) = &body.notes {
        if let Err(e) = conn.execute(
            "UPDATE breeding_groups SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        ) {
            return db_error(e);
        }
    }

    // Replace males if supplied. A non-empty male set means the group is
    // fertile again, so flip status back to 'active' (covers re-adding a male
    // to a previously infertile group).
    if let Some(males) = body.males() {
        if males.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "missing_male",
                    "message": "A breeding group needs at least one male.",
                })),
            )
                .into_response();
        }
        if let Err(e) = conn.execute(
            "DELETE FROM breeding_group_males WHERE group_id = ?1",
            params![id],
        ) {
            return db_error(e);
        }
        for m in &males {
            if let Err(e) = conn.execute(
                "INSERT INTO breeding_group_males (group_id, male_id) VALUES (?1, ?2)",
                params![id, m],
            ) {
                return db_error(e);
            }
        }
        if let Err(e) = conn.execute(
            "UPDATE breeding_groups SET status = 'active' WHERE id = ?1",
            params![id],
        ) {
            return db_error(e);
        }
    }

    // Replace females if supplied (transfer out of any other group).
    if let Some(females) = &body.female_ids {
        if let Err(e) = conn.execute(
            "DELETE FROM breeding_group_members WHERE group_id = ?1",
            params![id],
        ) {
            return db_error(e);
        }
        for fid in females {
            if let Err(e) = conn.execute(
                "DELETE FROM breeding_group_members WHERE female_id = ?1",
                params![fid],
            ) {
                return db_error(e);
            }
            if let Err(e) = conn.execute(
                "INSERT INTO breeding_group_members (group_id, female_id) VALUES (?1, ?2)",
                params![id, fid],
            ) {
                return db_error(e);
            }
        }
    }

    // Re-read the resulting group for the response.
    let row = conn.query_row(
        "SELECT name, start_date, notes, status FROM breeding_groups WHERE id = ?1",
        params![id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
            ))
        },
    );
    match row {
        Ok((name, start_str, notes, status)) => {
            let male_ids = read_group_male_ids(&conn, id);
            let female_ids = read_group_female_ids(&conn, id);
            (
                StatusCode::OK,
                Json(BreedingGroup {
                    id,
                    name,
                    male_ids,
                    female_ids,
                    start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d")
                        .unwrap_or_default(),
                    notes,
                    status,
                }),
            )
                .into_response()
        }
        Err(_) => {
            // Should not happen — we verified existence above.
            (StatusCode::NOT_FOUND, Json(None::<BreedingGroup>)).into_response()
        }
    }
}

/// Deletes a breeding group and its male/female membership rows. Idempotent:
/// deleting a missing group returns 404.
pub(crate) async fn delete_breeding_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "DELETE FROM breeding_group_members WHERE group_id = ?1",
        params![id],
    )
    .ok();
    conn.execute(
        "DELETE FROM breeding_group_males WHERE group_id = ?1",
        params![id],
    )
    .ok();
    let affected = conn
        .execute("DELETE FROM breeding_groups WHERE id = ?1", params![id])
        .unwrap_or(0);
    if affected > 0 {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": "No breeding group with that id.",
            })),
        )
            .into_response()
    }
}

// --- Breeding Suggestions ---

pub(crate) struct BirdRecord {
    id: i64,
    sex: Sex,
    /// Many-to-many lineage IDs from the bird_lineages junction.
    lineage_ids: Vec<i64>,
    mother_id: Option<i64>,
    father_id: Option<i64>,
}

/// Estimated inbreeding coefficient for a potential pair.
///
/// Parent-based checks are authoritative and binary:
///   - share both parents → 0.5 (full siblings)
///   - share one parent → 0.25 (half siblings)
///
/// When no parent overlap can be proven, fall back to a proportional
/// lineage-overlap score: `(|A ∩ B| / |A ∪ B|) * 0.25` (Jaccard × 0.25
/// ceiling). This lets a bird tagged `[Fernbank, NWQuail]` paired with
/// one tagged `[Fernbank]` score 0.125 — riskier than unrelated stock
/// (0.0) but safer than a same-single-bloodline pair (0.25). Closes #5.
///
/// If either bird has zero lineage tags, lineage overlap contributes 0.0.
pub(crate) fn compute_relatedness(m: &BirdRecord, f: &BirdRecord) -> f64 {
    let share_mother = matches!((m.mother_id, f.mother_id), (Some(a), Some(b)) if a == b);
    let share_father = matches!((m.father_id, f.father_id), (Some(a), Some(b)) if a == b);
    if share_mother && share_father {
        return 0.5;
    }
    if share_mother || share_father {
        return 0.25;
    }

    if m.lineage_ids.is_empty() || f.lineage_ids.is_empty() {
        return 0.0;
    }
    let m_set: std::collections::HashSet<i64> = m.lineage_ids.iter().copied().collect();
    let f_set: std::collections::HashSet<i64> = f.lineage_ids.iter().copied().collect();
    let intersection = m_set.intersection(&f_set).count();
    let union = m_set.union(&f_set).count();
    if union == 0 {
        return 0.0;
    }
    (intersection as f64 / union as f64) * 0.25
}

fn fetch_lineage_ids(conn: &rusqlite::Connection, bird_id: i64) -> Vec<i64> {
    let mut stmt = match conn.prepare("SELECT lineage_id FROM bird_lineages WHERE bird_id = ?1") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![bird_id], |row| row.get::<_, i64>(0))
        .map(|it| it.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

fn load_bird_records(conn: &std::sync::MutexGuard<'_, rusqlite::Connection>) -> Vec<BirdRecord> {
    let mut stmt = conn
        .prepare("SELECT id, sex, mother_id, father_id FROM birds WHERE status = 'Active'")
        .expect("prepare failed");
    let mut records: Vec<BirdRecord> = stmt
        .query_map([], |row| {
            let sex_str: String = row.get(1)?;
            Ok(BirdRecord {
                id: row.get(0)?,
                sex: str_to_sex(&sex_str),
                lineage_ids: Vec::new(),
                mother_id: row.get(2)?,
                father_id: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    for r in records.iter_mut() {
        r.lineage_ids = fetch_lineage_ids(conn, r.id);
    }
    records
}

/// Every active bird with its sex and probabilistic genetic profile (Phase 4).
/// Profiles come from `bird_genetic_profile`; the pairing/diversity scoring
/// works purely on these distributions, not the discrete `bird_lineages` tags.
fn load_active_profiles(conn: &rusqlite::Connection) -> Vec<(i64, Sex, GeneticProfile)> {
    let ids: Vec<(i64, Sex)> = conn
        .prepare("SELECT id, sex FROM birds WHERE status = 'Active'")
        .and_then(|mut s| {
            let it = s.query_map([], |row| {
                let sex_str: String = row.get(1)?;
                Ok((row.get::<_, i64>(0)?, str_to_sex(&sex_str)))
            })?;
            Ok(it.filter_map(|r| r.ok()).collect::<Vec<_>>())
        })
        .unwrap_or_default();
    ids.into_iter()
        .map(|(id, sex)| (id, sex, crate::genetics::read_profile(conn, id)))
        .collect()
}

/// `max(paternal_overlap, maternal_overlap)` — a pairing's inbreeding risk.
fn pair_risk(a: &GeneticProfile, b: &GeneticProfile) -> f64 {
    let (pat, mat) = crate::genetics::pair_overlap(a, b);
    pat.max(mat)
}

/// `GET /api/breeding/suggest` — every male×female pairing scored by
/// probability-weighted lineage overlap (Phase 4), lowest risk first.
pub(crate) async fn breeding_suggest(
    State(state): State<AppState>,
) -> Json<Vec<PairingSuggestion>> {
    let conn = acquire_db(&state);
    let profiles = load_active_profiles(&conn);
    let males: Vec<&(i64, Sex, GeneticProfile)> = profiles
        .iter()
        .filter(|(_, s, _)| *s == Sex::Male)
        .collect();
    let females: Vec<&(i64, Sex, GeneticProfile)> = profiles
        .iter()
        .filter(|(_, s, _)| *s == Sex::Female)
        .collect();

    let mut results: Vec<PairingSuggestion> = Vec::new();
    for (mid, _, mp) in &males {
        for (fid, _, fp) in &females {
            let (paternal_overlap, maternal_overlap) = crate::genetics::pair_overlap(mp, fp);
            let risk = paternal_overlap.max(maternal_overlap);
            results.push(PairingSuggestion {
                bird_a_id: *mid,
                bird_b_id: *fid,
                paternal_overlap,
                maternal_overlap,
                risk_percent: (risk * 100.0).round() as i64,
                risk_level: crate::genetics::risk_level(risk).to_string(),
            });
        }
    }
    // Lowest risk first; deterministic tiebreak by ids.
    results.sort_by(|a, b| {
        let ra = a.paternal_overlap.max(a.maternal_overlap);
        let rb = b.paternal_overlap.max(b.maternal_overlap);
        ra.partial_cmp(&rb)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.bird_a_id.cmp(&b.bird_a_id))
            .then_with(|| a.bird_b_id.cmp(&b.bird_b_id))
    });
    Json(results)
}

/// `GET /api/breeding/diversity` — flock-wide genetic-diversity snapshot that
/// powers the "new blood" alert (Phase 4).
pub(crate) async fn breeding_diversity(State(state): State<AppState>) -> Json<FlockDiversity> {
    let conn = acquire_db(&state);
    let profiles = load_active_profiles(&conn);

    // Confidence across all active birds.
    let confidences: Vec<f64> = profiles
        .iter()
        .map(|(_, _, p)| crate::genetics::confidence(p))
        .collect();
    let flock_confidence = if confidences.is_empty() {
        0.0
    } else {
        confidences.iter().sum::<f64>() / confidences.len() as f64
    };
    let min_confidence = confidences.iter().copied().fold(f64::INFINITY, f64::min);
    let min_confidence = if min_confidence.is_finite() {
        min_confidence
    } else {
        0.0
    };

    // Best (lowest) achievable overlap risk among candidate male×female pairings.
    let males: Vec<&GeneticProfile> = profiles
        .iter()
        .filter(|(_, s, _)| *s == Sex::Male)
        .map(|(_, _, p)| p)
        .collect();
    let females: Vec<&GeneticProfile> = profiles
        .iter()
        .filter(|(_, s, _)| *s == Sex::Female)
        .map(|(_, _, p)| p)
        .collect();
    let mut best = f64::INFINITY;
    for mp in &males {
        for fp in &females {
            best = best.min(pair_risk(mp, fp));
        }
    }
    let best_pairing_risk = if best.is_finite() { best } else { 1.0 };

    let needs_new_blood =
        best_pairing_risk > crate::genetics::RISK_AVOID_THRESHOLD || min_confidence < 0.50;

    // Distinct lineages appearing across all active profiles (both sides).
    let mut lineages = std::collections::HashSet::new();
    for (_, _, p) in &profiles {
        for c in p.paternal.iter().chain(p.maternal.iter()) {
            lineages.insert(c.lineage_id);
        }
    }

    Json(FlockDiversity {
        flock_confidence,
        min_confidence,
        best_pairing_risk,
        needs_new_blood,
        active_lineage_count: lineages.len() as i64,
    })
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
        let mut record = conn
            .query_row(
                "SELECT id, sex, mother_id, father_id FROM birds WHERE id = ?1",
                params![id],
                |row| {
                    let sex_str: String = row.get(1)?;
                    Ok(BirdRecord {
                        id: row.get(0)?,
                        sex: str_to_sex(&sex_str),
                        lineage_ids: Vec::new(),
                        mother_id: row.get(2)?,
                        father_id: row.get(3)?,
                    })
                },
            )
            .ok()?;
        record.lineage_ids = fetch_lineage_ids(&conn, record.id);
        Some(record)
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

// --- Flock breeding stats (powers the Flock-screen cull-mode guardrail) ---

/// Returns the snapshot the client needs to draw the cull-mode guardrail:
/// total males/females, the minimum-males line implied by the user's
/// settings, how many males are above that line (`safe_to_cull`), and per-
/// male safe-pairing counts (so the client can flag culls that would
/// leave a specific female with zero safe mates).
///
/// Used to return a prescribed `Vec<CullRecommendation>`. Replaced because
/// the new UX hands the cull-choice to the user — the server only enforces
/// the breeding-capacity guardrail.
pub(crate) async fn cull_recommendations(
    State(state): State<AppState>,
) -> Json<FlockBreedingStats> {
    let conn = acquire_db(&state);

    // Pull live settings — they drive minimum_males_needed below. Falls
    // back to AppSettings::default() if the keys are missing (shouldn't
    // happen post-init_db, but the math has to stay sane regardless).
    let settings = crate::routes::settings::load_settings(&conn);

    let all_birds = load_bird_records(&conn);
    let males: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Male).collect();
    let females: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Female).collect();
    let total_males = males.len() as u32;
    let total_females = females.len() as u32;

    // ceil(total_females / max_females_per_male) * desired_males_per_group.
    // `max(1, ...)` on the divisor defends against a malformed setting of 0
    // — the PUT handler rejects that, but a hand-edited DB row could still
    // smuggle one in.
    let max_per_male = settings.max_females_per_male.max(1);
    let minimum_males_needed = if total_females == 0 {
        0
    } else {
        let groups_needed = total_females.div_ceil(max_per_male);
        groups_needed * settings.desired_males_per_group
    };
    let safe_to_cull = total_males.saturating_sub(minimum_males_needed);

    // For each male, count + collect the females he can breed with safely
    // (relatedness < 0.0625). The id list lets the client answer the per-
    // female "would culling these males leave her with 0 safe mates?"
    // question without another round-trip.
    let mut per_male: Vec<PerMaleSafePairings> = males
        .iter()
        .map(|m| {
            let safe_ids: Vec<i64> = females
                .iter()
                .filter(|f| compute_relatedness(m, f) < 0.0625)
                .map(|f| f.id)
                .collect();
            PerMaleSafePairings {
                bird_id: m.id,
                safe_pairings: safe_ids.len() as u32,
                safe_female_ids: safe_ids,
            }
        })
        .collect();
    // Ascending safe-pairings: weakest breeders first, so the UI can
    // highlight them as the natural cull candidates.
    per_male.sort_by(|a, b| {
        a.safe_pairings
            .cmp(&b.safe_pairings)
            .then_with(|| a.bird_id.cmp(&b.bird_id))
    });

    Json(FlockBreedingStats {
        total_males,
        total_females,
        minimum_males_needed,
        safe_to_cull,
        per_male_safe_pairings: per_male,
        desired_males_per_group: settings.desired_males_per_group,
        max_females_per_male: settings.max_females_per_male,
    })
}

// --- Flock Summary ---

#[derive(Serialize)]
pub(crate) struct FlockSummary {
    total_birds: i64,
    active_birds: i64,
    males: i64,
    females: i64,
    /// Birds tagged with each lineage. A bird with multiple lineages contributes
    /// to each row, so the sum of `count` may exceed `active_birds`.
    lineages: Vec<LineageCount>,
}

#[derive(Serialize)]
pub(crate) struct LineageCount {
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

    let mut stmt = conn
        .prepare(
            "SELECT l.name, COUNT(*) AS c
             FROM birds bi
             JOIN bird_lineages bl ON bl.bird_id = bi.id
             JOIN lineages l ON l.id = bl.lineage_id
             WHERE bi.status = 'Active'
             GROUP BY l.id, l.name
             ORDER BY c DESC",
        )
        .expect("prepare failed");
    let lineages: Vec<LineageCount> = stmt
        .query_map([], |row| {
            Ok(LineageCount {
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
        lineages,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(id: i64, lineages: Vec<i64>, mother: Option<i64>, father: Option<i64>) -> BirdRecord {
        BirdRecord {
            id,
            sex: Sex::Unknown,
            lineage_ids: lineages,
            mother_id: mother,
            father_id: father,
        }
    }

    #[test]
    fn full_siblings_score_half() {
        // Both parents shared trumps any lineage analysis.
        let a = rec(1, vec![1], Some(10), Some(20));
        let b = rec(2, vec![2], Some(10), Some(20));
        assert!((compute_relatedness(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn half_siblings_score_quarter() {
        let a = rec(1, vec![1], Some(10), Some(20));
        let b = rec(2, vec![2], Some(10), Some(99));
        assert!((compute_relatedness(&a, &b) - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn identical_single_lineage_no_parents_scores_quarter() {
        // 1 ∩ 1 / 1 ∪ 1 = 1.0 × 0.25 = 0.25 — preserves the old
        // single-bloodline behavior as the upper bound of the new rule.
        let a = rec(1, vec![1], None, None);
        let b = rec(2, vec![1], None, None);
        assert!((compute_relatedness(&a, &b) - 0.25).abs() < f64::EPSILON);
    }

    /// Partial lineage overlap with no parental link should produce a
    /// fractional score between the old single-lineage "same" (0.25) and
    /// "different" (0.0). Concretely: [Fernbank, NWQuail] vs [Fernbank]
    /// is |{Fernbank}| / |{Fernbank, NWQuail}| × 0.25 = 0.5 × 0.25 = 0.125.
    #[test]
    fn partial_lineage_overlap_scores_proportionally() {
        let a = rec(1, vec![1, 2], None, None);
        let b = rec(2, vec![1], None, None);
        assert!((compute_relatedness(&a, &b) - 0.125).abs() < f64::EPSILON);
    }

    /// One-third overlap: [A, B, C] vs [A] → 1/3 × 0.25 ≈ 0.0833.
    /// Still unsafe (> 0.0625 threshold). Sanity-checks the rule on a
    /// three-way mix.
    #[test]
    fn one_third_lineage_overlap_scores_below_quarter() {
        let a = rec(1, vec![1, 2, 3], None, None);
        let b = rec(2, vec![1], None, None);
        let r = compute_relatedness(&a, &b);
        assert!((r - (1.0 / 3.0) * 0.25).abs() < f64::EPSILON);
        assert!(r > 0.0625, "1/3 × 0.25 ≈ 0.0833 is still flagged unsafe");
    }

    /// Identical multi-lineage sets behave like the single-lineage match:
    /// [A, B] vs [A, B] = 2/2 = 1.0 × 0.25 = 0.25.
    #[test]
    fn identical_multi_lineage_sets_score_quarter() {
        let a = rec(1, vec![1, 2], None, None);
        let b = rec(2, vec![1, 2], None, None);
        assert!((compute_relatedness(&a, &b) - 0.25).abs() < f64::EPSILON);
    }

    /// No lineage overlap, no parents known → 0.0 (safe).
    #[test]
    fn disjoint_lineage_sets_score_zero() {
        let a = rec(1, vec![1, 2], None, None);
        let b = rec(2, vec![3], None, None);
        assert!((compute_relatedness(&a, &b) - 0.0).abs() < f64::EPSILON);
        // Confirms the safety threshold logic: 0.0 < 0.0625 → safe.
        assert!(compute_relatedness(&a, &b) < 0.0625);
    }

    /// Empty lineage list on either side → no lineage contribution; with
    /// no parental link, total relatedness is 0.0. (We can't measure what
    /// we don't know — don't penalise the pair.)
    #[test]
    fn empty_lineage_list_treated_as_zero_overlap() {
        let a = rec(1, vec![], None, None);
        let b = rec(2, vec![1, 2], None, None);
        assert!((compute_relatedness(&a, &b) - 0.0).abs() < f64::EPSILON);

        let c = rec(3, vec![], None, None);
        let d = rec(4, vec![], None, None);
        assert!((compute_relatedness(&c, &d) - 0.0).abs() < f64::EPSILON);
    }

    /// Parental link beats lineage analysis even when lineages disagree.
    /// Half-siblings with disjoint lineage tags still score 0.25 from
    /// the parent rule (not 0.0 from the disjoint-lineage rule).
    #[test]
    fn parent_overlap_overrides_lineage_disjoint() {
        let a = rec(1, vec![1], Some(10), None);
        let b = rec(2, vec![2], Some(10), None);
        assert!((compute_relatedness(&a, &b) - 0.25).abs() < f64::EPSILON);
    }
}
