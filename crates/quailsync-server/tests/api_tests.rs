use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::{
    Bird, BirdStatus, ChickGroup, CreateBird, CreateChickGroup, CreateLineage, FlockDiversity,
    GraduateBird, GraduateRequest, Lineage, PairingSuggestion, Sex,
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

    let pairs: Vec<PairingSuggestion> = resp.json().await.unwrap();
    assert_eq!(pairs.len(), 1);
    // Both gen-0 birds are 100% lineage A on both sides → full overlap → avoid.
    assert!((pairs[0].maternal_overlap - 1.0).abs() < 1e-9);
    assert!((pairs[0].paternal_overlap - 1.0).abs() < 1e-9);
    assert_eq!(pairs[0].risk_percent, 100);
    assert_eq!(pairs[0].risk_level, "avoid");
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
    let pairs: Vec<PairingSuggestion> = resp.json().await.unwrap();
    assert_eq!(pairs.len(), 1);
    // Disjoint lineages → zero overlap → safe.
    assert_eq!(pairs[0].maternal_overlap, 0.0);
    assert_eq!(pairs[0].paternal_overlap, 0.0);
    assert_eq!(pairs[0].risk_percent, 0);
    assert_eq!(pairs[0].risk_level, "safe");
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
    let pairs: Vec<PairingSuggestion> = resp.json().await.unwrap();

    // Phase 4 scores by genetic-profile overlap, not parentage. The son×daughter
    // pair (id=3 × id=4) are both 100% lineage A, so they read as full overlap →
    // avoid. (Parent-based sibling detection still lives in /api/inbreeding-check.)
    let sibling_pair = pairs
        .iter()
        .find(|p| p.bird_a_id == 3 && p.bird_b_id == 4)
        .expect("should have son×daughter pair");
    assert!((sibling_pair.maternal_overlap - 1.0).abs() < 1e-9);
    assert_eq!(sibling_pair.risk_level, "avoid");
}

// ---------------------------------------------------------------------------
// Phase 4: weighted inbreeding scoring + flock diversity
// ---------------------------------------------------------------------------

async fn p4_lineage(client: &reqwest::Client, base: &str, name: &str) -> i64 {
    client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: name.into(),
            source: "X".into(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

async fn p4_bird(client: &reqwest::Client, base: &str, sex: Sex, lineages: Vec<i64>) -> i64 {
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex,
            lineage_ids: lineages,
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 0,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
        })
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn breeding_suggest_sorted_lowest_risk_first() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();
    let a = p4_lineage(&client, &base, "A").await;
    let b = p4_lineage(&client, &base, "B").await;
    let c = p4_lineage(&client, &base, "C").await;

    // One male (lineage A) against three females spanning safe/caution/avoid.
    p4_bird(&client, &base, Sex::Male, vec![a]).await;
    p4_bird(&client, &base, Sex::Female, vec![b]).await; // disjoint → 0.0 safe
    p4_bird(&client, &base, Sex::Female, vec![a, b, c]).await; // 1/3 A → 0.333 caution
    p4_bird(&client, &base, Sex::Female, vec![a]).await; // identical → 1.0 avoid

    let pairs: Vec<PairingSuggestion> = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pairs.len(), 3);

    // Ascending risk: safe, caution, avoid.
    assert_eq!(pairs[0].risk_level, "safe");
    assert_eq!(pairs[0].risk_percent, 0);
    assert_eq!(pairs[1].risk_level, "caution");
    assert_eq!(pairs[1].risk_percent, 33);
    assert_eq!(pairs[2].risk_level, "avoid");
    assert_eq!(pairs[2].risk_percent, 100);

    let risk = |p: &PairingSuggestion| p.paternal_overlap.max(p.maternal_overlap);
    assert!(risk(&pairs[0]) <= risk(&pairs[1]) && risk(&pairs[1]) <= risk(&pairs[2]));
}

