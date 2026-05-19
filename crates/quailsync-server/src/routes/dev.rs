//! Dev/test endpoints — only registered when DEV_MODE=true.
//!
//! All routes operate on the live SQLite connection held in AppState. Backups
//! use SQLite's built-in VACUUM INTO (atomic, transaction-aware) rather than
//! a raw filesystem copy that could capture a torn page during a write.
//! Restore uses ATTACH DATABASE + INSERT...SELECT so we never have to close
//! and reopen the live connection (which would require AppState surgery).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::state::{acquire_db, db_error, AppState};

/// Path of the one-shot production backup created by `/api/dev/seed`. Lives
/// next to `quailsync.db` so existing volume mounts pick it up automatically.
pub const BACKUP_PATH: &str = "quailsync.db.production-backup";

/// True when the DEV_MODE env var is set to "true". Used by `lib.rs` to decide
/// whether to register dev routes. Everything else in this module assumes the
/// caller already gated on this — handlers don't re-check.
pub fn dev_mode_enabled() -> bool {
    std::env::var("DEV_MODE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

// =====================================================================
// GET /api/dev/status
// =====================================================================

#[derive(Serialize)]
pub(crate) struct DevStatus {
    dev_mode: bool,
    has_backup: bool,
}

pub(crate) async fn status() -> Json<DevStatus> {
    // The route is only wired in when dev_mode_enabled() is true, so
    // dev_mode is unconditionally true here.
    Json(DevStatus {
        dev_mode: true,
        has_backup: std::path::Path::new(BACKUP_PATH).exists(),
    })
}

// =====================================================================
// POST /api/dev/seed
// POST /api/dev/stress-seed
// POST /api/dev/restore
// =====================================================================

#[derive(Serialize)]
struct SeedResult {
    status: &'static str,
    backup: &'static str,
}

#[derive(Serialize)]
struct RestoreResult {
    status: &'static str,
}

pub(crate) async fn seed(State(state): State<AppState>) -> axum::response::Response {
    let conn = acquire_db(&state);
    // Backup must succeed AND produce a usable file before we touch the
    // live DB. back_up_to_production already short-circuits on SQL error,
    // missing file, or zero-byte stub by returning a 500 Response — we
    // pass it straight through, leaving the live DB untouched.
    if let Err(resp) = back_up_to_production(&conn) {
        return resp;
    }
    if let Err(e) = wipe_all_tables(&conn) {
        return db_error(e);
    }
    if let Err(e) = insert_basic_seed(&conn) {
        return db_error(e);
    }
    (
        StatusCode::OK,
        Json(SeedResult {
            status: "seeded",
            backup: BACKUP_PATH,
        }),
    )
        .into_response()
}

pub(crate) async fn stress_seed(State(state): State<AppState>) -> axum::response::Response {
    let conn = acquire_db(&state);
    if let Err(resp) = back_up_to_production(&conn) {
        return resp;
    }
    if let Err(e) = wipe_all_tables(&conn) {
        return db_error(e);
    }
    if let Err(e) = insert_stress_seed(&conn) {
        return db_error(e);
    }
    (
        StatusCode::OK,
        Json(SeedResult {
            status: "stress-seeded",
            backup: BACKUP_PATH,
        }),
    )
        .into_response()
}

pub(crate) async fn restore(State(state): State<AppState>) -> axum::response::Response {
    if !std::path::Path::new(BACKUP_PATH).exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "no_backup",
                "message": "No production backup found. Seed first to create one.",
            })),
        )
            .into_response();
    }
    let conn = acquire_db(&state);
    if let Err(e) = restore_from_production(&conn) {
        return db_error(e);
    }
    // Restore complete — drop the backup file so subsequent /api/dev/status
    // reports has_backup=false (Android UI reflects this as "production").
    std::fs::remove_file(BACKUP_PATH).ok();
    (StatusCode::OK, Json(RestoreResult { status: "restored" })).into_response()
}

// =====================================================================
// Backup / restore plumbing
// =====================================================================

