//! Dropped-tag reconciliation: figure out which present, unbanded bird a
//! found NFC leg band belongs to.
//!
//! A quail's band sometimes falls off. The physical tag still works and its
//! stored tag→bird mapping is still correct — the bird is just unbanded right
//! now. This endpoint is **read-only deduction**: it never writes a band
//! assignment, mints a tag, or re-bands anything. It answers one question —
//! "based on what the keeper can observe, whose tag is this?" — and returns a
//! confident match, a narrowed short-list, or "none of these".
//!
//! The deduction is **elimination, not similarity ranking**. A mismatch on a
//! hard attribute (sex, bloodline) *disqualifies* a tag for a bird entirely.
//! Soft attributes (band colour) only rank among the survivors, because a
//! keeper might mis-read a shade. See `docs/dropped_tag_deduction.md`.
//!
//! Scoped to **breeding groups** (one male + N females). The single male is a
//! trivial short-circuit; only the female set needs the constraint-propagation
//! ("Sudoku") step.

use std::collections::HashSet;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::Sex;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::helpers::{fetch_bird_lineages, str_to_sex};
use crate::state::{acquire_db, AppState};

// ---------------------------------------------------------------------------
// Request / response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub(crate) struct ReconcileRequest {
    /// Tags found on the floor — their stored bird records are the "expected"
    /// identities we're trying to place.
    pub orphan_tag_ids: Vec<String>,
    /// Birds currently present in the group with no band, described by
    /// observation.
    pub observed_birds: Vec<ObservedBird>,
}

#[derive(Deserialize)]
pub(crate) struct ObservedBird {
    /// A temporary client-side handle so the response can refer back to this
    /// bird (e.g. "bird-1", "the one in the corner"). NOT a DB id.
    pub ref_id: String,
    /// `None` = couldn't tell. `Some(Unknown)` is also treated as "couldn't
    /// tell" and never eliminates anything.
    #[serde(default)]
    pub sex: Option<Sex>,
    /// A single observed bloodline/lineage name, if the keeper recognised one.
    #[serde(default)]
    pub bloodline: Option<String>,
    #[serde(default)]
    pub traits: ObservedTraits,
}

#[derive(Deserialize, Default)]
pub(crate) struct ObservedTraits {
    /// The only soft trait QuailSync actually stores per bird. Used for
    /// ranking survivors, never for elimination.
    #[serde(default)]
    pub band_color: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct ReconcileResponse {
    pub results: Vec<BirdResult>,
    /// Tags that resolved to no bird, or to a bird outside this group
    /// (e.g. the bird is actually missing). Reported, never matched.
    pub unmatched_tags: Vec<String>,
}

#[derive(Serialize, Debug)]
pub(crate) struct BirdResult {
    pub ref_id: String,
    pub outcome: MatchOutcome,
}

#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum MatchOutcome {
    /// Exactly one tag survived elimination (possibly via propagation).
    Resolved {
        tag_id: String,
        confidence: Confidence,
    },
    /// Multiple tags still consistent — ranked best-first by soft-trait score.
    Ambiguous { candidates: Vec<Candidate> },
    /// No orphan tag is consistent with this bird's hard attributes.
    NoCandidate,
}

#[derive(Serialize, Debug)]
pub(crate) struct Candidate {
    pub tag_id: String,
    /// Soft-trait Jaccard similarity, 0.0–1.0.
    pub score: f64,
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Confidence {
    /// Only one tag was ever consistent with this bird.
    Sole,
    /// Became sole after another bird was locked (propagation).
    Forced,
}

// ---------------------------------------------------------------------------
// Internal model
// ---------------------------------------------------------------------------

/// The stored identity behind one dropped tag, loaded from the DB. This is the
/// "expected" record we eliminate observed birds against.
#[derive(Clone, Debug)]
pub(crate) struct ExpectedTag {
    pub tag_id: String,
    pub sex: Sex,
    /// Lowercased lineage names, for case-insensitive bloodline comparison.
    pub lineages: HashSet<String>,
    /// Lowercased band colour, if recorded.
    pub band_color: Option<String>,
}

// ---------------------------------------------------------------------------
// Soft-trait Jaccard ranking (standalone — NOT the genetic relatedness helper)
// ---------------------------------------------------------------------------

/// Single-pass Jaccard similarity between two soft-trait token sets:
/// `|A ∩ B| / |A ∪ B|`. Two empty sets share no evidence, so they score 0.0.
///
/// Deliberately *not* `breeding::compute_relatedness` — that scores genetic
/// relatedness over lineage IDs and parents, which is the wrong question for
/// ranking observable traits like band colour.
fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// Soft traits for an observed bird, as namespaced tokens (e.g.
/// `band_color:red`). Namespacing keeps the set extensible if more soft traits
/// are added later. A missing/blank observation is simply absent — never a
/// penalised mismatch.
fn observed_soft_traits(o: &ObservedBird) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(bc) = o.traits.band_color.as_deref() {
        let bc = bc.trim();
        if !bc.is_empty() {
            set.insert(format!("band_color:{}", bc.to_lowercase()));
        }
    }
    set
}

