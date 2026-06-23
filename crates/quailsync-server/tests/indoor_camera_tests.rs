//! Integration tests for the indoor-camera subsystem:
//!   * observation ingest/read: `POST /api/indoorcam/observation`,
//!     `GET /api/indoorcam/{latest,history,cameras}/…`, image serving
//!   * registry/CRUD + assignment under `/api/indoor-cameras`, including the
//!     brooder/incubator-only scope (hutch targets are rejected).
//!
//! Mirrors `trailcam_tests.rs`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use quailsync_common::{CreateBrooder, HousingType, LifeStage};
use quailsync_server::state::{IndoorcamConfig, PhotoConfig, TrailcamConfig};
use quailsync_server::{build_app, init_db, AppState};
use reqwest::StatusCode;
use rusqlite::Connection;
use serde_json::{json, Value};

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, created temp dir to act as the indoor-cam `processed/` root.
fn unique_processed_dir() -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("qs-indoorcam-test-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn spawn_app(processed_dir: &Path) -> String {
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
        photos: PhotoConfig::for_dir(std::env::temp_dir().join("qs-indoorcam-test-photos")),
        trailcam: TrailcamConfig::for_dir(std::env::temp_dir().join("qs-indoorcam-test-trailcam")),
        indoorcam: IndoorcamConfig::for_dir(processed_dir.to_path_buf()),
    };

    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// POST one observation (mirroring the bridge payload). Asserts 201.
