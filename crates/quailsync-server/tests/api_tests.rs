use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{
    Bird, BirdStatus, Bloodline, ChickGroup, CreateBird, CreateBloodline, CreateChickGroup,
    GraduateBird, GraduateRequest, InbreedingCoefficient, Sex,
};
use quailsync_server::{build_app, init_db, AppState};
use rusqlite::Connection;

/// Spin up a test server on a random port with a fresh in-memory DB.
/// Returns the base URL (e.g. "http://127.0.0.1:12345").
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
        alert_config: quailsync_common::AlertConfig::default(),
        live_tx,
        last_seen: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        metrics_handle,
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

// ---------------------------------------------------------------------------
// Health
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_returns_ok() {
    let base = spawn_test_server().await;

    let resp = reqwest::get(format!("{base}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let body = resp.text().await.unwrap();
    assert_eq!(body, "quailsync-server ok");
}

// ---------------------------------------------------------------------------
// Bloodlines: POST then GET
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_list_bloodlines() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // POST a bloodline
    let create = CreateBloodline {
        name: "Texas A&M".into(),
        source: "Oregon".into(),
        notes: Some("white feathers".into()),
    };
    let resp = client
        .post(format!("{base}/api/bloodlines"))
        .json(&create)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let created: Bloodline = resp.json().await.unwrap();
    assert_eq!(created.id, 1);
    assert_eq!(created.name, "Texas A&M");
    assert_eq!(created.source, "Oregon");
    assert_eq!(created.notes.as_deref(), Some("white feathers"));

    // POST a second bloodline
    let create2 = CreateBloodline {
        name: "Pharaoh".into(),
        source: "California".into(),
        notes: None,
    };
    let resp2 = client
        .post(format!("{base}/api/bloodlines"))
        .json(&create2)
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 201);

    // GET all bloodlines
    let resp = reqwest::get(format!("{base}/api/bloodlines"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let list: Vec<Bloodline> = resp.json().await.unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].name, "Texas A&M");
    assert_eq!(list[1].name, "Pharaoh");
}

// ---------------------------------------------------------------------------
// Birds: POST then GET
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_list_birds() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Need a bloodline first
    let bl = CreateBloodline {
        name: "Coturnix".into(),
        source: "Local".into(),
        notes: None,
    };
    client
        .post(format!("{base}/api/bloodlines"))
        .json(&bl)
        .send()
        .await
        .unwrap();

    // POST a bird
    let bird = CreateBird {
        band_color: Some("red".into()),
        sex: Sex::Male,
        bloodline_id: 1,
        hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        mother_id: None,
        father_id: None,
        generation: 1,
        status: BirdStatus::Active,
        notes: None,
        nfc_tag_id: None,
    };
    let resp = client
        .post(format!("{base}/api/birds"))
        .json(&bird)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let created: Bird = resp.json().await.unwrap();
    assert_eq!(created.id, 1);
    assert_eq!(created.sex, Sex::Male);
    assert_eq!(created.band_color.as_deref(), Some("red"));

    // GET all birds
    let resp = reqwest::get(format!("{base}/api/birds")).await.unwrap();
    let list: Vec<Bird> = resp.json().await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, 1);
}

// ---------------------------------------------------------------------------
// Breeding suggest: same bloodline → 0.25
// ---------------------------------------------------------------------------

#[tokio::test]
async fn breeding_suggest_same_bloodline() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // One bloodline
    client
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "A".into(),
            source: "X".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();

    // Male in bloodline 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            bloodline_id: 1,
            hatch_date: today,
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

    // Female in same bloodline 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            bloodline_id: 1,
            hatch_date: today,
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

    let resp = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let pairs: Vec<InbreedingCoefficient> = resp.json().await.unwrap();
    assert_eq!(pairs.len(), 1);
    assert!((pairs[0].coefficient - 0.25).abs() < f64::EPSILON);
    assert!(!pairs[0].safe); // 0.25 >= 0.0625
}

// ---------------------------------------------------------------------------
// Breeding suggest: different bloodlines → 0.0
// ---------------------------------------------------------------------------