/// Soft traits for a stored tag identity (band colour is pre-lowercased).
fn expected_soft_traits(e: &ExpectedTag) -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(bc) = e.band_color.as_deref() {
        set.insert(format!("band_color:{bc}"));
    }
    set
}

// ---------------------------------------------------------------------------
// Hard-attribute elimination
// ---------------------------------------------------------------------------

/// Is this stored tag identity still *consistent* with what was observed?
/// Returns false only on a **provable** hard-attribute contradiction.
///
/// - Sex: eliminate only when both sides are a known sex and they differ.
///   `None` or `Unknown` on either side never eliminates.
/// - Bloodline: eliminate only when the keeper named a bloodline AND the tag's
///   bird has a known (non-empty) lineage set that does not contain it. Unknown
///   on either side never eliminates.
fn hard_consistent(obs: &ObservedBird, tag: &ExpectedTag) -> bool {
    if let Some(obs_sex) = &obs.sex {
        if *obs_sex != Sex::Unknown && tag.sex != Sex::Unknown && *obs_sex != tag.sex {
            return false;
        }
    }
    if let Some(bl) = obs.bloodline.as_deref() {
        let bl = bl.trim().to_lowercase();
        if !bl.is_empty() && !tag.lineages.is_empty() && !tag.lineages.contains(&bl) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Deduction core (pure — unit tested without a DB)
// ---------------------------------------------------------------------------

/// One observed bird's working state during propagation.
struct Slot<'a> {
    obs: &'a ObservedBird,
    /// Tag ids still consistent with this bird, mutated during propagation.
    candidates: Vec<String>,
    /// Candidate count immediately after hard elimination — distinguishes
    /// `Sole` (1 from the start) from `Forced` (reduced to 1 by a lock).
    original_count: usize,
    locked: Option<(String, Confidence)>,
}

/// Match a set of dropped tags against a set of present unbanded birds.
///
/// `expected` are the resolved orphan tags that belong to this group;
/// `observed` are the present unbanded birds. Returns one outcome per observed
/// bird, in input order. Pure and read-only.
pub(crate) fn deduce(expected: &[ExpectedTag], observed: &[ObservedBird]) -> Vec<BirdResult> {
    // --- Single-male short-circuit ----------------------------------------
    // A breeding group has exactly one male. If exactly one dropped tag is a
    // male's and exactly one observed bird is reported male, the rooster's
    // band is trivially his: resolve it `Sole` and keep that male tag out of
    // every female's candidate set up front. If we *can't* confidently
    // identify the observed male (none reported, or several), we fall through
    // to general deduction so the male tag is never silently lost.
    let male_tags: Vec<&ExpectedTag> = expected.iter().filter(|t| t.sex == Sex::Male).collect();
    let observed_male_refs: Vec<&str> = observed
        .iter()
        .filter(|o| matches!(o.sex, Some(Sex::Male)))
        .map(|o| o.ref_id.as_str())
        .collect();

    let male_lock: Option<(String, String)> =
        if male_tags.len() == 1 && observed_male_refs.len() == 1 {
            Some((
                observed_male_refs[0].to_string(),
                male_tags[0].tag_id.clone(),
            ))
        } else {
            None
        };
    let locked_male_ref = male_lock.as_ref().map(|(r, _)| r.as_str());
    let locked_male_tag = male_lock.as_ref().map(|(_, t)| t.as_str());

    // --- Build female candidate sets (hard elimination) -------------------
    // Every observed bird except the locked male: list the tags still
    // consistent with its hard attributes, with the male tag removed.
    let mut slots: Vec<Slot> = Vec::new();
    for obs in observed {
        if Some(obs.ref_id.as_str()) == locked_male_ref {
            continue; // resolved by the short-circuit
        }
        let candidates: Vec<String> = expected
            .iter()
            .filter(|t| Some(t.tag_id.as_str()) != locked_male_tag)
            .filter(|t| hard_consistent(obs, t))
            .map(|t| t.tag_id.clone())
            .collect();
        let original_count = candidates.len();
        slots.push(Slot {
            obs,
            candidates,
            original_count,
            locked: None,
        });
    }

    // --- Constraint propagation (the Sudoku step) -------------------------
    // Repeatedly lock any bird down to a single candidate, then strike that
    // tag from every other bird. A strike may create a new singleton, so we
    // loop until nothing changes.
    loop {
        let mut progressed = false;
        for i in 0..slots.len() {
            if slots[i].locked.is_none() && slots[i].candidates.len() == 1 {
                let tag = slots[i].candidates[0].clone();
                let confidence = if slots[i].original_count == 1 {
                    Confidence::Sole
                } else {
                    Confidence::Forced
                };
                slots[i].locked = Some((tag.clone(), confidence));
                for (j, s) in slots.iter_mut().enumerate() {
                    if j != i && s.locked.is_none() {
                        s.candidates.retain(|c| c != &tag);
                    }
                }
                progressed = true;
                break; // restart the scan after any mutation
            }
        }
        if !progressed {
            break;
        }
    }

    // --- Classify and assemble results in observed order ------------------
    observed
        .iter()
        .map(|obs| {
            // The short-circuited male.
            if let Some((mref, mtag)) = &male_lock {
                if &obs.ref_id == mref {
                    return BirdResult {
                        ref_id: obs.ref_id.clone(),
                        outcome: MatchOutcome::Resolved {
                            tag_id: mtag.clone(),
                            confidence: Confidence::Sole,
                        },
                    };
                }
            }
            let slot = slots
                .iter()
                .find(|s| s.obs.ref_id == obs.ref_id)
                .expect("every non-male observed bird has a slot");
            let outcome = if let Some((tag, confidence)) = &slot.locked {
                MatchOutcome::Resolved {
                    tag_id: tag.clone(),
                    confidence: *confidence,
                }
            } else if slot.candidates.is_empty() {
                MatchOutcome::NoCandidate
            } else {
                // 2+ survivors: rank by soft-trait Jaccard, best-first. Ties
                // break on tag_id for a stable, deterministic order.
                let obs_soft = observed_soft_traits(slot.obs);
                let mut candidates: Vec<Candidate> = slot
                    .candidates
                    .iter()
                    .map(|tag_id| {
                        let exp = expected
                            .iter()
                            .find(|e| &e.tag_id == tag_id)
                            .expect("candidate tag exists in expected set");
                        Candidate {
                            tag_id: tag_id.clone(),
                            score: jaccard(&obs_soft, &expected_soft_traits(exp)),
                        }
                    })
                    .collect();
                candidates.sort_by(|a, b| {
                    b.score
                        .partial_cmp(&a.score)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.tag_id.cmp(&b.tag_id))
                });
                MatchOutcome::Ambiguous { candidates }
            };
            BirdResult {
                ref_id: obs.ref_id.clone(),
                outcome,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Group membership (pure — unit tested without a DB)
// ---------------------------------------------------------------------------

/// Split loaded tag lookups into the ones that belong to this group (kept for
/// deduction) and the ones that resolve to no present group member (reported
/// as `unmatched_tags`).
///
/// Each entry is `(tag_id, Some((bird_id, identity)))` when the tag resolved to
/// a bird, or `(tag_id, None)` when it resolved to nothing at all.
fn partition_membership(
    loaded: Vec<(String, Option<(i64, ExpectedTag)>)>,
    member_ids: &HashSet<i64>,
) -> (Vec<ExpectedTag>, Vec<String>) {
    let mut expected = Vec::new();
    let mut unmatched = Vec::new();
    for (tag_id, found) in loaded {
        match found {
            Some((bird_id, identity)) if member_ids.contains(&bird_id) => expected.push(identity),
            _ => unmatched.push(tag_id),
        }
    }
    (expected, unmatched)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// Load the stored identity behind one tag. Returns `None` if the tag resolves
/// to no bird. Does not apply group-membership filtering — that's
/// `partition_membership`'s job.
fn load_tag_identity(conn: &rusqlite::Connection, tag_id: &str) -> Option<(i64, ExpectedTag)> {
    let (bird_id, sex_str, band_color): (i64, String, Option<String>) = conn
        .query_row(
            "SELECT id, sex, band_color FROM birds WHERE nfc_tag_id = ?1",
            params![tag_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()?;
    let lineages: HashSet<String> = fetch_bird_lineages(conn, bird_id)
        .into_iter()
        .map(|l| l.name.trim().to_lowercase())
        .filter(|n| !n.is_empty())
        .collect();
    Some((
        bird_id,
        ExpectedTag {
            tag_id: tag_id.to_string(),
            sex: str_to_sex(&sex_str),
            lineages,
            band_color: band_color
                .map(|b| b.trim().to_lowercase())
                .filter(|b| !b.is_empty()),
        },
    ))
}

/// `POST /api/groups/{id}/reconcile-tags`
///
/// Read-only deduction. The path id must resolve to a breeding group; orphan
/// tags whose bird isn't a member of that group land in `unmatched_tags`.
pub(crate) async fn reconcile_tags(
    State(state): State<AppState>,
    Path(group_id): Path<i64>,
    Json(body): Json<ReconcileRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    // Validate the group exists; reject otherwise.
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM breeding_groups WHERE id = ?1",
            params![group_id],
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

    // Membership = every male (from the breeding_group_males junction — there
    // may be more than one) plus every female row for this group.
    let mut member_ids: HashSet<i64> = HashSet::new();
    {
        let mut stmt = conn
            .prepare("SELECT male_id FROM breeding_group_males WHERE group_id = ?1")
            .expect("prepare failed");
        let males = stmt
            .query_map(params![group_id], |row| row.get::<_, i64>(0))
            .unwrap()
            .filter_map(|r| r.ok());
        for m in males {
            member_ids.insert(m);
        }
    }
    {
        let mut stmt = conn
            .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
            .expect("prepare failed");
        let females = stmt
            .query_map(params![group_id], |row| row.get::<_, i64>(0))
            .unwrap()
            .filter_map(|r| r.ok());
        for f in females {
            member_ids.insert(f);
        }
    }

    // Load each orphan tag's stored identity, then partition by membership.
    let loaded: Vec<(String, Option<(i64, ExpectedTag)>)> = body
        .orphan_tag_ids
        .iter()
        .map(|tag_id| (tag_id.clone(), load_tag_identity(&conn, tag_id)))
        .collect();
    let (expected, unmatched_tags) = partition_membership(loaded, &member_ids);

    // Deduce (no DB writes anywhere in this handler).
    let results = deduce(&expected, &body.observed_birds);

    Json(ReconcileResponse {
        results,
        unmatched_tags,
    })
    .into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: &str, sex: Sex, lineages: &[&str], band: Option<&str>) -> ExpectedTag {
        ExpectedTag {
            tag_id: id.to_string(),
            sex,
            lineages: lineages.iter().map(|s| s.to_lowercase()).collect(),
            band_color: band.map(|b| b.to_lowercase()),
        }
    }

    fn obs(
        ref_id: &str,
        sex: Option<Sex>,
        bloodline: Option<&str>,
        band: Option<&str>,
    ) -> ObservedBird {
        ObservedBird {
            ref_id: ref_id.to_string(),
            sex,
            bloodline: bloodline.map(|s| s.to_string()),
            traits: ObservedTraits {
                band_color: band.map(|s| s.to_string()),
            },
        }
    }

    fn outcome_for<'a>(results: &'a [BirdResult], ref_id: &str) -> &'a MatchOutcome {
        &results
            .iter()
            .find(|r| r.ref_id == ref_id)
            .expect("result exists for ref")
            .outcome
    }

    /// One dropped tag, one untagged bird, clean single match → Resolved/Sole.
    #[test]
    fn clean_single_match_is_sole() {
        let tags = vec![tag("T1", Sex::Female, &["fernbank"], Some("red"))];
        let observed = vec![obs("b1", Some(Sex::Female), None, Some("red"))];
        let results = deduce(&tags, &observed);
        match outcome_for(&results, "b1") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T1");
                assert_eq!(*confidence, Confidence::Sole);
            }
            other => panic!("expected Resolved/Sole, got {other:?}"),
        }
    }

    /// Two female tags, two present females, distinguishable only by band
    /// colour. Band colour is *soft*, so neither resolves — both stay
    /// Ambiguous — but each ranks its colour-matching tag first.
    #[test]
    fn two_females_ranked_by_band_color() {
        let tags = vec![
            tag("T_RED", Sex::Female, &[], Some("red")),
            tag("T_BLUE", Sex::Female, &[], Some("blue")),
        ];
        let observed = vec![
            obs("b_red", Some(Sex::Female), None, Some("red")),
            obs("b_blue", Some(Sex::Female), None, Some("blue")),
        ];
        let results = deduce(&tags, &observed);

        match outcome_for(&results, "b_red") {
            MatchOutcome::Ambiguous { candidates } => {
                assert_eq!(
                    candidates[0].tag_id, "T_RED",
                    "red bird ranks red tag first"
                );
                assert!(candidates[0].score > candidates[1].score);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
        match outcome_for(&results, "b_blue") {
            MatchOutcome::Ambiguous { candidates } => {
                assert_eq!(
                    candidates[0].tag_id, "T_BLUE",
                    "blue bird ranks blue tag first"
                );
                assert!(candidates[0].score > candidates[1].score);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    /// Propagation: three tags, three birds. Birds A and C each start with a
    /// sole candidate; locking A removes the tag shared with B, forcing B down
    /// to one. Assert B is `Forced` while A and C are `Sole`.
    #[test]
    fn propagation_forces_downstream_lock() {
        let tags = vec![
            tag("T1", Sex::Female, &["x", "p"], None),
            tag("T2", Sex::Female, &["x", "q"], None),
            tag("T3", Sex::Female, &["y"], None),
        ];
        // A: bloodline "p" → only T1.  B: "x" → T1 & T2.  C: "y" → only T3.
        let observed = vec![
            obs("A", Some(Sex::Female), Some("p"), None),
            obs("B", Some(Sex::Female), Some("x"), None),
            obs("C", Some(Sex::Female), Some("y"), None),
        ];
        let results = deduce(&tags, &observed);

        match outcome_for(&results, "A") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T1");
                assert_eq!(*confidence, Confidence::Sole);
            }
            other => panic!("A: expected Resolved/Sole, got {other:?}"),
        }
        match outcome_for(&results, "B") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T2");
                assert_eq!(*confidence, Confidence::Forced, "B is forced by A's lock");
            }
            other => panic!("B: expected Resolved/Forced, got {other:?}"),
        }
        match outcome_for(&results, "C") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T3");
                assert_eq!(*confidence, Confidence::Sole);
            }
            other => panic!("C: expected Resolved/Sole, got {other:?}"),
        }
    }

    /// A sex mismatch disqualifies a tag even when every soft trait matches.
    #[test]
    fn sex_mismatch_eliminates_despite_identical_traits() {
        let tags = vec![tag("T1", Sex::Female, &["fernbank"], Some("red"))];
        // Observed male with otherwise-identical traits.
        let observed = vec![obs("b1", Some(Sex::Male), Some("fernbank"), Some("red"))];
        let results = deduce(&tags, &observed);
        assert!(
            matches!(outcome_for(&results, "b1"), MatchOutcome::NoCandidate),
            "sex mismatch must eliminate the only tag → NoCandidate"
        );
    }

    /// An `Unknown` sex observation never eliminates (same rule as a missing
    /// observation): the female tag survives.
    #[test]
    fn unknown_sex_does_not_eliminate() {
        let tags = vec![tag("T1", Sex::Female, &[], Some("red"))];
        let observed = vec![obs("b1", Some(Sex::Unknown), None, None)];
        let results = deduce(&tags, &observed);
        assert!(matches!(
            outcome_for(&results, "b1"),
            MatchOutcome::Resolved { .. }
        ));
    }

    /// The single-male short-circuit: a dropped male tag resolves directly to
    /// the one observed male as `Sole`, and is kept out of the female's
    /// candidate set entirely (even though she was reported `Unknown`, which
    /// would not otherwise eliminate it).
    #[test]
    fn single_male_short_circuit() {
        let tags = vec![
            tag("T_MALE", Sex::Male, &[], Some("green")),
            tag("T_HEN", Sex::Female, &[], Some("red")),
        ];
        let observed = vec![
            obs("rooster", Some(Sex::Male), None, None),
            obs("hen", Some(Sex::Unknown), None, None),
        ];
        let results = deduce(&tags, &observed);

        match outcome_for(&results, "rooster") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T_MALE");
                assert_eq!(*confidence, Confidence::Sole);
            }
            other => panic!("rooster: expected Resolved/Sole, got {other:?}"),
        }
        // The hen, despite Unknown sex, must NOT see the male tag.
        match outcome_for(&results, "hen") {
            MatchOutcome::Resolved { tag_id, confidence } => {
                assert_eq!(tag_id, "T_HEN");
                assert_eq!(*confidence, Confidence::Sole);
            }
            other => panic!("hen: expected Resolved/Sole on the lone hen tag, got {other:?}"),
        }
    }

    /// A tag whose bird isn't a member of this group lands in `unmatched_tags`,
    /// while in-group tags are kept for deduction.
    #[test]
    fn out_of_group_tag_is_unmatched() {
        let member_ids: HashSet<i64> = [10, 11].into_iter().collect();
        let in_group = tag("T_IN", Sex::Female, &[], None);
        let foreign = tag("T_FOREIGN", Sex::Female, &[], None);
        let loaded = vec![
            ("T_IN".to_string(), Some((11, in_group))),
            // resolves to bird 999, not in this group
            ("T_FOREIGN".to_string(), Some((999, foreign))),
            // resolves to no bird at all
            ("T_GHOST".to_string(), None),
        ];
        let (expected, unmatched) = partition_membership(loaded, &member_ids);
        assert_eq!(expected.len(), 1);
        assert_eq!(expected[0].tag_id, "T_IN");
        assert_eq!(
            unmatched,
            vec!["T_FOREIGN".to_string(), "T_GHOST".to_string()]
        );
    }

    // -----------------------------------------------------------------------
    // Boundary cases
    // -----------------------------------------------------------------------

    /// Pull the candidate list out of an `Ambiguous` outcome (or panic).
    fn candidates_of(o: &MatchOutcome) -> &[Candidate] {
        match o {
            MatchOutcome::Ambiguous { candidates } => candidates,
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    /// Pull (tag_id, confidence) out of a `Resolved` outcome (or panic).
    fn resolved(o: &MatchOutcome) -> (&str, Confidence) {
        match o {
            MatchOutcome::Resolved { tag_id, confidence } => (tag_id.as_str(), *confidence),
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    // --- Empty / degenerate inputs ---

    /// Nothing in, nothing out — no panic on empty slices.
    #[test]
    fn both_empty_yields_empty_results() {
        assert!(deduce(&[], &[]).is_empty());
    }

    /// Tags but no birds present → no per-bird results at all.
    #[test]
    fn no_observed_birds_yields_empty_results() {
        let tags = vec![tag("T1", Sex::Female, &[], Some("red"))];
        assert!(deduce(&tags, &[]).is_empty());
    }

    /// Birds present but no dropped tags → every bird is NoCandidate, and the
    /// result is still keyed in observed order.
    #[test]
    fn no_tags_yields_no_candidate_for_each_bird() {
        let observed = vec![
            obs("b1", Some(Sex::Female), None, Some("red")),
            obs("b2", Some(Sex::Male), None, None),
        ];
        let results = deduce(&[], &observed);
        assert_eq!(results.len(), 2);
        assert!(matches!(
            outcome_for(&results, "b1"),
            MatchOutcome::NoCandidate
        ));
        assert!(matches!(
            outcome_for(&results, "b2"),
            MatchOutcome::NoCandidate
        ));
    }

    // --- Cardinality mismatches ---

    /// More birds than tags: two identical hens, one band. One gets it
    /// (`Sole`), the other is left with nothing once it's locked — a clean
    /// degradation rather than a double-assignment.
    #[test]
    fn more_birds_than_tags_one_is_left_without() {
        let tags = vec![tag("T1", Sex::Female, &[], Some("red"))];
        let observed = vec![
            obs("b1", Some(Sex::Female), None, Some("red")),
            obs("b2", Some(Sex::Female), None, Some("red")),
        ];
        let results = deduce(&tags, &observed);
        // First in observed order wins the contested tag.
        assert_eq!(
            resolved(outcome_for(&results, "b1")),
            ("T1", Confidence::Sole)
        );
        assert!(matches!(
            outcome_for(&results, "b2"),
            MatchOutcome::NoCandidate
        ));
    }

    /// More tags than birds: one hen, two consistent bands. She can't be
    /// resolved — both remain candidates — and the spare tag simply isn't
    /// assigned to anyone (it doesn't appear in results).
    #[test]
    fn more_tags_than_birds_stays_ambiguous() {
        let tags = vec![
            tag("T_RED", Sex::Female, &[], Some("red")),
            tag("T_BLUE", Sex::Female, &[], Some("blue")),
        ];
        let observed = vec![obs("b1", Some(Sex::Female), None, Some("red"))];
        let results = deduce(&tags, &observed);
        assert_eq!(results.len(), 1);
        let cands = candidates_of(outcome_for(&results, "b1"));
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].tag_id, "T_RED", "colour match ranks first");
    }

    // --- Bloodline (lineage) elimination edge cases ---

    /// A named bloodline that the tag's bird provably lacks eliminates it.
    #[test]
    fn bloodline_mismatch_eliminates() {
        let tags = vec![tag("T1", Sex::Female, &["fernbank"], None)];
        let observed = vec![obs("b1", Some(Sex::Female), Some("nw-quail"), None)];
        assert!(matches!(
            outcome_for(&deduce(&tags, &observed), "b1"),
            MatchOutcome::NoCandidate
        ));
    }

    /// A bloodline that matches *one of several* lineages on the tag's bird
    /// keeps it in play.
    #[test]
    fn bloodline_match_among_multiple_lineages_survives() {
        let tags = vec![tag("T1", Sex::Female, &["fernbank", "nw-quail"], None)];
        let observed = vec![obs("b1", Some(Sex::Female), Some("nw-quail"), None)];
        assert_eq!(
            resolved(outcome_for(&deduce(&tags, &observed), "b1")).0,
            "T1"
        );
    }

    /// We can't prove a mismatch against a bird whose lineage set is unknown
    /// (empty), so a named bloodline does NOT eliminate it.
    #[test]
    fn bloodline_does_not_eliminate_when_tag_has_no_lineages() {
        let tags = vec![tag("T1", Sex::Female, &[], None)];
        let observed = vec![obs("b1", Some(Sex::Female), Some("anything"), None)];
        assert!(matches!(
            outcome_for(&deduce(&tags, &observed), "b1"),
            MatchOutcome::Resolved { .. }
        ));
    }

    /// Bloodline comparison is case- and whitespace-insensitive.
    #[test]
    fn bloodline_match_is_case_insensitive() {
        // tag() lowercases the stored lineage; the observed value is messy.
        let tags = vec![tag("T1", Sex::Female, &["Fernbank"], None)];
        let observed = vec![obs("b1", Some(Sex::Female), Some("  FERNBANK "), None)];
        assert_eq!(
            resolved(outcome_for(&deduce(&tags, &observed), "b1")).0,
            "T1"
        );
    }

    // --- Soft-trait ranking edge cases ---

    /// Band-colour ranking is case-insensitive: an observed "RED" scores a
    /// perfect match against a stored "red".
    #[test]
    fn band_color_ranking_is_case_insensitive() {
        let tags = vec![
            tag("T_RED", Sex::Female, &[], Some("red")),
            tag("T_BLUE", Sex::Female, &[], Some("blue")),
        ];
        let observed = vec![obs("b1", Some(Sex::Female), None, Some("RED"))];
        let results = deduce(&tags, &observed);
        let cands = candidates_of(outcome_for(&results, "b1"));
        assert_eq!(cands[0].tag_id, "T_RED");
        assert!((cands[0].score - 1.0).abs() < f64::EPSILON);
        assert!((cands[1].score - 0.0).abs() < f64::EPSILON);
    }

    /// With no distinguishing soft trait (no band colour observed), every
    /// candidate scores 0.0 and the order is a stable tag_id sort — not
    /// arbitrary.
    #[test]
    fn ambiguous_ties_break_on_tag_id() {
        let tags = vec![
            tag("Z_TAG", Sex::Female, &[], None),
            tag("A_TAG", Sex::Female, &[], None),
        ];
        let observed = vec![obs("b1", Some(Sex::Female), None, None)];
        let results = deduce(&tags, &observed);
        let cands = candidates_of(outcome_for(&results, "b1"));
        assert_eq!(cands.len(), 2);
        assert_eq!(cands[0].tag_id, "A_TAG", "ties sort by tag_id ascending");
        assert_eq!(cands[1].tag_id, "Z_TAG");
        assert!(cands.iter().all(|c| c.score == 0.0));
    }

    /// A blank/whitespace band-colour observation is treated as *absent* — it
    /// doesn't act as a colour and doesn't favour any candidate.
    #[test]
    fn blank_band_color_is_absent_not_a_value() {
        let tags = vec![
            tag("A_TAG", Sex::Female, &[], Some("red")),
            tag("B_TAG", Sex::Female, &[], Some("blue")),
        ];
        let observed = vec![obs("b1", Some(Sex::Female), None, Some("   "))];
        let results = deduce(&tags, &observed);
        let cands = candidates_of(outcome_for(&results, "b1"));
        assert!(cands.iter().all(|c| c.score == 0.0));
        assert_eq!(cands[0].tag_id, "A_TAG", "no colour signal → tag_id order");
    }

    // --- Male short-circuit edge cases ---

    /// Two male tags is a data anomaly (a group has one male), so the
    /// short-circuit does NOT fire: both observed males fall through to general
    /// deduction and stay Ambiguous between the two male tags.
    #[test]
    fn two_male_tags_do_not_short_circuit() {
        let tags = vec![
            tag("T_M1", Sex::Male, &[], Some("green")),
            tag("T_M2", Sex::Male, &[], Some("yellow")),
        ];
        let observed = vec![
            obs("m1", Some(Sex::Male), None, Some("green")),
            obs("m2", Some(Sex::Male), None, Some("yellow")),
        ];
        let results = deduce(&tags, &observed);
        assert_eq!(candidates_of(outcome_for(&results, "m1")).len(), 2);
        assert_eq!(candidates_of(outcome_for(&results, "m2")).len(), 2);
    }

    /// A male tag with no confidently-identified observed male (the rooster was
    /// logged as `Unknown`) must NOT be lost: the short-circuit declines and
    /// general deduction still places it.
    #[test]
    fn male_tag_without_observed_male_is_not_lost() {
        let tags = vec![tag("T_MALE", Sex::Male, &[], None)];
        let observed = vec![obs("b1", Some(Sex::Unknown), None, None)];
        assert_eq!(
            resolved(outcome_for(&deduce(&tags, &observed), "b1")),
            ("T_MALE", Confidence::Sole)
        );
    }

    /// One male tag but two observed males (anomaly): no short-circuit; one
    /// male is resolved and the other is left without.
    #[test]
    fn one_male_tag_two_observed_males_contend() {
        let tags = vec![tag("T_MALE", Sex::Male, &[], None)];
        let observed = vec![
            obs("m1", Some(Sex::Male), None, None),
            obs("m2", Some(Sex::Male), None, None),
        ];
        let results = deduce(&tags, &observed);
        assert_eq!(
            resolved(outcome_for(&results, "m1")),
            ("T_MALE", Confidence::Sole)
        );
        assert!(matches!(
            outcome_for(&results, "m2"),
            MatchOutcome::NoCandidate
        ));
    }

    /// A present male with no male tag dropped: the only tags are the hens',
    /// all eliminated by sex → NoCandidate (none of these bands are his).
    #[test]
    fn present_male_with_only_female_tags_has_no_candidate() {
        let tags = vec![tag("T_HEN", Sex::Female, &[], Some("red"))];
        let observed = vec![obs("rooster", Some(Sex::Male), None, None)];
        assert!(matches!(
            outcome_for(&deduce(&tags, &observed), "rooster"),
            MatchOutcome::NoCandidate
        ));
    }

    // --- Pure-helper boundaries ---

    /// Jaccard endpoints: empty/empty is 0.0 (no shared evidence), identical is
    /// 1.0, disjoint is 0.0, partial overlap is the exact ratio.
    #[test]
    fn jaccard_boundaries() {
        let empty: HashSet<String> = HashSet::new();
        assert_eq!(jaccard(&empty, &empty), 0.0);

        let a: HashSet<String> = ["x".to_string()].into_iter().collect();
        assert_eq!(jaccard(&a, &a), 1.0);

        let b: HashSet<String> = ["y".to_string()].into_iter().collect();
        assert_eq!(jaccard(&a, &b), 0.0);

        let p: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let q: HashSet<String> = ["b", "c"].iter().map(|s| s.to_string()).collect();
        // share {b} of {a,b,c}
        assert!((jaccard(&p, &q) - 1.0 / 3.0).abs() < f64::EPSILON);
    }

    /// Partitioning an empty load list yields two empty vecs (no panic).
    #[test]
    fn partition_membership_empty() {
        let (expected, unmatched) = partition_membership(vec![], &HashSet::new());
        assert!(expected.is_empty());
        assert!(unmatched.is_empty());
    }
}
