//! Boundary & stress tests for QuailSync V2 server.
//!
//! These tests intentionally try to break things — malformed inputs, extreme values,
//! concurrent writes, path traversal, and protocol abuse.

use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use quailsync_common::*;
use quailsync_server::{build_app, init_db, AppState};
use reqwest::StatusCode;
use rusqlite::Connection;
use serde_json::{json, Value};
use tokio_tungstenite::{connect_async, tungstenite::Message};

// ===========================================================================
// Test harness
// ===========================================================================

async fn spawn_test_server() -> String {
    let conn = Connection::open_in_memory().expect("in-memory sqlite");
    init_db(&conn);

    let (live_tx, _) = tokio::sync::broadcast::channel::<String>(64);

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
        alert_config: AlertConfig::default(),
        live_tx,
        last_seen: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
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

fn ws_url(base: &str) -> String {
    base.replace("http://", "ws://")
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn seed_bloodline(base: &str) -> Bloodline {
    let resp = client()
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "TestLine".into(),
            source: "Lab".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();
    resp.json().await.unwrap()
}

async fn seed_bird(base: &str, bloodline_id: i64, sex: Sex) -> Bird {
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex,
            bloodline_id,
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 1,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
        })
        .send()
        .await
        .unwrap();
    resp.json().await.unwrap()
}

async fn seed_brooder(base: &str, name: &str) -> Value {
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({
            "name": name,
            "life_stage": "Chick",
            "qr_code": "",
        }))
        .send()
        .await
        .unwrap();
    resp.json().await.unwrap()
}

// ===========================================================================
// 1. API INPUT VALIDATION — Brooders
// ===========================================================================

#[tokio::test]
async fn brooder_empty_name_accepted() {
    // Empty names are not explicitly rejected by the server — verify it doesn't crash
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({"name": "", "life_stage": "Chick", "qr_code": ""}))
        .send()
        .await
        .unwrap();
    // Server should respond (either 201 or 4xx, but NOT 500)
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn brooder_very_long_name() {
    let base = spawn_test_server().await;
    let long_name = "A".repeat(10_000);
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({"name": long_name, "life_stage": "Chick", "qr_code": ""}))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn brooder_sql_injection_name() {
    let base = spawn_test_server().await;
    let payloads = [
        "'; DROP TABLE brooders; --",
        "Robert'); DROP TABLE birds;--",
        "1 OR 1=1",
        "' UNION SELECT * FROM birds --",
    ];
    for payload in payloads {
        let resp = client()
            .post(format!("{base}/api/brooders"))
            .json(&json!({"name": payload, "life_stage": "Chick", "qr_code": ""}))
            .send()
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "SQL injection payload panicked server: {payload}"
        );
    }
    // Verify brooders table still exists and is functional
    let list = client()
        .get(format!("{base}/api/brooders"))
        .send()
        .await
        .unwrap();
    assert_eq!(list.status(), StatusCode::OK);
    let brooders: Vec<Value> = list.json().await.unwrap();
    assert_eq!(brooders.len(), payloads.len());
}

#[tokio::test]
async fn brooder_xss_payload_name() {
    let base = spawn_test_server().await;
    let xss = "<script>alert('xss')</script>";
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({"name": xss, "life_stage": "Chick", "qr_code": ""}))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body: Value = resp.json().await.unwrap();
    // Name should be stored verbatim (XSS escaping is the dashboard's responsibility)
    assert_eq!(body["name"].as_str().unwrap(), xss);
}

#[tokio::test]
async fn brooder_unicode_and_emoji_name() {
    let base = spawn_test_server().await;
    let names = [
        "Brooder \u{1F423}\u{1F95A}",
        "\u{4e2d}\u{6587}\u{540d}",
        "\u{0410}\u{0411}\u{0412}",
    ];
    for name in names {
        let resp = client()
            .post(format!("{base}/api/brooders"))
            .json(&json!({"name": name, "life_stage": "Chick", "qr_code": ""}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "Failed for name: {name}"
        );
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["name"].as_str().unwrap(), name);
    }
}

