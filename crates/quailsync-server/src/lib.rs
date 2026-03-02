use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, NaiveDate, Utc};
use colored::Colorize;
use quailsync_common::{
    Alert, AlertConfig, Bird, BirdStatus, Bloodline, BreedingPair, BrooderReading, Clutch,
    ClutchStatus, CreateBird, CreateBloodline, CreateBreedingPair, CreateClutch,
    InbreedingCoefficient, Sex, Severity, Species, SystemMetrics, TelemetryPayload, UpdateClutch,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub agent_connected: Arc<AtomicBool>,
    pub alert_config: AlertConfig,
}

// ---------------------------------------------------------------------------
// Database setup
// ---------------------------------------------------------------------------

pub fn init_db(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS brooder_readings (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            temperature     REAL    NOT NULL,
            humidity        REAL    NOT NULL,
            timestamp       TEXT    NOT NULL,
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
            notes           TEXT
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
            notes               TEXT
        );",
    )
    .expect("failed to create tables");
}

// ---------------------------------------------------------------------------
// Database writes
// ---------------------------------------------------------------------------

fn store_payload(conn: &Connection, payload: &TelemetryPayload) {
    match payload {
        TelemetryPayload::Brooder(r) => {
            conn.execute(
                "INSERT INTO brooder_readings (temperature, humidity, timestamp)
                 VALUES (?1, ?2, ?3)",
                (r.temperature_celsius, r.humidity_percent, r.timestamp.to_rfc3339()),
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
                    let conn = state.db.lock().unwrap();
                    store_payload(&conn, &payload);
                    if let TelemetryPayload::Brooder(ref reading) = payload {
                        check_brooder_alerts(&conn, reading, &state.alert_config);
                    }
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
    let conn = state.db.lock().unwrap();
    let result = conn.query_row(
        "SELECT temperature, humidity, timestamp FROM brooder_readings
         ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_celsius: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
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
    let conn = state.db.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT temperature, humidity, timestamp FROM brooder_readings
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
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    Json(readings)
}

async fn system_latest(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();

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
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();
    conn.execute(
        "INSERT INTO birds (band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
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
        }),
    )
}

async fn list_birds(State(state): State<AppState>) -> Json<Vec<Bird>> {
    let conn = state.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, band_color, sex, bloodline_id, hatch_date, mother_id, father_id, generation, status, notes FROM birds ORDER BY id")
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
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    Json(rows)
}

// --- Breeding Pairs ---

async fn create_breeding_pair(
    State(state): State<AppState>,
    Json(body): Json<CreateBreedingPair>,
) -> impl IntoResponse {
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();
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
    let conn = state.db.lock().unwrap();
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
        }),
    )
}

async fn list_clutches(State(state): State<AppState>) -> Json<Vec<Clutch>> {
    let conn = state.db.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT id, breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes FROM clutches ORDER BY id")
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
    let conn = state.db.lock().unwrap();

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

    let clutch = conn
        .query_row(
            "SELECT id, breeding_pair_id, bloodline_id, eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status, notes FROM clutches WHERE id = ?1",
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
    let conn = state.db.lock().unwrap();

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
    let conn = state.db.lock().unwrap();
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

    results.sort_by(|a, b| a.coefficient.partial_cmp(&b.coefficient).unwrap());

    Json(results)
}

// ---------------------------------------------------------------------------
// Public: build the app
// ---------------------------------------------------------------------------

pub fn build_app(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/api/brooder/latest", get(brooder_latest))
        .route("/api/brooder/history", get(brooder_history))
        .route("/api/system/latest", get(system_latest))
        .route("/api/status", get(status))
        .route("/api/alerts", get(alerts))
        .route("/api/bloodlines", get(list_bloodlines).post(create_bloodline))
        .route("/api/birds", get(list_birds).post(create_bird))
        .route("/api/breeding-pairs", get(list_breeding_pairs).post(create_breeding_pair))
        .route("/api/clutches", get(list_clutches).post(create_clutch))
        .route("/api/clutches/{id}", axum::routing::put(update_clutch))
        .route("/api/flock/summary", get(flock_summary))
        .route("/api/breeding/suggest", get(breeding_suggest))
        .with_state(state)
}