/// Snapshot the live DB into `BACKUP_PATH` using `VACUUM INTO` — atomic, so
/// the destination only appears once SQLite has fully written a
/// self-consistent copy.
///
/// **Preserves the original production backup across multiple seed runs.**
/// If a backup file already exists we leave it alone: the user might have
/// seeded once (capturing real production data), then run stress-seed —
/// we don't want the second run to overwrite the original backup with
/// already-seeded fixture data, because Restore would then be a no-op.
/// Restore deletes the backup file, so a fresh seed cycle creates a fresh
/// backup.
///
/// Returns a pre-built 500 Response on any failure (SQL error, missing
/// backup file, or zero-byte stub). The caller short-circuits before
/// touching the live DB, so a botched backup never leaves the database
/// half-wiped.
fn back_up_to_production(conn: &Connection) -> Result<(), axum::response::Response> {
    if std::path::Path::new(BACKUP_PATH).exists() {
        // Even when we're preserving an existing backup, verify it's not
        // a zero-byte stub from a prior failed run — using a stub as the
        // restore source would silently wipe the DB on the next Restore.
        verify_backup_file()?;
        println!("[dev] backup at {BACKUP_PATH} already exists — preserving original");
        return Ok(());
    }
    // VACUUM INTO does not support parameter binding for the path, but the
    // value is a compile-time constant so there's no injection surface.
    conn.execute(&format!("VACUUM INTO '{}'", BACKUP_PATH), [])
        .map_err(|e| {
            eprintln!("[dev] backup VACUUM INTO failed: {e}");
            crate::state::internal_error_response()
        })?;
    verify_backup_file()?;
    println!("[dev] backed up live DB to {BACKUP_PATH}");
    Ok(())
}

/// Refuse to proceed unless the backup file exists and is non-zero.
///
/// A zero-byte stub typically means a previous run errored partway through
/// VACUUM INTO (disk full, permission denied between create and write).
/// Treating it as a valid backup would mean a later Restore loads an empty
/// DB and silently wipes the live data. Removing the stub here lets the
/// next attempt run VACUUM INTO from a clean slate.
fn verify_backup_file() -> Result<(), axum::response::Response> {
    let path = std::path::Path::new(BACKUP_PATH);
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > 0 => Ok(()),
        Ok(_) => {
            eprintln!("[dev] backup file {BACKUP_PATH} is zero bytes — removing stub");
            std::fs::remove_file(path).ok();
            Err(crate::state::internal_error_response())
        }
        Err(e) => {
            eprintln!("[dev] backup file {BACKUP_PATH} missing or unreadable: {e}");
            Err(crate::state::internal_error_response())
        }
    }
}

/// Restore the live DB from `BACKUP_PATH` without closing the connection.
/// Uses ATTACH + INSERT…SELECT so the live connection's prepared-statement
/// cache and schema view stay coherent (file-swap would leave both stale
/// until a process restart).
///
/// FK checks are disabled inside the txn so we can DELETE in any order and
/// INSERT in any order — we re-enable them on commit. The PRAGMA inside a
/// transaction is a no-op in SQLite, so the toggle wraps the txn instead.
fn restore_from_production(conn: &Connection) -> rusqlite::Result<()> {
    // ATTACH the backup file under an alias so we can read from it.
    conn.execute(
        &format!("ATTACH DATABASE '{}' AS backup_db", BACKUP_PATH),
        [],
    )?;
    let result = (|| -> rusqlite::Result<()> {
        conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let txn_result = (|| -> rusqlite::Result<()> {
            conn.execute_batch("BEGIN;")?;
            wipe_all_tables_inner(conn)?;
            // Copy every user table from the backup. The table list mirrors
            // wipe_all_tables_inner — keep them in sync. Order doesn't matter
            // here because FKs are off.
            for table in ALL_TABLES {
                conn.execute(
                    &format!("INSERT INTO main.{table} SELECT * FROM backup_db.{table}"),
                    [],
                )?;
            }
            // Restore the autoincrement counters so newly-created rows after
            // restore continue from where the production DB left off.
            // sqlite_sequence isn't in ALL_TABLES (it's an internal table).
            conn.execute("DELETE FROM main.sqlite_sequence", [])?;
            conn.execute(
                "INSERT INTO main.sqlite_sequence SELECT * FROM backup_db.sqlite_sequence",
                [],
            )
            .ok(); // Some test DBs may not have a sqlite_sequence row yet.
            conn.execute_batch("COMMIT;")?;
            Ok(())
        })();
        if txn_result.is_err() {
            conn.execute_batch("ROLLBACK;").ok();
        }
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        txn_result
    })();
    conn.execute("DETACH DATABASE backup_db", []).ok();
    result?;
    println!("[dev] restored live DB from {BACKUP_PATH}");
    Ok(())
}

