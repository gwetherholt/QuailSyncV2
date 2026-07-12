//! Integration tests for the indoor-camera assignment endpoints
//! (`GET`/`PUT /api/cameras/{id}/assignment`).
//!
//! Same black-box style as `api_tests.rs`: a real server on a random port over a
//! fresh in-memory DB seeded by `init_db` (which seeds `indoor_tapo` →
//! `incubator`).

use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::CameraAssignmentDto;
use quailsync_server::{build_app, init_db, AppState};
use rusqlite::Connection;
use serde_json::json;

/// Spin up a test server on a random port with a fresh in-memory DB.
async fn spawn_test_server() -> String {
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

    format!("http://{addr}")
}

#[tokio::test]
async fn get_returns_seeded_default_incubator() {
    let base = spawn_test_server().await;

    let resp = reqwest::get(format!("{base}/api/cameras/indoor_tapo/assignment"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let dto: CameraAssignmentDto = resp.json().await.unwrap();
    assert_eq!(dto.camera_id, "indoor_tapo");
    assert_eq!(dto.assignment, "incubator");
    assert_eq!(dto.active_model, "incubation"); // derived
    assert!(!dto.updated_at.is_empty());
}

#[tokio::test]
async fn get_unknown_camera_is_404() {
    let base = spawn_test_server().await;
    let resp = reqwest::get(format!("{base}/api/cameras/nope/assignment"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn put_switches_incubator_to_brooder_and_model_follows() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // incubator -> brooder
    let resp = client
        .put(format!("{base}/api/cameras/indoor_tapo/assignment"))
        .json(&json!({ "assignment": "brooder" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let dto: CameraAssignmentDto = resp.json().await.unwrap();
    assert_eq!(dto.assignment, "brooder");
    assert_eq!(dto.active_model, "chick"); // model follows the assignment

    // A follow-up GET reflects the persisted switch.
    let got: CameraAssignmentDto =
        reqwest::get(format!("{base}/api/cameras/indoor_tapo/assignment"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(got.assignment, "brooder");
    assert_eq!(got.active_model, "chick");

    // brooder -> incubator (switch back)
    let dto: CameraAssignmentDto = client
        .put(format!("{base}/api/cameras/indoor_tapo/assignment"))
        .json(&json!({ "assignment": "incubator" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(dto.assignment, "incubator");
    assert_eq!(dto.active_model, "incubation");
}

#[tokio::test]
async fn put_updates_the_updated_at_timestamp() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    let before: CameraAssignmentDto =
        reqwest::get(format!("{base}/api/cameras/indoor_tapo/assignment"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

    let after: CameraAssignmentDto = client
        .put(format!("{base}/api/cameras/indoor_tapo/assignment"))
        .json(&json!({ "assignment": "brooder" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // updated_at is refreshed on write (>= the previous value, ISO-8601 sorts).
    assert!(after.updated_at >= before.updated_at);
}

#[tokio::test]
async fn put_garbage_value_is_400_and_does_not_change_state() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{base}/api/cameras/indoor_tapo/assignment"))
        .json(&json!({ "assignment": "hutch" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);

    // State is unchanged — still the seeded default.
    let got: CameraAssignmentDto =
        reqwest::get(format!("{base}/api/cameras/indoor_tapo/assignment"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert_eq!(got.assignment, "incubator");
}
