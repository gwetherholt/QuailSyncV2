//! Shared harness for the integration tests.
//!
//! Cargo compiles this module into every test binary that declares `mod
//! common;`, and no single binary uses all of it, so `dead_code` is allowed
//! here rather than at each call site.
//!
//! Two groups of helpers:
//!
//! * **Harness + seeds** — spin up a server, insert the fixture rows a test
//!   needs. Extracted from the copies that had accumulated in `api_tests.rs`
//!   and `photo_upload_tests.rs`.
//! * **Contract assertions** — `media_type`, `assert_json_response`,
//!   `assert_entity_response`, `assert_put_matches_get`. These encode the
//!   properties the KAN-26/27/28/29 breaks slipped through: a route that is
//!   reachable, answers with the content-type its clients parse, and whose
//!   updaters return the same entity their GET does.

#![allow(dead_code)]

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{
    Bird, BirdStatus, ChickGroup, Clutch, ClutchStatus, CreateBird, CreateBrooder,
    CreateChickGroup, CreateClutch, CreateLineage, HousingType, LifeStage, Lineage, Sex,
};
use quailsync_server::state::PhotoConfig;
use quailsync_server::{build_app, init_db, AppState};
use rusqlite::Connection;

// ===========================================================================
// Harness
// ===========================================================================

pub fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Spin up a test server on a random port with a fresh in-memory DB.
/// Returns the base URL (e.g. "http://127.0.0.1:12345").
pub async fn spawn_test_server() -> String {
    spawn_test_server_with_photos(PhotoConfig::for_dir(
        std::env::temp_dir().join("quailsync-test-photos"),
    ))
    .await
}

/// As [`spawn_test_server`], but with a caller-supplied photo config — needed
/// by the photo-upload tests, which assert on files landing in their own dir.
pub async fn spawn_test_server_with_photos(photos: PhotoConfig) -> String {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_db(&conn);

    let (live_tx, _) = tokio::sync::broadcast::channel::<String>(64);

    let metrics_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
        settings: Arc::new(std::sync::RwLock::new(quailsync_common::Settings::default())),
        live_tx,
        last_seen: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        metrics_handle,
        photos,
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

    format!("http://{addr}")
}

// ===========================================================================
// Seeds
//
// Each returns the created row's id. Callers that care about a specific
// fixture value (a lineage name, a bird's sex) pass it in, so migrating a
// hand-rolled seed to one of these never changes what the test inserts.
// ===========================================================================

pub async fn seed_lineage(base: &str, client: &reqwest::Client, name: &str) -> i64 {
    let lineage: Lineage = client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: name.into(),
            source: "S".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    lineage.id
}

/// As [`seed_lineage`] but with an explicit `source`, for the fixtures that
/// set one.
pub async fn seed_lineage_with_source(
    base: &str,
    client: &reqwest::Client,
    name: &str,
    source: &str,
) -> i64 {
    let lineage: Lineage = client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: name.into(),
            source: source.into(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    lineage.id
}

/// A minimal active bird. `lineage_ids` must be non-empty — the handler
/// rejects an empty set with 400.
pub async fn seed_bird(
    base: &str,
    client: &reqwest::Client,
    lineage_ids: Vec<i64>,
    sex: Sex,
) -> i64 {
    let bird: Bird = client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex,
            lineage_ids,
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 1,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    bird.id
}

pub async fn seed_chick_group(base: &str, client: &reqwest::Client, lineage_ids: Vec<i64>) -> i64 {
    let group: ChickGroup = client
        .post(format!("{base}/api/chick-groups"))
        .json(&CreateChickGroup {
            clutch_id: None,
            lineage_ids,
            brooder_id: None,
            initial_count: 10,
            hatch_date: chrono::Local::now().date_naive(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    group.id
}

pub async fn seed_brooder(
    base: &str,
    client: &reqwest::Client,
    name: &str,
    qr_code: &str,
    housing_type: HousingType,
) -> i64 {
    let life_stage = match housing_type {
        HousingType::Hutch => LifeStage::Adult,
        _ => LifeStage::Chick,
    };
    let brooder: serde_json::Value = client
        .post(format!("{base}/api/brooders"))
        .json(&CreateBrooder {
            name: name.into(),
            lineage_id: None,
            life_stage,
            qr_code: qr_code.into(),
            notes: None,
            camera_url: None,
            housing_type: Some(housing_type),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    brooder["id"].as_i64().expect("brooder id")
}

pub async fn seed_clutch(base: &str, client: &reqwest::Client, lineage_id: Option<i64>) -> i64 {
    let clutch: Clutch = client
        .post(format!("{base}/api/clutches"))
        .json(&CreateClutch {
            breeding_group_id: None,
            lineage_id,
            eggs_set: 12,
            eggs_fertile: None,
            eggs_hatched: None,
            set_date: chrono::Local::now().date_naive(),
            status: ClutchStatus::Incubating,
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    clutch.id
}

// ===========================================================================
// Contract assertions
// ===========================================================================

/// The media type alone, with any `; charset=...` stripped. Comparing the raw
/// header is brittle: axum appends a charset to some responses and not others.
pub fn media_type(resp: &reqwest::Response) -> String {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Assert the response has `expected_status` and a JSON content-type, and
/// return the parsed body.
///
/// The content-type half is the KAN-26 lesson: the SPA fallback used to answer
/// unmatched `/api/*` paths with `200` + `index.html`, so a status-only check
/// passed on a route that did not exist.
pub async fn assert_json_response(
    resp: reqwest::Response,
    expected_status: u16,
    label: &str,
) -> serde_json::Value {
    let status = resp.status().as_u16();
    let mime = media_type(&resp);
    let body = resp.text().await.unwrap_or_default();
    assert_eq!(
        status, expected_status,
        "{label}: expected status {expected_status}, got {status}; body: {body}"
    );
    assert_eq!(
        mime, "application/json",
        "{label}: expected application/json, got '{mime}'; body: {body}"
    );
    serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("{label}: body was not valid JSON ({e}); body: {body}"))
}

/// Assert a 200 + JSON response whose `id` is `expected_id`, and return the
/// body. This is the shape every entity-returning handler owes its clients.
pub async fn assert_entity_response(
    resp: reqwest::Response,
    expected_id: i64,
    label: &str,
) -> serde_json::Value {
    let body = assert_json_response(resp, 200, label).await;
    assert_eq!(
        body["id"].as_i64(),
        Some(expected_id),
        "{label}: body id should be {expected_id}; body: {body}"
    );
    body
}

/// PUT `path` with `body`, then GET the same `path`, and assert the two
/// responses are identical.
///
/// This is the KAN-29 pin. `update_brooder` and `update_chick_group` used to
/// answer a bare `200` with no body, so clients could not distinguish a
/// successful write from a failed one. Requiring the PUT response to equal the
/// GET response catches both that and any future divergence in shape.
pub async fn assert_put_matches_get(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &serde_json::Value,
) {
    let label = format!("PUT {path}");

    let put = client
        .put(format!("{base}{path}"))
        .json(body)
        .send()
        .await
        .unwrap_or_else(|e| panic!("{label}: request failed: {e}"));
    let put_body = assert_json_response(put, 200, &label).await;

    let get = client
        .get(format!("{base}{path}"))
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {path}: request failed: {e}"));
    let get_body = assert_json_response(get, 200, &format!("GET {path}")).await;

    assert_eq!(
        put_body, get_body,
        "{label}: PUT response diverges from GET {path}.\n  PUT: {put_body}\n  GET: {get_body}"
    );
}