/// Every user table the seed/restore paths need to touch. Order is
/// FK-dependents-first so DELETE works even with foreign_keys = ON (the
/// wipe path doesn't actually need this because we toggle FKs off, but
/// keeping the order means we could enable per-statement FK checks later
/// without rewriting the list).
const ALL_TABLES: &[&str] = &[
    "bird_lineages",
    "chick_group_lineages",
    "breeding_group_members",
    "weight_records",
    "processing_records",
    "headcounts",
    "chick_mortality_log",
    "detection_results",
    "frame_captures",
    "camera_feeds",
    "brooder_readings",
    "system_metrics",
    "detection_events",
    "alerts",
    "system_alerts",
    "breeding_groups",
    "breeding_pairs",
    "chick_groups",
    "clutches",
    "birds",
    "brooders",
    "lineages",
];

/// Public-facing wipe: handles its own FK toggle + transaction so seed
/// callers can just `wipe_all_tables(&conn)?` and not worry about state.
fn wipe_all_tables(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    let result = (|| -> rusqlite::Result<()> {
        conn.execute_batch("BEGIN;")?;
        wipe_all_tables_inner(conn)?;
        // Reset autoincrement so seed IDs start at 1 for predictable testing.
        conn.execute("DELETE FROM sqlite_sequence", []).ok();
        conn.execute_batch("COMMIT;")?;
        Ok(())
    })();
    if result.is_err() {
        conn.execute_batch("ROLLBACK;").ok();
    }
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    result
}

/// DELETE FROM every user table. Caller is responsible for FK toggle and
/// transaction boundaries.
fn wipe_all_tables_inner(conn: &Connection) -> rusqlite::Result<()> {
    for table in ALL_TABLES {
        conn.execute(&format!("DELETE FROM {table}"), [])?;
    }
    Ok(())
}

// =====================================================================
// Basic seed — 5 lineages, 5 housing units, 4 chick groups, 2 clutches,
// 15 birds with mixed sexes/bands/NFC/lineages.
// =====================================================================

fn insert_basic_seed(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN;")?;
    let result = insert_basic_seed_inner(conn);
    if result.is_err() {
        conn.execute_batch("ROLLBACK;").ok();
    } else {
        conn.execute_batch("COMMIT;")?;
    }
    result
}

