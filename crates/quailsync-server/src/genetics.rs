//! Probabilistic genetic profiles (Phase 3).
//!
//! Shared math + DB helpers for the `bird_genetic_profile` table: every bird
//! carries a paternal + maternal probability distribution over lineages, each
//! side summing to 1.0. This supersedes the discrete `bird_lineages` junction
//! for genetic reasoning; `bird_lineages` is kept read-only during the
//! transition and is populated in parallel.

use quailsync_common::{GeneticProfile, LineageProbability};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Components below this are dropped, then the side is renormalized to sum 1.0.
/// A lineage contributing less than 1% is treated as untracked noise.
pub const TRACKING_FLOOR: f64 = 0.01;

/// `(lineage_id, probability)` pairs for one inherited side.
pub type Dist = Vec<(i64, f64)>;

/// Distribution over lineages from a set of birds' lineage-id lists: each bird
/// is weighted equally (1) and split across its lineage tags, then normalized by
/// the bird count so the probabilities sum to 1.0. Highest-probability first;
/// ties broken by lineage id for determinism. Not floored — the caller floors
/// when deriving a bird profile. Empty input yields an empty distribution.
pub fn distribution(birds: &[Vec<i64>]) -> Dist {
    let n = birds.len();
    if n == 0 {
        return Vec::new();
    }
    let mut acc: HashMap<i64, f64> = HashMap::new();
    for lineages in birds {
        if lineages.is_empty() {
            continue;
        }
        let w = 1.0 / lineages.len() as f64;
        for &lid in lineages {
            *acc.entry(lid).or_insert(0.0) += w;
        }
    }
    let mut out: Dist = acc
        .into_iter()
        .map(|(lid, sum)| (lid, sum / n as f64))
        .collect();
    sort_desc(&mut out);
    out
}

/// Sort a distribution highest-probability first, ties by ascending lineage id.
fn sort_desc(dist: &mut Dist) {
    dist.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
}

/// Apply the tracking floor: drop components below `floor`, then renormalize the
/// survivors to sum to 1.0. Renormalization only scales the kept components *up*,
/// so nothing can fall back below the floor — one pass suffices. If everything is
/// below the floor (degenerate input), the single largest component is kept at
/// 1.0 so a bird always has a profile. `floor` is a fraction (e.g. `0.01` = 1%);
/// callers pass the configured [`tracking_floor`].
pub fn apply_floor(dist: Dist, floor: f64) -> Dist {
    if dist.is_empty() {
        return dist;
    }
    let mut kept: Dist = dist.iter().copied().filter(|&(_, p)| p >= floor).collect();
    if kept.is_empty() {
        if let Some(&max) = dist
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            return vec![(max.0, 1.0)];
        }
        return kept;
    }
    let total: f64 = kept.iter().map(|&(_, p)| p).sum();
    if total > 0.0 {
        for c in kept.iter_mut() {
            c.1 /= total;
        }
    }
    sort_desc(&mut kept);
    kept
}

/// Write one inherited side of a bird's genetic profile. `dist` is expected to
/// be floored + normalized already; this defensively renormalizes so the stored
/// rows always satisfy the sum-to-1.0 invariant (tolerance 0.001) and the
/// table's `probability > 0.0` CHECK. Existing rows for `(bird_id, side)` are
/// replaced. Empty distributions clear the side.
pub fn write_profile_side(conn: &Connection, bird_id: i64, side: &str, dist: &[(i64, f64)]) {
    let _ = conn.execute(
        "DELETE FROM bird_genetic_profile WHERE bird_id = ?1 AND side = ?2",
        params![bird_id, side],
    );
    if dist.is_empty() {
        return;
    }
    let total: f64 = dist.iter().map(|&(_, p)| p).sum();
    for &(lineage_id, p) in dist {
        let prob = if total > 0.0 { p / total } else { p };
        if prob <= 0.0 {
            continue;
        }
        let prob = prob.min(1.0);
        let _ = conn.execute(
            "INSERT OR REPLACE INTO bird_genetic_profile (bird_id, side, lineage_id, probability)
             VALUES (?1, ?2, ?3, ?4)",
            params![bird_id, side, lineage_id, prob],
        );
    }
}