#[tokio::test]
async fn breeding_suggest_different_bloodlines() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Two bloodlines
    for name in ["A", "B"] {
        client
            .post(format!("{base}/api/bloodlines"))
            .json(&CreateBloodline {
                name: name.into(),
                source: "X".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap();
    }

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();

    // Male in bloodline 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            bloodline_id: 1,
            hatch_date: today,
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

    // Female in bloodline 2
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            bloodline_id: 2,
            hatch_date: today,
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

    let resp = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap();
    let pairs: Vec<InbreedingCoefficient> = resp.json().await.unwrap();
    assert_eq!(pairs.len(), 1);
    assert!((pairs[0].coefficient - 0.0).abs() < f64::EPSILON);
    assert!(pairs[0].safe); // 0.0 < 0.0625
}

// ---------------------------------------------------------------------------
// Breeding suggest: full siblings → 0.5
// ---------------------------------------------------------------------------

#[tokio::test]
async fn breeding_suggest_full_siblings() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    client
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "A".into(),
            source: "X".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();

    // Father (id=1)
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            bloodline_id: 1,
            hatch_date: today,
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

    // Mother (id=2)
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            bloodline_id: 1,
            hatch_date: today,
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

    // Son (id=3) — shares both parents
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            bloodline_id: 1,
            hatch_date: today,
            mother_id: Some(2),
            father_id: Some(1),
            generation: 2,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
        })
        .send()
        .await
        .unwrap();

    // Daughter (id=4) — shares both parents
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            bloodline_id: 1,
            hatch_date: today,
            mother_id: Some(2),
            father_id: Some(1),
            generation: 2,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
        })
        .send()
        .await
        .unwrap();

    let resp = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap();
    let pairs: Vec<InbreedingCoefficient> = resp.json().await.unwrap();

    // Find the son×daughter pairing (id=3 × id=4)
    let sibling_pair = pairs
        .iter()
        .find(|p| p.male_id == 3 && p.female_id == 4)
        .expect("should have son×daughter pair");
    assert!((sibling_pair.coefficient - 0.5).abs() < f64::EPSILON);
    assert!(!sibling_pair.safe);
}

// ---------------------------------------------------------------------------
// Chick groups: GET /api/chick-groups returns is_ready_to_transition correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn chick_groups_expose_is_ready_to_transition() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    client
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "Coturnix".into(),
            source: "Local".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();

    let today = chrono::Local::now().date_naive();
    let young_hatch = today - chrono::Duration::days(34);
    let mature_hatch = today - chrono::Duration::days(35);

    for hatch in [young_hatch, mature_hatch] {
        client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                bloodline_id: 1,
                brooder_id: None,
                initial_count: 10,
                hatch_date: hatch,
                notes: None,
            })
            .send()
            .await
            .unwrap();
    }

    let resp = reqwest::get(format!("{base}/api/chick-groups"))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Assert the field is serialized in the raw JSON.
    let raw: serde_json::Value = resp.json().await.unwrap();
    let arr = raw.as_array().expect("expected array");
    assert_eq!(arr.len(), 2);
    for g in arr {
        assert!(
            g.get("is_ready_to_transition").is_some(),
            "missing is_ready_to_transition field: {g}"
        );
    }

    let groups: Vec<ChickGroup> = serde_json::from_value(raw).unwrap();
    let young = groups
        .iter()
        .find(|g| g.hatch_date == young_hatch)
        .expect("young group");
    let mature = groups
        .iter()
        .find(|g| g.hatch_date == mature_hatch)
        .expect("mature group");
    assert!(
        !young.is_ready_to_transition,
        "day-34 group should not be ready"
    );
    assert!(
        mature.is_ready_to_transition,
        "day-35 group should be ready"
    );
}