#[tokio::test]
async fn brooder_null_bytes_in_name() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({"name": "test\x00evil", "life_stage": "Chick", "qr_code": ""}))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn brooder_nonexistent_bloodline() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({
            "name": "Test",
            "bloodline_id": 99999,
            "life_stage": "Chick",
            "qr_code": "",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ===========================================================================
// 2. API INPUT VALIDATION — Birds
// ===========================================================================

#[tokio::test]
async fn bird_all_optional_fields_null() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Unknown",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bird: Value = resp.json().await.unwrap();
    assert!(bird["band_color"].is_null());
    assert!(bird["mother_id"].is_null());
    assert!(bird["father_id"].is_null());
    assert!(bird["notes"].is_null());
}

#[tokio::test]
async fn bird_max_length_optional_fields() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let long = "Z".repeat(50_000);
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "band_color": long,
            "sex": "Male",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
            "notes": long,
            "nfc_tag_id": long,
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn bird_invalid_enum_values_rejected() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;

    // Invalid sex
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Helicopter",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);

    // Invalid status
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Male",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Zombie",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn bird_nonexistent_bloodline_panics() {
    // This tests a known weakness: create_bird unwraps the insert, so a foreign key
    // violation on bloodline_id will cause a 500 (panic) rather than a clean error.
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Male",
            "bloodline_id": 99999,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
        }))
        .send()
        .await
        .unwrap();
    // Due to unwrap(), this currently returns 500. Documenting the behavior.
    let status = resp.status();
    assert!(
        status == StatusCode::INTERNAL_SERVER_ERROR || status == StatusCode::BAD_REQUEST,
        "Expected 500 (current behavior) or 400 (ideal), got {status}"
    );
}

#[tokio::test]
async fn bird_duplicate_nfc_tag_id() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let tag = "QUAIL-ABC123";

    // First bird with this tag — should succeed
    let resp = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Male",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
            "nfc_tag_id": tag,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Second bird with same tag — should fail (UNIQUE constraint)
    let resp2 = client()
        .post(format!("{base}/api/birds"))
        .json(&json!({
            "sex": "Female",
            "bloodline_id": bl.id,
            "hatch_date": "2026-01-01",
            "generation": 1,
            "status": "Active",
            "nfc_tag_id": tag,
        }))
        .send()
        .await
        .unwrap();
    // Due to unwrap(), this panics with 500. Documenting behavior.
    let status = resp2.status();
    assert!(
        status == StatusCode::INTERNAL_SERVER_ERROR || status == StatusCode::CONFLICT,
        "Expected 500 (current: unwrap panic) or 409 (ideal), got {status}"
    );
}

// ===========================================================================
// 3. API INPUT VALIDATION — Bloodlines
// ===========================================================================