/// Read a bird's full genetic profile (both sides), with lineage names filled in
/// and each side sorted highest-probability first.
pub fn read_profile(conn: &Connection, bird_id: i64) -> GeneticProfile {
    let names = lineage_name_map(conn);
    GeneticProfile {
        paternal: read_side(conn, bird_id, "paternal", &names),
        maternal: read_side(conn, bird_id, "maternal", &names),
    }
}

fn read_side(
    conn: &Connection,
    bird_id: i64,
    side: &str,
    names: &HashMap<i64, String>,
) -> Vec<LineageProbability> {
    let mut rows: Dist = conn
        .prepare("SELECT lineage_id, probability FROM bird_genetic_profile WHERE bird_id = ?1 AND side = ?2")
        .and_then(|mut s| {
            let it = s.query_map(params![bird_id, side], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
            })?;
            Ok(it.filter_map(|r| r.ok()).collect::<Dist>())
        })
        .unwrap_or_default();
    sort_desc(&mut rows);
    rows.into_iter()
        .map(|(lineage_id, probability)| LineageProbability {
            lineage_id,
            lineage_name: names.get(&lineage_id).cloned().unwrap_or_default(),
            probability,
        })
        .collect()
}

/// Confidence in a bird's lineage: `min(max(paternal), max(maternal))` — the
/// weakest inherited side's strongest single lineage. `0.0` when either side is
/// empty (an unknown side caps confidence at zero).
pub fn confidence(profile: &GeneticProfile) -> f64 {
    let max_of =
        |side: &[LineageProbability]| side.iter().map(|c| c.probability).fold(0.0_f64, f64::max);
    max_of(&profile.paternal).min(max_of(&profile.maternal))
}

// ---------------------------------------------------------------------------
// Phase 4: weighted inbreeding scoring
// ---------------------------------------------------------------------------

/// Default overlap at/above which a pairing is "caution" (below it, "safe").
/// Overridden per request by `genetics.threshold.safe` (Phase 5).
pub const RISK_CAUTION_THRESHOLD: f64 = 0.15;
/// Default overlap strictly above which a pairing is "avoid". Overridden per
/// request by `genetics.threshold.avoid` (Phase 5).
pub const RISK_AVOID_THRESHOLD: f64 = 0.35;

/// Probability-weighted overlap between two lineage distributions:
/// `Σ over lineages of A.p[l] × B.p[l]`. `0.0` when either side is empty or the
/// two share no lineages; `1.0` when both are the same point mass. Symmetric.
pub fn side_overlap(a: &[LineageProbability], b: &[LineageProbability]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let b_map: HashMap<i64, f64> = b.iter().map(|c| (c.lineage_id, c.probability)).collect();
    a.iter()
        .filter_map(|c| b_map.get(&c.lineage_id).map(|&bp| c.probability * bp))
        .sum()
}

/// `(paternal_overlap, maternal_overlap)` between two birds' genetic profiles.
/// The pairing's inbreeding risk is the larger of the two.
pub fn pair_overlap(a: &GeneticProfile, b: &GeneticProfile) -> (f64, f64) {
    (
        side_overlap(&a.paternal, &b.paternal),
        side_overlap(&a.maternal, &b.maternal),
    )
}

/// Risk band for an overlap value: `"safe"` (< `safe`), `"caution"`
/// (`safe`..=`avoid`), `"avoid"` (> `avoid`). `safe`/`avoid` are fractions
/// (e.g. `0.15`/`0.35`), read from settings per request in Phase 5.
pub fn risk_level(overlap: f64, safe: f64, avoid: f64) -> &'static str {
    if overlap < safe {
        "safe"
    } else if overlap > avoid {
        "avoid"
    } else {
        "caution"
    }
}

