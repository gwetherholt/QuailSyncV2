use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{
    Bird, BirdStatus, Bloodline, ChickGroup, CreateBird, CreateBloodline, CreateChickGroup,
    InbreedingCoefficient, Sex,
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