fn insert_basic_seed_inner(conn: &Connection) -> rusqlite::Result<()> {
    use chrono::Duration;
    let today = chrono::Local::now().date_naive();
    let ymd = |offset_days: i64| {
        (today - Duration::days(offset_days))
            .format("%Y-%m-%d")
            .to_string()
    };

    // --- Lineages (5) ---
    let lineage_names = [
        ("Pharaoh", "Stromberg's 2024", "Foundation flock — fast growth"),
        ("Texas A&M", "University stock 2024", "White-feathered meat line"),
        ("Italian", "Hatchery Direct", "Goldspeckled, dual-purpose"),
        ("English White", "Private breeder", "Display line"),
        ("Tibetan", "Imported 2023", "Dark plumage, hardy"),
    ];
    for (name, source, notes) in &lineage_names {
        conn.execute(
            "INSERT INTO lineages (name, source, notes) VALUES (?1, ?2, ?3)",
            params![name, source, notes],
        )?;
    }
    // Lineage IDs are 1..=5 after this insert because we wiped sqlite_sequence.
    let (pharaoh, texas, italian, english, tibetan) = (1i64, 2i64, 3i64, 4i64, 5i64);

    // --- Housing (5) ---
    // (name, lineage_id, life_stage, qr_code, notes, housing_type, camera_url)
    let housing = [
        ("Incubator 1", None, "Chick", "INC-01", Some("Main hatchery incubator"), "incubator", None),
        ("Brooder 1", Some(pharaoh), "Chick", "BRD-01", Some("Camera-equipped"), "brooder", Some("rtsp://192.168.1.50:8554/cam1")),
        ("Brooder 2", Some(texas), "Chick", "BRD-02", None, "brooder", None),
        ("Hutch A", Some(pharaoh), "Adult", "HUT-A", Some("South barn"), "hutch", None),
        ("Hutch B", Some(italian), "Adult", "HUT-B", Some("North barn"), "hutch", None),
    ];
    for (name, lineage_id, stage, qr, notes, htype, camera) in &housing {
        conn.execute(
            "INSERT INTO brooders (name, lineage_id, life_stage, qr_code, notes, housing_type, camera_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![name, lineage_id, stage, qr, notes, htype, camera],
        )?;
    }
    let (incubator_1, brooder_1, brooder_2, hutch_a, hutch_b) = (1i64, 2i64, 3i64, 4i64, 5i64);

    // --- Clutches (2 — one incubating, one hatched) ---
    // (breeding_pair_id, lineage_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes)
    conn.execute(
        "INSERT INTO clutches (breeding_pair_id, lineage_id, eggs_set, eggs_fertile, eggs_hatched,
                               set_date, expected_hatch_date, status, notes)
         VALUES (NULL, ?1, 24, 22, NULL, ?2, ?3, 'Incubating', 'Day 12 candling: 22/24 fertile')",
        params![pharaoh, ymd(12), ymd(-6)],
    )?;
    conn.execute(
        "INSERT INTO clutches (breeding_pair_id, lineage_id, eggs_set, eggs_fertile, eggs_hatched,
                               set_date, expected_hatch_date, status, notes)
         VALUES (NULL, ?1, 30, 27, 25, ?2, ?3, 'Hatched', '25 chicks moved to Brooder 1')",
        params![texas, ymd(30), ymd(12)],
    )?;
    let (clutch_incubating, clutch_hatched) = (1i64, 2i64);

    // --- Chick groups (4 — incubating, active brooder, ready-to-graduate, graduated) ---
    // (clutch_id, brooder_id, initial_count, current_count, hatch_date, status, notes, housing_id)
    let groups = [
        // 1: incubating — eggs still in incubator, no hatch yet (hatch_date in the future)
        (Some(clutch_incubating), Some(incubator_1), 24, 24, ymd(-6), "Active", "Awaiting hatch", None),
        // 2: active in brooder (recently hatched)
        (Some(clutch_hatched), Some(brooder_1), 25, 23, ymd(12), "Active", "2 mortality day 3", None),
        // 3: ready to graduate (older, in second brooder)
        (None, Some(brooder_2), 18, 17, ymd(35), "Active", "Banding scheduled", None),
        // 4: graduated, housed in hutch
        (None, None, 14, 14, ymd(70), "Graduated", "Moved to Hutch A", Some(hutch_a)),
    ];
    for (clutch_id, brooder_id, initial, current, hatch, status, notes, housing_id) in &groups {
        conn.execute(
            "INSERT INTO chick_groups (clutch_id, brooder_id, initial_count, current_count,
                                       hatch_date, status, notes, housing_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![clutch_id, brooder_id, initial, current, hatch, status, notes, housing_id],
        )?;
    }
    let (cg_incubating, cg_brooder, cg_ready, cg_graduated) = (1i64, 2i64, 3i64, 4i64);

    // Attach lineages to chick groups (group 2 is multi-lineage to exercise that path).
    conn.execute(
        "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
        params![cg_incubating, pharaoh],
    )?;
    conn.execute(
        "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
        params![cg_brooder, texas],
    )?;
    conn.execute(
        "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
        params![cg_brooder, italian],
    )?;
    conn.execute(
        "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
        params![cg_ready, english],
    )?;
    conn.execute(
        "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
        params![cg_graduated, tibetan],
    )?;

    // --- Birds (15) ---
    // 8 adults housed in hutches, 7 chicks linked to chick groups.
    struct B {
        band: Option<&'static str>,
        sex: &'static str,
        hatch_offset: i64,
        status: &'static str,
        nfc: Option<&'static str>,
        current_brooder: Option<i64>,
        housing: Option<i64>,
        chick_group: Option<i64>,
        lineages: Vec<i64>,
        notes: Option<&'static str>,
    }
    let birds = vec![
        // --- Adults in Hutch A (Pharaoh foundation pair + 2 hens) ---
        B { band: Some("Red"), sex: "Male", hatch_offset: 180, status: "Active",
            nfc: Some("04A1B2C3D4E5F6"), current_brooder: None, housing: Some(hutch_a),
            chick_group: None, lineages: vec![pharaoh], notes: Some("Foundation male") },
        B { band: Some("Blue"), sex: "Female", hatch_offset: 180, status: "Active",
            nfc: Some("04A1B2C3D4E501"), current_brooder: None, housing: Some(hutch_a),
            chick_group: None, lineages: vec![pharaoh], notes: None },
        B { band: Some("Blue"), sex: "Female", hatch_offset: 175, status: "Active",
            nfc: None, current_brooder: None, housing: Some(hutch_a),
            chick_group: None, lineages: vec![pharaoh], notes: None },
        B { band: Some("Yellow"), sex: "Female", hatch_offset: 160, status: "Active",
            nfc: Some("04A1B2C3D4E502"), current_brooder: None, housing: Some(hutch_a),
            chick_group: None, lineages: vec![pharaoh, texas], notes: Some("Multi-lineage cross") },
        // --- Adults in Hutch B (Italian + crosses) ---
        B { band: Some("Green"), sex: "Male", hatch_offset: 200, status: "Active",
            nfc: Some("04A1B2C3D4E503"), current_brooder: None, housing: Some(hutch_b),
            chick_group: None, lineages: vec![italian], notes: None },
        B { band: Some("Orange"), sex: "Female", hatch_offset: 195, status: "Active",
            nfc: Some("04A1B2C3D4E504"), current_brooder: None, housing: Some(hutch_b),
            chick_group: None, lineages: vec![italian, english], notes: None },
        B { band: Some("Orange"), sex: "Female", hatch_offset: 190, status: "Active",
            nfc: None, current_brooder: None, housing: Some(hutch_b),
            chick_group: None, lineages: vec![italian], notes: None },
        B { band: Some("White"), sex: "Male", hatch_offset: 220, status: "Active",
            nfc: Some("04A1B2C3D4E505"), current_brooder: None, housing: Some(hutch_b),
            chick_group: None, lineages: vec![english], notes: Some("Retired breeder") },
        // --- Chicks linked to chick groups (current_brooder set, housing null) ---
        B { band: None, sex: "Unknown", hatch_offset: 12, status: "Active",
            nfc: None, current_brooder: Some(brooder_1), housing: None,
            chick_group: Some(cg_brooder), lineages: vec![texas], notes: None },
        B { band: None, sex: "Unknown", hatch_offset: 12, status: "Active",
            nfc: None, current_brooder: Some(brooder_1), housing: None,
            chick_group: Some(cg_brooder), lineages: vec![texas], notes: None },
        B { band: None, sex: "Unknown", hatch_offset: 12, status: "Active",
            nfc: None, current_brooder: Some(brooder_1), housing: None,
            chick_group: Some(cg_brooder), lineages: vec![texas, italian], notes: None },
        B { band: None, sex: "Unknown", hatch_offset: 35, status: "Active",
            nfc: None, current_brooder: Some(brooder_2), housing: None,
            chick_group: Some(cg_ready), lineages: vec![english], notes: None },
        B { band: None, sex: "Unknown", hatch_offset: 35, status: "Active",
            nfc: None, current_brooder: Some(brooder_2), housing: None,
            chick_group: Some(cg_ready), lineages: vec![english], notes: None },
        B { band: Some("Pink"), sex: "Female", hatch_offset: 70, status: "Active",
            nfc: Some("04A1B2C3D4E506"), current_brooder: None, housing: Some(hutch_a),
            chick_group: Some(cg_graduated), lineages: vec![tibetan], notes: Some("First-gen Tibetan") },
        B { band: Some("Pink"), sex: "Male", hatch_offset: 70, status: "Active",
            nfc: None, current_brooder: None, housing: Some(hutch_a),
            chick_group: Some(cg_graduated), lineages: vec![tibetan, pharaoh], notes: None },
    ];
    for b in &birds {
        conn.execute(
            "INSERT INTO birds (band_color, sex, hatch_date, mother_id, father_id, generation,
                                status, notes, nfc_tag_id, current_brooder_id, photo_path,
                                housing_id, chick_group_id)
             VALUES (?1, ?2, ?3, NULL, NULL, 1, ?4, ?5, ?6, ?7, NULL, ?8, ?9)",
            params![
                b.band,
                b.sex,
                ymd(b.hatch_offset),
                b.status,
                b.notes,
                b.nfc,
                b.current_brooder,
                b.housing,
                b.chick_group,
            ],
        )?;
        let bird_id = conn.last_insert_rowid();
        for &lineage_id in &b.lineages {
            conn.execute(
                "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
                params![bird_id, lineage_id],
            )?;
        }
    }

    // --- A breeding pair so /api/breeding/suggest has data to reason over ---
    // Pair the Pharaoh foundation male (bird 1, Red) and Pharaoh hen (bird 2, Blue).
    conn.execute(
        "INSERT INTO breeding_pairs (male_id, female_id, start_date, notes)
         VALUES (1, 2, ?1, 'Foundation Pharaoh pair')",
        params![ymd(45)],
    )?;

    // Re-point clutch 1 to this pair for realistic provenance.
    conn.execute(
        "UPDATE clutches SET breeding_pair_id = 1 WHERE id = ?1",
        params![clutch_incubating],
    )?;

    println!("[dev] basic seed installed (5 lineages, 5 housing units, 4 chick groups, 15 birds)");
    Ok(())
}

