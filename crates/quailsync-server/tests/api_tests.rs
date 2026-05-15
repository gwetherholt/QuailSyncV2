use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{
    Bird, BirdStatus, ChickGroup, CreateBird, CreateChickGroup, CreateLineage, GraduateBird,
    GraduateRequest, InbreedingCoefficient, Lineage, Sex,
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
// Lineages: POST then GET
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_list_lineages() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // POST a lineage
    let create = CreateLineage {
        name: "Texas A&M".into(),
        source: "Oregon".into(),
        notes: Some("white feathers".into()),
    };
    let resp = client
        .post(format!("{base}/api/lineages"))
        .json(&create)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);

    let created: Lineage = resp.json().await.unwrap();
    assert_eq!(created.id, 1);
    assert_eq!(created.name, "Texas A&M");
    assert_eq!(created.source, "Oregon");
    assert_eq!(created.notes.as_deref(), Some("white feathers"));

    // POST a second lineage
    let create2 = CreateLineage {
        name: "Pharaoh".into(),
        source: "California".into(),
        notes: None,
    };
    let resp2 = client
        .post(format!("{base}/api/lineages"))
        .json(&create2)
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), 201);

    // GET all lineages
    let resp = reqwest::get(format!("{base}/api/lineages")).await.unwrap();
    assert_eq!(resp.status(), 200);

    let list: Vec<Lineage> = resp.json().await.unwrap();
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

    // Need a lineage first
    let bl = CreateLineage {
        name: "Coturnix".into(),
        source: "Local".into(),
        notes: None,
    };
    client
        .post(format!("{base}/api/lineages"))
        .json(&bl)
        .send()
        .await
        .unwrap();

    // POST a bird
    let bird = CreateBird {
        band_color: Some("red".into()),
        sex: Sex::Male,
        lineage_ids: vec![1],
        hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 15).unwrap(),
        mother_id: None,
        father_id: None,
        generation: 1,
        status: BirdStatus::Active,
        notes: None,
        nfc_tag_id: None,
        chick_group_id: None,
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
// Breeding suggest: same lineage → 0.25
// ---------------------------------------------------------------------------

#[tokio::test]
async fn breeding_suggest_same_lineage() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // One lineage
    client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: "A".into(),
            source: "X".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap();

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();

    // Male in lineage 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            lineage_ids: vec![1],
            hatch_date: today,
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
        .unwrap();

    // Female in same lineage 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            lineage_ids: vec![1],
            hatch_date: today,
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
// Breeding suggest: different lineages → 0.0
// ---------------------------------------------------------------------------