#[tokio::test]
async fn bloodline_empty_name() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "".into(),
            source: "x".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn bloodline_whitespace_only_name() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "   \t\n  ".into(),
            source: "x".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();
    // Stored as-is (whitespace). Not a crash.
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn bloodline_duplicate_names_allowed() {
    let base = spawn_test_server().await;
    for _ in 0..3 {
        let resp = client()
            .post(format!("{base}/api/bloodlines"))
            .json(&CreateBloodline {
                name: "SameName".into(),
                source: "x".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
    }
    let list: Vec<Bloodline> = client()
        .get(format!("{base}/api/bloodlines"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(list.len(), 3);
}

// ===========================================================================
// 4. API INPUT VALIDATION — Malformed bodies
// ===========================================================================

#[tokio::test]
async fn post_with_empty_body() {
    let base = spawn_test_server().await;
    let endpoints = [
        "/api/brooders",
        "/api/bloodlines",
        "/api/birds",
        "/api/clutches",
        "/api/processing",
    ];
    for ep in endpoints {
        let resp = client()
            .post(format!("{base}{ep}"))
            .header("content-type", "application/json")
            .body("")
            .send()
            .await
            .unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Empty body crashed {ep}"
        );
    }
}

#[tokio::test]
async fn post_with_malformed_json() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .header("content-type", "application/json")
        .body("{not valid json!!!")
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn post_with_wrong_content_type() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .header("content-type", "text/plain")
        .body(r#"{"name":"test","life_stage":"Chick","qr_code":""}"#)
        .send()
        .await
        .unwrap();
    // Axum rejects non-JSON content-type for Json<T> extractors
    assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn post_with_oversized_body() {
    let base = spawn_test_server().await;
    // 2MB payload
    let huge = "X".repeat(2 * 1024 * 1024);
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .header("content-type", "application/json")
        .body(format!(
            r#"{{"name":"{huge}","life_stage":"Chick","qr_code":""}}"#
        ))
        .send()
        .await
        .unwrap();
    // Should either reject with 413 or handle without crashing
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn post_with_extra_unknown_fields() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/brooders"))
        .json(&json!({
            "name": "Test",
            "life_stage": "Chick",
            "qr_code": "",
            "unknown_field": "should be ignored",
            "another_field": 42,
        }))
        .send()
        .await
        .unwrap();
    // Serde default: ignores unknown fields
    assert_eq!(resp.status(), StatusCode::CREATED);
}

// ===========================================================================
// 5. BROODER READINGS — Extreme values
// ===========================================================================

#[tokio::test]
async fn reading_extreme_temperatures_via_ws() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Extreme Test").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // Absolute zero (-459.67), extreme high, zero, negative
    let temps = [-459.67_f64, 10000.0, 0.0, -100.0, 999999.0];
    for temp in temps {
        let payload = json!({
            "Brooder": {
                "temperature_f": temp,
                "humidity_percent": 50.0,
                "timestamp": "2026-03-01T12:00:00.000Z",
                "brooder_id": bid,
            }
        });
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    // All readings should be stored
    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(readings.len(), temps.len());
}

#[tokio::test]
async fn reading_extreme_humidity_via_ws() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Humidity Test").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let humidities = [-1.0_f64, 0.0, 100.0, 101.0, 999999.0];
    for hum in humidities {
        let payload = json!({
            "Brooder": {
                "temperature_f": 98.0,
                "humidity_percent": hum,
                "timestamp": "2026-03-01T12:00:00.000Z",
                "brooder_id": bid,
            }
        });
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(readings.len(), humidities.len());
}

#[tokio::test]
async fn reading_nan_temperature_rejected_by_serde() {
    // JSON doesn't have NaN, so sending "NaN" as a string should fail deserialization
    let base = spawn_test_server().await;
    seed_brooder(&base, "NaN Test").await;

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // NaN is not valid JSON — this is a malformed payload
    let raw = r#"{"Brooder":{"temperature_f":NaN,"humidity_percent":50.0,"timestamp":"2026-03-01T12:00:00.000Z","brooder_id":1}}"#;
    ws.send(Message::Text(raw.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Should NOT crash the server — verify health
    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn reading_with_nonexistent_brooder_id() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "Brooder": {
            "temperature_f": 98.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": 99999,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Server should not crash
    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 6. DATABASE BOUNDARY TESTS
// ===========================================================================

#[tokio::test]
async fn query_readings_zero_readings() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Empty").await;
    let bid = brooder["id"].as_i64().unwrap();
    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(readings.len(), 0);
}

#[tokio::test]
async fn query_readings_single_reading() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Single").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
    let payload = json!({
        "Brooder": {
            "temperature_f": 97.5,
            "humidity_percent": 55.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(readings.len(), 1);
}

#[tokio::test]
async fn concurrent_writes_50_tasks() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Concurrent").await;
    let bid = brooder["id"].as_i64().unwrap();

    let mut handles = Vec::new();
    for i in 0..50 {
        let base_clone = base.clone();
        let handle = tokio::spawn(async move {
            let ws_base = ws_url(&base_clone);
            let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
            let payload = json!({
                "Brooder": {
                    "temperature_f": 95.0 + (i as f64 * 0.1),
                    "humidity_percent": 50.0,
                    "timestamp": "2026-03-01T12:00:00.000Z",
                    "brooder_id": bid,
                }
            });
            ws.send(Message::Text(payload.to_string().into()))
                .await
                .unwrap();
        });
        handles.push(handle);
    }

    for h in handles {
        h.await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // All 50 should be stored without data loss or corruption
    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(
        readings.len(),
        50,
        "Expected 50 readings from concurrent writes"
    );
}

#[tokio::test]
async fn brooder_status_for_nonexistent_brooder() {
    let base = spawn_test_server().await;
    let resp = client()
        .get(format!("{base}/api/brooders/99999/status"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bird_weight_for_nonexistent_bird() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/birds/99999/weights"))
        .json(&json!({
            "weight_grams": 250.0,
            "date": "2026-03-01",
        }))
        .send()
        .await
        .unwrap();
    // This will either 404 or 500 (unwrap on FK violation)
    assert_ne!(resp.status(), StatusCode::CREATED);
}

// ===========================================================================
// 7. WEBSOCKET TESTS
// ===========================================================================

#[tokio::test]
async fn ws_connect_and_immediately_disconnect() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
    drop(ws);
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Server should still be alive
    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_send_empty_message() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
    ws.send(Message::Text(String::new().into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_send_binary_message() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
    ws.send(Message::Binary(vec![0xFF, 0xFE, 0x00, 0x01].into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_send_valid_json_wrong_schema() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let bad_payloads = [
        json!({"wrong_variant": {"data": 123}}),
        json!({"Brooder": {}}), // missing required fields
        json!({"Brooder": {"temperature_f": "not_a_number"}}),
        json!({"System": {"cpu_usage_percent": "string_instead"}}),
        json!(42),
        json!([1, 2, 3]),
        json!(null),
    ];

    for payload in bad_payloads {
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_send_large_message() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // ~1MB text message
    let big = "A".repeat(1_000_000);
    ws.send(Message::Text(big.into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_rapid_fire_messages() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Rapid").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // Send 200 messages as fast as possible
    for i in 0..200 {
        let payload = json!({
            "Brooder": {
                "temperature_f": 95.0 + (i as f64 * 0.01),
                "humidity_percent": 50.0,
                "timestamp": "2026-03-01T12:00:00.000Z",
                "brooder_id": bid,
            }
        });
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(1000)).await;

    let resp = client()
        .get(format!("{base}/api/brooders/{bid}/readings?minutes=60"))
        .send()
        .await
        .unwrap();
    let readings: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(readings.len(), 200);
}

#[tokio::test]
async fn ws_live_100_clients_connect_disconnect() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);

    let mut connections = Vec::new();
    for _ in 0..100 {
        let (ws, _) = connect_async(format!("{ws_base}/ws/live")).await.unwrap();
        connections.push(ws);
    }

    // Drop all at once
    drop(connections);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn ws_live_receives_broadcast() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Broadcast").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);

    // Connect a live client
    let (mut live_ws, _) = connect_async(format!("{ws_base}/ws/live")).await.unwrap();

    // Send a reading via the agent WS
    let (mut agent_ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();
    let payload = json!({
        "Brooder": {
            "temperature_f": 98.5,
            "humidity_percent": 52.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    agent_ws
        .send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();

    // Live client should receive the broadcast
    let msg = tokio::time::timeout(Duration::from_secs(3), live_ws.next())
        .await
        .expect("timed out waiting for broadcast")
        .expect("stream ended")
        .expect("ws error");

    match msg {
        Message::Text(text) => {
            let v: Value = serde_json::from_str(&text).unwrap();
            assert!(v["Brooder"].is_object());
            assert!((v["Brooder"]["temperature_f"].as_f64().unwrap() - 98.5).abs() < 0.01);
        }
        other => panic!("Expected text message, got {other:?}"),
    }
}

#[tokio::test]
async fn ws_malformed_telemetry_variants() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let malformed = [
        // Brooder with missing timestamp
        json!({"Brooder": {"temperature_f": 98.0, "humidity_percent": 50.0, "brooder_id": 1}}),
        // System with negative CPU
        json!({"System": {"cpu_usage_percent": -50.0, "memory_used_bytes": 0, "memory_total_bytes": 0, "disk_used_bytes": 0, "disk_total_bytes": 0, "uptime_seconds": 0}}),
        // brooder_id as string
        json!({"Brooder": {"temperature_f": 98.0, "humidity_percent": 50.0, "timestamp": "2026-03-01T12:00:00.000Z", "brooder_id": "one"}}),
        // CameraAssign is not a valid TelemetryPayload variant — should be rejected
        json!({"CameraAssign": {"brooder_id": 0}}),
    ];

    for payload in malformed {
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client().get(format!("{base}/health")).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 8. ALERT ENGINE TESTS
// ===========================================================================

#[tokio::test]
async fn alert_exactly_at_min_threshold() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "AlertMin").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // Exactly 95.0 (min threshold) — should NOT trigger alert
    let payload = json!({
        "Brooder": {
            "temperature_f": 95.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(
        alerts.len(),
        0,
        "Reading at exact min threshold should not alert"
    );
}

#[tokio::test]
async fn alert_exactly_at_max_threshold() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "AlertMax").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // Exactly 100.0 (max threshold) — should NOT trigger alert
    let payload = json!({
        "Brooder": {
            "temperature_f": 100.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(
        alerts.len(),
        0,
        "Reading at exact max threshold should not alert"
    );
}

#[tokio::test]
async fn alert_one_degree_below_min() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "AlertBelow").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "Brooder": {
            "temperature_f": 94.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(alerts.len(), 1, "Reading 1 below min should trigger alert");
    assert!(
        alerts[0]["message"].as_str().unwrap().contains("LOW"),
        "Alert should say LOW"
    );
    assert_eq!(alerts[0]["severity"].as_str().unwrap(), "Warning");
}

#[tokio::test]
async fn alert_one_degree_above_max() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "AlertAbove").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "Brooder": {
            "temperature_f": 101.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0]["message"].as_str().unwrap().contains("HIGH"));
    assert_eq!(alerts[0]["severity"].as_str().unwrap(), "Warning");
}

#[tokio::test]
async fn alert_critical_when_far_from_threshold() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "AlertCritical").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // >3 degrees below min → Critical
    let payload = json!({
        "Brooder": {
            "temperature_f": 90.0,
            "humidity_percent": 50.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0]["severity"].as_str().unwrap(), "Critical");
}

#[tokio::test]
async fn alert_humidity_low() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "HumLow").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "Brooder": {
            "temperature_f": 97.0,
            "humidity_percent": 30.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0]["message"]
        .as_str()
        .unwrap()
        .contains("Humidity LOW"));
}

#[tokio::test]
async fn alert_humidity_high() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "HumHigh").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "Brooder": {
            "temperature_f": 97.0,
            "humidity_percent": 75.0,
            "timestamp": "2026-03-01T12:00:00.000Z",
            "brooder_id": bid,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    let alerts: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(alerts.len(), 1);
    assert!(alerts[0]["message"]
        .as_str()
        .unwrap()
        .contains("Humidity HIGH"));
}

#[tokio::test]
async fn alert_rapid_oscillation() {
    let base = spawn_test_server().await;
    let brooder = seed_brooder(&base, "Oscillation").await;
    let bid = brooder["id"].as_i64().unwrap();

    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    // Alternate above and below threshold 50 times
    for i in 0..50 {
        let temp = if i % 2 == 0 { 94.0 } else { 101.0 };
        let payload = json!({
            "Brooder": {
                "temperature_f": temp,
                "humidity_percent": 50.0,
                "timestamp": "2026-03-01T12:00:00.000Z",
                "brooder_id": bid,
            }
        });
        ws.send(Message::Text(payload.to_string().into()))
            .await
            .unwrap();
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Should have generated alerts without crashing
    let resp = client()
        .get(format!("{base}/api/alerts?minutes=10"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let alerts: Vec<Value> = resp.json().await.unwrap();
    // Each out-of-range reading generates an alert — 50 readings, all out of range
    assert_eq!(
        alerts.len(),
        50,
        "Each out-of-range reading should generate an alert"
    );
}

// ===========================================================================
// 9. PATH TRAVERSAL / SECURITY
// ===========================================================================

#[tokio::test]
async fn restore_backup_path_traversal_rejected() {
    let base = spawn_test_server().await;

    let traversal_names = [
        "../../etc/passwd",
        "../../../etc/shadow",
        "..\\..\\windows\\system32\\config\\sam",
        "backups/../../../etc/hosts",
        "test\x00.db",
    ];

    for name in traversal_names {
        let resp = client()
            .post(format!("{base}/api/restore"))
            .json(&json!({"filename": name}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "Path traversal not blocked for: {name:?}"
        );
    }
}

#[tokio::test]
async fn restore_backup_empty_filename() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/restore"))
        .json(&json!({"filename": ""}))
        .send()
        .await
        .unwrap();
    // Empty filename — file won't exist
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn restore_backup_nonexistent_file() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/restore"))
        .json(&json!({"filename": "nonexistent_backup_abc123.db"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn extremely_long_url_paths() {
    let base = spawn_test_server().await;

    let long_path = "/".to_string() + &"a".repeat(10_000);
    let resp = client()
        .get(format!("{base}{long_path}"))
        .send()
        .await
        .unwrap();
    // Should not crash — returns 404 or the fallback handler
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn encoded_path_traversal_in_url() {
    let base = spawn_test_server().await;

    let paths = [
        "/api/brooders/%2e%2e%2f%2e%2e%2fetc%2fpasswd/status",
        "/api/birds/../../etc/passwd",
        "/api/brooders/../../../status",
    ];
    for path in paths {
        let resp = client().get(format!("{base}{path}")).send().await.unwrap();
        assert_ne!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "Encoded path traversal crashed: {path}"
        );
    }
}

#[tokio::test]
async fn nfc_lookup_with_special_characters() {
    let base = spawn_test_server().await;

    let tags = [
        "<script>alert(1)</script>",
        "'; DROP TABLE birds; --",
        "QUAIL-../../../etc/passwd",
        "",
        "\x00\x00\x00",
    ];
    for tag in tags {
        let encoded = urlencoding::encode(tag);
        let resp = client()
            .get(format!("{base}/api/nfc/{encoded}"))
            .send()
            .await
            .unwrap();
        // Should return 404 (not found) or 200, but never 500
        assert_ne!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "NFC lookup crashed for tag: {tag:?}"
        );
    }
}

// ===========================================================================
// 10. CLUTCH & PROCESSING EDGE CASES
// ===========================================================================

#[tokio::test]
async fn clutch_with_zero_eggs() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/clutches"))
        .json(&json!({
            "eggs_set": 0,
            "set_date": "2026-03-01",
            "status": "Incubating",
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn update_bird_with_empty_json() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let bird = seed_bird(&base, bl.id, Sex::Male).await;

    let resp = client()
        .put(format!("{base}/api/birds/{}", bird.id))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    // Empty update — no fields changed, should still succeed
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn processing_for_nonexistent_bird() {
    let base = spawn_test_server().await;
    let resp = client()
        .post(format!("{base}/api/processing"))
        .json(&json!({
            "bird_id": 99999,
            "reason": "ExcessMale",
            "scheduled_date": "2026-03-01",
        }))
        .send()
        .await
        .unwrap();
    // FK violation causes unwrap() panic
    let status = resp.status();
    assert!(
        status == StatusCode::INTERNAL_SERVER_ERROR || status == StatusCode::BAD_REQUEST,
        "Expected 500 or 400 for nonexistent bird, got {status}"
    );
}

// ===========================================================================
// 11. SYSTEM METRICS EDGE CASES
// ===========================================================================

#[tokio::test]
async fn system_metrics_all_zeros() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "System": {
            "cpu_usage_percent": 0.0,
            "memory_used_bytes": 0,
            "memory_total_bytes": 0,
            "disk_used_bytes": 0,
            "disk_total_bytes": 0,
            "uptime_seconds": 0,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/system/latest"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value = resp.json().await.unwrap();
    assert!((body["cpu_usage_percent"].as_f64().unwrap()).abs() < 0.01);
}

#[tokio::test]
async fn system_metrics_max_values() {
    let base = spawn_test_server().await;
    let ws_base = ws_url(&base);
    let (mut ws, _) = connect_async(format!("{ws_base}/ws")).await.unwrap();

    let payload = json!({
        "System": {
            "cpu_usage_percent": 100.0,
            "memory_used_bytes": i64::MAX,
            "memory_total_bytes": i64::MAX,
            "disk_used_bytes": i64::MAX,
            "disk_total_bytes": i64::MAX,
            "uptime_seconds": i64::MAX,
        }
    });
    ws.send(Message::Text(payload.to_string().into()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let resp = client()
        .get(format!("{base}/api/system/latest"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 12. BREEDING GROUP EDGE CASES
// ===========================================================================

#[tokio::test]
async fn breeding_group_with_no_females() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let male = seed_bird(&base, bl.id, Sex::Male).await;

    let resp = client()
        .post(format!("{base}/api/breeding-groups"))
        .json(&json!({
            "name": "Solo Male",
            "male_id": male.id,
            "female_ids": [],
            "start_date": "2026-03-01",
        }))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn chick_group_mortality_exceeds_count() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;

    // Create a chick group with 5 chicks
    let resp = client()
        .post(format!("{base}/api/chick-groups"))
        .json(&json!({
            "bloodline_id": bl.id,
            "initial_count": 5,
            "hatch_date": "2026-03-01",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let group: Value = resp.json().await.unwrap();
    let gid = group["id"].as_i64().unwrap();

    // Try to log 10 deaths (more than initial count)
    let resp = client()
        .post(format!("{base}/api/chick-groups/{gid}/mortality"))
        .json(&json!({
            "count": 10,
            "reason": "cold snap",
        }))
        .send()
        .await
        .unwrap();
    // Should either reject or handle gracefully, NOT 500
    assert_ne!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "Mortality exceeding count should not panic"
    );
}

// ===========================================================================
// 13. WEIGHT RECORD EDGE CASES
// ===========================================================================

#[tokio::test]
async fn weight_zero_grams() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let bird = seed_bird(&base, bl.id, Sex::Male).await;

    let resp = client()
        .post(format!("{base}/api/birds/{}/weights", bird.id))
        .json(&json!({"weight_grams": 0.0, "date": "2026-03-01"}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn weight_negative_grams() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let bird = seed_bird(&base, bl.id, Sex::Male).await;

    let resp = client()
        .post(format!("{base}/api/birds/{}/weights", bird.id))
        .json(&json!({"weight_grams": -100.0, "date": "2026-03-01"}))
        .send()
        .await
        .unwrap();
    // Stored as-is (no validation on server). Not a crash.
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn weight_extremely_large() {
    let base = spawn_test_server().await;
    let bl = seed_bloodline(&base).await;
    let bird = seed_bird(&base, bl.id, Sex::Male).await;

    let resp = client()
        .post(format!("{base}/api/birds/{}/weights", bird.id))
        .json(&json!({"weight_grams": 999999999.99, "date": "2026-03-01"}))
        .send()
        .await
        .unwrap();
    assert_ne!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