#[allow(clippy::too_many_arguments)]
async fn post_observation(
    base: &str,
    client: &reqwest::Client,
    camera_id: &str,
    timestamp: &str,
    detection_count: i64,
    avg_conf: f64,
    image_filename: &str,
    annotated_image_filename: Option<&str>,
) {
    let mut body = json!({
        "camera_id": camera_id,
        "timestamp": timestamp,
        "detection_count": detection_count,
        "average_confidence": avg_conf,
        "min_confidence": avg_conf,
        "detections": [{"class_name": "quail", "confidence": avg_conf, "bbox": [10.0, 10.0, 20.0, 20.0]}],
        "inference_time_ms": 5.0,
        "image_filename": image_filename,
    });
    if let Some(a) = annotated_image_filename {
        body["annotated_image_filename"] = json!(a);
    }
    let resp = client
        .post(format!("{base}/api/indoorcam/observation"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "observation POST failed"
    );
}

fn write_image(processed_dir: &Path, camera_id: &str, filename: &str, bytes: &[u8]) {
    let dir = processed_dir.join(camera_id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(filename), bytes).unwrap();
}

/// Create a housing unit of the given type; returns its id.
async fn create_unit(
    base: &str,
    client: &reqwest::Client,
    name: &str,
    housing: HousingType,
) -> i64 {
    let unit: Value = client
        .post(format!("{base}/api/brooders"))
        .json(&CreateBrooder {
            name: name.into(),
            lineage_id: None,
            life_stage: LifeStage::Chick,
            qr_code: String::new(),
            notes: None,
            camera_url: None,
            housing_type: Some(housing),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    unit["id"].as_i64().unwrap()
}

// ---------------------------------------------------------------------------
// observation read: latest / cameras / history / images
// ---------------------------------------------------------------------------

#[tokio::test]
async fn latest_returns_most_recent_for_camera() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T07:30:00",
        5,
        0.7,
        "c.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camB",
        "2026-06-15T08:00:00",
        1,
        0.9,
        "b.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T05:00:00",
        3,
        0.8,
        "a.jpg",
        None,
    )
    .await;

    let body: Value = c
        .get(format!("{base}/api/indoorcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["camera_id"], "camA");
    assert_eq!(body["detection_count"].as_i64(), Some(5));
    assert_eq!(body["timestamp"], "2026-06-15T07:30:00");
    assert_eq!(body["confidence_avg"].as_f64(), Some(0.7));
    assert_eq!(body["detections"].as_array().unwrap().len(), 1);
    assert_eq!(body["image_url"], "/api/indoorcam/image/camA/c.jpg");
}

#[tokio::test]
async fn latest_404_for_unknown_camera_and_empty_db() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    // Empty DB -> 404.
    let resp = c
        .get(format!("{base}/api/indoorcam/latest/camA"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T05:00:00",
        3,
        0.8,
        "x.jpg",
        None,
    )
    .await;
    let resp = c
        .get(format!("{base}/api/indoorcam/latest/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn annotated_url_present_only_when_file_on_disk() {
    let processed = unique_processed_dir();
    // The annotated copy exists for camA but not camB.
    write_image(&processed, "camA", "a_annotated.jpg", &[0xFF, 0xD8, 0xFF]);
    let base = spawn_app(&processed).await;
    let c = client();
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T07:30:00",
        2,
        0.8,
        "a.jpg",
        Some("a_annotated.jpg"),
    )
    .await;
    post_observation(
        &base,
        &c,
        "camB",
        "2026-06-15T07:30:00",
        2,
        0.8,
        "b.jpg",
        Some("b_annotated.jpg"),
    )
    .await;

    let a: Value = c
        .get(format!("{base}/api/indoorcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        a["annotated_image_url"],
        "/api/indoorcam/image/camA/a_annotated.jpg"
    );

    let b: Value = c
        .get(format!("{base}/api/indoorcam/latest/camB"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(b["annotated_image_url"].is_null());
}

#[tokio::test]
async fn cameras_lists_distinct_in_first_appearance_order() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    post_observation(
        &base,
        &c,
        "camB",
        "2026-06-15T05:00:00",
        1,
        0.9,
        "b1.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T05:30:00",
        2,
        0.8,
        "a1.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camB",
        "2026-06-15T06:00:00",
        3,
        0.7,
        "b2.jpg",
        None,
    )
    .await;

    let cams: Value = c
        .get(format!("{base}/api/indoorcam/cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = cams.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["camera_id"], "camB");
    assert_eq!(arr[0]["label"], "Indoor Cam 1");
    assert_eq!(arr[1]["camera_id"], "camA");
    assert_eq!(arr[1]["label"], "Indoor Cam 2");
}

#[tokio::test]
async fn history_returns_camera_observations_oldest_first() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T05:00:00",
        2,
        0.8,
        "a1.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T06:00:00",
        4,
        0.9,
        "a2.jpg",
        None,
    )
    .await;
    post_observation(
        &base,
        &c,
        "camB",
        "2026-06-15T06:00:00",
        1,
        0.7,
        "b1.jpg",
        None,
    )
    .await;

    let hist: Value = c
        .get(format!("{base}/api/indoorcam/history/camA?hours=24"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = hist.as_array().unwrap();
    assert_eq!(arr.len(), 2, "only camA's two observations");
    assert_eq!(arr[0]["detection_count"].as_i64(), Some(2));
    assert_eq!(arr[1]["detection_count"].as_i64(), Some(4));
    assert_eq!(arr[1]["image_url"], "/api/indoorcam/image/camA/a2.jpg");
    assert!(arr[0]["created_at"].is_string());

    // Unknown camera -> empty array, not 404.
    let resp = c
        .get(format!("{base}/api/indoorcam/history/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp
        .json::<Value>()
        .await
        .unwrap()
        .as_array()
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn serves_processed_image_and_404s_for_missing_or_non_jpg() {
    let processed = unique_processed_dir();
    let jpeg = vec![0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
    write_image(&processed, "camA", "frame.jpg", &jpeg);
    let base = spawn_app(&processed).await;
    let c = client();

    let resp = c
        .get(format!("{base}/api/indoorcam/image/camA/frame.jpg"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ct.contains("image/jpeg"), "unexpected content-type: {ct}");
    assert_eq!(resp.bytes().await.unwrap().as_ref(), jpeg.as_slice());

    assert_eq!(
        c.get(format!("{base}/api/indoorcam/image/camA/missing.jpg"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        c.get(format!("{base}/api/indoorcam/image/camA/notes.txt"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// clearing image refs (after Roboflow upload + local delete)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clearing_observation_image_nulls_urls() {
    let processed = unique_processed_dir();
    // The raw frame is on disk, so `latest` initially advertises an image_url.
    write_image(&processed, "camA", "f.jpg", &[0xFF, 0xD8, 0xFF]);
    let base = spawn_app(&processed).await;
    let c = client();

    // POST directly so we capture the new observation id.
    let body: Value = c
        .post(format!("{base}/api/indoorcam/observation"))
        .json(&json!({
            "camera_id": "camA",
            "timestamp": "2026-06-22T10:00:00",
            "detection_count": 3,
            "average_confidence": 0.8,
            "min_confidence": 0.8,
            "detections": [],
            "inference_time_ms": 5.0,
            "image_filename": "f.jpg",
            "annotated_image_filename": "f_annotated.jpg",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = body["id"].as_i64().unwrap();

    // Before clearing: latest carries the image_url.
    let before: Value = c
        .get(format!("{base}/api/indoorcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(before["image_url"], "/api/indoorcam/image/camA/f.jpg");

    // Clear it.
    let resp = c
        .patch(format!("{base}/api/indoorcam/observation/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cleared: Value = resp.json().await.unwrap();
    assert_eq!(cleared["image_cleared"], true);

    // After clearing: both image URLs are null (no more 404-prone links).
    let after: Value = c
        .get(format!("{base}/api/indoorcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(after["image_url"].is_null());
    assert!(after["annotated_image_url"].is_null());
    assert_eq!(after["detection_count"].as_i64(), Some(3)); // the count is untouched
}

#[tokio::test]
async fn clearing_unknown_observation_404s() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let resp = client()
        .patch(format!("{base}/api/indoorcam/observation/999999"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// auto-registration on observation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn observed_camera_auto_registers_in_registry() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    let before: Value = c
        .get(format!("{base}/api/indoor-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(before.as_array().unwrap().is_empty());

    post_observation(
        &base,
        &c,
        "auto-cam-1",
        "2026-06-15T07:30:00",
        2,
        0.8,
        "p.jpg",
        None,
    )
    .await;

    let after: Value = c
        .get(format!("{base}/api/indoor-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = after.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["camera_id"], "auto-cam-1");
    assert!(arr[0]["assignment"].is_null());
}

// ---------------------------------------------------------------------------
// registry CRUD
// ---------------------------------------------------------------------------

#[tokio::test]
async fn crud_create_get_update_delete() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    // Create (201).
    let resp = c
        .post(format!("{base}/api/indoor-cameras"))
        .json(&json!({"camera_id": "indoor-1", "name": "Brooder Cam", "rtsp_url": "rtsp://192.168.0.181:554/stream1"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let cam: Value = resp.json().await.unwrap();
    let id = cam["id"].as_i64().unwrap();
    assert_eq!(cam["camera_id"], "indoor-1");
    assert_eq!(cam["name"], "Brooder Cam");
    assert_eq!(cam["rtsp_url"], "rtsp://192.168.0.181:554/stream1");

    // Re-POST same camera_id updates metadata, returns 200 (upsert).
    let resp = c
        .post(format!("{base}/api/indoor-cameras"))
        .json(&json!({"camera_id": "indoor-1", "name": "Renamed"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.json::<Value>().await.unwrap()["name"], "Renamed");

    // Get one.
    let got: Value = c
        .get(format!("{base}/api/indoor-cameras/{id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(got["camera_id"], "indoor-1");

    // Update via PUT.
    let resp = c
        .put(format!("{base}/api/indoor-cameras/{id}"))
        .json(&json!({"rtsp_url": "rtsp://10.0.0.5:554/stream1", "model": "tapo-c100"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let updated: Value = resp.json().await.unwrap();
    assert_eq!(updated["rtsp_url"], "rtsp://10.0.0.5:554/stream1");
    assert_eq!(updated["model"], "tapo-c100");
    assert_eq!(
        updated["name"], "Renamed",
        "name left unchanged by partial PUT"
    );

    // Delete (204), then GET -> 404.
    let resp = c
        .delete(format!("{base}/api/indoor-cameras/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = c
        .get(format!("{base}/api/indoor-cameras/{id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// assignment: brooder/incubator OK, hutch rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn assign_to_brooder_and_incubator_ok_hutch_rejected() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    let brooder_id = create_unit(&base, &c, "Brooder 1", HousingType::Brooder).await;
    let incubator_id = create_unit(&base, &c, "Incubator 1", HousingType::Incubator).await;
    let hutch_id = create_unit(&base, &c, "Hutch 1", HousingType::Hutch).await;

    // Register a camera.
    let cam: Value = c
        .post(format!("{base}/api/indoor-cameras"))
        .json(&json!({"camera_id": "indoor-1"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let cam_id = cam["id"].as_i64().unwrap();

    // Assign to a brooder -> OK, assignment populated with housing_type.
    let resp = c
        .put(format!("{base}/api/indoor-cameras/{cam_id}/assign"))
        .json(&json!({"brooder_id": brooder_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let assigned: Value = resp.json().await.unwrap();
    assert_eq!(
        assigned["assignment"]["brooder_id"].as_i64(),
        Some(brooder_id)
    );
    assert_eq!(assigned["assignment"]["housing_type"], "brooder");

    // Re-assign to an incubator -> OK (closes the prior assignment).
    let resp = c
        .put(format!("{base}/api/indoor-cameras/{cam_id}/assign"))
        .json(&json!({"brooder_id": incubator_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let reassigned: Value = resp.json().await.unwrap();
    assert_eq!(
        reassigned["assignment"]["brooder_id"].as_i64(),
        Some(incubator_id)
    );
    assert_eq!(reassigned["assignment"]["housing_type"], "incubator");

    // The brooder no longer lists the camera; the incubator does.
    let brooder_cams: Value = c
        .get(format!("{base}/api/brooders/{brooder_id}/indoor-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(brooder_cams.as_array().unwrap().is_empty());
    let incubator_cams: Value = c
        .get(format!("{base}/api/brooders/{incubator_id}/indoor-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(incubator_cams.as_array().unwrap().len(), 1);

    // Assigning to a hutch is rejected.
    let resp = c
        .put(format!("{base}/api/indoor-cameras/{cam_id}/assign"))
        .json(&json!({"brooder_id": hutch_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Assigning to a non-existent unit is rejected.
    let resp = c
        .put(format!("{base}/api/indoor-cameras/{cam_id}/assign"))
        .json(&json!({"brooder_id": 99999}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Unassign -> 204, then assignment is null.
    let resp = c
        .delete(format!("{base}/api/indoor-cameras/{cam_id}/assign"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let cam: Value = c
        .get(format!("{base}/api/indoor-cameras/{cam_id}"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(cam["assignment"].is_null());
}

#[tokio::test]
async fn assign_unknown_camera_404() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    let brooder_id = create_unit(&base, &c, "Brooder 1", HousingType::Brooder).await;

    let resp = c
        .put(format!("{base}/api/indoor-cameras/4242/assign"))
        .json(&json!({"brooder_id": brooder_id}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
