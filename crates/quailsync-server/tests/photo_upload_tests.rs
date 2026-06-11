//! Integration tests for the bird-photo upload handler
//! (`POST /api/birds/{id}/photo`).
//!
//! Covers the full contract: valid upload → 200 + timestamped file on disk +
//! DB updated; history retained across uploads; oversized → 413 + ntfy alert +
//! nothing written; non-JPEG → 415; write failure → DB untouched; unknown bird
//! → 404.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use axum::routing::post;
use axum::Router;
use quailsync_common::{Bird, BirdStatus, CreateBird, CreateLineage, Lineage, Sex};
use quailsync_server::state::PhotoConfig;
use quailsync_server::{build_app, init_db, AppState};
use reqwest::multipart;
use reqwest::StatusCode;
use rusqlite::Connection;

// ===========================================================================
// Harness
// ===========================================================================

static DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// A fresh, empty temp dir path (not yet created) unique to this test run.
fn unique_temp_dir() -> PathBuf {
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("qs-photo-test-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// Spin up a server with the given photo config; return its base URL.
async fn spawn_app(photos: PhotoConfig) -> String {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_db(&conn);

    let (live_tx, _) = tokio::sync::broadcast::channel::<String>(64);
    let metrics_handle = metrics_exporter_prometheus::PrometheusBuilder::new()
        .build_recorder()
        .handle();

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
        alert_config: quailsync_common::AlertConfig::default(),
        live_tx,
        last_seen: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        metrics_handle,
        photos,
    };

    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind random port");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// A mock ntfy server. Returns its base URL and the shared list of received
/// request bodies (one entry per POST it accepts).
async fn spawn_mock_ntfy() -> (String, Arc<Mutex<Vec<String>>>) {
    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new().route(
        "/{topic}",
        post({
            let received = received.clone();
            move |body: String| {
                let received = received.clone();
                async move {
                    received.lock().unwrap().push(body);
                    axum::http::StatusCode::OK
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock ntfy");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{addr}"), received)
}

fn photos_no_alerts(dir: &Path) -> PhotoConfig {
    PhotoConfig::for_dir(dir.to_path_buf())
}

fn photos_with_ntfy(dir: &Path, server: String, topic: &str) -> PhotoConfig {
    PhotoConfig {
        dir: Arc::new(dir.to_path_buf()),
        ntfy_server: server,
        ntfy_topic: Some(topic.to_string()),
    }
}

/// Build a syntactically-valid JPEG blob of (at least) `len` bytes: SOI/APP0
/// header, zero padding, EOI trailer.
fn jpeg_bytes(len: usize) -> Vec<u8> {
    let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0]; // SOI + APP0 marker
    v.resize(len.max(6), 0x00);
    let n = v.len();
    v[n - 2] = 0xFF; // EOI
    v[n - 1] = 0xD9;
    v
}

async fn seed_bird(base: &str) -> i64 {
    let lineage: Lineage = client()
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: "PhotoLine".into(),
            source: "Lab".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let bird: Bird = client()
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            lineage_ids: vec![lineage.id],
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

async fn get_bird(base: &str, id: i64) -> Bird {
    let birds: Vec<Bird> = client()
        .get(format!("{base}/api/birds"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    birds
        .into_iter()
        .find(|b| b.id == id)
        .expect("bird present")
}

async fn upload(base: &str, id: i64, bytes: Vec<u8>, mime: &str) -> reqwest::Response {
    let part = multipart::Part::bytes(bytes)
        .file_name(format!("bird_{id}.jpg"))
        .mime_str(mime)
        .unwrap();
    let form = multipart::Form::new().part("photo", part);
    client()
        .post(format!("{base}/api/birds/{id}/photo"))
        .multipart(form)
        .send()
        .await
        .unwrap()
}

fn count_files(dir: &PathBuf) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

// ===========================================================================
// Tests
// ===========================================================================

#[tokio::test]
async fn valid_jpeg_under_cap_is_stored() {
    let dir = unique_temp_dir();
    let base = spawn_app(photos_no_alerts(&dir)).await;
    let id = seed_bird(&base).await;

    let resp = upload(&base, id, jpeg_bytes(2048), "image/jpeg").await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = resp.json().await.unwrap();
    let stored = body["photo_path"].as_str().unwrap().to_string();
    assert!(body["photo_uploaded_at"].as_str().is_some());

    // File actually on disk under a timestamped, id-keyed name.
    assert!(PathBuf::from(&stored).exists(), "stored file should exist");
    let fname = PathBuf::from(&stored)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert!(
        fname.starts_with(&format!("bird_{id}_")) && fname.ends_with(".jpg"),
        "unexpected filename: {fname}"
    );
    assert_eq!(count_files(&dir), 1);

    // DB now points at it, with an upload timestamp.
    let bird = get_bird(&base, id).await;
    assert_eq!(bird.photo_path.as_deref(), Some(stored.as_str()));
    assert!(bird.photo_uploaded_at.is_some());
}

#[tokio::test]
async fn second_upload_keeps_history_and_advances_pointer() {
    let dir = unique_temp_dir();
    let base = spawn_app(photos_no_alerts(&dir)).await;
    let id = seed_bird(&base).await;

    let first: serde_json::Value = upload(&base, id, jpeg_bytes(1024), "image/jpeg")
        .await
        .json()
        .await
        .unwrap();
    let first_path = first["photo_path"].as_str().unwrap().to_string();

    let second: serde_json::Value = upload(&base, id, jpeg_bytes(1024), "image/jpeg")
        .await
        .json()
        .await
        .unwrap();
    let second_path = second["photo_path"].as_str().unwrap().to_string();

    // History kept: two distinct files, old one still present.
    assert_ne!(first_path, second_path, "second upload must not reuse name");
    assert!(PathBuf::from(&first_path).exists(), "old file kept");
    assert!(PathBuf::from(&second_path).exists(), "new file written");
    assert_eq!(count_files(&dir), 2);

    // Pointer now references the newer upload.
    let bird = get_bird(&base, id).await;
    assert_eq!(bird.photo_path.as_deref(), Some(second_path.as_str()));
}

#[tokio::test]
async fn oversized_upload_rejected_and_alerts() {
    let dir = unique_temp_dir();
    let (ntfy_url, received) = spawn_mock_ntfy().await;
    let base = spawn_app(photos_with_ntfy(&dir, ntfy_url, "test-topic")).await;
    let id = seed_bird(&base).await;

    // 11 MB > 10 MB cap, but < the route body limit so it reaches the handler.
    let resp = upload(&base, id, jpeg_bytes(11 * 1024 * 1024), "image/jpeg").await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

    // Nothing written, DB unchanged.
    assert_eq!(count_files(&dir), 0);
    let bird = get_bird(&base, id).await;
    assert!(bird.photo_path.is_none());
    assert!(bird.photo_uploaded_at.is_none());

    // ntfy alert fired, generic + non-sensitive, naming the bird.
    let msgs = received.lock().unwrap().clone();
    assert_eq!(msgs.len(), 1, "exactly one alert expected");
    assert!(
        msgs[0].contains("oversized") && msgs[0].contains(&id.to_string()),
        "unexpected alert body: {}",
        msgs[0]
    );
}

#[tokio::test]
async fn png_rejected_as_unsupported_media() {
    let dir = unique_temp_dir();
    let base = spawn_app(photos_no_alerts(&dir)).await;
    let id = seed_bird(&base).await;

    // PNG signature + declared image/png.
    let png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
    let resp = upload(&base, id, png, "image/png").await;
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    assert_eq!(count_files(&dir), 0);
    assert!(get_bird(&base, id).await.photo_path.is_none());
}

#[tokio::test]
async fn jpeg_mime_but_non_jpeg_bytes_rejected() {
    let dir = unique_temp_dir();
    let base = spawn_app(photos_no_alerts(&dir)).await;
    let id = seed_bird(&base).await;

    // A renamed .mp4: declared image/jpeg, but the bytes are an MP4 ftyp box —
    // content-type alone is spoofable, so the magic-byte check must catch it.
    let mut mp4 = vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70]; // ....ftyp
    mp4.extend_from_slice(b"mp42mp42isom");
    let resp = upload(&base, id, mp4, "image/jpeg").await;
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    assert_eq!(count_files(&dir), 0);
    assert!(get_bird(&base, id).await.photo_path.is_none());
}

#[tokio::test]
async fn write_failure_leaves_db_untouched() {
    // Point the photo dir at a path that is actually a *file*, so the
    // handler's create_dir_all fails and no write can happen.
    let parent = unique_temp_dir();
    std::fs::create_dir_all(&parent).unwrap();
    let blocking_file = parent.join("not-a-dir");
    std::fs::write(&blocking_file, b"x").unwrap();

    let base = spawn_app(photos_no_alerts(&blocking_file)).await;
    let id = seed_bird(&base).await;

    let resp = upload(&base, id, jpeg_bytes(1024), "image/jpeg").await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    // photo_path must NOT be set when the file never landed.
    let bird = get_bird(&base, id).await;
    assert!(bird.photo_path.is_none());
    assert!(bird.photo_uploaded_at.is_none());
}

#[tokio::test]
async fn unknown_bird_is_404_and_writes_nothing() {
    let dir = unique_temp_dir();
    let base = spawn_app(photos_no_alerts(&dir)).await;

    let resp = upload(&base, 9999, jpeg_bytes(1024), "image/jpeg").await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(count_files(&dir), 0);
}
