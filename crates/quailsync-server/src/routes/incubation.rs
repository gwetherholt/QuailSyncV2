//! Incubation-event read endpoints (SQLite-backed).
//!
//! The stage-1 incubator capture pipeline (separate process, see `incubator/`)
//! is the ONLY writer of the `incubation_events` table: a Python sidecar watches
//! per-slot ROIs over the incubator tray, runs frame-difference detection, and
//! inserts one `change_detected` row per event. The backend owns the schema (see
//! `db::init_db`) and exposes read-only aggregates over it here — it never writes
//! this table.
//!
//! These handlers are reads only: single short `SELECT`s, no transactions and no
//! long-held locks, since the DB is shared with the sidecar under WAL.
//!
//! `clutch_id` is nullable and static-null today; the per-clutch breakdown in
//! the summary is derived purely from rows where `clutch_id IS NOT NULL`, so it's
//! empty now and populates for free once slots carry clutch ids.

use axum::extract::{Query, State};
use axum::Json;
use rusqlite::params;

use quailsync_common::{
    ClutchActivityDto, IncubationEventDto, IncubationSummaryDto, SlotActivityDto,
};

use crate::state::{acquire_db, AppState};

/// Newest-first list, default 100 rows, hard cap 500.
const DEFAULT_EVENT_LIMIT: i64 = 100;
const MAX_EVENT_LIMIT: i64 = 500;

/// Default aggregation window for `/summary` when `window_hours` is omitted.
const DEFAULT_WINDOW_HOURS: u32 = 24;
/// Guard rail so a bogus `window_hours` can't ask for an unbounded span.
const MAX_WINDOW_HOURS: u32 = 24 * 365;

#[derive(serde::Deserialize)]
pub(crate) struct ListEventsQuery {
    /// ISO-8601 lower bound (inclusive) on `created_at`. Optional.
    since: Option<String>,
    /// Restrict to a single tray slot. Optional.
    slot_id: Option<String>,
    /// Max rows (default 100, capped at 500). Optional.
    limit: Option<i64>,
}

/// `GET /api/incubation/events` — newest-first list of change events, optionally
/// filtered by `since` (ISO-8601, inclusive) and `slot_id`.
pub(crate) async fn list_events(
    State(state): State<AppState>,
    Query(q): Query<ListEventsQuery>,
) -> Json<Vec<IncubationEventDto>> {
    let limit = q
        .limit
        .unwrap_or(DEFAULT_EVENT_LIMIT)
        .clamp(1, MAX_EVENT_LIMIT);
    let conn = acquire_db(&state);

    // Both filters are optional and applied via `?N IS NULL OR …` so a single
    // prepared statement covers every combination. `created_at` is stored as an
    // ISO-8601 `…Z` string, so a lexicographic `>=` orders/filters correctly.
    let rows: Vec<IncubationEventDto> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, slot_id, event_type, diff_score, high_threshold,
                        clutch_id, frame_path, created_at
                 FROM incubation_events
                 WHERE (?1 IS NULL OR created_at >= ?1)
                   AND (?2 IS NULL OR slot_id = ?2)
                 ORDER BY created_at DESC, id DESC
                 LIMIT ?3",
            )
            .expect("prepare failed");
        stmt.query_map(params![q.since, q.slot_id, limit], |r| {
            Ok(IncubationEventDto {
                id: r.get(0)?,
                slot_id: r.get(1)?,
                event_type: r.get(2)?,
                diff_score: r.get(3)?,
                high_threshold: r.get(4)?,
                clutch_id: r.get(5)?,
                frame_path: r.get(6)?,
                created_at: r.get(7)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    Json(rows)
}

#[derive(serde::Deserialize)]
pub(crate) struct SummaryQuery {
    /// Rolling window in hours (default 24). Optional.
    window_hours: Option<u32>,
}

/// `GET /api/incubation/summary` — aggregate change activity over the last
/// `window_hours` (default 24): a total, a per-slot breakdown, and a per-clutch
/// breakdown (non-null `clutch_id` only). Reflects `change_detected` events
/// only — slot active/quiet state is not in this table.
pub(crate) async fn summary(
    State(state): State<AppState>,
    Query(q): Query<SummaryQuery>,
) -> Json<IncubationSummaryDto> {
    let window_hours = q
        .window_hours
        .unwrap_or(DEFAULT_WINDOW_HOURS)
        .clamp(1, MAX_WINDOW_HOURS);
    let modifier = format!("-{window_hours} hours");
    let conn = acquire_db(&state);

    // Compute the window cutoff once, in the same ISO-8601 `…Z` format the rows
    // are stored in, so the `created_at >= cutoff` comparisons are lexicographic.
    let cutoff: String = conn
        .query_row(
            "SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now',?1)",
            params![modifier],
            |r| r.get(0),
        )
        .expect("cutoff");

    let total_events: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM incubation_events WHERE created_at >= ?1",
            params![cutoff],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Per-slot aggregation. `last_event_at` / `last_diff_score` come from each
    // slot's most-recent row, picked explicitly with ROW_NUMBER() ordered by
    // created_at DESC then id DESC — id is monotonic and unique, so it breaks any
    // same-timestamp tie deterministically. This does not rely on SQLite's
    // bare-column-with-MAX() behaviour. COUNT() OVER the partition is the total.
    let slots: Vec<SlotActivityDto> = {
        let mut stmt = conn
            .prepare(
                "SELECT slot_id, event_count, last_event_at, last_diff_score
                 FROM (
                     SELECT
                         slot_id,
                         COUNT(*) OVER (PARTITION BY slot_id) AS event_count,
                         created_at AS last_event_at,
                         diff_score AS last_diff_score,
                         ROW_NUMBER() OVER (
                             PARTITION BY slot_id
                             ORDER BY created_at DESC, id DESC
                         ) AS rn
                     FROM incubation_events
                     WHERE created_at >= ?1
                 )
                 WHERE rn = 1
                 ORDER BY last_event_at DESC, slot_id ASC",
            )
            .expect("prepare failed");
        stmt.query_map(params![cutoff], |r| {
            Ok(SlotActivityDto {
                slot_id: r.get(0)?,
                event_count: r.get(1)?,
                last_event_at: r.get(2)?,
                last_diff_score: r.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    // Per-clutch aggregation — non-null clutch_id only. Empty today.
    let clutches: Vec<ClutchActivityDto> = {
        let mut stmt = conn
            .prepare(
                "SELECT clutch_id, COUNT(*), MAX(created_at)
                 FROM incubation_events
                 WHERE created_at >= ?1 AND clutch_id IS NOT NULL
                 GROUP BY clutch_id
                 ORDER BY MAX(created_at) DESC",
            )
            .expect("prepare failed");
        stmt.query_map(params![cutoff], |r| {
            Ok(ClutchActivityDto {
                clutch_id: r.get(0)?,
                event_count: r.get(1)?,
                last_event_at: r.get(2)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    Json(IncubationSummaryDto {
        window_hours,
        total_events,
        slots,
        clutches,
    })
}