#[tokio::test]
async fn breeding_suggest_different_lineages() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Two lineages
    for name in ["A", "B"] {
        client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: name.into(),
                source: "X".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap();
    }

    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();

    // Male in lineage 1
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            lineage_ids: vec![1],
            hatch_date: today,
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
        .unwrap();

    // Female in lineage 2
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            lineage_ids: vec![2],
            hatch_date: today,
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
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
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
            lineage_ids: vec![1],
            hatch_date: today,
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
        .unwrap();

    // Mother (id=2)
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Female,
            lineage_ids: vec![1],
            hatch_date: today,
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
        .unwrap();

    // Son (id=3) — shares both parents
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex: Sex::Male,
            lineage_ids: vec![1],
            hatch_date: today,
            mother_id: Some(2),
            father_id: Some(1),
            generation: 2,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
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
            lineage_ids: vec![1],
            hatch_date: today,
            mother_id: Some(2),
            father_id: Some(1),
            generation: 2,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
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
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
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
                lineage_ids: vec![1],
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
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
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
            lineage_ids: vec![1],
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
        target_housing_id: None,
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
    let bird_a = birds
        .iter()
        .find(|b| b.nfc_tag_id.as_deref() == Some("TAG-A"))
        .unwrap();
    assert_eq!(bird_a.photo_path.as_deref(), Some("bird_photos/grad_a.jpg"));
    assert_eq!(bird_a.sex, Sex::Male);
    let bird_b = birds
        .iter()
        .find(|b| b.nfc_tag_id.as_deref() == Some("TAG-B"))
        .unwrap();
    assert!(bird_b.photo_path.is_none());

    // Weight history: bird A and bird C should each have one weight_record;
    // bird B should have none.
    let weights_a: Vec<serde_json::Value> = client
        .get(format!("{base}/api/birds/{}/weights", bird_a.id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        weights_a.len(),
        1,
        "bird A should have its initial weight logged"
    );
    assert!((weights_a[0]["weight_grams"].as_f64().unwrap() - 142.5).abs() < f64::EPSILON);

    let weights_b: Vec<serde_json::Value> = client
        .get(format!("{base}/api/birds/{}/weights", bird_b.id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        weights_b.is_empty(),
        "bird B had no weight_grams in payload"
    );

    // Status should be Active and group should be flipped to Graduated.
    assert!(birds.iter().all(|b| b.status == BirdStatus::Active));
    let groups: Vec<ChickGroup> = client
        .get(format!("{base}/api/chick-groups"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let g = groups.iter().find(|g| g.id == group.id).unwrap();
    assert_eq!(format!("{:?}", g.status), "Graduated");
}

// Backwards-compat: a payload that omits the new optional fields still works
// (existing CLI/API clients).
#[tokio::test]
async fn graduate_accepts_payload_without_new_fields() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: "X".into(),
            source: "L".into(),
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
            lineage_ids: vec![1],
            brooder_id: None,
            initial_count: 2,
            hatch_date: today - chrono::Duration::days(40),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

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
        .send()
        .await
        .unwrap();
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
            .send()
            .await
            .unwrap();
        assert_eq!(resp1.status(), 201);
        let first: SystemAlert = resp1.json().await.unwrap();
        assert!(first.is_active);

        // Second post with same key — should update in place, return 200.
        let resp2 = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "second failure"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp2.status(), 200);
        let second: SystemAlert = resp2.json().await.unwrap();
        assert_eq!(second.id, first.id, "should reuse the existing row");
        assert_eq!(second.message, "second failure");
        assert!(second.is_active);

        // metadata_json should now contain occurrences=2.
        let meta: serde_json::Value = serde_json::from_str(
            second
                .metadata_json
                .as_deref()
                .expect("metadata populated on collapse"),
        )
        .unwrap();
        assert_eq!(meta.get("occurrences").and_then(|v| v.as_i64()), Some(2));

        // Active list should have exactly one row.
        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
    }

    #[tokio::test]
    async fn resolve_clears_active_state() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        client
            .post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "boom"))
            .send()
            .await
            .unwrap();

        let resp = client
            .post(format!("{base}/api/alerts/resolve"))
            .json(&ResolveSystemAlertRequest {
                alert_key: "backup_failed".into(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: ResolveSystemAlertResponse = resp.json().await.unwrap();
        assert_eq!(body.resolved, 1);

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(
            active.is_empty(),
            "resolved alert should not appear in active list"
        );

        // The row is still in the recent list, just with resolved_at set.
        let recent: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/recent"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert!(recent[0].resolved_at.is_some());
        assert!(!recent[0].is_active);
    }

    #[tokio::test]
    async fn dismiss_clears_active_independently_of_resolve() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        let created: SystemAlert = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("cleanup_failed", "disk full"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let resp = client
            .post(format!("{base}/api/alerts/{}/dismiss", created.id))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let dismissed: SystemAlert = resp.json().await.unwrap();
        assert!(dismissed.dismissed_at.is_some());
        assert!(
            dismissed.resolved_at.is_none(),
            "dismiss is independent of resolve"
        );
        assert!(!dismissed.is_active);

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(active.is_empty());
    }

    #[tokio::test]
    async fn active_list_excludes_dismissed_and_resolved_but_includes_independent_ones() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        // Three independent alert keys.
        let a: SystemAlert = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("backup_failed", "a"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let _b: SystemAlert = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("deadman_no_recent_backup", "b"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let c: SystemAlert = client
            .post(format!("{base}/api/alerts"))
            .json(&sample("cleanup_failed", "c"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // Resolve "a", dismiss "c". "b" remains active.
        client
            .post(format!("{base}/api/alerts/resolve"))
            .json(&ResolveSystemAlertRequest {
                alert_key: a.alert_key.clone(),
            })
            .send()
            .await
            .unwrap();
        client
            .post(format!("{base}/api/alerts/{}/dismiss", c.id))
            .send()
            .await
            .unwrap();

        let active: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/active"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].alert_key, "deadman_no_recent_backup");

        // Recent should still surface all three.
        let recent: Vec<SystemAlert> = reqwest::get(format!("{base}/api/alerts/recent?limit=10"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(recent.len(), 3);
    }

    #[tokio::test]
    async fn dismiss_unknown_id_returns_404() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/api/alerts/9999/dismiss"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }
}

// ---------------------------------------------------------------------------
// Many-to-many lineage tests (Approach 3 — Hybrid)
// ---------------------------------------------------------------------------

mod lineage_tests {
    use super::*;
    use quailsync_common::ReplaceLineagesRequest;

    async fn seed_two_lineages(base: &str, client: &reqwest::Client) -> (Lineage, Lineage) {
        let a: Lineage = client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: "A".into(),
                source: "S".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let b: Lineage = client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: "B".into(),
                source: "S".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        (a, b)
    }

    #[tokio::test]
    async fn create_chick_group_with_empty_lineage_ids_returns_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/api/chick-groups"))
            .json(&serde_json::json!({
                "lineage_ids": [],
                "initial_count": 5,
                "hatch_date": "2026-03-01",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn create_chick_group_with_single_lineage_succeeds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (a, _b) = seed_two_lineages(&base, &client).await;
        let resp = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: None,
                initial_count: 7,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                notes: None,
                lineage_ids: vec![a.id],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let g: ChickGroup = resp.json().await.unwrap();
        assert_eq!(g.lineages.len(), 1);
        assert_eq!(g.lineages[0].id, a.id);
        assert_eq!(g.lineages[0].name, "A");
    }

    #[tokio::test]
    async fn create_chick_group_with_multiple_lineages_succeeds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (a, b) = seed_two_lineages(&base, &client).await;
        let resp = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: None,
                initial_count: 7,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                notes: None,
                lineage_ids: vec![a.id, b.id],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let g: ChickGroup = resp.json().await.unwrap();
        let mut names: Vec<String> = g.lineages.iter().map(|l| l.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["A".to_string(), "B".to_string()]);
    }

    #[tokio::test]
    async fn put_lineages_replaces_set_atomically() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (a, b) = seed_two_lineages(&base, &client).await;
        let g: ChickGroup = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: None,
                initial_count: 5,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                notes: None,
                lineage_ids: vec![a.id],
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(g.lineages.len(), 1);

        // Replace [a] with [a, b].
        let resp = client
            .put(format!("{base}/api/chick-groups/{}/lineages", g.id))
            .json(&ReplaceLineagesRequest {
                lineage_ids: vec![a.id, b.id],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let updated: ChickGroup = resp.json().await.unwrap();
        assert_eq!(updated.lineages.len(), 2);

        // Replace with [b] only — should remove a.
        let resp = client
            .put(format!("{base}/api/chick-groups/{}/lineages", g.id))
            .json(&ReplaceLineagesRequest {
                lineage_ids: vec![b.id],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let updated: ChickGroup = resp.json().await.unwrap();
        assert_eq!(updated.lineages.len(), 1);
        assert_eq!(updated.lineages[0].id, b.id);
    }

    #[tokio::test]
    async fn put_lineages_with_empty_returns_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (a, _b) = seed_two_lineages(&base, &client).await;
        let g: ChickGroup = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: None,
                initial_count: 5,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
                notes: None,
                lineage_ids: vec![a.id],
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let resp = client
            .put(format!("{base}/api/chick-groups/{}/lineages", g.id))
            .json(&ReplaceLineagesRequest {
                lineage_ids: vec![],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn create_bird_with_empty_lineage_ids_returns_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/api/birds"))
            .json(&serde_json::json!({
                "sex": "Male",
                "lineage_ids": [],
                "hatch_date": "2026-01-01",
                "generation": 1,
                "status": "Active",
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn legacy_bloodline_id_rows_migrate_into_junction() {
        // Seed an OLD-shape DB (bloodlines table, chick_groups.bloodline_id,
        // birds.bloodline_id NOT NULL), then run init_db and assert that the
        // junction tables end up populated and the old columns are gone.
        use quailsync_server::init_db;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE bloodlines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                source TEXT NOT NULL,
                notes TEXT
            );
             CREATE TABLE birds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                band_color TEXT,
                sex TEXT NOT NULL,
                bloodline_id INTEGER NOT NULL REFERENCES bloodlines(id),
                hatch_date TEXT NOT NULL,
                mother_id INTEGER,
                father_id INTEGER,
                generation INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'Active',
                notes TEXT,
                nfc_tag_id TEXT UNIQUE
            );
             CREATE TABLE brooders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                bloodline_id INTEGER REFERENCES bloodlines(id),
                life_stage TEXT NOT NULL DEFAULT 'Chick',
                qr_code TEXT NOT NULL DEFAULT '',
                notes TEXT
            );
             CREATE TABLE chick_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clutch_id INTEGER,
                bloodline_id INTEGER NOT NULL REFERENCES bloodlines(id),
                brooder_id INTEGER,
                initial_count INTEGER NOT NULL,
                current_count INTEGER NOT NULL,
                hatch_date TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Active',
                notes TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bloodlines (name, source) VALUES ('Fernbank', 'Local')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bloodlines (name, source) VALUES ('NWQuail', 'NW')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO birds (sex, bloodline_id, hatch_date) VALUES ('Male', 1, '2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO birds (sex, bloodline_id, hatch_date) VALUES ('Female', 2, '2026-01-01')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO chick_groups (bloodline_id, initial_count, current_count, hatch_date)
             VALUES (1, 10, 10, '2026-03-01')",
            [],
        )
        .unwrap();

        // Run the migration.
        init_db(&conn);

        // The legacy table is gone; the new one carries the same rows.
        let lineage_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM lineages", [], |r| r.get(0))
            .unwrap();
        assert_eq!(lineage_count, 2);

        // Old bloodline_id columns are dropped.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(birds)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(!cols.contains(&"bloodline_id".to_string()));

        // Junction rows match the legacy assignments.
        let bird_lineage_pairs: Vec<(i64, i64)> = conn
            .prepare("SELECT bird_id, lineage_id FROM bird_lineages ORDER BY bird_id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(bird_lineage_pairs, vec![(1, 1), (2, 2)]);

        let group_lineage_pairs: Vec<(i64, i64)> = conn
            .prepare("SELECT chick_group_id, lineage_id FROM chick_group_lineages")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(group_lineage_pairs, vec![(1, 1)]);

        // Brooders renamed in place — column is now lineage_id.
        let brooder_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(brooders)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(brooder_cols.contains(&"lineage_id".to_string()));
        assert!(!brooder_cols.contains(&"bloodline_id".to_string()));
    }

    /// Regression test for the partially-applied migration on the live Pi.
    ///
    /// The earlier `legacy_bloodline_id_rows_migrate_into_junction` test
    /// seeded the old schema but did not create the `idx_birds_bloodline`
    /// secondary index that the original schema shipped with. Without that
    /// index, `ALTER TABLE birds DROP COLUMN bloodline_id` silently
    /// succeeds; with it, SQLite refuses and the buggy `.ok()` swallowed
    /// the error — leaving live DBs with an orphaned NOT NULL column that
    /// breaks every bird insert.
    ///
    /// This test seeds the *exact* broken state (column + blocking index +
    /// existing rows) and asserts that the corrective migration drops both
    /// the index and the column, preserves the data via the junction, and
    /// is idempotent across two `init_db` calls.
    #[tokio::test]
    async fn corrective_migration_drops_birds_bloodline_index_and_column() {
        use quailsync_server::init_db;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        // Seed the original (pre-refactor) schema, complete with the index
        // that blocks DROP COLUMN.
        conn.execute_batch(
            "CREATE TABLE bloodlines (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                source TEXT NOT NULL,
                notes TEXT
            );
             CREATE TABLE birds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                band_color TEXT,
                sex TEXT NOT NULL,
                bloodline_id INTEGER NOT NULL REFERENCES bloodlines(id),
                hatch_date TEXT NOT NULL,
                mother_id INTEGER,
                father_id INTEGER,
                generation INTEGER NOT NULL DEFAULT 1,
                status TEXT NOT NULL DEFAULT 'Active',
                notes TEXT,
                nfc_tag_id TEXT UNIQUE
            );
             CREATE INDEX idx_birds_bloodline ON birds(bloodline_id);
             CREATE TABLE brooders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                bloodline_id INTEGER REFERENCES bloodlines(id),
                life_stage TEXT NOT NULL DEFAULT 'Chick',
                qr_code TEXT NOT NULL DEFAULT '',
                notes TEXT
            );
             CREATE TABLE chick_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                clutch_id INTEGER,
                bloodline_id INTEGER NOT NULL REFERENCES bloodlines(id),
                brooder_id INTEGER,
                initial_count INTEGER NOT NULL,
                current_count INTEGER NOT NULL,
                hatch_date TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'Active',
                notes TEXT
            );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO bloodlines (name, source) VALUES ('Fernbank', 'Local')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO birds (sex, bloodline_id, hatch_date) VALUES ('Male', 1, '2026-01-01')",
            [],
        )
        .unwrap();

        // Sanity: the index exists in the seed before migration.
        let pre_index: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='idx_birds_bloodline'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(pre_index, 1, "seed should include idx_birds_bloodline");

        // Run the migration.
        init_db(&conn);

        // Column dropped.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(birds)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            !cols.contains(&"bloodline_id".to_string()),
            "birds.bloodline_id should be dropped, got cols: {cols:?}",
        );

        // Index dropped.
        let post_index: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type='index' AND name='idx_birds_bloodline'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(post_index, 0, "idx_birds_bloodline should be dropped");

        // Data preserved in the junction.
        let bird_lineage_pairs: Vec<(i64, i64)> = conn
            .prepare("SELECT bird_id, lineage_id FROM bird_lineages ORDER BY bird_id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(bird_lineage_pairs, vec![(1, 1)]);

        // Inserting a bird without bloodline_id now succeeds (the symptom we
        // were getting on the Pi was: NOT NULL constraint failed: birds.bloodline_id).
        conn.execute(
            "INSERT INTO birds (sex, hatch_date, status) VALUES ('Female', '2026-02-01', 'Active')",
            [],
        )
        .expect("insert into post-migration birds should succeed without bloodline_id");

        // Idempotency: running init_db a second time on the already-migrated
        // schema must not panic or change the result.
        init_db(&conn);
        let cols2: Vec<String> = conn
            .prepare("PRAGMA table_info(birds)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(!cols2.contains(&"bloodline_id".to_string()));
    }
}

// ---------------------------------------------------------------------------
// Issue #13 — hutch resident assignment (assign-birds / unassign-birds /
// residents-by-housing_id)
// ---------------------------------------------------------------------------

mod housing_assignment_tests {
    use super::*;
    use quailsync_common::{
        BirdAssignmentRequest, BirdAssignmentResponse, BrooderResidentsResponse, CreateBrooder,
        LifeStage,
    };
    use serde_json::json;

    /// Helper: seed a lineage + a hutch + two unhoused birds.
    async fn seed_hutch_and_birds(base: &str, client: &reqwest::Client) -> (i64, Vec<i64>) {
        // Lineage so birds have something to reference.
        let bl: Lineage = client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: "L".into(),
                source: "S".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // Hutch.
        let hutch: serde_json::Value = client
            .post(format!("{base}/api/brooders"))
            .json(&CreateBrooder {
                name: "Outdoor Hutch".into(),
                lineage_id: None,
                life_stage: LifeStage::Adult,
                qr_code: "hutch-1".into(),
                notes: None,
                camera_url: None,
                housing_type: Some(quailsync_common::HousingType::Hutch),
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let hutch_id = hutch["id"].as_i64().unwrap();

        // Two birds. Use the typed CreateBird struct to avoid leaning on the
        // dashboard's loose JSON shape.
        let mut bird_ids = Vec::new();
        for _ in 0..2 {
            let resp = client
                .post(format!("{base}/api/birds"))
                .json(&CreateBird {
                    band_color: None,
                    sex: Sex::Unknown,
                    lineage_ids: vec![bl.id],
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
                .unwrap();
            let b: Bird = resp.json().await.unwrap();
            bird_ids.push(b.id);
        }
        (hutch_id, bird_ids)
    }

    #[tokio::test]
    async fn assign_then_residents_returns_housed_birds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (hutch_id, bird_ids) = seed_hutch_and_birds(&base, &client).await;

        // Residents start empty.
        let pre: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert!(
            pre.individual_birds.is_empty(),
            "no residents before assignment"
        );

        // Assign both birds.
        let resp = client
            .post(format!("{base}/api/brooders/{hutch_id}/assign-birds"))
            .json(&BirdAssignmentRequest {
                bird_ids: bird_ids.clone(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: BirdAssignmentResponse = resp.json().await.unwrap();
        assert_eq!(body.updated, 2);

        // Residents now lists both.
        let post: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(post.individual_birds.len(), 2);
        let returned_ids: std::collections::HashSet<i64> =
            post.individual_birds.iter().map(|b| b.id).collect();
        assert_eq!(returned_ids, bird_ids.iter().copied().collect());
        // Each bird's housing_id is reflected in the DTO.
        for bird in &post.individual_birds {
            assert_eq!(bird.housing_id, Some(hutch_id));
        }
    }

    #[tokio::test]
    async fn unassign_clears_housing_and_drops_from_residents() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (hutch_id, bird_ids) = seed_hutch_and_birds(&base, &client).await;

        client
            .post(format!("{base}/api/brooders/{hutch_id}/assign-birds"))
            .json(&BirdAssignmentRequest {
                bird_ids: bird_ids.clone(),
            })
            .send()
            .await
            .unwrap();

        let resp = client
            .post(format!("{base}/api/brooders/{hutch_id}/unassign-birds"))
            .json(&BirdAssignmentRequest {
                bird_ids: vec![bird_ids[0]],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: BirdAssignmentResponse = resp.json().await.unwrap();
        assert_eq!(body.updated, 1);

        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(residents.individual_birds.len(), 1);
        assert_eq!(residents.individual_birds[0].id, bird_ids[1]);
    }

    #[tokio::test]
    async fn assign_to_nonexistent_housing_returns_404() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_h, bird_ids) = seed_hutch_and_birds(&base, &client).await;
        let resp = client
            .post(format!("{base}/api/brooders/9999/assign-birds"))
            .json(&BirdAssignmentRequest {
                bird_ids: bird_ids.clone(),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn assign_with_nonexistent_bird_returns_400_and_no_partial_write() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (hutch_id, bird_ids) = seed_hutch_and_birds(&base, &client).await;
        // Mix a valid id with an invalid one.
        let resp = client
            .post(format!("{base}/api/brooders/{hutch_id}/assign-birds"))
            .json(&BirdAssignmentRequest {
                bird_ids: vec![bird_ids[0], 99999],
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);

        // Confirm the valid bird was NOT assigned — validation must fail
        // before any writes happen.
        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert!(
            residents.individual_birds.is_empty(),
            "no partial assignment"
        );
    }

    #[tokio::test]
    async fn empty_bird_ids_returns_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (hutch_id, _) = seed_hutch_and_birds(&base, &client).await;
        let resp = client
            .post(format!("{base}/api/brooders/{hutch_id}/assign-birds"))
            .json(&json!({ "bird_ids": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn update_bird_can_set_housing_id() {
        // Verifies the PUT /api/birds/{id} path's housing_id field works
        // independently of the dedicated assign-birds endpoint.
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (hutch_id, bird_ids) = seed_hutch_and_birds(&base, &client).await;
        let bird_id = bird_ids[0];

        let resp = client
            .put(format!("{base}/api/birds/{bird_id}"))
            .json(&json!({ "housing_id": hutch_id }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(residents.individual_birds.len(), 1);
        assert_eq!(residents.individual_birds[0].id, bird_id);
    }
}

// ---------------------------------------------------------------------------
// Issue #14 — graduate-to-hutch + assign-graduated-group
// ---------------------------------------------------------------------------

mod graduate_to_hutch_tests {
    use super::*;
    use quailsync_common::{
        AssignGraduatedGroupRequest, AssignGraduatedGroupResponse, BrooderResidentsResponse,
        ChickGroupStatus, CreateBrooder, CreateChickGroup, GraduateBird, GraduateRequest,
        HousingType, LifeStage,
    };
    use serde_json::json;

    /// Seed: lineage + brooder (chick nursery) + hutch + chick group, then
    /// return their ids so individual tests can pick what they need.
    async fn seed_pipeline(base: &str, client: &reqwest::Client) -> (i64, i64, i64) {
        let bl: Lineage = client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: "L".into(),
                source: "S".into(),
                notes: None,
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let brooder: serde_json::Value = client
            .post(format!("{base}/api/brooders"))
            .json(&CreateBrooder {
                name: "Brooder A".into(),
                lineage_id: None,
                life_stage: LifeStage::Chick,
                qr_code: "b-1".into(),
                notes: None,
                camera_url: None,
                housing_type: Some(HousingType::Brooder),
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let brooder_id = brooder["id"].as_i64().unwrap();

        let hutch: serde_json::Value = client
            .post(format!("{base}/api/brooders"))
            .json(&CreateBrooder {
                name: "Hutch A".into(),
                lineage_id: None,
                life_stage: LifeStage::Adult,
                qr_code: "h-1".into(),
                notes: None,
                camera_url: None,
                housing_type: Some(HousingType::Hutch),
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let hutch_id = hutch["id"].as_i64().unwrap();

        let group: ChickGroup = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: Some(brooder_id),
                initial_count: 2,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                notes: None,
                lineage_ids: vec![bl.id],
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        (brooder_id, hutch_id, group.id)
    }

    fn two_birds() -> Vec<GraduateBird> {
        vec![
            GraduateBird {
                sex: Sex::Male,
                band_color: Some("red".into()),
                nfc_tag_id: None,
                notes: None,
                weight_grams: None,
                photo_path: None,
            },
            GraduateBird {
                sex: Sex::Female,
                band_color: Some("blue".into()),
                nfc_tag_id: None,
                notes: None,
                weight_grams: None,
                photo_path: None,
            },
        ]
    }

    #[tokio::test]
    async fn graduate_with_target_housing_stamps_birds_and_group() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;

        let resp = client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: Some(hutch_id),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let birds: Vec<Bird> = resp.json().await.unwrap();
        assert_eq!(birds.len(), 2);
        for b in &birds {
            assert_eq!(b.housing_id, Some(hutch_id));
            assert_eq!(b.chick_group_id, Some(group_id));
        }

        // Group itself: status=Graduated, housing_id set.
        let group: ChickGroup = reqwest::get(format!("{base}/api/chick-groups/{group_id}"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(matches!(group.status, ChickGroupStatus::Graduated));
        assert_eq!(group.housing_id, Some(hutch_id));

        // Residents endpoint surfaces the group + its birds.
        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(residents.chick_groups.len(), 1);
        assert_eq!(residents.chick_groups[0].id, group_id);
        assert_eq!(residents.individual_birds.len(), 2);
    }

    #[tokio::test]
    async fn graduate_without_target_leaves_group_unhoused() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, _hutch, group_id) = seed_pipeline(&base, &client).await;

        let resp = client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: None,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let group: ChickGroup = reqwest::get(format!("{base}/api/chick-groups/{group_id}"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(matches!(group.status, ChickGroupStatus::Graduated));
        assert_eq!(group.housing_id, None);
    }

    #[tokio::test]
    async fn graduate_with_brooder_target_returns_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (brooder_id, _hutch, group_id) = seed_pipeline(&base, &client).await;

        // brooder_id is a brooder, NOT a hutch — server must reject.
        let resp = client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: Some(brooder_id),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn assign_graduated_group_moves_group_and_birds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;

        // Graduate first WITHOUT a target — group has housing_id = NULL.
        client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: None,
            })
            .send()
            .await
            .unwrap();

        // Now assign to the hutch.
        let resp = client
            .post(format!(
                "{base}/api/brooders/{hutch_id}/assign-graduated-group"
            ))
            .json(&AssignGraduatedGroupRequest { group_id })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: AssignGraduatedGroupResponse = resp.json().await.unwrap();
        assert_eq!(body.group_id, group_id);
        assert_eq!(body.housing_id, hutch_id);
        assert_eq!(body.birds_updated, 2);

        // Residents now lists the group + its birds.
        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(residents.chick_groups.len(), 1);
        assert_eq!(residents.individual_birds.len(), 2);
    }

    #[tokio::test]
    async fn assign_graduated_group_rejects_non_hutch_target() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (brooder_id, _hutch, group_id) = seed_pipeline(&base, &client).await;
        client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: None,
            })
            .send()
            .await
            .unwrap();
        let resp = client
            .post(format!(
                "{base}/api/brooders/{brooder_id}/assign-graduated-group"
            ))
            .json(&AssignGraduatedGroupRequest { group_id })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn assign_graduated_group_rejects_active_group() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;
        // Group is still Active — should be rejected.
        let resp = client
            .post(format!(
                "{base}/api/brooders/{hutch_id}/assign-graduated-group"
            ))
            .json(&AssignGraduatedGroupRequest { group_id })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn put_chick_group_housing_id_null_clears_assignment() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;
        client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: two_birds(),
                target_housing_id: Some(hutch_id),
            })
            .send()
            .await
            .unwrap();

        // Now clear housing_id via the generic PUT.
        client
            .put(format!("{base}/api/chick-groups/{group_id}"))
            .json(&json!({ "housing_id": serde_json::Value::Null }))
            .send()
            .await
            .unwrap();

        let group: ChickGroup = reqwest::get(format!("{base}/api/chick-groups/{group_id}"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(group.housing_id, None);
    }
}