// ---------------------------------------------------------------------------
// Chick groups: PUT /api/chick-groups/{id}/graduate creates birds with unique
// IDs and persists optional weight + photo intake fields.
// Regression test for the "all birds share the same ID" summary-screen bug
// (the summary had a state-management/key bug; the data layer must produce
// unique ids regardless).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn graduate_creates_birds_with_unique_ids_and_intake_fields() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    client
        .post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline {
            name: "Coturnix".into(),
            source: "Local".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();

    let today = chrono::Local::now().date_naive();
    let group: ChickGroup = client
        .post(format!("{base}/api/chick-groups"))
        .json(&CreateChickGroup {
            clutch_id: None,
            bloodline_id: 1,
            brooder_id: None,
            initial_count: 3,
            hatch_date: today - chrono::Duration::days(40),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Mixed payload: bird 0 has weight + photo, bird 1 has nothing extra,
    // bird 2 has only weight. Tests that backwards-compatible defaults work.
    let req = GraduateRequest {
        birds: vec![
            GraduateBird {
                sex: Sex::Male,
                band_color: Some("blue".into()),
                nfc_tag_id: Some("TAG-A".into()),
                notes: None,
                weight_grams: Some(142.5),
                photo_path: Some("bird_photos/grad_a.jpg".into()),
            },
            GraduateBird {
                sex: Sex::Female,
                band_color: Some("red".into()),
                nfc_tag_id: Some("TAG-B".into()),
                notes: None,
                weight_grams: None,
                photo_path: None,
            },
            GraduateBird {
                sex: Sex::Unknown,
                band_color: None,
                nfc_tag_id: Some("TAG-C".into()),
                notes: Some("note".into()),
                weight_grams: Some(160.0),
                photo_path: None,
            },
        ],
    };

    let resp = client
        .post(format!("{base}/api/chick-groups/{}/graduate", group.id))
        .json(&req)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let birds: Vec<Bird> = resp.json().await.unwrap();
    assert_eq!(birds.len(), 3);

    // Each bird must have a unique DB id and unique NFC tag.
    let ids: std::collections::HashSet<i64> = birds.iter().map(|b| b.id).collect();
    assert_eq!(ids.len(), 3, "graduated birds must have unique ids");
    let tags: std::collections::HashSet<&Option<String>> =
        birds.iter().map(|b| &b.nfc_tag_id).collect();
    assert_eq!(tags.len(), 3, "graduated birds must have unique tags");

    // Optional fields are reflected on the response.
    let bird_a = birds.iter().find(|b| b.nfc_tag_id.as_deref() == Some("TAG-A")).unwrap();
    assert_eq!(bird_a.photo_path.as_deref(), Some("bird_photos/grad_a.jpg"));
    assert_eq!(bird_a.sex, Sex::Male);
    let bird_b = birds.iter().find(|b| b.nfc_tag_id.as_deref() == Some("TAG-B")).unwrap();
    assert!(bird_b.photo_path.is_none());

    // Weight history: bird A and bird C should each have one weight_record;
    // bird B should have none.
    let weights_a: Vec<serde_json::Value> = client
        .get(format!("{base}/api/birds/{}/weights", bird_a.id))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(weights_a.len(), 1, "bird A should have its initial weight logged");
    assert!((weights_a[0]["weight_grams"].as_f64().unwrap() - 142.5).abs() < f64::EPSILON);

    let weights_b: Vec<serde_json::Value> = client
        .get(format!("{base}/api/birds/{}/weights", bird_b.id))
        .send().await.unwrap().json().await.unwrap();
    assert!(weights_b.is_empty(), "bird B had no weight_grams in payload");

    // Status should be Active and group should be flipped to Graduated.
    assert!(birds.iter().all(|b| b.status == BirdStatus::Active));
    let groups: Vec<ChickGroup> = client
        .get(format!("{base}/api/chick-groups"))
        .send().await.unwrap().json().await.unwrap();
    let g = groups.iter().find(|g| g.id == group.id).unwrap();
    assert_eq!(format!("{:?}", g.status), "Graduated");
}

// Backwards-compat: a payload that omits the new optional fields still works
// (existing CLI/API clients).
#[tokio::test]
async fn graduate_accepts_payload_without_new_fields() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    client.post(format!("{base}/api/bloodlines"))
        .json(&CreateBloodline { name: "X".into(), source: "L".into(), notes: None })
        .send().await.unwrap();

    let today = chrono::Local::now().date_naive();
    let group: ChickGroup = client.post(format!("{base}/api/chick-groups"))
        .json(&CreateChickGroup {
            clutch_id: None, bloodline_id: 1, brooder_id: None,
            initial_count: 2, hatch_date: today - chrono::Duration::days(40), notes: None,
        })
        .send().await.unwrap().json().await.unwrap();

    // Send a raw JSON payload with only the legacy fields (no weight_grams / photo_path).
    let raw_payload = serde_json::json!({
        "birds": [
            { "sex": "Male",   "band_color": null, "nfc_tag_id": null, "notes": null },
            { "sex": "Female", "band_color": null, "nfc_tag_id": null, "notes": null },
        ],
    });
    let resp = client
        .post(format!("{base}/api/chick-groups/{}/graduate", group.id))
        .json(&raw_payload)
        .send().await.unwrap();
    assert_eq!(resp.status(), 200, "legacy payload must still graduate");
    let birds: Vec<Bird> = resp.json().await.unwrap();
    assert_eq!(birds.len(), 2);
    assert!(birds.iter().all(|b| b.photo_path.is_none()));
}

// ---------------------------------------------------------------------------
// System alerts (POST /api/alerts collapse, resolve, dismiss, listings)
// ---------------------------------------------------------------------------

mod system_alerts_tests {
    use super::*;
    use quailsync_common::{
        CreateSystemAlert, ResolveSystemAlertRequest, ResolveSystemAlertResponse, SystemAlert,
    };

    fn sample(key: &str, msg: &str) -> CreateSystemAlert {
        CreateSystemAlert {
            alert_key: key.into(),
            severity: "critical".into(),
            title: "Backup failed".into(),
            message: msg.into(),
            source: "nightly-backup".into(),
            metadata_json: None,
        }
    }

    #[tokio::test]
    async fn duplicate_alert_key_collapses_to_one_row() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        // First post — fresh row, expect 201.
        let resp1 = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "first failure"))
            .send().await.unwrap();
        assert_eq!(resp1.status(), 201);
        let first: SystemAlert = resp1.json().await.unwrap();
        assert!(first.is_active);

        // Second post with same key — should update in place, return 200.
        let resp2 = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "second failure"))
            .send().await.unwrap();
        assert_eq!(resp2.status(), 200);
        let second: SystemAlert = resp2.json().await.unwrap();
        assert_eq!(second.id, first.id, "should reuse the existing row");
        assert_eq!(second.message, "second failure");
        assert!(second.is_active);

        // metadata_json should now contain occurrences=2.
        let meta: serde_json::Value = serde_json::from_str(
            second.metadata_json.as_deref().expect("metadata populated on collapse"),
        ).unwrap();
        assert_eq!(meta.get("occurrences").and_then(|v| v.as_i64()), Some(2));

        // Active list should have exactly one row.
        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await.unwrap().json().await.unwrap();
        assert_eq!(active.len(), 1);
    }

    #[tokio::test]
    async fn resolve_clears_active_state() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        client.post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "boom")).send().await.unwrap();

        let resp = client.post(format!("{base}/api/alerts/resolve"))
            .json(&ResolveSystemAlertRequest { alert_key: "backup_failed".into() })
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let body: ResolveSystemAlertResponse = resp.json().await.unwrap();
        assert_eq!(body.resolved, 1);

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await.unwrap().json().await.unwrap();
        assert!(active.is_empty(), "resolved alert should not appear in active list");

        // The row is still in the recent list, just with resolved_at set.
        let recent: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/recent"))
            .await.unwrap().json().await.unwrap();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].resolved_at.is_some());
        assert!(!recent[0].is_active);
    }

    #[tokio::test]
    async fn dismiss_clears_active_independently_of_resolve() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        let created: SystemAlert = client.post(format!("{base}/api/alerts"))
            .json(&sample("cleanup_failed", "disk full"))
            .send().await.unwrap()
            .json().await.unwrap();

        let resp = client.post(format!("{base}/api/alerts/{}/dismiss", created.id))
            .send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let dismissed: SystemAlert = resp.json().await.unwrap();
        assert!(dismissed.dismissed_at.is_some());
        assert!(dismissed.resolved_at.is_none(), "dismiss is independent of resolve");
        assert!(!dismissed.is_active);

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await.unwrap().json().await.unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn active_list_excludes_dismissed_and_resolved_but_includes_independent_ones() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        // Three independent alert keys.
        let a: SystemAlert = client.post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "a")).send().await.unwrap().json().await.unwrap();
        let _b: SystemAlert = client.post(format!("{base}/api/alerts"))
            .json(&sample("deadman_no_recent_backup", "b")).send().await.unwrap().json().await.unwrap();
        let c: SystemAlert = client.post(format!("{base}/api/alerts"))
            .json(&sample("cleanup_failed", "c")).send().await.unwrap().json().await.unwrap();

        // Resolve "a", dismiss "c". "b" remains active.
        client.post(format!("{base}/api/alerts/resolve"))
            .json(&ResolveSystemAlertRequest { alert_key: a.alert_key.clone() })
            .send().await.unwrap();
        client.post(format!("{base}/api/alerts/{}/dismiss", c.id)).send().await.unwrap();

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await.unwrap().json().await.unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].alert_key, "deadman_no_recent_backup");

        // Recent should still surface all three.
        let recent: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/recent?limit=10"))
            .await.unwrap().json().await.unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn dismiss_unknown_id_returns_404() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = client.post(format!("{base}/api/alerts/9999/dismiss")).send().await.unwrap();
        assert_eq!(resp.status(), 404);
    }
}
