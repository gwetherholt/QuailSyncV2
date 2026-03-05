use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};
use tokio::sync::broadcast;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use colored::Colorize;
use quailsync_common::{
    Alert, AlertConfig, Bird, BirdStatus, Bloodline, BreedingGroup, BreedingPair, Brooder,
    BrooderReading, CameraFeed, CameraStatus, ChickGroup, ChickGroupStatus, ChickMortalityLog,
    Clutch, ClutchStatus, CreateBird, CreateBloodline, CreateBreedingGroup, CreateBreedingPair,
    CreateBrooder, CreateCameraFeed, CreateChickGroup, CreateClutch, CreateDetectionResult,
    CreateFrameCapture, CreateProcessingRecord, CreateWeightRecord, CullReason,
    CullRecommendation, DetectionResult, FrameCapture, GraduateRequest, InbreedingCoefficient,
    LifeStage, MortalityRequest, ProcessingRecord, ProcessingReason, ProcessingStatus, Sex,
    Severity, Species, SystemMetrics, TelemetryPayload, UpdateBird, UpdateClutch,
    UpdateProcessingRecord, WeightRecord, COTURNIX_MIN_BREEDING_WEIGHT_GRAMS,
    MAX_FEMALES_PER_MALE, MIN_FEMALES_PER_MALE,
};
use rust_embed::Embed;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Embedded dashboard assets
// ---------------------------------------------------------------------------

#[derive(Embed)]
#[folder = "../../dashboard/"]
struct Asset;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub agent_connected: Arc<AtomicBool>,
    pub alert_config: AlertConfig,
    pub live_tx: broadcast::Sender<String>,
}

// ---------------------------------------------------------------------------
// Mutex helper — recover from poison instead of cascading panics
// ---------------------------------------------------------------------------

/// Acquire the database connection, recovering from a poisoned mutex.
/// A poisoned mutex means a previous thread panicked while holding the lock,
/// but the underlying SQLite connection is still valid and usable.
fn acquire_db(state: &AppState) -> std::sync::MutexGuard<'_, Connection> {
    state.db.lock().unwrap_or_else(|poisoned| {
        eprintln!("{}", "[WARN] Database mutex was poisoned — recovering".yellow());
        poisoned.into_inner()
    })
}

// ---------------------------------------------------------------------------
// Database setup
// ---------------------------------------------------------------------------

pub fn init_db(conn: &Connection) {
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("failed to enable foreign keys");
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS brooders (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            name         TEXT NOT NULL,
            bloodline_id INTEGER REFERENCES bloodlines(id),
            life_stage   TEXT NOT NULL DEFAULT 'Chick',
            qr_code      TEXT NOT NULL DEFAULT '',
            notes        TEXT
        );

        CREATE TABLE IF NOT EXISTS brooder_readings (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            temperature     REAL    NOT NULL,
            humidity        REAL    NOT NULL,
            timestamp       TEXT    NOT NULL,
            brooder_id      INTEGER REFERENCES brooders(id),
            received_at     TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS system_metrics (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            cpu_usage       REAL    NOT NULL,
            memory_used     INTEGER NOT NULL,
            memory_total    INTEGER NOT NULL,
            disk_used       INTEGER NOT NULL,
            disk_total      INTEGER NOT NULL,
            uptime_seconds  INTEGER NOT NULL,
            received_at     TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS detection_events (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            species         TEXT    NOT NULL,
            confidence      REAL    NOT NULL,
            timestamp       TEXT    NOT NULL,
            received_at     TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS alerts (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            severity        TEXT    NOT NULL,
            message         TEXT    NOT NULL,
            timestamp       TEXT    NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS bloodlines (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            name            TEXT    NOT NULL,
            source          TEXT    NOT NULL,
            notes           TEXT
        );

        CREATE TABLE IF NOT EXISTS birds (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            band_color      TEXT,
            sex             TEXT    NOT NULL,
            bloodline_id    INTEGER NOT NULL REFERENCES bloodlines(id),
            hatch_date      TEXT    NOT NULL,
            mother_id       INTEGER REFERENCES birds(id),
            father_id       INTEGER REFERENCES birds(id),
            generation      INTEGER NOT NULL DEFAULT 1,
            status          TEXT    NOT NULL DEFAULT 'Active',
            notes           TEXT,
            nfc_tag_id      TEXT    UNIQUE
        );

        CREATE TABLE IF NOT EXISTS breeding_pairs (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            male_id         INTEGER NOT NULL REFERENCES birds(id),
            female_id       INTEGER NOT NULL REFERENCES birds(id),
            start_date      TEXT    NOT NULL,
            end_date        TEXT,
            notes           TEXT
        );

        CREATE TABLE IF NOT EXISTS clutches (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            breeding_pair_id    INTEGER REFERENCES breeding_pairs(id),
            bloodline_id        INTEGER REFERENCES bloodlines(id),
            eggs_set            INTEGER NOT NULL,
            eggs_fertile        INTEGER,
            eggs_hatched        INTEGER,
            set_date            TEXT    NOT NULL,
            expected_hatch_date TEXT    NOT NULL,
            status              TEXT    NOT NULL DEFAULT 'Incubating',
            notes               TEXT,
            eggs_stillborn      INTEGER,
            eggs_quit           INTEGER,
            eggs_infertile      INTEGER,
            eggs_damaged        INTEGER,
            hatch_notes         TEXT
        );

        CREATE TABLE IF NOT EXISTS weight_records (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            bird_id      INTEGER NOT NULL REFERENCES birds(id),
            weight_grams REAL    NOT NULL,
            date         TEXT    NOT NULL,
            notes        TEXT
        );

        CREATE TABLE IF NOT EXISTS processing_records (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            bird_id            INTEGER NOT NULL REFERENCES birds(id),
            reason             TEXT    NOT NULL,
            scheduled_date     TEXT    NOT NULL,
            processed_date     TEXT,
            final_weight_grams REAL,
            status             TEXT    NOT NULL DEFAULT 'Scheduled',
            notes              TEXT
        );

        CREATE TABLE IF NOT EXISTS breeding_groups (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT    NOT NULL,
            male_id    INTEGER NOT NULL REFERENCES birds(id),
            start_date TEXT    NOT NULL,
            notes      TEXT
        );

        CREATE TABLE IF NOT EXISTS breeding_group_members (
            group_id  INTEGER NOT NULL REFERENCES breeding_groups(id),
            female_id INTEGER NOT NULL REFERENCES birds(id),
            PRIMARY KEY (group_id, female_id)
        );

        CREATE TABLE IF NOT EXISTS camera_feeds (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT NOT NULL,
            location   TEXT NOT NULL,
            feed_url   TEXT NOT NULL,
            status     TEXT NOT NULL DEFAULT 'Active',
            brooder_id INTEGER REFERENCES brooders(id)
        );

        CREATE TABLE IF NOT EXISTS frame_captures (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            camera_id  INTEGER NOT NULL REFERENCES camera_feeds(id),
            timestamp  TEXT    NOT NULL,
            image_path TEXT    NOT NULL,
            life_stage TEXT    NOT NULL
        );

        CREATE TABLE IF NOT EXISTS detection_results (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            frame_id        INTEGER NOT NULL REFERENCES frame_captures(id),
            label           TEXT    NOT NULL,
            confidence      REAL    NOT NULL,
            bounding_box_x  REAL    NOT NULL,
            bounding_box_y  REAL    NOT NULL,
            bounding_box_w  REAL    NOT NULL,
            bounding_box_h  REAL    NOT NULL,
            notes           TEXT
        );",
    )
    .expect("failed to create tables");

    // --- Idempotent migrations ---

    // Hatch detail columns on clutches
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_stillborn INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_quit INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_infertile INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_damaged INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN hatch_notes TEXT", []).ok();

    // NFC tag on birds
    conn.execute("ALTER TABLE birds ADD COLUMN nfc_tag_id TEXT UNIQUE", []).ok();

    // Camera URL on brooders
    conn.execute("ALTER TABLE brooders ADD COLUMN camera_url TEXT", []).ok();

    // Chick groups (nursery)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chick_groups (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            clutch_id     INTEGER REFERENCES clutches(id),
            bloodline_id  INTEGER NOT NULL REFERENCES bloodlines(id),
            brooder_id    INTEGER REFERENCES brooders(id),
            initial_count INTEGER NOT NULL,
            current_count INTEGER NOT NULL,
            hatch_date    TEXT    NOT NULL,
            status        TEXT    NOT NULL DEFAULT 'Active',
            notes         TEXT
        );

        CREATE TABLE IF NOT EXISTS chick_mortality_log (
            id       INTEGER PRIMARY KEY AUTOINCREMENT,
            group_id INTEGER NOT NULL REFERENCES chick_groups(id),
            count    INTEGER NOT NULL,
            reason   TEXT    NOT NULL,
            date     TEXT    NOT NULL DEFAULT (date('now'))
        );",
    )
    .expect("failed to create chick group tables");
}

// ---------------------------------------------------------------------------
// Database writes
// ---------------------------------------------------------------------------

fn store_payload(conn: &Connection, payload: &TelemetryPayload) {
    match payload {
        TelemetryPayload::Brooder(r) => {
            conn.execute(
                "INSERT INTO brooder_readings (temperature, humidity, timestamp, brooder_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![r.temperature_celsius, r.humidity_percent, r.timestamp.to_rfc3339(), r.brooder_id],
            )
            .ok();
        }
        TelemetryPayload::System(m) => {
            conn.execute(
                "INSERT INTO system_metrics
                    (cpu_usage, memory_used, memory_total, disk_used, disk_total, uptime_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                (
                    m.cpu_usage_percent,
                    m.memory_used_bytes,
                    m.memory_total_bytes,
                    m.disk_used_bytes,
                    m.disk_total_bytes,
                    m.uptime_seconds,
                ),
            )
            .ok();
        }
        TelemetryPayload::Detection(d) => {
            let species_str = match &d.species {
                Species::BobwhiteQuail => "BobwhiteQuail".to_string(),
                Species::CoturnixQuail => "CoturnixQuail".to_string(),
                Species::Unknown(s) => format!("Unknown:{s}"),
            };
            conn.execute(
                "INSERT INTO detection_events (species, confidence, timestamp)
                 VALUES (?1, ?2, ?3)",
                (species_str, d.confidence, d.timestamp.to_rfc3339()),
            )
            .ok();
        }
    }
}

fn store_alert(conn: &Connection, severity: &Severity, message: &str) {
    let sev_str = match severity {
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    };
    conn.execute(
        "INSERT INTO alerts (severity, message) VALUES (?1, ?2)",
        (sev_str, message),
    )
    .ok();
}

// ---------------------------------------------------------------------------
// Alert engine
// ---------------------------------------------------------------------------

fn check_brooder_alerts(conn: &Connection, reading: &BrooderReading, config: &AlertConfig) {
    let temp = reading.temperature_celsius;
    let hum = reading.humidity_percent;

    if temp < config.brooder_temp_min {
        let delta = config.brooder_temp_min - temp;
        let severity = if delta > 3.0 {
            Severity::Critical
        } else {
            Severity::Warning
        };
        let msg = format!(
            "Temperature LOW: {:.1}\u{00b0}F (min {:.1}\u{00b0}F, {:.1}\u{00b0}F below)",
            temp, config.brooder_temp_min, delta,
        );
        print_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    } else if temp > config.brooder_temp_max {
        let delta = temp - config.brooder_temp_max;
        let severity = if delta > 3.0 {
            Severity::Critical
        } else {
            Severity::Warning
        };
        let msg = format!(
            "Temperature HIGH: {:.1}\u{00b0}F (max {:.1}\u{00b0}F, {:.1}\u{00b0}F above)",
            temp, config.brooder_temp_max, delta,
        );
        print_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    }

    if hum < config.humidity_min {
        let msg = format!(
            "Humidity LOW: {:.1}% (min {:.1}%)",
            hum, config.humidity_min,
        );
        let severity = Severity::Warning;
        print_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    } else if hum > config.humidity_max {
        let msg = format!(
            "Humidity HIGH: {:.1}% (max {:.1}%)",
            hum, config.humidity_max,
        );
        let severity = Severity::Warning;
        print_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    }
}

fn print_alert(severity: &Severity, message: &str) {
    match severity {
        Severity::Warning => {
            eprintln!("{} {}", "[WARN]".yellow().bold(), message.yellow());
        }
        Severity::Critical => {
            eprintln!("{} {}", "[CRIT]".red().bold(), message.red().bold());
        }
    }
}

// ---------------------------------------------------------------------------
// WebSocket
// ---------------------------------------------------------------------------

async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    println!("[ws] agent connected");
    state.agent_connected.store(true, Ordering::Relaxed);

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<TelemetryPayload>(&text) {
                Ok(payload) => {
                    print_payload(&payload);
                    let conn = acquire_db(&state);
                    store_payload(&conn, &payload);
                    if let TelemetryPayload::Brooder(ref reading) = payload {
                        check_brooder_alerts(&conn, reading, &state.alert_config);
                    }
                    let _ = state.live_tx.send(text.to_string());
                }
                Err(e) => eprintln!("[ws] bad payload: {e}"),
            },
            Message::Close(_) => {
                println!("[ws] agent disconnected");
                break;
            }
            _ => {}
        }
    }

    state.agent_connected.store(false, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Live WebSocket (dashboard clients subscribe to telemetry broadcasts)
// ---------------------------------------------------------------------------

async fn ws_live_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_live_socket(socket, state))
}

