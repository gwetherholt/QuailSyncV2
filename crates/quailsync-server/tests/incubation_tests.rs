//! Integration tests for the read-only incubation endpoints
//! (`GET /api/incubation/events`, `GET /api/incubation/summary`).
//!
//! Same black-box style as `api_tests.rs`: a real server on a random port over a
//! fresh in-memory DB. There is no write endpoint for `incubation_events` (the
//! Python sidecar is the only writer), so these tests seed rows directly through
//! a clone of the server's DB handle before hitting the HTTP API.

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{IncubationEventDto, IncubationSummaryDto};
use quailsync_server::{build_app, init_db, AppState};
use rusqlite::{params, Connection};

/// Spin up a test server and return `(base_url, db_handle)`. The returned handle
/// is a clone of the server's `Arc<Mutex<Connection>>` so a test can seed
/// `incubation_events` rows the way the sidecar would.
async fn spawn() -> (String, Arc<Mutex<Connection>>) {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_db(&conn);
    let db = Arc::new(Mutex::new(conn));

    let (live_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let metrics_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();

    let state = AppState {
        db: db.clone(),
        agent_connected: Arc::new(AtomicBool::new(false)),
        settings: Arc::new(std::sync::RwLock::new(quailsync_common::Settings::default())),
        live_tx,
        last_seen: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        metrics_handle,
        photos: quailsync_server::state::PhotoConfig::for_dir(
            std::env::temp_dir().join("quailsync-test-photos"),
        ),
        trailcam: quailsync_server::state::TrailcamConfig::for_dir(
            std::env::temp_dir().join("quailsync-test-trailcam"),
        ),
        indoorcam: quailsync_server::state::IndoorcamConfig::for_dir(
            std::env::temp_dir().join("quailsync-test-indoorcam"),
        ),
    };

    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to random port");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (format!("http://{addr}"), db)
}

/// Insert one event whose `created_at` is `now + modifier` (e.g. `"-2 hours"`),
/// so window/ordering assertions don't depend on the absolute wall clock.
fn insert_event(
    db: &Arc<Mutex<Connection>>,
    slot_id: &str,
    diff_score: f64,
    clutch_id: Option<i64>,
    modifier: &str,
) {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO incubation_events
             (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path, created_at)
         VALUES (?1, 'change_detected', ?2, 18.0, ?3, NULL,
                 strftime('%Y-%m-%dT%H:%M:%fZ','now',?4))",
        params![slot_id, diff_score, clutch_id, modifier],
    )
    .unwrap();
}

/// Insert one event with an EXPLICIT `created_at` string (not derived from
/// `now`), so two rows can be given the exact same timestamp — used to prove the
/// summary's `id` tie-break.
fn insert_event_at(
    db: &Arc<Mutex<Connection>>,
    slot_id: &str,
    diff_score: f64,
    clutch_id: Option<i64>,
    created_at: &str,
) {
    let conn = db.lock().unwrap();
    conn.execute(
        "INSERT INTO incubation_events
             (slot_id, event_type, diff_score, high_threshold, clutch_id, frame_path, created_at)
         VALUES (?1, 'change_detected', ?2, 18.0, ?3, NULL, ?4)",
        params![slot_id, diff_score, clutch_id, created_at],
    )
    .unwrap();
}

/// `now + modifier` in the same ISO-8601 format rows are stored in — used to
/// build a `since` boundary for the events filter test.
fn timestamp_at(db: &Arc<Mutex<Connection>>, modifier: &str) -> String {
    let conn = db.lock().unwrap();
    conn.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%fZ','now',?1)",
        params![modifier],
        |r| r.get(0),
    )
    .unwrap()
}

