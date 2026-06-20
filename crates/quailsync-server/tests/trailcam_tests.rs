//! Integration tests for the SQLite-backed trail-cam observation endpoints:
//! `POST /api/trailcam/observation`, `GET /api/trailcam/latest/{camera_id}`,
//! `GET /api/trailcam/cameras`, `GET /api/trailcam/history/{camera_id}`, and
//! image serving from `processed/{camera_id}/`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use quailsync_server::state::{PhotoConfig, TrailcamConfig};
use quailsync_server::{build_app, init_db, AppState};
use reqwest::StatusCode;
use rusqlite::Connection;
use serde_json::{json, Value};

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, created temp dir to act as the trail-cam `processed/` root.
fn unique_processed_dir() -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("qs-trailcam-test-{}-{}", std::process::id(), n));
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
        photos: PhotoConfig::for_dir(std::env::temp_dir().join("qs-trailcam-test-photos")),
        trailcam: TrailcamConfig::for_dir(processed_dir.to_path_buf()),
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
    bird_count: i64,
    avg_conf: f64,
    image_filename: &str,
    annotated_image_filename: Option<&str>,
) {
    let mut body = json!({
        "camera_id": camera_id,
        "timestamp": timestamp,
        "bird_count": bird_count,
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
        .post(format!("{base}/api/trailcam/observation"))
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

// ---------------------------------------------------------------------------
// latest
// ---------------------------------------------------------------------------

#[tokio::test]
async fn latest_returns_most_recent_for_camera() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    // camA has two observations (out of order), camB one — latest must pick the
    // greatest-timestamp camA entry, not just the last inserted.
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T07:30:00",
        5,
        0.7,
        "20260615-073000_c.jpg",
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
        "20260615-080000_b.jpg",
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
        "20260615-050000_a.jpg",
        None,
    )
    .await;

    let body: Value = c
        .get(format!("{base}/api/trailcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(body["camera_id"], "camA");
    assert_eq!(body["bird_count"].as_i64(), Some(5));
    assert_eq!(body["timestamp"], "2026-06-15T07:30:00");
    assert_eq!(body["confidence_avg"].as_f64(), Some(0.7));
    assert_eq!(body["detections"].as_array().unwrap().len(), 1);
    assert_eq!(
        body["image_url"],
        "/api/trailcam/image/camA/20260615-073000_c.jpg"
    );
}

#[tokio::test]
async fn latest_annotated_url_null_when_no_annotated_file() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    // Annotated filename is stored, but the file isn't on disk -> url is null.
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T07:30:00",
        2,
        0.8,
        "20260615-073000_c.jpg",
        Some("20260615-073000_c_annotated.jpg"),
    )
    .await;

    let body: Value = c
        .get(format!("{base}/api/trailcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        body["image_url"],
        "/api/trailcam/image/camA/20260615-073000_c.jpg"
    );
    assert!(body["annotated_image_url"].is_null());
}

#[tokio::test]
async fn latest_returns_annotated_url_when_file_present() {
    let processed = unique_processed_dir();
    // The detector's annotated copy exists on disk -> it's advertised.
    write_image(
        &processed,
        "camA",
        "20260615-073000_c_annotated.jpg",
        &[0xFF, 0xD8, 0xFF],
    );
    let base = spawn_app(&processed).await;
    let c = client();
    post_observation(
        &base,
        &c,
        "camA",
        "2026-06-15T07:30:00",
        2,
        0.8,
        "20260615-073000_c.jpg",
        Some("20260615-073000_c_annotated.jpg"),
    )
    .await;

    let body: Value = c
        .get(format!("{base}/api/trailcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        body["image_url"],
        "/api/trailcam/image/camA/20260615-073000_c.jpg"
    );
    assert_eq!(
        body["annotated_image_url"],
        "/api/trailcam/image/camA/20260615-073000_c_annotated.jpg"
    );
}

#[tokio::test]
async fn latest_404_for_unknown_camera() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
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
        .get(format!("{base}/api/trailcam/latest/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn latest_404_when_no_observations() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await; // empty DB

    let resp = client()
        .get(format!("{base}/api/trailcam/latest/camA"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// cameras
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cameras_lists_distinct_in_first_appearance_order() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();
    // camB appears first, then camA, then camB again (dup). First-appearance
    // order is camB, camA; labels number accordingly.
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
        .get(format!("{base}/api/trailcam/cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let arr = cams.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["camera_id"], "camB");
    assert_eq!(arr[0]["label"], "Outdoor Cam 1");
    assert_eq!(arr[1]["camera_id"], "camA");
    assert_eq!(arr[1]["label"], "Outdoor Cam 2");
}

#[tokio::test]
async fn cameras_empty_when_no_observations() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;

    let resp = client()
        .get(format!("{base}/api/trailcam/cameras"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let cams: Value = resp.json().await.unwrap();
    assert!(cams.as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// history
// ---------------------------------------------------------------------------

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
        .get(format!("{base}/api/trailcam/history/camA?hours=24"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = hist.as_array().unwrap();
    assert_eq!(arr.len(), 2, "only camA's two observations");
    // Oldest first (insertion order).
    assert_eq!(arr[0]["bird_count"].as_i64(), Some(2));
    assert_eq!(arr[1]["bird_count"].as_i64(), Some(4));
    assert_eq!(arr[0]["camera_id"], "camA");
    assert!(arr[0]["created_at"].is_string());
    assert_eq!(arr[1]["image_url"], "/api/trailcam/image/camA/a2.jpg");
}

#[tokio::test]
async fn history_defaults_to_24h_and_empty_for_unknown_camera() {
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

    // No `hours` -> default window still returns the just-posted row.
    let hist: Value = c
        .get(format!("{base}/api/trailcam/history/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(hist.as_array().unwrap().len(), 1);

    // Unknown camera -> empty array, not 404.
    let resp = c
        .get(format!("{base}/api/trailcam/history/ghost"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ghost: Value = resp.json().await.unwrap();
    assert!(ghost.as_array().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// ambient temperature round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ambient_temperature_round_trips_through_latest_and_history() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    // One observation WITH a temperature, one WITHOUT (column stays null).
    let with_temp = json!({
        "camera_id": "camA",
        "timestamp": "2026-06-15T05:00:00",
        "bird_count": 3,
        "average_confidence": 0.8,
        "min_confidence": 0.7,
        "detections": [],
        "inference_time_ms": 5.0,
        "image_filename": "a1.jpg",
        "ambient_temperature_f": 72.5,
    });
    let resp = c
        .post(format!("{base}/api/trailcam/observation"))
        .json(&with_temp)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

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

    // history: first row carries the temp, second is null.
    let hist: Value = c
        .get(format!("{base}/api/trailcam/history/camA?hours=24"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = hist.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    assert_eq!(arr[0]["ambient_temperature_f"].as_f64(), Some(72.5));
    assert!(arr[1]["ambient_temperature_f"].is_null());

    // latest reflects the most recent observation (no temp).
    let latest: Value = c
        .get(format!("{base}/api/trailcam/latest/camA"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(latest["ambient_temperature_f"].is_null());
}

// ---------------------------------------------------------------------------
// image serving
// ---------------------------------------------------------------------------

#[tokio::test]
async fn serves_processed_image_with_jpeg_content_type() {
    let processed = unique_processed_dir();
    let jpeg = vec![
        0xFFu8, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00,
    ];
    write_image(&processed, "camA", "20260615-073000_c.jpg", &jpeg);
    let base = spawn_app(&processed).await;

    let resp = client()
        .get(format!(
            "{base}/api/trailcam/image/camA/20260615-073000_c.jpg"
        ))
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
}

#[tokio::test]
async fn image_404_for_missing_and_non_jpg() {
    let processed = unique_processed_dir();
    write_image(&processed, "camA", "real.jpg", &[0xFF, 0xD8, 0xFF]);
    let base = spawn_app(&processed).await;

    // Missing file under a real camera.
    assert_eq!(
        client()
            .get(format!("{base}/api/trailcam/image/camA/missing.jpg"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    // Non-.jpg names are refused outright (only JPEGs are served).
    assert_eq!(
        client()
            .get(format!("{base}/api/trailcam/image/camA/notes.txt"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

// ---------------------------------------------------------------------------
// auto-registration: posting an observation from an unknown camera creates a
// trail_cameras row (the same way Govee readings auto-register sensors).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn observed_camera_auto_registers_as_trail_camera() {
    let processed = unique_processed_dir();
    let base = spawn_app(&processed).await;
    let c = client();

    // Registry starts empty.
    let before: Value = c
        .get(format!("{base}/api/trail-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(before.as_array().unwrap().is_empty());

    // Posting an observation auto-registers the camera.
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

    // It's now a registered (unassigned) trail camera, and shows in the list.
    let after: Value = c
        .get(format!("{base}/api/trail-cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let arr = after.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["spypoint_camera_id"], "auto-cam-1");
    assert!(arr[0]["assignment"].is_null());

    let cams: Value = c
        .get(format!("{base}/api/trailcam/cameras"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(cams.as_array().unwrap().len(), 1);
}