async fn handle_live_socket(mut socket: WebSocket, state: AppState) {
    println!("[ws/live] dashboard client connected");
    let mut rx = state.live_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[ws/live] client lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // ignore client-sent messages
                }
            }
        }
    }

    println!("[ws/live] dashboard client disconnected");
}

fn print_payload(payload: &TelemetryPayload) {
    match payload {
        TelemetryPayload::System(m) => {
            println!(
                "[telemetry] system  | cpu: {:.1}%  mem: {}/{}MB  disk: {}/{}GB  up: {}s",
                m.cpu_usage_percent,
                m.memory_used_bytes / 1_048_576,
                m.memory_total_bytes / 1_048_576,
                m.disk_used_bytes / 1_073_741_824,
                m.disk_total_bytes / 1_073_741_824,
                m.uptime_seconds,
            );
        }
        TelemetryPayload::Brooder(r) => {
            println!(
                "[telemetry] brooder | temp: {:.1}\u{00b0}F  humidity: {:.1}%  at {}",
                r.temperature_celsius, r.humidity_percent, r.timestamp,
            );
        }
        TelemetryPayload::Detection(d) => {
            println!(
                "[telemetry] detect  | {:?} ({:.1}% confidence) at {}",
                d.species,
                d.confidence * 100.0,
                d.timestamp,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// REST endpoints
// ---------------------------------------------------------------------------

async fn health() -> &'static str {
    "quailsync-server ok"
}

async fn brooder_latest(State(state): State<AppState>) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let result = conn.query_row(
        "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
         ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_celsius: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
                brooder_id: row.get(3)?,
            })
        },
    );
    match result {
        Ok(reading) => Json(reading).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "no brooder readings yet").into_response(),
    }
}

#[derive(Deserialize)]
struct HistoryParams {
    minutes: Option<u64>,
}

async fn brooder_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
             WHERE received_at >= datetime('now', ?1)
             ORDER BY id DESC",
        )
        .unwrap();

    let cutoff = format!("-{minutes} minutes");
    let readings: Vec<BrooderReading> = stmt
        .query_map([&cutoff], |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_celsius: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
                brooder_id: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(readings)
}

async fn system_latest(State(state): State<AppState>) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let result = conn.query_row(
        "SELECT cpu_usage, memory_used, memory_total, disk_used, disk_total, uptime_seconds
         FROM system_metrics ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            Ok(SystemMetrics {
                cpu_usage_percent: row.get(0)?,
                memory_used_bytes: row.get(1)?,
                memory_total_bytes: row.get(2)?,
                disk_used_bytes: row.get(3)?,
                disk_total_bytes: row.get(4)?,
                uptime_seconds: row.get(5)?,
            })
        },
    );
    match result {
        Ok(metrics) => Json(metrics).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "no system metrics yet").into_response(),
    }
}

#[derive(Serialize)]
struct StatusSummary {
    agent_connected: bool,
    last_brooder_reading: Option<String>,
    last_system_metric: Option<String>,
    last_detection_event: Option<String>,
}