#[tokio::test]
async fn breeding_diversity_flags_new_blood_for_single_lineage() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();
    let a = p4_lineage(&client, &base, "A").await;
    // A one-lineage flock: the only pairing is full overlap → new blood needed.
    p4_bird(&client, &base, Sex::Male, vec![a]).await;
    p4_bird(&client, &base, Sex::Female, vec![a]).await;

    let div: FlockDiversity = reqwest::get(format!("{base}/api/breeding/diversity"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!((div.flock_confidence - 1.0).abs() < 1e-9); // gen-0 birds are certain
    assert!((div.min_confidence - 1.0).abs() < 1e-9);
    assert!((div.best_pairing_risk - 1.0).abs() < 1e-9);
    assert!(div.needs_new_blood);
    assert_eq!(div.active_lineage_count, 1);
}

#[tokio::test]
async fn gen0_flock_different_lineages_no_new_blood() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();
    let a = p4_lineage(&client, &base, "A").await;
    let b = p4_lineage(&client, &base, "B").await;
    // Gen-0 birds, all 100% certain, on two distinct lineages.
    let male = p4_bird(&client, &base, Sex::Male, vec![a]).await;
    let female = p4_bird(&client, &base, Sex::Female, vec![b]).await;

    // A different-lineage pairing reads as 0% overlap.
    let pairs: Vec<PairingSuggestion> = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pair = pairs
        .iter()
        .find(|p| p.bird_a_id == male && p.bird_b_id == female)
        .unwrap();
    assert_eq!(pair.risk_percent, 0);
    assert_eq!(pair.risk_level, "safe");

    let div: FlockDiversity = reqwest::get(format!("{base}/api/breeding/diversity"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!((div.best_pairing_risk - 0.0).abs() < 1e-9);
    assert!((div.min_confidence - 1.0).abs() < 1e-9);
    assert!(!div.needs_new_blood);
    assert_eq!(div.active_lineage_count, 2);
}

// ---------------------------------------------------------------------------
// Phase 5: configurable genetics settings
// ---------------------------------------------------------------------------

type StrMap = std::collections::HashMap<String, String>;

async fn get_genetics(base: &str) -> StrMap {
    reqwest::get(format!("{base}/api/settings/genetics"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn genetics_settings_crud_and_reset() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Defaults are seeded at init.
    let d = get_genetics(&base).await;
    assert_eq!(d["genetics.threshold.safe"], "15");
    assert_eq!(d["genetics.threshold.avoid"], "35");
    assert_eq!(d["genetics.tracking_floor"], "1");
    assert_eq!(d["genetics.display_cap"], "4");
    assert_eq!(d["genetics.new_blood_confidence"], "50");

    // Update one value; others stay put; response echoes the full set.
    let resp = client
        .put(format!("{base}/api/settings/genetics"))
        .json(&serde_json::json!({ "genetics.threshold.safe": "20" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let after: StrMap = resp.json().await.unwrap();
    assert_eq!(after["genetics.threshold.safe"], "20");
    assert_eq!(after["genetics.threshold.avoid"], "35");

    // Persists across a fresh GET.
    assert_eq!(get_genetics(&base).await["genetics.threshold.safe"], "20");

    // "Reset to defaults" = PUT every default value back.
    client
        .put(format!("{base}/api/settings/genetics"))
        .json(&serde_json::json!({
            "genetics.threshold.safe": "15",
            "genetics.threshold.avoid": "35",
            "genetics.tracking_floor": "1",
            "genetics.display_cap": "4",
            "genetics.new_blood_confidence": "50",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(get_genetics(&base).await["genetics.threshold.safe"], "15");
}

#[tokio::test]
async fn genetics_settings_validation_rejects_bad_input() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();
    let put = |body: serde_json::Value| {
        let client = client.clone();
        let base = base.clone();
        async move {
            client
                .put(format!("{base}/api/settings/genetics"))
                .json(&body)
                .send()
                .await
                .unwrap()
                .status()
                .as_u16()
        }
    };

    // display_cap max is 10; tracking_floor min is 1; thresholds are 0..=100.
    assert_eq!(
        put(serde_json::json!({ "genetics.display_cap": "11" })).await,
        400
    );
    assert_eq!(
        put(serde_json::json!({ "genetics.tracking_floor": "0" })).await,
        400
    );
    assert_eq!(
        put(serde_json::json!({ "genetics.threshold.safe": "101" })).await,
        400
    );
    // Non-integer value.
    assert_eq!(
        put(serde_json::json!({ "genetics.threshold.safe": "abc" })).await,
        400
    );
    // Unknown key.
    assert_eq!(put(serde_json::json!({ "genetics.bogus": "5" })).await, 400);

    // A bad key in a batch rejects the whole request — no partial write.
    assert_eq!(
        put(serde_json::json!({ "genetics.threshold.safe": "22", "genetics.display_cap": "99" }))
            .await,
        400
    );
    assert_eq!(get_genetics(&base).await["genetics.threshold.safe"], "15");
}

#[tokio::test]
async fn breeding_suggest_respects_configured_safe_threshold() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Ten lineages. Male = 100% L0; female = 10% each across L0..L9.
    // Shared L0 → overlap = 1.0 × 0.1 = 0.10 (10%).
    let mut lineages = Vec::new();
    for i in 0..10 {
        lineages.push(p4_lineage(&client, &base, &format!("L{i}")).await);
    }
    p4_bird(&client, &base, Sex::Male, vec![lineages[0]]).await;
    p4_bird(&client, &base, Sex::Female, lineages.clone()).await;

    // Default safe = 15%, so a 10% overlap reads as safe.
    let pairs: Vec<PairingSuggestion> = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0].risk_percent, 10);
    assert_eq!(pairs[0].risk_level, "safe");

    // Tighten safe to 5% → the same 10% pairing now reads as caution.
    let r = client
        .put(format!("{base}/api/settings/genetics"))
        .json(&serde_json::json!({ "genetics.threshold.safe": "5" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let pairs: Vec<PairingSuggestion> = reqwest::get(format!("{base}/api/breeding/suggest"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pairs[0].risk_percent, 10);
    assert_eq!(pairs[0].risk_level, "caution");
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
    async fn legacy_breeding_group_male_id_migrates_to_junction() {
        // Seed an OLD-shape breeding_groups (scalar male_id column) plus its
        // female members, then run init_db and assert the normalization
        // migration rebuilt the table: male_id dropped, status added, the male
        // backfilled into the junction, and the rest of the row intact.
        use quailsync_server::init_db;
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        // male_id / female_id are plain INTEGERs here (no birds FK) — init_db
        // creates the real birds table itself, and the ids are dangling on
        // purpose; the migration cares only about the column + values. The
        // members -> breeding_groups FK is kept so the rebuild is exercised
        // with a dependent FK present.
        conn.execute_batch(
            "CREATE TABLE breeding_groups (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                male_id INTEGER NOT NULL,
                start_date TEXT NOT NULL,
                notes TEXT
            );
             CREATE TABLE breeding_group_members (
                group_id INTEGER NOT NULL REFERENCES breeding_groups(id),
                female_id INTEGER NOT NULL,
                PRIMARY KEY (group_id, female_id)
             );
             INSERT INTO breeding_groups (id, name, male_id, start_date, notes)
                 VALUES (1, 'Legacy Group', 42, '2026-01-01', 'kept note');
             INSERT INTO breeding_group_members (group_id, female_id) VALUES (1, 99);
             INSERT INTO breeding_group_members (group_id, female_id) VALUES (1, 100);",
        )
        .unwrap();

        // Run the migration.
        init_db(&conn);

        // (1) male_id column is gone; status column is present.
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(breeding_groups)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            !cols.contains(&"male_id".to_string()),
            "male_id should be dropped"
        );
        assert!(
            cols.contains(&"status".to_string()),
            "status should be added"
        );

        // (2) the junction was backfilled from the old male_id.
        let males: Vec<i64> = conn
            .prepare("SELECT male_id FROM breeding_group_males WHERE group_id = 1")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(males, vec![42]);

        // (3) status is 'active' (the group had a male).
        let status: String = conn
            .query_row("SELECT status FROM breeding_groups WHERE id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(status, "active");

        // (4) the rest of the group survived the rebuild: name/start_date/notes
        // and the female membership rows.
        let (name, start_date, notes): (String, String, Option<String>) = conn
            .query_row(
                "SELECT name, start_date, notes FROM breeding_groups WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "Legacy Group");
        assert_eq!(start_date, "2026-01-01");
        assert_eq!(notes.as_deref(), Some("kept note"));

        let females: Vec<i64> = conn
            .prepare(
                "SELECT female_id FROM breeding_group_members WHERE group_id = 1 ORDER BY female_id",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(females, vec![99, 100]);
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
        assert_eq!(pre.active_bird_count, 0, "headcount starts at zero");

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
        assert_eq!(
            post.active_bird_count, 2,
            "headcount counts active housed birds"
        );
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
        // Headcount drops to reflect the unassignment — the count is "active
        // birds housed here right now", not a stale tally.
        assert_eq!(residents.active_bird_count, 1);
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
        HousingType, LifeStage, UpdateBird,
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
        assert_eq!(residents.active_bird_count, 2);
    }

    /// The headcount is "Active birds housed here right now" — it must drop when
    /// a resident is sold/culled, while the graduated group's `current_count`
    /// (provenance) stays put.
    #[tokio::test]
    async fn headcount_tracks_active_birds_not_stale_group_count() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;

        // Graduate two birds straight into the hutch.
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

        let before: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(before.active_bird_count, 2);
        let group_count = before.chick_groups[0].current_count;

        // Sell one of the two — its status leaves 'Active'.
        let resp = client
            .put(format!("{base}/api/birds/{}", birds[0].id))
            .json(&UpdateBird {
                status: Some(BirdStatus::Sold),
                notes: None,
                nfc_tag_id: None,
                band_color: None,
                sex: None,
                hatch_date: None,
                housing_id: None,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let after: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        // Headcount reflects reality: one active bird remains.
        assert_eq!(
            after.active_bird_count, 1,
            "sold bird drops out of the headcount"
        );
        assert_eq!(after.individual_birds.len(), 1);
        // The graduated group is provenance only — its count is unchanged.
        assert_eq!(
            after.chick_groups[0].current_count, group_count,
            "graduated group current_count must not change with the sale"
        );
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

    /// Graduating straight into a hutch with an empty banding batch is refused —
    /// there'd be no individual bird records to house.
    #[tokio::test]
    async fn graduate_into_hutch_requires_banded_birds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;

        let resp = client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: vec![],
                target_housing_id: Some(hutch_id),
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let msg = resp.text().await.unwrap();
        assert!(msg.contains("hasn't been banded"), "unexpected body: {msg}");
    }

    /// A count-only graduated group (graduated without banding) cannot be
    /// assigned to a hutch — the server returns the "band first" warning and the
    /// group stays unhoused.
    #[tokio::test]
    async fn assign_graduated_group_requires_banded_birds() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (_b, hutch_id, group_id) = seed_pipeline(&base, &client).await;

        // Graduate with no birds and no target — a count-only group, no records.
        let resp = client
            .post(format!("{base}/api/chick-groups/{group_id}/graduate"))
            .json(&GraduateRequest {
                birds: vec![],
                target_housing_id: None,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        // Assigning it to a hutch must be refused with the banding warning.
        let resp = client
            .post(format!(
                "{base}/api/brooders/{hutch_id}/assign-graduated-group"
            ))
            .json(&AssignGraduatedGroupRequest { group_id })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
        let msg = resp.text().await.unwrap();
        assert!(msg.contains("hasn't been banded"), "unexpected body: {msg}");

        // The group must NOT have been housed.
        let residents: BrooderResidentsResponse =
            reqwest::get(format!("{base}/api/brooders/{hutch_id}/residents"))
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
        assert_eq!(residents.active_bird_count, 0);
        assert!(
            residents.chick_groups.is_empty(),
            "un-banded group must not be housed in the hutch"
        );
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
        assert_eq!(residents.active_bird_count, 2);
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

// ---------------------------------------------------------------------------
// NFC tag reassignment — UNIQUE(birds.nfc_tag_id) used to surface as 500 when
// the batch flow re-used a tag from a prior session. The fix in
// `clear_nfc_tag_from_others` clears the prior owner first; these tests
// pin that behaviour across the three INSERT/UPDATE paths.
// ---------------------------------------------------------------------------

mod nfc_tag_reassignment_tests {
    use super::*;
    use serde_json::json;

    async fn seed_lineage(base: &str, client: &reqwest::Client) -> i64 {
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
        bl.id
    }

    fn create_bird_with_tag(lineage_id: i64, tag: Option<&str>) -> CreateBird {
        CreateBird {
            band_color: None,
            sex: Sex::Unknown,
            lineage_ids: vec![lineage_id],
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 1,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: tag.map(|t| t.to_string()),
            chick_group_id: None,
        }
    }

    #[tokio::test]
    async fn create_bird_reuses_tag_by_clearing_prior_owner() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let lineage_id = seed_lineage(&base, &client).await;

        // Bird 1 takes the tag.
        let first: Bird = client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, Some("TAG-REUSE")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(first.nfc_tag_id.as_deref(), Some("TAG-REUSE"));

        // Bird 2 takes the SAME tag — should succeed (201), not 500.
        let resp = client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, Some("TAG-REUSE")))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);
        let second: Bird = resp.json().await.unwrap();
        assert_eq!(second.nfc_tag_id.as_deref(), Some("TAG-REUSE"));

        // Bird 1 must have had its tag cleared.
        let first_after: Bird = reqwest::get(format!("{base}/api/birds"))
            .await
            .unwrap()
            .json::<Vec<Bird>>()
            .await
            .unwrap()
            .into_iter()
            .find(|b| b.id == first.id)
            .unwrap();
        assert_eq!(first_after.nfc_tag_id, None);
    }

    #[tokio::test]
    async fn update_bird_reassigns_tag_from_another_bird() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let lineage_id = seed_lineage(&base, &client).await;

        let owner: Bird = client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, Some("TAG-MOVE")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let other: Bird = client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, None))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // Move TAG-MOVE from `owner` to `other` via PUT.
        let resp = client
            .put(format!("{base}/api/birds/{}", other.id))
            .json(&json!({ "nfc_tag_id": "TAG-MOVE" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let all: Vec<Bird> = reqwest::get(format!("{base}/api/birds"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let owner_after = all.iter().find(|b| b.id == owner.id).unwrap();
        let other_after = all.iter().find(|b| b.id == other.id).unwrap();
        assert_eq!(owner_after.nfc_tag_id, None);
        assert_eq!(other_after.nfc_tag_id.as_deref(), Some("TAG-MOVE"));
    }

    #[tokio::test]
    async fn update_bird_keeps_same_tag_on_same_bird() {
        // No-op path: re-PUTting the same tag onto the same bird must not
        // clear it (the `except_id` guard).
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let lineage_id = seed_lineage(&base, &client).await;

        let bird: Bird = client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, Some("TAG-SAME")))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let resp = client
            .put(format!("{base}/api/birds/{}", bird.id))
            .json(&json!({ "nfc_tag_id": "TAG-SAME" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let after: Vec<Bird> = reqwest::get(format!("{base}/api/birds"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let same = after.iter().find(|b| b.id == bird.id).unwrap();
        assert_eq!(same.nfc_tag_id.as_deref(), Some("TAG-SAME"));
    }

    #[tokio::test]
    async fn graduate_handler_reuses_tags_across_batches() {
        // The graduate handler INSERTs birds with the same UNIQUE column —
        // pin that it also clears prior owners so a second batch graduation
        // can re-program the same physical tags.
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let lineage_id = seed_lineage(&base, &client).await;

        // Owner of TAG-G claims it first via direct create.
        client
            .post(format!("{base}/api/birds"))
            .json(&create_bird_with_tag(lineage_id, Some("TAG-G")))
            .send()
            .await
            .unwrap();

        // Now a chick group graduates a bird that wants TAG-G.
        let group: ChickGroup = client
            .post(format!("{base}/api/chick-groups"))
            .json(&CreateChickGroup {
                clutch_id: None,
                brooder_id: None,
                initial_count: 1,
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                notes: None,
                lineage_ids: vec![lineage_id],
            })
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let resp = client
            .post(format!("{base}/api/chick-groups/{}/graduate", group.id))
            .json(&GraduateRequest {
                target_housing_id: None,
                birds: vec![GraduateBird {
                    sex: Sex::Female,
                    band_color: None,
                    nfc_tag_id: Some("TAG-G".into()),
                    notes: None,
                    weight_grams: None,
                    photo_path: None,
                }],
            })
            .send()
            .await
            .unwrap();
        // Pre-fix this would have been a 500 on the second occupant.
        assert_eq!(resp.status(), 200);

        // The graduated bird ends up with the tag; the prior owner doesn't.
        let all: Vec<Bird> = reqwest::get(format!("{base}/api/birds"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let with_tag: Vec<&Bird> = all
            .iter()
            .filter(|b| b.nfc_tag_id.as_deref() == Some("TAG-G"))
            .collect();
        assert_eq!(with_tag.len(), 1, "exactly one bird should own TAG-G");
    }
}

// ---------------------------------------------------------------------------
// Dropped-tag reconciliation: POST /api/groups/{id}/reconcile-tags
// ---------------------------------------------------------------------------

mod reconcile_tests {
    use super::*;
    use serde_json::{json, Value};

    async fn seed_lineage(base: &str, client: &reqwest::Client, name: &str) -> i64 {
        let bl: Lineage = client
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
        bl.id
    }

    async fn make_bird(
        base: &str,
        client: &reqwest::Client,
        sex: Sex,
        band: Option<&str>,
        tag: Option<&str>,
        lineage_id: i64,
    ) -> i64 {
        let bird: Bird = client
            .post(format!("{base}/api/birds"))
            .json(&CreateBird {
                band_color: band.map(|b| b.to_string()),
                sex,
                lineage_ids: vec![lineage_id],
                hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                mother_id: None,
                father_id: None,
                generation: 1,
                status: BirdStatus::Active,
                notes: None,
                nfc_tag_id: tag.map(|t| t.to_string()),
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

    /// Build a one-male/two-female breeding group and return its id. The hens
    /// carry tags "T-HEN-A" (red band) and "T-HEN-B" (blue band); the male
    /// carries "T-MALE".
    async fn seed_group(base: &str, client: &reqwest::Client) -> (i64, i64, i64, i64) {
        let lineage_id = seed_lineage(base, client, "Fernbank").await;
        let male = make_bird(
            base,
            client,
            Sex::Male,
            Some("green"),
            Some("T-MALE"),
            lineage_id,
        )
        .await;
        let hen_a = make_bird(
            base,
            client,
            Sex::Female,
            Some("red"),
            Some("T-HEN-A"),
            lineage_id,
        )
        .await;
        let hen_b = make_bird(
            base,
            client,
            Sex::Female,
            Some("blue"),
            Some("T-HEN-B"),
            lineage_id,
        )
        .await;

        let resp = client
            .post(format!("{base}/api/breeding-groups"))
            .json(&json!({
                "name": "Group 1",
                "male_ids": [male],
                "female_ids": [hen_a, hen_b],
                "start_date": "2026-01-01",
                "notes": null,
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201);

        let groups: Vec<Value> = reqwest::get(format!("{base}/api/breeding-groups"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let group_id = groups[0]["id"].as_i64().unwrap();
        (group_id, male, hen_a, hen_b)
    }

    #[tokio::test]
    async fn unknown_group_is_rejected() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{base}/api/groups/9999/reconcile-tags"))
            .json(&json!({ "orphan_tag_ids": [], "observed_birds": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn end_to_end_resolves_dropped_hen_tag() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (group_id, _male, _a, _b) = seed_group(&base, &client).await;

        // One hen lost her band; the keeper sees one red-banded female.
        let body: Value = client
            .post(format!("{base}/api/groups/{group_id}/reconcile-tags"))
            .json(&json!({
                "orphan_tag_ids": ["T-HEN-A"],
                "observed_birds": [{
                    "ref_id": "corner-hen",
                    "sex": "Female",
                    "traits": { "band_color": "red" }
                }],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        assert!(body["unmatched_tags"].as_array().unwrap().is_empty());
        let result = &body["results"][0];
        assert_eq!(result["ref_id"], "corner-hen");
        assert_eq!(result["outcome"]["kind"], "resolved");
        assert_eq!(result["outcome"]["tag_id"], "T-HEN-A");
        assert_eq!(result["outcome"]["confidence"], "sole");
    }

    #[tokio::test]
    async fn foreign_tag_lands_in_unmatched() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (group_id, _male, _a, _b) = seed_group(&base, &client).await;

        // A bird (and tag) that belongs to no group at all.
        let lineage_id = seed_lineage(&base, &client, "Outsider").await;
        make_bird(
            &base,
            &client,
            Sex::Female,
            None,
            Some("T-OUTSIDER"),
            lineage_id,
        )
        .await;

        let body: Value = client
            .post(format!("{base}/api/groups/{group_id}/reconcile-tags"))
            .json(&json!({
                "orphan_tag_ids": ["T-OUTSIDER", "T-NONEXISTENT"],
                "observed_birds": [],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let unmatched: Vec<String> = body["unmatched_tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(unmatched.contains(&"T-OUTSIDER".to_string()));
        assert!(unmatched.contains(&"T-NONEXISTENT".to_string()));
        assert!(body["results"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_request_to_valid_group_is_empty_200() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (group_id, _male, _a, _b) = seed_group(&base, &client).await;

        let resp = client
            .post(format!("{base}/api/groups/{group_id}/reconcile-tags"))
            .json(&json!({ "orphan_tag_ids": [], "observed_birds": [] }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        assert!(body["results"].as_array().unwrap().is_empty());
        assert!(body["unmatched_tags"].as_array().unwrap().is_empty());
    }

    /// End-to-end rooster case: the male's band drops. With the male observed,
    /// the single-male short-circuit resolves it `sole` over the wire, and the
    /// hen resolves on her lone remaining tag. Exercises serde + DB + the
    /// confidence enum round-trip, not just the pure core.
    #[tokio::test]
    async fn rooster_band_resolves_via_short_circuit() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let (group_id, _male, _a, _b) = seed_group(&base, &client).await;

        let body: Value = client
            .post(format!("{base}/api/groups/{group_id}/reconcile-tags"))
            .json(&json!({
                "orphan_tag_ids": ["T-MALE", "T-HEN-A"],
                "observed_birds": [
                    { "ref_id": "the-cock", "sex": "Male" },
                    { "ref_id": "a-hen", "sex": "Female" }
                ],
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        assert!(body["unmatched_tags"].as_array().unwrap().is_empty());
        let results = body["results"].as_array().unwrap();
        let cock = results.iter().find(|r| r["ref_id"] == "the-cock").unwrap();
        assert_eq!(cock["outcome"]["kind"], "resolved");
        assert_eq!(cock["outcome"]["tag_id"], "T-MALE");
        assert_eq!(cock["outcome"]["confidence"], "sole");

        let hen = results.iter().find(|r| r["ref_id"] == "a-hen").unwrap();
        assert_eq!(hen["outcome"]["kind"], "resolved");
        assert_eq!(hen["outcome"]["tag_id"], "T-HEN-A");
    }

    /// Reconcile membership is read from the `breeding_group_males` junction,
    /// so EVERY male counts — not just a scalar "primary". A group with two
    /// males must treat both their tags as members (the second male's tag was
    /// the case the old scalar `male_id` got wrong).
    #[tokio::test]
    async fn reconcile_reads_all_junction_males() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let lineage_id = seed_lineage(&base, &client, "Multi").await;
        let m1 = make_bird(
            &base,
            &client,
            Sex::Male,
            Some("green"),
            Some("T-MALE-1"),
            lineage_id,
        )
        .await;
        let m2 = make_bird(
            &base,
            &client,
            Sex::Male,
            Some("black"),
            Some("T-MALE-2"),
            lineage_id,
        )
        .await;
        let hen = make_bird(
            &base,
            &client,
            Sex::Female,
            Some("red"),
            Some("T-HEN-A"),
            lineage_id,
        )
        .await;

        let group: Value = client
            .post(format!("{base}/api/breeding-groups"))
            .json(&json!({
                "name": "Two Toms", "male_ids": [m1, m2],
                "female_ids": [hen], "start_date": "2026-01-01",
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(group["male_ids"].as_array().unwrap().len(), 2);
        let gid = group["id"].as_i64().unwrap();

        // The SECOND male's tag must be recognized as a member (not unmatched).
        let body: Value = client
            .post(format!("{base}/api/groups/{gid}/reconcile-tags"))
            .json(&json!({ "orphan_tag_ids": ["T-MALE-2"], "observed_birds": [] }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(
            body["unmatched_tags"].as_array().unwrap().is_empty(),
            "the second junction male's tag should be a member, got: {body}"
        );

        // Contrast: a non-member tag still lands in unmatched (single-male path
        // intact — the first male's tag is a member, the outsider's isn't).
        make_bird(&base, &client, Sex::Female, None, Some("T-OUT"), lineage_id).await;
        let body2: Value = client
            .post(format!("{base}/api/groups/{gid}/reconcile-tags"))
            .json(&json!({ "orphan_tag_ids": ["T-MALE-1", "T-OUT"], "observed_birds": [] }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let unmatched: Vec<String> = body2["unmatched_tags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert!(unmatched.contains(&"T-OUT".to_string()));
        assert!(!unmatched.contains(&"T-MALE-1".to_string()));
    }
}

// ---------------------------------------------------------------------------
// Govee H5179 sensor ingest + dynamic assignment
// ---------------------------------------------------------------------------

mod govee_tests {
    use super::*;
    use quailsync_common::{
        AssignSensorRequest, CreateBrooder, GoveeReadingInput, GoveeReadingsRequest,
        GoveeReadingsResponse, GoveeSensor, HousingType, LifeStage,
    };

    /// Create a housing unit and return its id.
    async fn create_brooder(base: &str, client: &reqwest::Client, name: &str) -> i64 {
        let b: serde_json::Value = client
            .post(format!("{base}/api/brooders"))
            .json(&CreateBrooder {
                name: name.into(),
                lineage_id: None,
                life_stage: LifeStage::Adult,
                qr_code: String::new(),
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
        b["id"].as_i64().unwrap()
    }

    fn reading(device_id: &str, temp: f64, hum: f64, at: &str) -> GoveeReadingInput {
        GoveeReadingInput {
            device_id: device_id.into(),
            model: Some("H5179".into()),
            name: Some("Sensor A - White Label".into()),
            temperature_f: temp,
            humidity: hum,
            recorded_at: at.into(),
        }
    }

    async fn post_readings(
        base: &str,
        client: &reqwest::Client,
        readings: Vec<GoveeReadingInput>,
    ) -> reqwest::Response {
        client
            .post(format!("{base}/api/govee/readings"))
            .json(&GoveeReadingsRequest { readings })
            .send()
            .await
            .unwrap()
    }

    async fn list_sensors(base: &str, client: &reqwest::Client) -> Vec<GoveeSensor> {
        client
            .get(format!("{base}/api/govee/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn ingest_auto_registers_sensor_and_stores_reading() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        let resp = post_readings(
            &base,
            &client,
            vec![reading(
                "AA:BB:CC:DD:EE:FF",
                78.5,
                45.2,
                "2025-06-17T12:00:00Z",
            )],
        )
        .await;
        assert_eq!(resp.status(), 201);
        let body: GoveeReadingsResponse = resp.json().await.unwrap();
        assert_eq!(body.stored, 1);

        let sensors = list_sensors(&base, &client).await;
        assert_eq!(sensors.len(), 1, "device auto-registered exactly once");
        let s = &sensors[0];
        assert_eq!(s.govee_device_id, "AA:BB:CC:DD:EE:FF");
        assert_eq!(s.name.as_deref(), Some("Sensor A - White Label"));
        assert_eq!(s.model.as_deref(), Some("H5179"));
        assert!(s.assignment.is_none(), "unassigned on first sight");
        let latest = s.latest_reading.as_ref().expect("has a latest reading");
        assert!((latest.temperature_f - 78.5).abs() < f64::EPSILON);
        assert!((latest.humidity - 45.2).abs() < f64::EPSILON);
        assert_eq!(latest.recorded_at, "2025-06-17T12:00:00Z");
    }

    #[tokio::test]
    async fn ingest_known_device_does_not_duplicate_sensor() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        post_readings(
            &base,
            &client,
            vec![reading("DEV-1", 70.0, 40.0, "2025-06-17T10:00:00Z")],
        )
        .await;
        // Same device, later reading — must reuse the sensor and update latest.
        post_readings(
            &base,
            &client,
            vec![reading("DEV-1", 72.5, 41.0, "2025-06-17T11:00:00Z")],
        )
        .await;

        let sensors = list_sensors(&base, &client).await;
        assert_eq!(sensors.len(), 1, "no duplicate sensor for the same device");
        let latest = sensors[0].latest_reading.as_ref().unwrap();
        assert_eq!(latest.recorded_at, "2025-06-17T11:00:00Z");
        assert!((latest.temperature_f - 72.5).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn ingest_batch_stores_all_and_registers_each_device() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        let resp = post_readings(
            &base,
            &client,
            vec![
                reading("DEV-A", 71.0, 40.0, "2025-06-17T12:00:00Z"),
                reading("DEV-B", 80.0, 50.0, "2025-06-17T12:00:00Z"),
                reading("DEV-A", 71.5, 41.0, "2025-06-17T12:05:00Z"),
            ],
        )
        .await;
        assert_eq!(resp.status(), 201);
        let body: GoveeReadingsResponse = resp.json().await.unwrap();
        assert_eq!(body.stored, 3);

        let sensors = list_sensors(&base, &client).await;
        assert_eq!(sensors.len(), 2, "two distinct devices registered");
    }

    #[tokio::test]
    async fn assign_reassign_and_query_by_brooder() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder_a = create_brooder(&base, &client, "Brooder A").await;
        let brooder_b = create_brooder(&base, &client, "Brooder B").await;

        post_readings(
            &base,
            &client,
            vec![reading("DEV-MOVE", 75.0, 44.0, "2025-06-17T12:00:00Z")],
        )
        .await;
        let sensor_id = list_sensors(&base, &client).await[0].id;

        // Assign to A — response carries the new assignment.
        let resp = client
            .put(format!("{base}/api/govee/sensors/{sensor_id}/assign"))
            .json(&AssignSensorRequest {
                brooder_id: brooder_a,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let s: GoveeSensor = resp.json().await.unwrap();
        let asg = s.assignment.as_ref().expect("assigned");
        assert_eq!(asg.brooder_id, brooder_a);
        assert_eq!(asg.brooder_name, "Brooder A");

        // Brooder A lists it (with its latest reading); B doesn't.
        let a_sensors: Vec<GoveeSensor> = client
            .get(format!("{base}/api/brooders/{brooder_a}/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(a_sensors.len(), 1);
        assert!(a_sensors[0].latest_reading.is_some());
        let b_sensors: Vec<GoveeSensor> = client
            .get(format!("{base}/api/brooders/{brooder_b}/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(b_sensors.is_empty());

        // Reassign to B — exactly one active assignment moves over.
        let resp = client
            .put(format!("{base}/api/govee/sensors/{sensor_id}/assign"))
            .json(&AssignSensorRequest {
                brooder_id: brooder_b,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let s: GoveeSensor = resp.json().await.unwrap();
        assert_eq!(s.assignment.as_ref().unwrap().brooder_id, brooder_b);

        let a_after: Vec<GoveeSensor> = client
            .get(format!("{base}/api/brooders/{brooder_a}/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(a_after.is_empty(), "old assignment was closed");
        let b_after: Vec<GoveeSensor> = client
            .get(format!("{base}/api/brooders/{brooder_b}/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(b_after.len(), 1, "exactly one active assignment");
    }

    #[tokio::test]
    async fn unassign_clears_active_assignment() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder = create_brooder(&base, &client, "Brooder").await;
        post_readings(
            &base,
            &client,
            vec![reading("DEV-X", 70.0, 40.0, "2025-06-17T12:00:00Z")],
        )
        .await;
        let sensor_id = list_sensors(&base, &client).await[0].id;

        client
            .put(format!("{base}/api/govee/sensors/{sensor_id}/assign"))
            .json(&AssignSensorRequest {
                brooder_id: brooder,
            })
            .send()
            .await
            .unwrap();

        let resp = client
            .delete(format!("{base}/api/govee/sensors/{sensor_id}/assign"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);

        // No longer assigned, and the brooder lists no sensors.
        let sensors = list_sensors(&base, &client).await;
        assert!(sensors[0].assignment.is_none());
        let b_sensors: Vec<GoveeSensor> = client
            .get(format!("{base}/api/brooders/{brooder}/sensors"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(b_sensors.is_empty());
    }

    #[tokio::test]
    async fn assign_unknown_sensor_is_404() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder = create_brooder(&base, &client, "Brooder").await;

        let resp = client
            .put(format!("{base}/api/govee/sensors/9999/assign"))
            .json(&AssignSensorRequest {
                brooder_id: brooder,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn assign_unknown_brooder_is_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        post_readings(
            &base,
            &client,
            vec![reading("DEV-Y", 70.0, 40.0, "2025-06-17T12:00:00Z")],
        )
        .await;
        let sensor_id = list_sensors(&base, &client).await[0].id;

        let resp = client
            .put(format!("{base}/api/govee/sensors/{sensor_id}/assign"))
            .json(&AssignSensorRequest { brooder_id: 9999 })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }
}

// ---------------------------------------------------------------------------
// System settings (GET/PUT /api/system-settings)
// ---------------------------------------------------------------------------

mod system_settings_tests {
    use super::*;
    use quailsync_common::Settings;
    use serde_json::{json, Value};

    async fn fetch(base: &str) -> Settings {
        reqwest::get(format!("{base}/api/system-settings"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    async fn fetch_raw(base: &str) -> Value {
        reqwest::get(format!("{base}/api/system-settings"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn get_returns_all_keys_with_correct_types_and_defaults() {
        let base = spawn_test_server().await;
        let v = fetch_raw(&base).await;

        // Floats serialize as JSON numbers with a fractional part.
        assert_eq!(v["alert_temp_min_f"], json!(68.0));
        assert_eq!(v["alert_temp_max_f"], json!(72.0));
        assert_eq!(v["alert_humidity_min"], json!(40.0));
        assert_eq!(v["alert_humidity_max"], json!(60.0));
        assert_eq!(v["adult_temp_min_f"], json!(65.0));
        assert_eq!(v["adult_temp_max_f"], json!(75.0));
        assert_eq!(v["butcher_weight_grams"], json!(250.0));
        assert_eq!(v["min_breeding_weight_grams"], json!(200.0));
        // Integers.
        assert_eq!(v["incubation_days"], json!(17));
        assert_eq!(v["ready_to_transition_age_days"], json!(35));
        assert_eq!(v["sensor_stale_seconds"], json!(15));
        // Array.
        assert!(v["brooder_week_temps_f"].is_array());
        assert_eq!(v["brooder_week_temps_f"], json!([97, 92, 87, 82, 77, 72]));

        // Typed parse equals the canonical defaults.
        assert_eq!(fetch(&base).await, Settings::default());
    }

    #[tokio::test]
    async fn put_partial_update_changes_only_specified_keys() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let before = fetch(&base).await;

        let resp = client
            .put(format!("{base}/api/system-settings"))
            .json(&json!({ "incubation_days": 21, "alert_temp_min_f": 70.0 }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let returned: Settings = resp.json().await.unwrap();

        // Changed keys reflect the new values.
        assert_eq!(returned.incubation_days, 21);
        assert!((returned.alert_temp_min_f - 70.0).abs() < f64::EPSILON);
        // Everything else is untouched.
        assert_eq!(
            returned.ready_to_transition_age_days,
            before.ready_to_transition_age_days
        );
        assert!((returned.alert_temp_max_f - before.alert_temp_max_f).abs() < f64::EPSILON);
        assert_eq!(returned.sensor_stale_seconds, before.sensor_stale_seconds);
        assert_eq!(returned.brooder_week_temps_f, before.brooder_week_temps_f);

        // Persisted — a fresh GET shows the same.
        let after = fetch(&base).await;
        assert_eq!(after.incubation_days, 21);
        assert!((after.alert_temp_min_f - 70.0).abs() < f64::EPSILON);
        assert_eq!(
            after.ready_to_transition_age_days,
            before.ready_to_transition_age_days
        );
    }

    #[tokio::test]
    async fn put_week_temps_round_trips_as_json_array() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        let resp = client
            .put(format!("{base}/api/system-settings"))
            .json(&json!({ "brooder_week_temps_f": [10, 20, 30, 40] }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);

        let v = fetch_raw(&base).await;
        assert!(v["brooder_week_temps_f"].is_array());
        assert_eq!(v["brooder_week_temps_f"], json!([10, 20, 30, 40]));
        assert_eq!(
            fetch(&base).await.brooder_week_temps_f,
            vec![10, 20, 30, 40]
        );
    }

    #[tokio::test]
    async fn indoor_cam_toggles_default_on_and_round_trip_via_put() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        // Default ON (seeded true).
        let before = fetch(&base).await;
        assert!(before.indoor_cam_roboflow_upload_enabled);
        assert!(before.indoor_cam_image_save_enabled);

        // Turn the Roboflow toggle off, leave image-save alone.
        let resp = client
            .put(format!("{base}/api/system-settings"))
            .json(&json!({ "indoor_cam_roboflow_upload_enabled": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let returned: Settings = resp.json().await.unwrap();
        assert!(!returned.indoor_cam_roboflow_upload_enabled);
        assert!(returned.indoor_cam_image_save_enabled); // untouched

        // Persisted as a real JSON boolean (not a string), and survives a GET.
        let v = fetch_raw(&base).await;
        assert_eq!(v["indoor_cam_roboflow_upload_enabled"], json!(false));
        assert_eq!(v["indoor_cam_image_save_enabled"], json!(true));
        assert!(!fetch(&base).await.indoor_cam_roboflow_upload_enabled);
    }

    #[tokio::test]
    async fn missing_rows_fall_back_to_defaults() {
        use quailsync_server::routes::system_settings::load_system_settings;

        // No system_settings table at all -> graceful fallback to defaults.
        let conn = Connection::open_in_memory().unwrap();
        assert_eq!(load_system_settings(&conn), Settings::default());

        // Table present but empty -> still all defaults.
        let conn2 = Connection::open_in_memory().unwrap();
        conn2
            .execute_batch(
                "CREATE TABLE system_settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
                );",
            )
            .unwrap();
        assert_eq!(load_system_settings(&conn2), Settings::default());

        // A single overridden row -> only that key differs; the rest default.
        conn2
            .execute(
                "INSERT INTO system_settings (key, value) VALUES ('sensor_stale_seconds', '99')",
                [],
            )
            .unwrap();
        let s = load_system_settings(&conn2);
        assert_eq!(s.sensor_stale_seconds, 99);
        assert_eq!(s.incubation_days, Settings::default().incubation_days);
        assert_eq!(
            s.brooder_week_temps_f,
            Settings::default().brooder_week_temps_f
        );
    }
}

// ---------------------------------------------------------------------------
// SPYPOINT trail-camera registry + assignment (/api/trail-cameras)
// ---------------------------------------------------------------------------

mod trail_camera_tests {
    use super::*;
    use quailsync_common::{
        AssignCameraRequest, CreateBrooder, HousingType, LifeStage, RegisterCameraRequest,
        TrailCamera,
    };

    async fn create_brooder(base: &str, client: &reqwest::Client, name: &str) -> i64 {
        let b: serde_json::Value = client
            .post(format!("{base}/api/brooders"))
            .json(&CreateBrooder {
                name: name.into(),
                lineage_id: None,
                life_stage: LifeStage::Adult,
                qr_code: String::new(),
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
        b["id"].as_i64().unwrap()
    }

    async fn register(
        base: &str,
        client: &reqwest::Client,
        body: RegisterCameraRequest,
    ) -> reqwest::Response {
        client
            .post(format!("{base}/api/trail-cameras/register"))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn list_cameras(base: &str, client: &reqwest::Client) -> Vec<TrailCamera> {
        client
            .get(format!("{base}/api/trail-cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn register_creates_then_updates() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();

        // New camera -> 201 with the record.
        let resp = register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "6a304dac5a82bf1a819b56d9".into(),
                name: Some("Front Hutch Cam".into()),
                model: Some("Flex-M".into()),
            },
        )
        .await;
        assert_eq!(resp.status(), 201);
        let cam: TrailCamera = resp.json().await.unwrap();
        assert_eq!(cam.spypoint_camera_id, "6a304dac5a82bf1a819b56d9");
        assert_eq!(cam.name.as_deref(), Some("Front Hutch Cam"));
        assert_eq!(cam.model.as_deref(), Some("Flex-M"));
        assert!(cam.assignment.is_none());

        // Re-register same id -> 200 and updates the provided field; no duplicate.
        let resp = register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "6a304dac5a82bf1a819b56d9".into(),
                name: Some("Renamed Cam".into()),
                model: None,
            },
        )
        .await;
        assert_eq!(resp.status(), 200);
        let cam: TrailCamera = resp.json().await.unwrap();
        assert_eq!(cam.name.as_deref(), Some("Renamed Cam"));
        assert_eq!(cam.model.as_deref(), Some("Flex-M")); // unchanged (not provided)

        assert_eq!(list_cameras(&base, &client).await.len(), 1, "no duplicate");
    }

    #[tokio::test]
    async fn register_requires_spypoint_id() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let resp = register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "  ".into(),
                name: None,
                model: None,
            },
        )
        .await;
        assert_eq!(resp.status(), 400);
    }

    #[tokio::test]
    async fn assign_reassign_and_query_by_brooder() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder_a = create_brooder(&base, &client, "Hutch A").await;
        let brooder_b = create_brooder(&base, &client, "Hutch B").await;

        register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "cam-move".into(),
                name: Some("Mover".into()),
                model: None,
            },
        )
        .await;
        let camera_id = list_cameras(&base, &client).await[0].id;

        // Assign to A — response carries the new assignment.
        let resp = client
            .put(format!("{base}/api/trail-cameras/{camera_id}/assign"))
            .json(&AssignCameraRequest {
                brooder_id: brooder_a,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let cam: TrailCamera = resp.json().await.unwrap();
        let asg = cam.assignment.as_ref().expect("assigned");
        assert_eq!(asg.brooder_id, brooder_a);
        assert_eq!(asg.brooder_name, "Hutch A");

        // Brooder A lists it; B doesn't.
        let a_cams: Vec<TrailCamera> = client
            .get(format!("{base}/api/brooders/{brooder_a}/cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(a_cams.len(), 1);
        let b_cams: Vec<TrailCamera> = client
            .get(format!("{base}/api/brooders/{brooder_b}/cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(b_cams.is_empty());

        // Reassign to B — exactly one active assignment moves over.
        let resp = client
            .put(format!("{base}/api/trail-cameras/{camera_id}/assign"))
            .json(&AssignCameraRequest {
                brooder_id: brooder_b,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.json::<TrailCamera>()
                .await
                .unwrap()
                .assignment
                .unwrap()
                .brooder_id,
            brooder_b
        );

        let a_after: Vec<TrailCamera> = client
            .get(format!("{base}/api/brooders/{brooder_a}/cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(a_after.is_empty(), "old assignment was closed");
        let b_after: Vec<TrailCamera> = client
            .get(format!("{base}/api/brooders/{brooder_b}/cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(b_after.len(), 1, "exactly one active assignment");
    }

    #[tokio::test]
    async fn unassign_clears_active_assignment() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder = create_brooder(&base, &client, "Hutch").await;
        register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "cam-x".into(),
                name: None,
                model: None,
            },
        )
        .await;
        let camera_id = list_cameras(&base, &client).await[0].id;

        client
            .put(format!("{base}/api/trail-cameras/{camera_id}/assign"))
            .json(&AssignCameraRequest {
                brooder_id: brooder,
            })
            .send()
            .await
            .unwrap();

        let resp = client
            .delete(format!("{base}/api/trail-cameras/{camera_id}/assign"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 204);

        assert!(list_cameras(&base, &client).await[0].assignment.is_none());
        let b_cams: Vec<TrailCamera> = client
            .get(format!("{base}/api/brooders/{brooder}/cameras"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(b_cams.is_empty());
    }

    #[tokio::test]
    async fn assign_unknown_camera_is_404() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        let brooder = create_brooder(&base, &client, "Hutch").await;
        let resp = client
            .put(format!("{base}/api/trail-cameras/9999/assign"))
            .json(&AssignCameraRequest {
                brooder_id: brooder,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 404);
    }

    #[tokio::test]
    async fn assign_unknown_brooder_is_400() {
        let base = spawn_test_server().await;
        let client = reqwest::Client::new();
        register(
            &base,
            &client,
            RegisterCameraRequest {
                spypoint_camera_id: "cam-y".into(),
                name: None,
                model: None,
            },
        )
        .await;
        let camera_id = list_cameras(&base, &client).await[0].id;
        let resp = client
            .put(format!("{base}/api/trail-cameras/{camera_id}/assign"))
            .json(&AssignCameraRequest { brooder_id: 9999 })
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400);
    }
}

// ---------------------------------------------------------------------------
// Phase 2: clutch group-composition snapshots
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clutch_snapshot_captures_group_composition_and_cascades() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    async fn make_lineage(client: &reqwest::Client, base: &str, name: &str) -> i64 {
        client
            .post(format!("{base}/api/lineages"))
            .json(&CreateLineage {
                name: name.into(),
                source: String::new(),
                notes: None,
            })
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    }
    async fn make_bird(client: &reqwest::Client, base: &str, sex: Sex, lineage: i64) -> i64 {
        let b = CreateBird {
            band_color: None,
            sex,
            lineage_ids: vec![lineage],
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 0,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
        };
        client
            .post(format!("{base}/api/birds"))
            .json(&b)
            .send()
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()["id"]
            .as_i64()
            .unwrap()
    }

    // Spec example: 1 NWQuail male + (4 NWQuail + 1 Fernbank) hens.
    let nw = make_lineage(&client, &base, "NWQuail").await;
    let fb = make_lineage(&client, &base, "Fernbank").await;
    let male = make_bird(&client, &base, Sex::Male, nw).await;
    let mut females = Vec::new();
    for _ in 0..4 {
        females.push(make_bird(&client, &base, Sex::Female, nw).await);
    }
    females.push(make_bird(&client, &base, Sex::Female, fb).await);

    let group_id = client
        .post(format!("{base}/api/breeding-groups"))
        .json(&serde_json::json!({
            "name": "Mixed hens", "male_ids": [male], "female_ids": females,
            "start_date": "2026-01-01", "notes": null,
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    // Create a clutch linked to the group — this snapshots the composition.
    let created = client
        .post(format!("{base}/api/clutches"))
        .json(&serde_json::json!({
            "breeding_group_id": group_id, "lineage_id": null, "eggs_set": 12,
            "eggs_fertile": null, "eggs_hatched": null, "set_date": "2026-06-01",
            "status": "Incubating", "notes": null,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let created_body: serde_json::Value = created.json().await.unwrap();
    let clutch_id = created_body["id"].as_i64().unwrap();
    // POST already returns the snapshot.
    assert!(created_body["snapshot"].is_object());

    // GET the clutch detail and check the frozen composition + distributions.
    let detail: serde_json::Value = reqwest::get(format!("{base}/api/clutches/{clutch_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(detail["breeding_group_id"].as_i64(), Some(group_id));
    let snap = &detail["snapshot"];
    assert_eq!(snap["males"].as_array().unwrap().len(), 1);
    assert_eq!(snap["females"].as_array().unwrap().len(), 5);

    // Paternal: certain (single male) — 100% NWQuail.
    let pat = snap["paternal_distribution"].as_array().unwrap();
    assert_eq!(pat.len(), 1);
    assert_eq!(pat[0]["lineage_id"].as_i64(), Some(nw));
    assert!((pat[0]["probability"].as_f64().unwrap() - 1.0).abs() < 1e-9);

    // Maternal: 0.8 NWQuail, 0.2 Fernbank (highest first, names included).
    let mat = snap["maternal_distribution"].as_array().unwrap();
    assert_eq!(mat.len(), 2);
    assert_eq!(mat[0]["lineage_id"].as_i64(), Some(nw));
    assert_eq!(mat[0]["lineage_name"], "NWQuail");
    assert!((mat[0]["probability"].as_f64().unwrap() - 0.8).abs() < 1e-9);
    assert_eq!(mat[1]["lineage_id"].as_i64(), Some(fb));
    assert!((mat[1]["probability"].as_f64().unwrap() - 0.2).abs() < 1e-9);

    // Deleting the clutch cascades away its snapshot; GET then 404s.
    let del = client
        .delete(format!("{base}/api/clutches/{clutch_id}"))
        .send()
        .await
        .unwrap();
    assert_eq!(del.status(), 204);
    let after = reqwest::get(format!("{base}/api/clutches/{clutch_id}"))
        .await
        .unwrap();
    assert_eq!(after.status(), 404);
}

#[tokio::test]
async fn clutch_without_group_has_null_snapshot() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();
    let lin = client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: "Solo".into(),
            source: String::new(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let created: serde_json::Value = client
        .post(format!("{base}/api/clutches"))
        .json(&serde_json::json!({
            "breeding_group_id": null, "lineage_id": lin, "eggs_set": 10,
            "eggs_fertile": null, "eggs_hatched": null, "set_date": "2026-06-01",
            "status": "Incubating", "notes": null,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let clutch_id = created["id"].as_i64().unwrap();
    let detail: serde_json::Value = reqwest::get(format!("{base}/api/clutches/{clutch_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    // Lineage-only clutch: no snapshot.
    assert!(detail["snapshot"].is_null());
}

// ---------------------------------------------------------------------------
// Phase 3: probabilistic genetic profiles on birds
// ---------------------------------------------------------------------------

async fn make_lineage_p3(client: &reqwest::Client, base: &str, name: &str) -> i64 {
    client
        .post(format!("{base}/api/lineages"))
        .json(&CreateLineage {
            name: name.into(),
            source: String::new(),
            notes: None,
        })
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

async fn make_source_bird_p3(client: &reqwest::Client, base: &str, sex: Sex, lineage: i64) -> i64 {
    client
        .post(format!("{base}/api/birds"))
        .json(&CreateBird {
            band_color: None,
            sex,
            lineage_ids: vec![lineage],
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            mother_id: None,
            father_id: None,
            generation: 0,
            status: BirdStatus::Active,
            notes: None,
            nfc_tag_id: None,
            chick_group_id: None,
        })
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn get_bird_includes_source_bird_genetic_profile() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    let lin = make_lineage_p3(&client, &base, "Pharaoh").await;
    let bird_id = make_source_bird_p3(&client, &base, Sex::Female, lin).await;

    // The new GET /api/birds/{id} endpoint returns the probabilistic profile.
    let bird: Bird = reqwest::get(format!("{base}/api/birds/{bird_id}"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // A gen-0 source bird is 100% its single lineage on both sides.
    assert_eq!(bird.genetic_profile.paternal.len(), 1);
    assert_eq!(bird.genetic_profile.paternal[0].lineage_id, lin);
    assert!((bird.genetic_profile.paternal[0].probability - 1.0).abs() < 1e-9);
    assert_eq!(bird.genetic_profile.maternal.len(), 1);
    assert!((bird.genetic_profile.maternal[0].probability - 1.0).abs() < 1e-9);
    // confidence = min(max(pat), max(mat)) = 1.0.
    assert!((bird.confidence - 1.0).abs() < 1e-9);

    // Unknown id 404s.
    let missing = reqwest::get(format!("{base}/api/birds/999999"))
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn graduated_bird_inherits_clutch_snapshot_genetics() {
    let base = spawn_test_server().await;
    let client = reqwest::Client::new();

    // Spec example: 1 NWQuail male + (4 NWQuail + 1 Fernbank) hens.
    let nw = make_lineage_p3(&client, &base, "NWQuail").await;
    let fb = make_lineage_p3(&client, &base, "Fernbank").await;
    let male = make_source_bird_p3(&client, &base, Sex::Male, nw).await;
    let mut females = Vec::new();
    for _ in 0..4 {
        females.push(make_source_bird_p3(&client, &base, Sex::Female, nw).await);
    }
    females.push(make_source_bird_p3(&client, &base, Sex::Female, fb).await);

    let group_id = client
        .post(format!("{base}/api/breeding-groups"))
        .json(&serde_json::json!({
            "name": "Mixed hens", "male_ids": [male], "female_ids": females,
            "start_date": "2026-01-01", "notes": null,
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    // Clutch linked to the group freezes the composition snapshot.
    let clutch_id = client
        .post(format!("{base}/api/clutches"))
        .json(&serde_json::json!({
            "breeding_group_id": group_id, "lineage_id": null, "eggs_set": 12,
            "eggs_fertile": null, "eggs_hatched": null, "set_date": "2026-06-01",
            "status": "Incubating", "notes": null,
        }))
        .send()
        .await
        .unwrap()
        .json::<serde_json::Value>()
        .await
        .unwrap()["id"]
        .as_i64()
        .unwrap();

    // A chick group descended from that clutch, then graduate one bird.
    let group: ChickGroup = client
        .post(format!("{base}/api/chick-groups"))
        .json(&CreateChickGroup {
            clutch_id: Some(clutch_id),
            brooder_id: None,
            initial_count: 1,
            hatch_date: chrono::NaiveDate::from_ymd_opt(2026, 6, 18).unwrap(),
            notes: None,
            lineage_ids: vec![nw],
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let graduated: Vec<Bird> = client
        .post(format!("{base}/api/chick-groups/{}/graduate", group.id))
        .json(&GraduateRequest {
            target_housing_id: None,
            birds: vec![GraduateBird {
                sex: Sex::Female,
                band_color: None,
                nfc_tag_id: None,
                notes: None,
                weight_grams: None,
                photo_path: None,
            }],
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(graduated.len(), 1);
    let chick = &graduated[0];

    // Paternal: certain (single NWQuail male) — 100%.
    assert_eq!(chick.genetic_profile.paternal.len(), 1);
    assert_eq!(chick.genetic_profile.paternal[0].lineage_id, nw);
    assert!((chick.genetic_profile.paternal[0].probability - 1.0).abs() < 1e-9);

    // Maternal: 0.8 NWQuail, 0.2 Fernbank (highest first).
    let mat = &chick.genetic_profile.maternal;
    assert_eq!(mat.len(), 2);
    assert_eq!(mat[0].lineage_id, nw);
    assert!((mat[0].probability - 0.8).abs() < 1e-9);
    assert_eq!(mat[1].lineage_id, fb);
    assert!((mat[1].probability - 0.2).abs() < 1e-9);

    // confidence = min(1.0, 0.8) = 0.8.
    assert!((chick.confidence - 0.8).abs() < 1e-9);

    // The same profile is served by GET /api/birds/{id}.
    let refetched: Bird = reqwest::get(format!("{base}/api/birds/{}", chick.id))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!((refetched.confidence - 0.8).abs() < 1e-9);
}