/// The configured tracking floor as a fraction (percent / 100), falling back to
/// [`TRACKING_FLOOR`] when `genetics.tracking_floor` is absent or unparseable.
pub fn tracking_floor(conn: &Connection) -> f64 {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![quailsync_common::GeneticsSettings::KEY_TRACKING_FLOOR],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<f64>().ok())
    .map(|pct| pct / 100.0)
    .unwrap_or(TRACKING_FLOOR)
}

fn lineage_name_map(conn: &Connection) -> HashMap<i64, String> {
    conn.prepare("SELECT id, name FROM lineages")
        .and_then(|mut s| {
            let it = s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            Ok(it.filter_map(|r| r.ok()).collect::<HashMap<i64, String>>())
        })
        .unwrap_or_default()
}

/// The raw (unfloored) paternal + maternal distributions from a clutch's frozen
/// snapshot, or `None` when the clutch has no snapshot rows (a lineage-only
/// clutch). Mirrors the maternal/paternal split in `routes::clutches`.
pub fn snapshot_side_distributions(conn: &Connection, clutch_id: i64) -> Option<(Dist, Dist)> {
    let rows: Vec<(String, i64, i64)> = conn
        .prepare("SELECT sex, bird_id, lineage_id FROM clutch_snapshots WHERE clutch_id = ?1")
        .ok()?
        .query_map(params![clutch_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .ok()?
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        return None;
    }
    let (mut males, mut females): (HashMap<i64, Vec<i64>>, HashMap<i64, Vec<i64>>) =
        (HashMap::new(), HashMap::new());
    for (sex, bird_id, lineage_id) in rows {
        let side = if sex == "Male" {
            &mut males
        } else {
            &mut females
        };
        side.entry(bird_id).or_default().push(lineage_id);
    }
    let male_lists: Vec<Vec<i64>> = males.into_values().collect();
    let female_lists: Vec<Vec<i64>> = females.into_values().collect();
    Some((distribution(&male_lists), distribution(&female_lists)))
}

/// The clutch a chick group descends from, if any.
pub fn clutch_id_for_group(conn: &Connection, group_id: i64) -> Option<i64> {
    conn.query_row(
        "SELECT clutch_id FROM chick_groups WHERE id = ?1",
        params![group_id],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
}

fn clutch_lineage(conn: &Connection, clutch_id: i64) -> Option<i64> {
    conn.query_row(
        "SELECT lineage_id FROM clutches WHERE id = ?1",
        params![clutch_id],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
}

/// Populate a newly-created bird's genetic profile. Resolution order:
///
/// 1. If `clutch_id` has a group snapshot → derive from its paternal/maternal
///    distributions (floored + renormalized).
/// 2. Else if that clutch names a single `lineage_id` → 100% that lineage on
///    both sides (lineage-only clutch fallback).
/// 3. Else → `fallback_lineages` (the bird's own tags) at 100%, split equally
///    on both sides — the gen-0 / manually-created source-bird case.
///
/// A no-op when nothing resolves (no clutch and no fallback lineages).
pub fn populate_bird_profile(
    conn: &Connection,
    bird_id: i64,
    clutch_id: Option<i64>,
    fallback_lineages: &[i64],
) {
    let floor = tracking_floor(conn);
    let derived = clutch_id.and_then(|cid| match snapshot_side_distributions(conn, cid) {
        Some((pat, mat)) => Some((apply_floor(pat, floor), apply_floor(mat, floor))),
        None => clutch_lineage(conn, cid).map(|lid| {
            let d = vec![(lid, 1.0)];
            (d.clone(), d)
        }),
    });
    let (paternal, maternal) = match derived {
        Some(pm) => pm,
        None => {
            if fallback_lineages.is_empty() {
                return;
            }
            let p = 1.0 / fallback_lineages.len() as f64;
            let d = apply_floor(fallback_lineages.iter().map(|&l| (l, p)).collect(), floor);
            (d.clone(), d)
        }
    };
    write_profile_side(conn, bird_id, "paternal", &paternal);
    write_profile_side(conn, bird_id, "maternal", &maternal);
}

/// Additive, idempotent migration: seed `bird_genetic_profile` from the existing
/// `bird_lineages` junction. Every currently-tagged bird is a gen-0 source bird,
/// so it becomes 100% on *both* sides, split equally across its lineages
/// (2 lineages → 50%/50% each side). Only birds that have no profile rows yet
/// are touched, so re-running on a live DB is safe. `bird_lineages` is left
/// intact (read-only during the transition).
pub fn migrate_bird_lineages(conn: &Connection) {
    let bird_ids: Vec<i64> = conn
        .prepare(
            "SELECT DISTINCT bl.bird_id FROM bird_lineages bl
             WHERE NOT EXISTS (
                 SELECT 1 FROM bird_genetic_profile g WHERE g.bird_id = bl.bird_id
             )",
        )
        .and_then(|mut s| {
            let it = s.query_map([], |r| r.get::<_, i64>(0))?;
            Ok(it.filter_map(|r| r.ok()).collect::<Vec<i64>>())
        })
        .unwrap_or_default();

    let floor = tracking_floor(conn);
    for bird_id in bird_ids {
        let lineages: Vec<i64> = conn
            .prepare("SELECT lineage_id FROM bird_lineages WHERE bird_id = ?1")
            .and_then(|mut s| {
                let it = s.query_map(params![bird_id], |r| r.get::<_, i64>(0))?;
                Ok(it.filter_map(|r| r.ok()).collect::<Vec<i64>>())
            })
            .unwrap_or_default();
        if lineages.is_empty() {
            continue;
        }
        let p = 1.0 / lineages.len() as f64;
        let dist = apply_floor(lineages.iter().map(|&l| (l, p)).collect(), floor);
        write_profile_side(conn, bird_id, "paternal", &dist);
        write_profile_side(conn, bird_id, "maternal", &dist);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init_db;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn);
        conn
    }

    fn add_lineage(conn: &Connection, name: &str) -> i64 {
        conn.execute(
            "INSERT INTO lineages (name, source, notes) VALUES (?1, 'test', NULL)",
            params![name],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn add_bird(conn: &Connection) -> i64 {
        conn.execute(
            "INSERT INTO birds (band_color, sex, hatch_date, generation, status)
             VALUES (NULL, 'Female', '2026-01-01', 1, 'Active')",
            [],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn side(conn: &Connection, bird_id: i64, side: &str) -> Dist {
        let mut rows: Dist = conn
            .prepare("SELECT lineage_id, probability FROM bird_genetic_profile WHERE bird_id=?1 AND side=?2")
            .unwrap()
            .query_map(params![bird_id, side], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        sort_desc(&mut rows);
        rows
    }

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn distribution_matches_spec_example() {
        // 4 birds of lineage 1, 1 bird of lineage 2 → 0.8 / 0.2.
        let birds = vec![vec![1], vec![1], vec![1], vec![1], vec![2]];
        let d = distribution(&birds);
        assert_eq!(d[0], (1, 0.8));
        assert_eq!(d[1], (2, 0.2));
    }

    #[test]
    fn distribution_splits_multi_lineage_birds_equally() {
        // One bird tagged with both lineages → 0.5 / 0.5.
        let d = distribution(&[vec![1, 2]]);
        assert!(approx(d.iter().find(|x| x.0 == 1).unwrap().1, 0.5));
        assert!(approx(d.iter().find(|x| x.0 == 2).unwrap().1, 0.5));
    }

    #[test]
    fn apply_floor_drops_below_1pct_and_renormalizes() {
        let floored = apply_floor(vec![(1, 0.98), (2, 0.015), (3, 0.005)], TRACKING_FLOOR);
        // The 0.5% component is dropped; the rest renormalize to sum 1.0.
        assert_eq!(floored.len(), 2);
        let total: f64 = floored.iter().map(|&(_, p)| p).sum();
        assert!((total - 1.0).abs() < 0.001);
        assert!(floored.iter().all(|&(_, p)| p >= TRACKING_FLOOR));
        assert_eq!(floored[0].0, 1);
        assert!(approx(floored[0].1, 0.98 / 0.995));
    }

    #[test]
    fn confidence_is_weakest_sides_strongest() {
        let profile = GeneticProfile {
            paternal: vec![lp(1, 0.9), lp(2, 0.1)],
            maternal: vec![lp(1, 0.6), lp(2, 0.4)],
        };
        assert!(approx(confidence(&profile), 0.6));
    }

    #[test]
    fn confidence_zero_when_a_side_is_empty() {
        let profile = GeneticProfile {
            paternal: vec![lp(1, 1.0)],
            maternal: vec![],
        };
        assert_eq!(confidence(&profile), 0.0);
    }

    fn lp(id: i64, p: f64) -> LineageProbability {
        LineageProbability {
            lineage_id: id,
            lineage_name: String::new(),
            probability: p,
        }
    }

    #[test]
    fn overlap_matches_spec_example() {
        // A {NWQuail:0.8, Fernbank:0.2} vs B {Fernbank:0.83, NWQuail:0.17}
        // = 0.8*0.17 + 0.2*0.83 = 0.302. (lineage 1 = NWQuail, 2 = Fernbank)
        let a = vec![lp(1, 0.8), lp(2, 0.2)];
        let b = vec![lp(2, 0.83), lp(1, 0.17)];
        assert!(approx(side_overlap(&a, &b), 0.302));
    }

    #[test]
    fn overlap_zero_for_disjoint_lineages() {
        assert_eq!(side_overlap(&[lp(1, 1.0)], &[lp(2, 1.0)]), 0.0);
    }

    #[test]
    fn overlap_full_for_identical_point_mass() {
        let a = vec![lp(1, 1.0)];
        assert!(approx(side_overlap(&a, &a), 1.0));
    }

    #[test]
    fn overlap_empty_side_is_zero() {
        assert_eq!(side_overlap(&[], &[lp(1, 1.0)]), 0.0);
    }

    #[test]
    fn pair_overlap_scores_each_side_independently() {
        let a = GeneticProfile {
            paternal: vec![lp(1, 1.0)],
            maternal: vec![lp(1, 0.8), lp(2, 0.2)],
        };
        let b = GeneticProfile {
            paternal: vec![lp(2, 1.0)],
            maternal: vec![lp(2, 0.83), lp(1, 0.17)],
        };
        let (pat, mat) = pair_overlap(&a, &b);
        assert_eq!(pat, 0.0);
        assert!(approx(mat, 0.302));
    }

    #[test]
    fn risk_levels_follow_thresholds() {
        let (safe, avoid) = (RISK_CAUTION_THRESHOLD, RISK_AVOID_THRESHOLD);
        assert_eq!(risk_level(0.0, safe, avoid), "safe");
        assert_eq!(risk_level(0.1499, safe, avoid), "safe");
        assert_eq!(risk_level(0.15, safe, avoid), "caution");
        assert_eq!(risk_level(0.302, safe, avoid), "caution");
        assert_eq!(risk_level(0.35, safe, avoid), "caution");
        assert_eq!(risk_level(0.3501, safe, avoid), "avoid");
        assert_eq!(risk_level(1.0, safe, avoid), "avoid");
    }

    #[test]
    fn migration_seeds_100_percent_on_both_sides() {
        let conn = mem();
        let lin = add_lineage(&conn, "pharaoh");
        let bird = add_bird(&conn);
        conn.execute(
            "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
            params![bird, lin],
        )
        .unwrap();

        migrate_bird_lineages(&conn);

        assert_eq!(side(&conn, bird, "paternal"), vec![(lin, 1.0)]);
        assert_eq!(side(&conn, bird, "maternal"), vec![(lin, 1.0)]);
    }

    #[test]
    fn migration_splits_multi_lineage_bird_50_50() {
        let conn = mem();
        let a = add_lineage(&conn, "a");
        let b = add_lineage(&conn, "b");
        let bird = add_bird(&conn);
        for l in [a, b] {
            conn.execute(
                "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
                params![bird, l],
            )
            .unwrap();
        }

        migrate_bird_lineages(&conn);

        for s in ["paternal", "maternal"] {
            let rows = side(&conn, bird, s);
            assert_eq!(rows.len(), 2);
            assert!(rows.iter().all(|&(_, p)| approx(p, 0.5)));
        }
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = mem();
        let lin = add_lineage(&conn, "pharaoh");
        let bird = add_bird(&conn);
        conn.execute(
            "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
            params![bird, lin],
        )
        .unwrap();
        migrate_bird_lineages(&conn);
        migrate_bird_lineages(&conn);
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM bird_genetic_profile WHERE bird_id = ?1",
                params![bird],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2); // one paternal + one maternal row, not doubled.
    }

    #[test]
    fn graduation_from_snapshot_matches_distributions() {
        let conn = mem();
        let a = add_lineage(&conn, "nwquail");
        let b = add_lineage(&conn, "fernbank");
        // A lineage-only clutch row (no group), snapshot filled in manually:
        conn.execute(
            "INSERT INTO clutches (lineage_id, eggs_set, set_date, expected_hatch_date, status)
             VALUES (?1, 10, '2026-01-01', '2026-01-18', 'Incubating')",
            params![a],
        )
        .unwrap();
        let clutch = conn.last_insert_rowid();
        // 1 male of lineage A → paternal certain; 4 females A + 1 female B → 0.8/0.2.
        let male = add_bird(&conn);
        conn.execute(
            "INSERT INTO clutch_snapshots (clutch_id, bird_id, sex, lineage_id) VALUES (?1,?2,'Male',?3)",
            params![clutch, male, a],
        )
        .unwrap();
        for i in 0..4 {
            let f = add_bird(&conn);
            conn.execute(
                "INSERT INTO clutch_snapshots (clutch_id, bird_id, sex, lineage_id) VALUES (?1,?2,'Female',?3)",
                params![clutch, f, a],
            )
            .unwrap();
            let _ = i;
        }
        let f = add_bird(&conn);
        conn.execute(
            "INSERT INTO clutch_snapshots (clutch_id, bird_id, sex, lineage_id) VALUES (?1,?2,'Female',?3)",
            params![clutch, f, b],
        )
        .unwrap();

        let chick = add_bird(&conn);
        populate_bird_profile(&conn, chick, Some(clutch), &[]);

        assert_eq!(side(&conn, chick, "paternal"), vec![(a, 1.0)]);
        let mat = side(&conn, chick, "maternal");
        assert_eq!(mat[0].0, a);
        assert!(approx(mat[0].1, 0.8));
        assert!(approx(mat[1].1, 0.2));
    }

    #[test]
    fn graduation_without_snapshot_falls_back_to_clutch_lineage() {
        let conn = mem();
        let a = add_lineage(&conn, "pharaoh");
        conn.execute(
            "INSERT INTO clutches (lineage_id, eggs_set, set_date, expected_hatch_date, status)
             VALUES (?1, 10, '2026-01-01', '2026-01-18', 'Incubating')",
            params![a],
        )
        .unwrap();
        let clutch = conn.last_insert_rowid();
        let chick = add_bird(&conn);

        populate_bird_profile(&conn, chick, Some(clutch), &[]);

        assert_eq!(side(&conn, chick, "paternal"), vec![(a, 1.0)]);
        assert_eq!(side(&conn, chick, "maternal"), vec![(a, 1.0)]);
    }

    #[test]
    fn source_bird_uses_fallback_lineages_at_100_percent() {
        let conn = mem();
        let a = add_lineage(&conn, "a");
        let bird = add_bird(&conn);
        // No clutch → falls back to the bird's own tags, 100% both sides.
        populate_bird_profile(&conn, bird, None, &[a]);
        assert_eq!(side(&conn, bird, "paternal"), vec![(a, 1.0)]);
        assert_eq!(side(&conn, bird, "maternal"), vec![(a, 1.0)]);
    }
}