async fn status(State(state): State<AppState>) -> Json<StatusSummary> {
    let conn = acquire_db(&state);

    let last_brooder: Option<String> = conn
        .query_row(
            "SELECT received_at FROM brooder_readings ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let last_system: Option<String> = conn
        .query_row(
            "SELECT received_at FROM system_metrics ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    let last_detection: Option<String> = conn
        .query_row(
            "SELECT received_at FROM detection_events ORDER BY id DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    Json(StatusSummary {
        agent_connected: state.agent_connected.load(Ordering::Relaxed),
        last_brooder_reading: last_brooder,
        last_system_metric: last_system,
        last_detection_event: last_detection,
    })
}

async fn alerts(
    State(state): State<AppState>,
    Query(params): Query<HistoryParams>,
) -> Json<Vec<Alert>> {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT severity, message, timestamp FROM alerts
             WHERE timestamp >= datetime('now', ?1)
             ORDER BY id DESC",
        )
        .unwrap();

    let cutoff = format!("-{minutes} minutes");
    let alerts: Vec<Alert> = stmt
        .query_map([&cutoff], |row| {
            let sev_str: String = row.get(0)?;
            let severity = match sev_str.as_str() {
                "critical" => Severity::Critical,
                _ => Severity::Warning,
            };
            Ok(Alert {
                severity,
                message: row.get(1)?,
                timestamp: row.get(2)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(alerts)
}

// ---------------------------------------------------------------------------
// Flock & Lineage endpoints
// ---------------------------------------------------------------------------

fn sex_to_str(s: &Sex) -> &'static str {
    match s {
        Sex::Male => "Male",
        Sex::Female => "Female",
        Sex::Unknown => "Unknown",
    }
}

fn str_to_sex(s: &str) -> Sex {
    match s {
        "Male" => Sex::Male,
        "Female" => Sex::Female,
        _ => Sex::Unknown,
    }
}

fn bird_status_to_str(s: &BirdStatus) -> &'static str {
    match s {
        BirdStatus::Active => "Active",
        BirdStatus::Culled => "Culled",
        BirdStatus::Deceased => "Deceased",
        BirdStatus::Sold => "Sold",
    }
}

fn str_to_bird_status(s: &str) -> BirdStatus {
    match s {
        "Culled" => BirdStatus::Culled,
        "Deceased" => BirdStatus::Deceased,
        "Sold" => BirdStatus::Sold,
        _ => BirdStatus::Active,
    }
}

fn clutch_status_to_str(s: &ClutchStatus) -> &'static str {
    match s {
        ClutchStatus::Incubating => "Incubating",
        ClutchStatus::Hatched => "Hatched",
        ClutchStatus::Failed => "Failed",
    }
}

fn str_to_clutch_status(s: &str) -> ClutchStatus {
    match s {
        "Hatched" => ClutchStatus::Hatched,
        "Failed" => ClutchStatus::Failed,
        _ => ClutchStatus::Incubating,
    }
}

// --- Bloodlines ---

async fn create_bloodline(
    State(state): State<AppState>,
    Json(body): Json<CreateBloodline>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO bloodlines (name, source, notes) VALUES (?1, ?2, ?3)",
        params![body.name, body.source, body.notes],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(Bloodline {
            id,
            name: body.name,
            source: body.source,
            notes: body.notes,
        }),
    )
}