// ---------------------------------------------------------------------------
// GET /api/incubation/events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn events_newest_first_with_slot_and_since_filters() {
    let (base, db) = spawn().await;
    // A1 at -3h and -1h; B2 at -2h with a populated clutch_id.
    insert_event(&db, "A1", 10.0, None, "-3 hours");
    insert_event(&db, "B2", 20.0, Some(5), "-2 hours");
    insert_event(&db, "A1", 30.0, None, "-1 hours");

    let client = reqwest::Client::new();

    // Full list, newest first.
    let all: Vec<IncubationEventDto> = reqwest::get(format!("{base}/api/incubation/events"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].slot_id, "A1");
    assert_eq!(all[0].diff_score, 30.0); // -1h, most recent
    assert_eq!(all[0].event_type, "change_detected");
    assert!(all[0].frame_path.is_none());
    assert_eq!(all[1].slot_id, "B2");
    assert_eq!(all[1].clutch_id, Some(5)); // nullable column surfaced
    assert_eq!(all[2].diff_score, 10.0); // -3h, oldest
    assert!(all[0].created_at >= all[1].created_at);
    assert!(all[1].created_at >= all[2].created_at);

    // slot_id filter.
    let a1: Vec<IncubationEventDto> = client
        .get(format!("{base}/api/incubation/events"))
        .query(&[("slot_id", "A1")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(a1.len(), 2);
    assert!(a1.iter().all(|e| e.slot_id == "A1"));
    assert!(a1.iter().all(|e| e.clutch_id.is_none()));

    // since filter: a boundary between the -3h and -2h rows excludes the oldest.
    let since = timestamp_at(&db, "-150 minutes");
    let recent: Vec<IncubationEventDto> = client
        .get(format!("{base}/api/incubation/events"))
        .query(&[("since", since.as_str())])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(recent.len(), 2);
    assert!(recent.iter().all(|e| e.created_at >= since));

    // explicit small limit is honoured.
    let one: Vec<IncubationEventDto> = client
        .get(format!("{base}/api/incubation/events"))
        .query(&[("limit", "1")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].diff_score, 30.0);
}

#[tokio::test]
async fn events_limit_is_capped_at_500() {
    let (base, db) = spawn().await;
    // 501 rows, all within the last ~8.5h so none are windowed out.
    for i in 1..=501 {
        insert_event(&db, "A1", i as f64, None, &format!("-{i} minutes"));
    }
    let capped: Vec<IncubationEventDto> = reqwest::Client::new()
        .get(format!("{base}/api/incubation/events"))
        .query(&[("limit", "1000")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(capped.len(), 500);
}

#[tokio::test]
async fn events_default_limit_is_100() {
    let (base, db) = spawn().await;
    for i in 1..=150 {
        insert_event(&db, "A1", i as f64, None, &format!("-{i} minutes"));
    }
    let list: Vec<IncubationEventDto> = reqwest::get(format!("{base}/api/incubation/events"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 100);
}

// ---------------------------------------------------------------------------
// GET /api/incubation/summary
// ---------------------------------------------------------------------------

#[tokio::test]
async fn summary_counts_and_per_slot_aggregation() {
    let (base, db) = spawn().await;
    // A1: two events (-3h diff 10, -1h diff 30). B2: one event (-2h diff 20).
    insert_event(&db, "A1", 10.0, None, "-3 hours");
    insert_event(&db, "B2", 20.0, None, "-2 hours");
    insert_event(&db, "A1", 30.0, None, "-1 hours");

    let s: IncubationSummaryDto = reqwest::get(format!("{base}/api/incubation/summary"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(s.window_hours, 24);
    assert_eq!(s.total_events, 3);
    assert_eq!(s.slots.len(), 2);
    // Ordered by last_event_at desc: A1 (latest -1h) before B2 (latest -2h).
    assert_eq!(s.slots[0].slot_id, "A1");
    assert_eq!(s.slots[0].event_count, 2);
    assert_eq!(s.slots[0].last_diff_score, 30.0); // latest A1 event's score
    assert_eq!(s.slots[1].slot_id, "B2");
    assert_eq!(s.slots[1].event_count, 1);
    assert_eq!(s.slots[1].last_diff_score, 20.0);
    assert!(s.slots[0].last_event_at >= s.slots[1].last_event_at);
    // No clutch ids populated -> clutch breakdown is empty.
    assert!(s.clutches.is_empty());
}

#[tokio::test]
async fn summary_last_diff_score_breaks_tie_on_id() {
    let (base, db) = spawn().await;
    // Two A1 events sharing the EXACT same created_at: the later insert has the
    // higher (monotonic, unique) id, so it must win last_diff_score — proving the
    // `ORDER BY created_at DESC, id DESC` tie-break, not bare-column luck.
    let ts = timestamp_at(&db, "-1 hours");
    insert_event_at(&db, "A1", 11.0, None, &ts); // earlier row, lower id
    insert_event_at(&db, "A1", 22.0, None, &ts); // later row, higher id

    let s: IncubationSummaryDto = reqwest::get(format!("{base}/api/incubation/summary"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(s.total_events, 2);
    assert_eq!(s.slots.len(), 1);
    assert_eq!(s.slots[0].event_count, 2);
    assert_eq!(s.slots[0].last_event_at, ts);
    assert_eq!(s.slots[0].last_diff_score, 22.0); // higher-id row wins the tie
}

#[tokio::test]
async fn summary_clutches_empty_when_all_null_and_populated_when_set() {
    let (base, db) = spawn().await;
    insert_event(&db, "A1", 10.0, None, "-2 hours"); // null clutch
    insert_event(&db, "A2", 15.0, Some(7), "-90 minutes");
    insert_event(&db, "A2", 25.0, Some(7), "-30 minutes");

    let s: IncubationSummaryDto = reqwest::get(format!("{base}/api/incubation/summary"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(s.total_events, 3);
    // Only the two clutch_id=7 rows feed the clutch breakdown.
    assert_eq!(s.clutches.len(), 1);
    assert_eq!(s.clutches[0].clutch_id, 7);
    assert_eq!(s.clutches[0].event_count, 2);
}

#[tokio::test]
async fn summary_window_excludes_events_outside_it() {
    let (base, db) = spawn().await;
    insert_event(&db, "A1", 10.0, None, "-1 hours"); // inside 24h
    insert_event(&db, "A1", 20.0, None, "-100 hours"); // outside 24h, inside 200h

    let client = reqwest::Client::new();

    let s24: IncubationSummaryDto = client
        .get(format!("{base}/api/incubation/summary"))
        .query(&[("window_hours", "24")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s24.window_hours, 24);
    assert_eq!(s24.total_events, 1);
    assert_eq!(s24.slots.len(), 1);
    assert_eq!(s24.slots[0].event_count, 1);

    let s200: IncubationSummaryDto = client
        .get(format!("{base}/api/incubation/summary"))
        .query(&[("window_hours", "200")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s200.window_hours, 200);
    assert_eq!(s200.total_events, 2);
    assert_eq!(s200.slots[0].event_count, 2);
}

#[tokio::test]
async fn summary_empty_db_is_zeroed() {
    let (base, _db) = spawn().await;
    let s: IncubationSummaryDto = reqwest::get(format!("{base}/api/incubation/summary"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s.total_events, 0);
    assert!(s.slots.is_empty());
    assert!(s.clutches.is_empty());
}
