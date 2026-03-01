use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, Mutex};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use quailsync_common::{
    BrooderReading, Species, SystemMetrics, TelemetryPayload,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AppState {
    db: Arc<Mutex<Connection>>,
    agent_connected: Arc<AtomicBool>,
}

// ---------------------------------------------------------------------------
// Database setup
// ---------------------------------------------------------------------------

fn init_db(conn: &Connection) {
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
                "[telemetry] brooder | temp: {:.1}°F  humidity: {:.1}%  at {}",
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
                timestamp: ts
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_default(),
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
                timestamp: ts
                    .parse::<DateTime<Utc>>()
                    .unwrap_or_default(),
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

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let conn = Connection::open("quailsync.db").expect("failed to open database");
    init_db(&conn);
    println!("[db] SQLite initialized (quailsync.db)");

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/api/brooder/latest", get(brooder_latest))
        .route("/api/brooder/history", get(brooder_history))
        .route("/api/system/latest", get(system_latest))
        .route("/api/status", get(status))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("quailsync-server listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