// =====================================================================
// Stress seed — exercises Jaccard relatedness at scale.
// 10 lineages, 8 housing units, ~20 chick groups, ~60 birds with multi-
// lineage crosses, multiple breeding pairs.
// =====================================================================

fn insert_stress_seed(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN;")?;
    let result = insert_stress_seed_inner(conn);
    if result.is_err() {
        conn.execute_batch("ROLLBACK;").ok();
    } else {
        conn.execute_batch("COMMIT;")?;
    }
    result
}

fn insert_stress_seed_inner(conn: &Connection) -> rusqlite::Result<()> {
    use chrono::Duration;
    let today = chrono::Local::now().date_naive();
    let ymd = |offset_days: i64| {
        (today - Duration::days(offset_days))
            .format("%Y-%m-%d")
            .to_string()
    };

    // --- 10 lineages ---
    let lineage_names: [(&str, &str); 10] = [
        ("Pharaoh", "Stromberg's"),
        ("Texas A&M", "University stock"),
        ("Italian", "Hatchery Direct"),
        ("English White", "Private breeder"),
        ("Tibetan", "Imported 2023"),
        ("Manchurian Golden", "Heritage line"),
        ("Tuxedo", "Color mutation 2024"),
        ("Rosetta", "Pattern line"),
        ("Range", "Wild-cross 2022"),
        ("Jumbo Brown", "Meat selection"),
    ];
    for (i, (name, source)) in lineage_names.iter().enumerate() {
        conn.execute(
            "INSERT INTO lineages (name, source, notes) VALUES (?1, ?2, ?3)",
            params![name, source, format!("Stress lineage {}", i + 1)],
        )?;
    }
    let lineage_ids: Vec<i64> = (1..=10).collect();

    // --- 8 housing units (2 incubator, 3 brooder, 3 hutch) ---
    let housing = [
        ("Incubator A", "incubator", None::<&str>),
        ("Incubator B", "incubator", None),
        ("Brooder 1", "brooder", Some("rtsp://10.0.0.51:8554/cam1")),
        ("Brooder 2", "brooder", None),
        ("Brooder 3", "brooder", Some("rtsp://10.0.0.53:8554/cam3")),
        ("Hutch North", "hutch", None),
        ("Hutch South", "hutch", None),
        ("Hutch East", "hutch", None),
    ];
    for (i, (name, htype, camera)) in housing.iter().enumerate() {
        conn.execute(
            "INSERT INTO brooders (name, lineage_id, life_stage, qr_code, notes, housing_type, camera_url)
             VALUES (?1, NULL, ?2, ?3, NULL, ?4, ?5)",
            params![
                name,
                if *htype == "hutch" { "Adult" } else { "Chick" },
                format!("STR-{:02}", i + 1),
                htype,
                camera,
            ],
        )?;
    }
    let hutches = [6i64, 7i64, 8i64];
    let brooders = [3i64, 4i64, 5i64];

    // --- 60 birds with 2-4 lineages each ---
    // Deterministic pseudo-random distribution so seed runs are reproducible.
    // We don't pull in `rand`; a small linear-congruential generator is fine
    // for fixture data.
    let mut lcg: u32 = 0x12345678;
    let mut next_u32 = || {
        lcg = lcg.wrapping_mul(1664525).wrapping_add(1013904223);
        lcg
    };
    let bands = ["Red", "Blue", "Yellow", "Green", "White", "Pink", "Orange", "Purple"];

    for i in 0..60 {
        // 2-4 lineages per bird, drawn without replacement from the 10.
        let lineage_count = 2 + (next_u32() % 3) as usize;
        let mut assigned: Vec<i64> = Vec::with_capacity(lineage_count);
        while assigned.len() < lineage_count {
            let candidate = lineage_ids[(next_u32() as usize) % lineage_ids.len()];
            if !assigned.contains(&candidate) {
                assigned.push(candidate);
            }
        }
        let sex = match next_u32() % 3 {
            0 => "Male",
            1 => "Female",
            _ => "Unknown",
        };
        let age_days = 30 + (next_u32() % 300) as i64;
        // First 40 are adults housed in hutches; remaining 20 are chicks in
        // brooders so the Jaccard calculation has both pools to work with.
        let (housing_id, current_brooder_id) = if i < 40 {
            (Some(hutches[i % hutches.len()]), None)
        } else {
            (None, Some(brooders[i % brooders.len()]))
        };
        let band = if sex == "Unknown" {
            None
        } else {
            Some(bands[(next_u32() as usize) % bands.len()])
        };
        // ~70% of adults carry an NFC tag; chicks never do.
        let nfc = if i < 40 && next_u32() % 10 < 7 {
            Some(format!("04STRESS{:08X}", next_u32()))
        } else {
            None
        };

        conn.execute(
            "INSERT INTO birds (band_color, sex, hatch_date, mother_id, father_id, generation,
                                status, notes, nfc_tag_id, current_brooder_id, photo_path,
                                housing_id, chick_group_id)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, 'Active', NULL, ?5, ?6, NULL, ?7, NULL)",
            params![
                band,
                sex,
                ymd(age_days),
                1 + (i as i64 / 20), // generations 1..=3
                nfc,
                current_brooder_id,
                housing_id,
            ],
        )?;
        let bird_id = conn.last_insert_rowid();
        for lineage_id in &assigned {
            conn.execute(
                "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
                params![bird_id, lineage_id],
            )?;
        }
    }

    // --- 5 breeding pairs across lineage boundaries — gives /breeding/suggest
    //     plenty of cross-lineage candidate pairings to score ---
    // Pick the first male and first female encountered for each pair.
    let male_ids: Vec<i64> = conn
        .prepare("SELECT id FROM birds WHERE sex='Male' ORDER BY id LIMIT 5")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();
    let female_ids: Vec<i64> = conn
        .prepare("SELECT id FROM birds WHERE sex='Female' ORDER BY id LIMIT 5")?
        .query_map([], |r| r.get::<_, i64>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for (m, f) in male_ids.iter().zip(female_ids.iter()) {
        conn.execute(
            "INSERT INTO breeding_pairs (male_id, female_id, start_date, notes)
             VALUES (?1, ?2, ?3, 'Stress pair')",
            params![m, f, ymd(60)],
        )?;
    }

    // --- 20 chick groups spread across the brooders + a few graduated to
    //     hutches — exercises the Hatchery card rendering at volume ---
    for i in 0..20 {
        let is_graduated = i >= 15;
        let (brooder_id, housing_id, status) = if is_graduated {
            (None, Some(hutches[i % hutches.len()]), "Graduated")
        } else {
            (Some(brooders[i % brooders.len()]), None, "Active")
        };
        let initial = 10 + (i as i64 * 3);
        let current = initial - (i as i64 % 5);
        conn.execute(
            "INSERT INTO chick_groups (clutch_id, brooder_id, initial_count, current_count,
                                       hatch_date, status, notes, housing_id)
             VALUES (NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                brooder_id,
                initial,
                current,
                ymd(7 + (i as i64 * 5)),
                status,
                format!("Stress group {}", i + 1),
                housing_id,
            ],
        )?;
        let group_id = conn.last_insert_rowid();
        // 1-3 lineages per group.
        let group_lineage_count = 1 + (i % 3);
        let mut assigned: Vec<i64> = Vec::new();
        for j in 0..group_lineage_count {
            let candidate = lineage_ids[(i + j) % lineage_ids.len()];
            if !assigned.contains(&candidate) {
                assigned.push(candidate);
                conn.execute(
                    "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
                    params![group_id, candidate],
                )?;
            }
        }
    }

    println!("[dev] stress seed installed (10 lineages, 8 housing, 60 birds, 20 chick groups, 5 breeding pairs)");
    Ok(())
}