async fn list_bloodlines(State(state): State<AppState>) -> Json<Vec<Bloodline>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, source, notes FROM bloodlines ORDER BY id")
        .unwrap();
    let rows: Vec<Bloodline> = stmt
        .query_map([], |row| {
            Ok(Bloodline {
                id: row.get(0)?,
                name: row.get(1)?,
                source: row.get(2)?,
                notes: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// --- Birds ---

async fn create_bird(
    State(state): State<AppState>,
    Json(body): Json<CreateBird>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO birds (band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            body.band_color,
            sex_to_str(&body.sex),
            body.bloodline_id,
            body.hatch_date.to_string(),
            body.mother_id,
            body.father_id,
            body.generation,
            bird_status_to_str(&body.status),
            body.notes,
            body.nfc_tag_id,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(Bird {
            id,
            band_color: body.band_color,
            sex: body.sex,
            bloodline_id: body.bloodline_id,
            hatch_date: body.hatch_date,
            mother_id: body.mother_id,
            father_id: body.father_id,
            generation: body.generation,
            status: body.status,
            notes: body.notes,
            nfc_tag_id: body.nfc_tag_id,
        }),
    )
}

async fn list_birds(State(state): State<AppState>) -> Json<Vec<Bird>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id FROM birds ORDER BY id")
        .unwrap();
    let rows: Vec<Bird> = stmt
        .query_map([], |row| {
            let sex_str: String = row.get(2)?;
            let hatch_str: String = row.get(4)?;
            let status_str: String = row.get(8)?;
            Ok(Bird {
                id: row.get(0)?,
                band_color: row.get(1)?,
                sex: str_to_sex(&sex_str),
                bloodline_id: row.get(3)?,
                hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                mother_id: row.get(5)?,
                father_id: row.get(6)?,
                generation: row.get(7)?,
                status: str_to_bird_status(&status_str),
                notes: row.get(9)?,
                nfc_tag_id: row.get(10)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn update_bird(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateBird>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<Bird>)).into_response();
    }

    if let Some(ref status) = body.status {
        conn.execute(
            "UPDATE birds SET status = ?1 WHERE id = ?2",
            params![bird_status_to_str(status), id],
        )
        .unwrap();
    }
    if let Some(ref notes) = body.notes {
        conn.execute(
            "UPDATE birds SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        )
        .unwrap();
    }
    if let Some(ref nfc) = body.nfc_tag_id {
        conn.execute(
            "UPDATE birds SET nfc_tag_id = ?1 WHERE id = ?2",
            params![nfc, id],
        )
        .unwrap();
    }

    let bird = conn
        .query_row(
            "SELECT id, band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id FROM birds WHERE id = ?1",
            params![id],
            |row| {
                let sex_str: String = row.get(2)?;
                let hatch_str: String = row.get(4)?;
                let status_str: String = row.get(8)?;
                Ok(Bird {
                    id: row.get(0)?,
                    band_color: row.get(1)?,
                    sex: str_to_sex(&sex_str),
                    bloodline_id: row.get(3)?,
                    hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                    mother_id: row.get(5)?,
                    father_id: row.get(6)?,
                    generation: row.get(7)?,
                    status: str_to_bird_status(&status_str),
                    notes: row.get(9)?,
                    nfc_tag_id: row.get(10)?,
                })
            },
        )
        .unwrap();

    (StatusCode::OK, Json(Some(bird))).into_response()
}

// --- NFC lookup ---

async fn get_bird_by_nfc(
    State(state): State<AppState>,
    Path(tag_id): Path<String>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let bird = conn.query_row(
        "SELECT id, band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id FROM birds WHERE nfc_tag_id = ?1",
        params![tag_id],
        |row| {
            let sex_str: String = row.get(2)?;
            let hatch_str: String = row.get(4)?;
            let status_str: String = row.get(8)?;
            Ok(Bird {
                id: row.get(0)?,
                band_color: row.get(1)?,
                sex: str_to_sex(&sex_str),
                bloodline_id: row.get(3)?,
                hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                mother_id: row.get(5)?,
                father_id: row.get(6)?,
                generation: row.get(7)?,
                status: str_to_bird_status(&status_str),
                notes: row.get(9)?,
                nfc_tag_id: row.get(10)?,
            })
        },
    );
    match bird {
        Ok(b) => (StatusCode::OK, Json(Some(b))).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<Bird>)).into_response(),
    }
}

// --- Breeding Pairs ---

async fn create_breeding_pair(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingPair>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO breeding_pairs (male_id, female_id, start_date, end_date, notes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            body.male_id,
            body.female_id,
            body.start_date.to_string(),
            body.end_date.map(|d| d.to_string()),
            body.notes,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(BreedingPair {
            id,
            male_id: body.male_id,
            female_id: body.female_id,
            start_date: body.start_date,
            end_date: body.end_date,
            notes: body.notes,
        }),
    )
}

async fn list_breeding_pairs(State(state): State<AppState>) -> Json<Vec<BreedingPair>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, male_id, female_id, start_date, end_date, notes FROM breeding_pairs ORDER BY id")
        .unwrap();
    let rows: Vec<BreedingPair> = stmt
        .query_map([], |row| {
            let start_str: String = row.get(3)?;
            let end_str: Option<String> = row.get(4)?;
            Ok(BreedingPair {
                id: row.get(0)?,
                male_id: row.get(1)?,
                female_id: row.get(2)?,
                start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d").unwrap_or_default(),
                end_date: end_str.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
                notes: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// --- Clutches ---

async fn create_clutch(
    State(state): State<AppState>,
    Json(body): Json<CreateClutch>,
) -> impl IntoResponse {
    let expected = body.set_date + chrono::Duration::days(17);
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO clutches (breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            body.breeding_pair_id,
            body.bloodline_id,
            body.eggs_set,
            body.eggs_fertile,
            body.eggs_hatched,
            body.set_date.to_string(),
            expected.to_string(),
            clutch_status_to_str(&body.status),
            body.notes,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(Clutch {
            id,
            breeding_pair_id: body.breeding_pair_id,
            bloodline_id: body.bloodline_id,
            eggs_set: body.eggs_set,
            eggs_fertile: body.eggs_fertile,
            eggs_hatched: body.eggs_hatched,
            set_date: body.set_date,
            expected_hatch_date: expected,
            status: body.status,
            notes: body.notes,
            eggs_stillborn: None,
            eggs_quit: None,
            eggs_infertile: None,
            eggs_damaged: None,
            hatch_notes: None,
        }),
    )
}

async fn list_clutches(State(state): State<AppState>) -> Json<Vec<Clutch>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes, eggs_stillborn, eggs_quit, eggs_infertile, eggs_damaged, hatch_notes FROM clutches ORDER BY id")
        .unwrap();
    let rows: Vec<Clutch> = stmt
        .query_map([], |row| {
            let set_str: String = row.get(6)?;
            let exp_str: String = row.get(7)?;
            let status_str: String = row.get(8)?;
            Ok(Clutch {
                id: row.get(0)?,
                breeding_pair_id: row.get(1)?,
                bloodline_id: row.get(2)?,
                eggs_set: row.get(3)?,
                eggs_fertile: row.get(4)?,
                eggs_hatched: row.get(5)?,
                set_date: NaiveDate::parse_from_str(&set_str, "%Y-%m-%d").unwrap_or_default(),
                expected_hatch_date: NaiveDate::parse_from_str(&exp_str, "%Y-%m-%d").unwrap_or_default(),
                status: str_to_clutch_status(&status_str),
                notes: row.get(9)?,
                eggs_stillborn: row.get(10)?,
                eggs_quit: row.get(11)?,
                eggs_infertile: row.get(12)?,
                eggs_damaged: row.get(13)?,
                hatch_notes: row.get(14)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn update_clutch(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateClutch>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM clutches WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<Clutch>)).into_response();
    }

    if let Some(fertile) = body.eggs_fertile {
        conn.execute(
            "UPDATE clutches SET eggs_fertile = ?1 WHERE id = ?2",
            params![fertile, id],
        )
        .unwrap();
    }
    if let Some(hatched) = body.eggs_hatched {
        conn.execute(
            "UPDATE clutches SET eggs_hatched = ?1 WHERE id = ?2",
            params![hatched, id],
        )
        .unwrap();
    }
    if let Some(ref status) = body.status {
        conn.execute(
            "UPDATE clutches SET status = ?1 WHERE id = ?2",
            params![clutch_status_to_str(status), id],
        )
        .unwrap();
    }
    if let Some(ref notes) = body.notes {
        conn.execute(
            "UPDATE clutches SET notes = ?1 WHERE id = ?2",
            params![notes, id],
        )
        .unwrap();
    }
    if let Some(stillborn) = body.eggs_stillborn {
        conn.execute(
            "UPDATE clutches SET eggs_stillborn = ?1 WHERE id = ?2",
            params![stillborn, id],
        )
        .unwrap();
    }
    if let Some(quit) = body.eggs_quit {
        conn.execute(
            "UPDATE clutches SET eggs_quit = ?1 WHERE id = ?2",
            params![quit, id],
        )
        .unwrap();
    }
    if let Some(infertile) = body.eggs_infertile {
        conn.execute(
            "UPDATE clutches SET eggs_infertile = ?1 WHERE id = ?2",
            params![infertile, id],
        )
        .unwrap();
    }
    if let Some(damaged) = body.eggs_damaged {
        conn.execute(
            "UPDATE clutches SET eggs_damaged = ?1 WHERE id = ?2",
            params![damaged, id],
        )
        .unwrap();
    }
    if let Some(ref hatch_notes) = body.hatch_notes {
        conn.execute(
            "UPDATE clutches SET hatch_notes = ?1 WHERE id = ?2",
            params![hatch_notes, id],
        )
        .unwrap();
    }

    let clutch = conn
        .query_row(
            "SELECT id, breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes, eggs_stillborn, eggs_quit, eggs_infertile, eggs_damaged, hatch_notes FROM clutches WHERE id = ?1",
            params![id],
            |row| {
                let set_str: String = row.get(6)?;
                let exp_str: String = row.get(7)?;
                let status_str: String = row.get(8)?;
                Ok(Clutch {
                    id: row.get(0)?,
                    breeding_pair_id: row.get(1)?,
                    bloodline_id: row.get(2)?,
                    eggs_set: row.get(3)?,
                    eggs_fertile: row.get(4)?,
                    eggs_hatched: row.get(5)?,
                    set_date: NaiveDate::parse_from_str(&set_str, "%Y-%m-%d").unwrap_or_default(),
                    expected_hatch_date: NaiveDate::parse_from_str(&exp_str, "%Y-%m-%d").unwrap_or_default(),
                    status: str_to_clutch_status(&status_str),
                    notes: row.get(9)?,
                    eggs_stillborn: row.get(10)?,
                    eggs_quit: row.get(11)?,
                    eggs_infertile: row.get(12)?,
                    eggs_damaged: row.get(13)?,
                    hatch_notes: row.get(14)?,
                })
            },
        )
        .unwrap();

    (StatusCode::OK, Json(Some(clutch))).into_response()
}

// --- Flock Summary ---

#[derive(Serialize)]
struct FlockSummary {
    total_birds: i64,
    active_birds: i64,
    males: i64,
    females: i64,
    bloodlines: Vec<BloodlineCount>,
}

#[derive(Serialize)]
struct BloodlineCount {
    name: String,
    count: i64,
}

async fn flock_summary(State(state): State<AppState>) -> Json<FlockSummary> {
    let conn = acquire_db(&state);

    let total_birds: i64 = conn
        .query_row("SELECT COUNT(*) FROM birds", [], |row| row.get(0))
        .unwrap_or(0);

    let active_birds: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let males: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Male' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let females: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Female' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut stmt = conn
        .prepare(
            "SELECT b.name, COUNT(*)
             FROM birds bi JOIN bloodlines b ON bi.bloodline_id = b.id
             WHERE bi.status = 'Active'
             GROUP BY b.name ORDER BY COUNT(*) DESC",
        )
        .unwrap();

    let bloodlines: Vec<BloodlineCount> = stmt
        .query_map([], |row| {
            Ok(BloodlineCount {
                name: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(FlockSummary {
        total_birds,
        active_birds,
        males,
        females,
        bloodlines,
    })
}

// --- Breeding Suggestions ---

struct BirdRecord {
    id: i64,
    sex: Sex,
    bloodline_id: i64,
    mother_id: Option<i64>,
    father_id: Option<i64>,
}

fn compute_relatedness(m: &BirdRecord, f: &BirdRecord) -> f64 {
    let share_mother = match (m.mother_id, f.mother_id) {
        (Some(a), Some(b)) if a == b => true,
        _ => false,
    };
    let share_father = match (m.father_id, f.father_id) {
        (Some(a), Some(b)) if a == b => true,
        _ => false,
    };

    if share_mother && share_father {
        return 0.5;
    }
    if share_mother || share_father {
        return 0.25;
    }
    if m.bloodline_id == f.bloodline_id {
        return 0.25;
    }
    0.0
}

async fn breeding_suggest(State(state): State<AppState>) -> Json<Vec<InbreedingCoefficient>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT id, sex, bloodline_id, mother_id, father_id
             FROM birds WHERE status = 'Active'",
        )
        .unwrap();

    let birds: Vec<BirdRecord> = stmt
        .query_map([], |row| {
            let sex_str: String = row.get(1)?;
            Ok(BirdRecord {
                id: row.get(0)?,
                sex: str_to_sex(&sex_str),
                bloodline_id: row.get(2)?,
                mother_id: row.get(3)?,
                father_id: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let males: Vec<&BirdRecord> = birds.iter().filter(|b| b.sex == Sex::Male).collect();
    let females: Vec<&BirdRecord> = birds.iter().filter(|b| b.sex == Sex::Female).collect();

    let mut results: Vec<InbreedingCoefficient> = Vec::new();
    for m in &males {
        for f in &females {
            let coefficient = compute_relatedness(m, f);
            results.push(InbreedingCoefficient {
                male_id: m.id,
                female_id: f.id,
                coefficient,
                safe: coefficient < 0.0625,
            });
        }
    }

    results.sort_by(|a, b| {
        a.coefficient
            .partial_cmp(&b.coefficient)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Json(results)
}

// ---------------------------------------------------------------------------
// Weight tracking
// ---------------------------------------------------------------------------

async fn create_weight(
    State(state): State<AppState>,
    Path(bird_id): Path<i64>,
    Json(body): Json<CreateWeightRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO weight_records (bird_id, weight_grams, date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![bird_id, body.weight_grams, body.date.to_string(), body.notes],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(WeightRecord {
            id,
            bird_id,
            weight_grams: body.weight_grams,
            date: body.date,
            notes: body.notes,
        }),
    )
}

async fn list_weights(
    State(state): State<AppState>,
    Path(bird_id): Path<i64>,
) -> Json<Vec<WeightRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT id, bird_id, weight_grams, date, notes FROM weight_records
             WHERE bird_id = ?1 ORDER BY date DESC",
        )
        .unwrap();
    let rows: Vec<WeightRecord> = stmt
        .query_map(params![bird_id], |row| {
            let date_str: String = row.get(3)?;
            Ok(WeightRecord {
                id: row.get(0)?,
                bird_id: row.get(1)?,
                weight_grams: row.get(2)?,
                date: NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").unwrap_or_default(),
                notes: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// ---------------------------------------------------------------------------
// Processing queue
// ---------------------------------------------------------------------------

fn processing_reason_to_str(r: &ProcessingReason) -> &'static str {
    match r {
        ProcessingReason::ExcessMale => "ExcessMale",
        ProcessingReason::LowWeight => "LowWeight",
        ProcessingReason::PoorGenetics => "PoorGenetics",
        ProcessingReason::Age => "Age",
        ProcessingReason::Other => "Other",
    }
}

fn str_to_processing_reason(s: &str) -> ProcessingReason {
    match s {
        "ExcessMale" => ProcessingReason::ExcessMale,
        "LowWeight" => ProcessingReason::LowWeight,
        "PoorGenetics" => ProcessingReason::PoorGenetics,
        "Age" => ProcessingReason::Age,
        _ => ProcessingReason::Other,
    }
}

fn processing_status_to_str(s: &ProcessingStatus) -> &'static str {
    match s {
        ProcessingStatus::Scheduled => "Scheduled",
        ProcessingStatus::Completed => "Completed",
        ProcessingStatus::Cancelled => "Cancelled",
    }
}

fn str_to_processing_status(s: &str) -> ProcessingStatus {
    match s {
        "Completed" => ProcessingStatus::Completed,
        "Cancelled" => ProcessingStatus::Cancelled,
        _ => ProcessingStatus::Scheduled,
    }
}

fn row_to_processing_record(row: &rusqlite::Row) -> rusqlite::Result<ProcessingRecord> {
    let reason_str: String = row.get(2)?;
    let sched_str: String = row.get(3)?;
    let proc_str: Option<String> = row.get(4)?;
    let status_str: String = row.get(6)?;
    Ok(ProcessingRecord {
        id: row.get(0)?,
        bird_id: row.get(1)?,
        reason: str_to_processing_reason(&reason_str),
        scheduled_date: NaiveDate::parse_from_str(&sched_str, "%Y-%m-%d").unwrap_or_default(),
        processed_date: proc_str
            .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        final_weight_grams: row.get(5)?,
        status: str_to_processing_status(&status_str),
        notes: row.get(7)?,
    })
}

async fn create_processing(
    State(state): State<AppState>,
    Json(body): Json<CreateProcessingRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO processing_records (bird_id, reason, scheduled_date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![
            body.bird_id,
            processing_reason_to_str(&body.reason),
            body.scheduled_date.to_string(),
            body.notes,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(ProcessingRecord {
            id,
            bird_id: body.bird_id,
            reason: body.reason,
            scheduled_date: body.scheduled_date,
            processed_date: None,
            final_weight_grams: None,
            status: ProcessingStatus::Scheduled,
            notes: body.notes,
        }),
    )
}

async fn update_processing(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateProcessingRecord>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM processing_records WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return (StatusCode::NOT_FOUND, Json(None::<ProcessingRecord>)).into_response();
    }

    if let Some(ref d) = body.processed_date {
        conn.execute(
            "UPDATE processing_records SET processed_date = ?1 WHERE id = ?2",
            params![d.to_string(), id],
        )
        .unwrap();
    }
    if let Some(w) = body.final_weight_grams {
        conn.execute(
            "UPDATE processing_records SET final_weight_grams = ?1 WHERE id = ?2",
            params![w, id],
        )
        .unwrap();
    }
    if let Some(ref s) = body.status {
        conn.execute(
            "UPDATE processing_records SET status = ?1 WHERE id = ?2",
            params![processing_status_to_str(s), id],
        )
        .unwrap();
    }
    if let Some(ref n) = body.notes {
        conn.execute(
            "UPDATE processing_records SET notes = ?1 WHERE id = ?2",
            params![n, id],
        )
        .unwrap();
    }

    let rec = conn
        .query_row(
            "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes
             FROM processing_records WHERE id = ?1",
            params![id],
            row_to_processing_record,
        )
        .unwrap();

    (StatusCode::OK, Json(Some(rec))).into_response()
}

async fn list_processing(State(state): State<AppState>) -> Json<Vec<ProcessingRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes
             FROM processing_records ORDER BY id",
        )
        .unwrap();
    let rows: Vec<ProcessingRecord> = stmt
        .query_map([], row_to_processing_record)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn list_processing_queue(State(state): State<AppState>) -> Json<Vec<ProcessingRecord>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT id, bird_id, reason, scheduled_date, processed_date, final_weight_grams, status, notes
             FROM processing_records WHERE status = 'Scheduled' ORDER BY scheduled_date",
        )
        .unwrap();
    let rows: Vec<ProcessingRecord> = stmt
        .query_map([], row_to_processing_record)
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// ---------------------------------------------------------------------------
// Breeding groups
// ---------------------------------------------------------------------------

async fn create_breeding_group(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingGroup>,
) -> impl IntoResponse {
    let count = body.female_ids.len();
    let warning = if count < MIN_FEMALES_PER_MALE || count > MAX_FEMALES_PER_MALE {
        Some(format!(
            "Warning: {count} females per male is outside the recommended {MIN_FEMALES_PER_MALE}-{MAX_FEMALES_PER_MALE} range"
        ))
    } else {
        None
    };

    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO breeding_groups (name, male_id, start_date, notes) VALUES (?1, ?2, ?3, ?4)",
        params![body.name, body.male_id, body.start_date.to_string(), body.notes],
    )
    .unwrap();
    let id = conn.last_insert_rowid();

    for fid in &body.female_ids {
        conn.execute(
            "INSERT INTO breeding_group_members (group_id, female_id) VALUES (?1, ?2)",
            params![id, fid],
        )
        .unwrap();
    }

    #[derive(Serialize)]
    struct BreedingGroupResponse {
        #[serde(flatten)]
        group: BreedingGroup,
        warning: Option<String>,
    }

    (
        StatusCode::CREATED,
        Json(BreedingGroupResponse {
            group: BreedingGroup {
                id,
                name: body.name,
                male_id: body.male_id,
                female_ids: body.female_ids,
                start_date: body.start_date,
                notes: body.notes,
            },
            warning,
        }),
    )
}

async fn list_breeding_groups(State(state): State<AppState>) -> Json<Vec<BreedingGroup>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, male_id, start_date, notes FROM breeding_groups ORDER BY id")
        .unwrap();
    let groups: Vec<(i64, String, i64, String, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, String>(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::new();
    for (id, name, male_id, start_str, notes) in groups {
        let mut fstmt = conn
            .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
            .unwrap();
        let female_ids: Vec<i64> = fstmt
            .query_map(params![id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        result.push(BreedingGroup {
            id,
            name,
            male_id,
            female_ids,
            start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d").unwrap_or_default(),
            notes,
        });
    }
    Json(result)
}

async fn get_breeding_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let group = conn.query_row(
        "SELECT id, name, male_id, start_date, notes FROM breeding_groups WHERE id = ?1",
        params![id],
        |row| {
            let start_str: String = row.get(3)?;
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                start_str,
                row.get::<_, Option<String>>(4)?,
            ))
        },
    );

    match group {
        Ok((gid, name, male_id, start_str, notes)) => {
            let mut fstmt = conn
                .prepare("SELECT female_id FROM breeding_group_members WHERE group_id = ?1")
                .unwrap();
            let female_ids: Vec<i64> = fstmt
                .query_map(params![gid], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect();
            (
                StatusCode::OK,
                Json(Some(BreedingGroup {
                    id: gid,
                    name,
                    male_id,
                    female_ids,
                    start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d")
                        .unwrap_or_default(),
                    notes,
                })),
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<BreedingGroup>)).into_response(),
    }
}

// ---------------------------------------------------------------------------
// Cull recommendations
// ---------------------------------------------------------------------------

async fn cull_recommendations(State(state): State<AppState>) -> Json<Vec<CullRecommendation>> {
    let conn = acquire_db(&state);
    let mut recs: Vec<CullRecommendation> = Vec::new();

    // 1. Excess males: ideal males = ceil(active_females / MAX_FEMALES_PER_MALE)
    let active_females: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM birds WHERE sex = 'Female' AND status = 'Active'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let ideal_males = if active_females > 0 {
        ((active_females as f64) / (MAX_FEMALES_PER_MALE as f64)).ceil() as i64
    } else {
        0
    };

    let mut male_stmt = conn
        .prepare(
            "SELECT id FROM birds WHERE sex = 'Male' AND status = 'Active' ORDER BY id DESC",
        )
        .unwrap();
    let active_male_ids: Vec<i64> = male_stmt
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let surplus = (active_male_ids.len() as i64) - ideal_males;
    if surplus > 0 {
        for &mid in active_male_ids.iter().take(surplus as usize) {
            recs.push(CullRecommendation {
                bird_id: mid,
                reason: CullReason::ExcessMale,
            });
        }
    }

    // 2. Low-weight females: latest weight < MIN_BREEDING_WEIGHT
    let mut fw_stmt = conn
        .prepare(
            "SELECT b.id, w.weight_grams FROM birds b
             JOIN weight_records w ON w.bird_id = b.id
             WHERE b.sex = 'Female' AND b.status = 'Active'
               AND w.id = (SELECT w2.id FROM weight_records w2 WHERE w2.bird_id = b.id ORDER BY w2.date DESC LIMIT 1)",
        )
        .unwrap();
    let low_weight: Vec<(i64, f64)> = fw_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .filter(|(_, w)| *w < COTURNIX_MIN_BREEDING_WEIGHT_GRAMS)
        .collect();

    for (bid, w) in low_weight {
        recs.push(CullRecommendation {
            bird_id: bid,
            reason: CullReason::LowWeight { weight_grams: w },
        });
    }

    // 3. High inbreeding risk: birds with no safe pairing options
    let mut bird_stmt = conn
        .prepare(
            "SELECT id, sex, bloodline_id, mother_id, father_id
             FROM birds WHERE status = 'Active'",
        )
        .unwrap();
    let all_birds: Vec<BirdRecord> = bird_stmt
        .query_map([], |row| {
            let sex_str: String = row.get(1)?;
            Ok(BirdRecord {
                id: row.get(0)?,
                sex: str_to_sex(&sex_str),
                bloodline_id: row.get(2)?,
                mother_id: row.get(3)?,
                father_id: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let males: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Male).collect();
    let females: Vec<&BirdRecord> = all_birds.iter().filter(|b| b.sex == Sex::Female).collect();

    // Check males with no safe female pairing
    for m in &males {
        let has_safe = females
            .iter()
            .any(|f| compute_relatedness(m, f) < 0.0625);
        if !has_safe && !females.is_empty() {
            let worst = females
                .iter()
                .map(|f| compute_relatedness(m, f))
                .fold(0.0_f64, f64::max);
            if !recs.iter().any(|r| r.bird_id == m.id) {
                recs.push(CullRecommendation {
                    bird_id: m.id,
                    reason: CullReason::HighInbreeding {
                        coefficient: worst,
                    },
                });
            }
        }
    }

    // Check females with no safe male pairing
    for f in &females {
        let has_safe = males
            .iter()
            .any(|m| compute_relatedness(m, f) < 0.0625);
        if !has_safe && !males.is_empty() {
            let worst = males
                .iter()
                .map(|m| compute_relatedness(m, f))
                .fold(0.0_f64, f64::max);
            if !recs.iter().any(|r| r.bird_id == f.id) {
                recs.push(CullRecommendation {
                    bird_id: f.id,
                    reason: CullReason::HighInbreeding {
                        coefficient: worst,
                    },
                });
            }
        }
    }

    Json(recs)
}

// ---------------------------------------------------------------------------
// Camera feed infrastructure
// ---------------------------------------------------------------------------

fn camera_status_to_str(s: &CameraStatus) -> &'static str {
    match s {
        CameraStatus::Active => "Active",
        CameraStatus::Offline => "Offline",
    }
}

fn str_to_camera_status(s: &str) -> CameraStatus {
    match s {
        "Offline" => CameraStatus::Offline,
        _ => CameraStatus::Active,
    }
}

fn life_stage_to_str(s: &LifeStage) -> &'static str {
    match s {
        LifeStage::Chick => "Chick",
        LifeStage::Adolescent => "Adolescent",
        LifeStage::Adult => "Adult",
    }
}

fn str_to_life_stage(s: &str) -> LifeStage {
    match s {
        "Chick" => LifeStage::Chick,
        "Adolescent" => LifeStage::Adolescent,
        _ => LifeStage::Adult,
    }
}

async fn create_camera(
    State(state): State<AppState>,
    Json(body): Json<CreateCameraFeed>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO camera_feeds (name, location, feed_url, status, brooder_id) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            body.name,
            body.location,
            body.feed_url,
            camera_status_to_str(&body.status),
            body.brooder_id,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(CameraFeed {
            id,
            name: body.name,
            location: body.location,
            feed_url: body.feed_url,
            status: body.status,
            brooder_id: body.brooder_id,
        }),
    )
}

async fn list_cameras(State(state): State<AppState>) -> Json<Vec<CameraFeed>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, location, feed_url, status, brooder_id FROM camera_feeds ORDER BY id")
        .unwrap();
    let rows: Vec<CameraFeed> = stmt
        .query_map([], |row| {
            let status_str: String = row.get(4)?;
            Ok(CameraFeed {
                id: row.get(0)?,
                name: row.get(1)?,
                location: row.get(2)?,
                feed_url: row.get(3)?,
                status: str_to_camera_status(&status_str),
                brooder_id: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn create_frame(
    State(state): State<AppState>,
    Json(body): Json<CreateFrameCapture>,
) -> impl IntoResponse {
    let now = Utc::now();
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO frame_captures (camera_id, timestamp, image_path, life_stage) VALUES (?1, ?2, ?3, ?4)",
        params![
            body.camera_id,
            now.to_rfc3339(),
            body.image_path,
            life_stage_to_str(&body.life_stage),
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(FrameCapture {
            id,
            camera_id: body.camera_id,
            timestamp: now,
            image_path: body.image_path,
            life_stage: body.life_stage,
        }),
    )
}

async fn create_frame_detections(
    State(state): State<AppState>,
    Path(frame_id): Path<i64>,
    Json(body): Json<Vec<CreateDetectionResult>>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let mut results = Vec::new();
    for d in body {
        conn.execute(
            "INSERT INTO detection_results (frame_id, label, confidence, bounding_box_x, bounding_box_y, bounding_box_w, bounding_box_h, notes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                frame_id,
                d.label,
                d.confidence,
                d.bounding_box_x,
                d.bounding_box_y,
                d.bounding_box_w,
                d.bounding_box_h,
                d.notes,
            ],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        results.push(DetectionResult {
            id,
            frame_id,
            label: d.label,
            confidence: d.confidence,
            bounding_box_x: d.bounding_box_x,
            bounding_box_y: d.bounding_box_y,
            bounding_box_w: d.bounding_box_w,
            bounding_box_h: d.bounding_box_h,
            notes: d.notes,
        });
    }
    (StatusCode::CREATED, Json(results))
}

#[derive(Deserialize)]
struct FrameQueryParams {
    camera_id: Option<i64>,
    minutes: Option<u64>,
}

async fn list_frames(
    State(state): State<AppState>,
    Query(params): Query<FrameQueryParams>,
) -> Json<Vec<FrameCapture>> {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);

    let (sql, binds): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match params.camera_id {
        Some(cid) => (
            "SELECT id, camera_id, timestamp, image_path, life_stage FROM frame_captures
             WHERE camera_id = ?1 AND timestamp >= datetime('now', ?2)
             ORDER BY id DESC"
                .to_string(),
            vec![
                Box::new(cid) as Box<dyn rusqlite::types::ToSql>,
                Box::new(format!("-{minutes} minutes")),
            ],
        ),
        None => (
            "SELECT id, camera_id, timestamp, image_path, life_stage FROM frame_captures
             WHERE timestamp >= datetime('now', ?1)
             ORDER BY id DESC"
                .to_string(),
            vec![Box::new(format!("-{minutes} minutes")) as Box<dyn rusqlite::types::ToSql>],
        ),
    };

    let mut stmt = conn.prepare(&sql).unwrap();
    let rows: Vec<FrameCapture> = stmt
        .query_map(rusqlite::params_from_iter(binds.iter()), |row| {
            let ts_str: String = row.get(2)?;
            let stage_str: String = row.get(4)?;
            Ok(FrameCapture {
                id: row.get(0)?,
                camera_id: row.get(1)?,
                timestamp: ts_str
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_default(),
                image_path: row.get(3)?,
                life_stage: str_to_life_stage(&stage_str),
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

#[derive(Serialize)]
struct DetectionSummaryEntry {
    label: String,
    count: i64,
    avg_confidence: f64,
}

async fn camera_detection_summary(
    State(state): State<AppState>,
    Path(camera_id): Path<i64>,
) -> Json<Vec<DetectionSummaryEntry>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT dr.label, COUNT(*), AVG(dr.confidence)
             FROM detection_results dr
             JOIN frame_captures fc ON fc.id = dr.frame_id
             WHERE fc.camera_id = ?1
               AND fc.timestamp >= datetime('now', '-60 minutes')
             GROUP BY dr.label
             ORDER BY COUNT(*) DESC",
        )
        .unwrap();
    let rows: Vec<DetectionSummaryEntry> = stmt
        .query_map(params![camera_id], |row| {
            Ok(DetectionSummaryEntry {
                label: row.get(0)?,
                count: row.get(1)?,
                avg_confidence: row.get(2)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// ---------------------------------------------------------------------------
// Brooder management endpoints
// ---------------------------------------------------------------------------

async fn create_brooder(
    State(state): State<AppState>,
    Json(body): Json<CreateBrooder>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    if let Some(bl_id) = body.bloodline_id {
        let exists = conn
            .query_row(
                "SELECT COUNT(*) FROM bloodlines WHERE id = ?1",
                params![bl_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0);
        if exists == 0 {
            return (
                StatusCode::BAD_REQUEST,
                format!("Bloodline #{bl_id} does not exist"),
            )
                .into_response();
        }
    }

    match conn.execute(
        "INSERT INTO brooders (name, bloodline_id, life_stage, qr_code, notes, camera_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            body.name,
            body.bloodline_id,
            life_stage_to_str(&body.life_stage),
            body.qr_code,
            body.notes,
            body.camera_url,
        ],
    ) {
        Ok(_) => {
            let id = conn.last_insert_rowid();
            (
                StatusCode::CREATED,
                Json(Brooder {
                    id,
                    name: body.name,
                    bloodline_id: body.bloodline_id,
                    life_stage: body.life_stage,
                    qr_code: body.qr_code,
                    notes: body.notes,
                    camera_url: body.camera_url,
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create brooder: {e}"),
        )
            .into_response(),
    }
}

async fn update_brooder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let exists = conn
        .query_row(
            "SELECT COUNT(*) FROM brooders WHERE id = ?1",
            params![id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0);
    if exists == 0 {
        return (StatusCode::NOT_FOUND, "brooder not found").into_response();
    }

    if let Some(url) = body.get("camera_url") {
        let val = if url.is_null() {
            None
        } else {
            url.as_str().map(|s| s.to_string())
        };
        conn.execute(
            "UPDATE brooders SET camera_url = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }
    if let Some(name) = body.get("name").and_then(|v| v.as_str()) {
        conn.execute(
            "UPDATE brooders SET name = ?1 WHERE id = ?2",
            params![name, id],
        )
        .ok();
    }
    if let Some(notes) = body.get("notes") {
        let val = if notes.is_null() {
            None
        } else {
            notes.as_str().map(|s| s.to_string())
        };
        conn.execute(
            "UPDATE brooders SET notes = ?1 WHERE id = ?2",
            params![val, id],
        )
        .ok();
    }

    StatusCode::OK.into_response()
}

async fn list_brooders(State(state): State<AppState>) -> Json<Vec<Brooder>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, name, bloodline_id, life_stage, qr_code, notes, camera_url FROM brooders ORDER BY id")
        .unwrap();
    let rows: Vec<Brooder> = stmt
        .query_map([], |row| {
            let stage_str: String = row.get(3)?;
            Ok(Brooder {
                id: row.get(0)?,
                name: row.get(1)?,
                bloodline_id: row.get(2)?,
                life_stage: str_to_life_stage(&stage_str),
                qr_code: row.get(4)?,
                notes: row.get(5)?,
                camera_url: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn brooder_readings(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<HistoryParams>,
) -> Json<Vec<BrooderReading>> {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare(
            "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
             WHERE brooder_id = ?1 AND received_at >= datetime('now', ?2)
             ORDER BY id DESC",
        )
        .unwrap();

    let cutoff = format!("-{minutes} minutes");
    let readings: Vec<BrooderReading> = stmt
        .query_map(params![id, cutoff], |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_celsius: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
                brooder_id: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(readings)
}

#[derive(Serialize)]
struct BrooderStatus {
    brooder: Brooder,
    latest_temp: Option<f64>,
    latest_humidity: Option<f64>,
    has_alert: bool,
    alert_message: Option<String>,
}

async fn brooder_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let brooder = conn.query_row(
        "SELECT id, name, bloodline_id, life_stage, qr_code, notes, camera_url FROM brooders WHERE id = ?1",
        params![id],
        |row| {
            let stage_str: String = row.get(3)?;
            Ok(Brooder {
                id: row.get(0)?,
                name: row.get(1)?,
                bloodline_id: row.get(2)?,
                life_stage: str_to_life_stage(&stage_str),
                qr_code: row.get(4)?,
                notes: row.get(5)?,
                camera_url: row.get(6)?,
            })
        },
    );

    let brooder = match brooder {
        Ok(b) => b,
        Err(_) => return (StatusCode::NOT_FOUND, "brooder not found").into_response(),
    };

    let latest = conn.query_row(
        "SELECT temperature, humidity FROM brooder_readings WHERE brooder_id = ?1 ORDER BY id DESC LIMIT 1",
        params![id],
        |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
    );

    let (latest_temp, latest_humidity, has_alert, alert_message) = match latest {
        Ok((temp, hum)) => {
            let config = state.alert_config.clone();
            let mut alert = false;
            let mut msg = None;
            if temp < config.brooder_temp_min || temp > config.brooder_temp_max {
                alert = true;
                msg = Some(format!("Temperature {:.1}\u{00b0}F out of range ({:.1}-{:.1})", temp, config.brooder_temp_min, config.brooder_temp_max));
            } else if hum < config.humidity_min || hum > config.humidity_max {
                alert = true;
                msg = Some(format!("Humidity {:.1}% out of range ({:.1}-{:.1})", hum, config.humidity_min, config.humidity_max));
            }
            (Some(temp), Some(hum), alert, msg)
        }
        Err(_) => (None, None, false, None),
    };

    Json(BrooderStatus {
        brooder,
        latest_temp,
        latest_humidity,
        has_alert,
        alert_message,
    })
    .into_response()
}

async fn update_camera_brooder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let brooder_id = body.get("brooder_id").and_then(|v| v.as_i64());
    let conn = acquire_db(&state);
    conn.execute(
        "UPDATE camera_feeds SET brooder_id = ?1 WHERE id = ?2",
        params![brooder_id, id],
    )
    .unwrap();
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// Chick groups (nursery)
// ---------------------------------------------------------------------------

fn str_to_chick_group_status(s: &str) -> ChickGroupStatus {
    match s {
        "Graduated" => ChickGroupStatus::Graduated,
        "Lost" => ChickGroupStatus::Lost,
        _ => ChickGroupStatus::Active,
    }
}

async fn create_chick_group(
    State(state): State<AppState>,
    Json(body): Json<CreateChickGroup>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    conn.execute(
        "INSERT INTO chick_groups (clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'Active', ?7)",
        params![
            body.clutch_id,
            body.bloodline_id,
            body.brooder_id,
            body.initial_count,
            body.initial_count,
            body.hatch_date.to_string(),
            body.notes,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (
        StatusCode::CREATED,
        Json(ChickGroup {
            id,
            clutch_id: body.clutch_id,
            bloodline_id: body.bloodline_id,
            brooder_id: body.brooder_id,
            initial_count: body.initial_count,
            current_count: body.initial_count,
            hatch_date: body.hatch_date,
            status: ChickGroupStatus::Active,
            notes: body.notes,
        }),
    )
}

async fn list_chick_groups(State(state): State<AppState>) -> Json<Vec<ChickGroup>> {
    let conn = acquire_db(&state);
    let mut stmt = conn
        .prepare("SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes FROM chick_groups ORDER BY status='Active' DESC, id DESC")
        .unwrap();
    let rows: Vec<ChickGroup> = stmt
        .query_map([], |row| {
            let hatch_str: String = row.get(6)?;
            let status_str: String = row.get(7)?;
            Ok(ChickGroup {
                id: row.get(0)?,
                clutch_id: row.get(1)?,
                bloodline_id: row.get(2)?,
                brooder_id: row.get(3)?,
                initial_count: row.get(4)?,
                current_count: row.get(5)?,
                hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                status: str_to_chick_group_status(&status_str),
                notes: row.get(8)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

async fn get_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let group = conn.query_row(
        "SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes FROM chick_groups WHERE id = ?1",
        params![id],
        |row| {
            let hatch_str: String = row.get(6)?;
            let status_str: String = row.get(7)?;
            Ok(ChickGroup {
                id: row.get(0)?,
                clutch_id: row.get(1)?,
                bloodline_id: row.get(2)?,
                brooder_id: row.get(3)?,
                initial_count: row.get(4)?,
                current_count: row.get(5)?,
                hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                status: str_to_chick_group_status(&status_str),
                notes: row.get(8)?,
            })
        },
    );
    match group {
        Ok(g) => (StatusCode::OK, Json(Some(g))).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, Json(None::<ChickGroup>)).into_response(),
    }
}

async fn log_mortality(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<MortalityRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let current: u32 = match conn.query_row(
        "SELECT current_count FROM chick_groups WHERE id = ?1 AND status = 'Active'",
        params![id],
        |row| row.get(0),
    ) {
        Ok(c) => c,
        Err(_) => return (StatusCode::NOT_FOUND, "chick group not found or not active").into_response(),
    };

    if body.count > current {
        return (StatusCode::BAD_REQUEST, "mortality count exceeds current count").into_response();
    }

    let new_count = current - body.count;
    let today = chrono::Local::now().date_naive();

    conn.execute(
        "INSERT INTO chick_mortality_log (group_id, count, reason, date) VALUES (?1, ?2, ?3, ?4)",
        params![id, body.count, body.reason, today.to_string()],
    )
    .unwrap();

    conn.execute(
        "UPDATE chick_groups SET current_count = ?1 WHERE id = ?2",
        params![new_count, id],
    )
    .unwrap();

    if new_count == 0 {
        conn.execute(
            "UPDATE chick_groups SET status = 'Lost' WHERE id = ?1",
            params![id],
        )
        .unwrap();
    }

    let log_id = conn.last_insert_rowid();
    Json(ChickMortalityLog {
        id: log_id,
        group_id: id,
        count: body.count,
        reason: body.reason,
        date: today,
    })
    .into_response()
}

async fn graduate_chick_group(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<GraduateRequest>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);

    let group = conn.query_row(
        "SELECT id, clutch_id, bloodline_id, brooder_id, initial_count, current_count, hatch_date, status, notes FROM chick_groups WHERE id = ?1",
        params![id],
        |row| {
            let hatch_str: String = row.get(6)?;
            let status_str: String = row.get(7)?;
            Ok(ChickGroup {
                id: row.get(0)?,
                clutch_id: row.get(1)?,
                bloodline_id: row.get(2)?,
                brooder_id: row.get(3)?,
                initial_count: row.get(4)?,
                current_count: row.get(5)?,
                hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
                status: str_to_chick_group_status(&status_str),
                notes: row.get(8)?,
            })
        },
    );

    let group = match group {
        Ok(g) => g,
        Err(_) => return (StatusCode::NOT_FOUND, "chick group not found").into_response(),
    };

    if group.status != ChickGroupStatus::Active {
        return (StatusCode::BAD_REQUEST, "group is not active").into_response();
    }

    let mut birds_created = Vec::new();
    for gb in &body.birds {
        conn.execute(
            "INSERT INTO birds (band_color, sex, bloodline_id, hatch_date, generation, status, notes, nfc_tag_id)
             VALUES (?1, ?2, ?3, ?4, 1, 'Active', ?5, ?6)",
            params![
                gb.band_color,
                sex_to_str(&gb.sex),
                group.bloodline_id,
                group.hatch_date.to_string(),
                gb.notes,
                gb.nfc_tag_id,
            ],
        )
        .unwrap();
        let bird_id = conn.last_insert_rowid();
        birds_created.push(Bird {
            id: bird_id,
            band_color: gb.band_color.clone(),
            sex: gb.sex.clone(),
            bloodline_id: group.bloodline_id,
            hatch_date: group.hatch_date,
            mother_id: None,
            father_id: None,
            generation: 1,
            status: BirdStatus::Active,
            notes: gb.notes.clone(),
            nfc_tag_id: gb.nfc_tag_id.clone(),
        });
    }

    conn.execute(
        "UPDATE chick_groups SET status = 'Graduated' WHERE id = ?1",
        params![id],
    )
    .unwrap();

    Json(birds_created).into_response()
}

// ---------------------------------------------------------------------------
// Database backup
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct BackupInfo {
    filename: String,
    size_bytes: u64,
    created: String,
}

async fn create_backup() -> impl IntoResponse {
    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        std::fs::create_dir_all(backup_dir).ok();
    }

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("quailsync_{}.db", timestamp);
    let dest = backup_dir.join(&filename);

    match std::fs::copy("quailsync.db", &dest) {
        Ok(_) => {
            let meta = std::fs::metadata(&dest).ok();
            let size = meta.map(|m| m.len()).unwrap_or(0);
            (
                StatusCode::CREATED,
                Json(BackupInfo {
                    filename,
                    size_bytes: size,
                    created: chrono::Local::now().to_rfc3339(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Backup failed: {e}"),
        )
            .into_response(),
    }
}

async fn list_backups() -> Json<Vec<BackupInfo>> {
    let backup_dir = std::path::Path::new("backups");
    let mut backups = Vec::new();

    if let Ok(entries) = std::fs::read_dir(backup_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".db") {
                    let created = meta
                        .modified()
                        .ok()
                        .map(|t| {
                            let dt: chrono::DateTime<chrono::Local> = t.into();
                            dt.to_rfc3339()
                        })
                        .unwrap_or_default();
                    backups.push(BackupInfo {
                        filename: fname,
                        size_bytes: meta.len(),
                        created,
                    });
                }
            }
        }
    }

    backups.sort_by(|a, b| b.created.cmp(&a.created));
    Json(backups)
}

#[derive(Deserialize)]
struct RestoreRequest {
    filename: String,
}

async fn restore_backup(Json(body): Json<RestoreRequest>) -> impl IntoResponse {
    let backup_dir = std::path::Path::new("backups");

    if body.filename.contains('/')
        || body.filename.contains('\\')
        || body.filename.contains("..")
    {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    let source = backup_dir.join(&body.filename);

    if !source.exists() {
        return (StatusCode::NOT_FOUND, "Backup file not found").into_response();
    }

    // Create a pre-restore backup
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let pre_restore = backup_dir.join(format!("quailsync_pre_restore_{}.db", timestamp));
    std::fs::copy("quailsync.db", &pre_restore).ok();

    match std::fs::copy(&source, "quailsync.db") {
        Ok(_) => (StatusCode::OK, "Database restored. Restart server to apply.").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Restore failed: {e}"),
        )
            .into_response(),
    }
}

/// Creates an auto-backup if none exists or the latest is older than 24 hours.
pub fn auto_backup_if_needed() {
    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        std::fs::create_dir_all(backup_dir).ok();
    }

    let db_path = std::path::Path::new("quailsync.db");
    if !db_path.exists() {
        return;
    }

    let should_backup = match std::fs::read_dir(backup_dir) {
        Ok(entries) => {
            let latest = entries
                .flatten()
                .filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .ends_with(".db")
                })
                .filter_map(|e| e.metadata().ok()?.modified().ok())
                .max();

            match latest {
                Some(t) => {
                    let age = std::time::SystemTime::now()
                        .duration_since(t)
                        .unwrap_or_default();
                    age.as_secs() > 86400
                }
                None => true,
            }
        }
        Err(_) => true,
    };

    if should_backup {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let dest = backup_dir.join(format!("quailsync_auto_{}.db", timestamp));
        match std::fs::copy("quailsync.db", &dest) {
            Ok(_) => println!(
                "[backup] Auto-backup created: {}",
                dest.display()
            ),
            Err(e) => eprintln!("[backup] Auto-backup failed: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Dashboard: embedded static files
// ---------------------------------------------------------------------------

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // Serve the requested file, or fall back to index.html for SPA-style routing
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => {
            // Fallback: serve index.html for any non-API path (SPA support)
            match Asset::get("index.html") {
                Some(content) => Html(content.data.into_owned()).into_response(),
                None => (StatusCode::NOT_FOUND, "not found").into_response(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Public: build the app
// ---------------------------------------------------------------------------

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/ws/live", get(ws_live_handler))
        .route("/api/brooder/latest", get(brooder_latest))
        .route("/api/brooder/history", get(brooder_history))
        .route("/api/system/latest", get(system_latest))
        .route("/api/status", get(status))
        .route("/api/alerts", get(alerts))
        .route("/api/bloodlines", get(list_bloodlines).post(create_bloodline))
        .route("/api/birds", get(list_birds).post(create_bird))
        .route("/api/birds/{id}", axum::routing::put(update_bird))
        .route("/api/birds/{id}/weight", axum::routing::post(create_weight))
        .route("/api/birds/{id}/weights", get(list_weights))
        .route("/api/breeding-pairs", get(list_breeding_pairs).post(create_breeding_pair))
        .route("/api/clutches", get(list_clutches).post(create_clutch))
        .route("/api/clutches/{id}", axum::routing::put(update_clutch))
        .route("/api/processing", get(list_processing).post(create_processing))
        .route("/api/processing/queue", get(list_processing_queue))
        .route("/api/processing/{id}", axum::routing::put(update_processing))
        .route("/api/breeding-groups", get(list_breeding_groups).post(create_breeding_group))
        .route("/api/breeding-groups/{id}", get(get_breeding_group))
        .route("/api/flock/summary", get(flock_summary))
        .route("/api/flock/cull-recommendations", get(cull_recommendations))
        .route("/api/breeding/suggest", get(breeding_suggest))
        .route("/api/brooders", get(list_brooders).post(create_brooder))
        .route("/api/brooders/{id}", axum::routing::put(update_brooder))
        .route("/api/brooders/{id}/readings", get(brooder_readings))
        .route("/api/brooders/{id}/status", get(brooder_status))
        .route("/api/cameras", get(list_cameras).post(create_camera))
        .route("/api/cameras/{id}/brooder", axum::routing::put(update_camera_brooder))
        .route("/api/cameras/{id}/detections/summary", get(camera_detection_summary))
        .route("/api/frames", get(list_frames).post(create_frame))
        .route("/api/frames/{id}/detections", axum::routing::post(create_frame_detections))
        .route("/api/nfc/{tag_id}", get(get_bird_by_nfc))
        .route("/api/chick-groups", get(list_chick_groups).post(create_chick_group))
        .route("/api/chick-groups/{id}", get(get_chick_group))
        .route("/api/chick-groups/{id}/mortality", axum::routing::put(log_mortality))
        .route("/api/chick-groups/{id}/graduate", axum::routing::put(graduate_chick_group))
        .route("/api/backup", axum::routing::post(create_backup))
        .route("/api/backups", get(list_backups))
        .route("/api/restore", axum::routing::post(restore_backup))
        .fallback(static_handler)
        .with_state(state)
}
