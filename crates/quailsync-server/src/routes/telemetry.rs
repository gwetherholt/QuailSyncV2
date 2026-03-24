use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use quailsync_common::*;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

use crate::state::{acquire_db, AppState};

pub(crate) async fn health() -> &'static str {
    "quailsync-server ok"
}

pub(crate) async fn brooder_latest(State(state): State<AppState>) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let result = conn.query_row(
        "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
         ORDER BY id DESC LIMIT 1",
        [],
        |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_f: row.get(0)?,
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
pub(crate) struct HistoryParams {
    pub(crate) minutes: Option<u64>,
}

pub(crate) async fn brooder_history(
    State(state): State<AppState>,
    Query(params): Query<HistoryParams>,
) -> impl IntoResponse {
    let minutes = params.minutes.unwrap_or(60);
    let conn = acquire_db(&state);
    let mut stmt = match conn.prepare(
        "SELECT temperature, humidity, timestamp, brooder_id FROM brooder_readings
         WHERE received_at >= datetime('now', ?1)
         ORDER BY id DESC",
    ) {
        Ok(s) => s,
        Err(e) => return crate::state::db_error(e),
    };

    let cutoff = format!("-{minutes} minutes");
    // TODO: filter_map silently drops row-mapping errors
    let readings: Vec<BrooderReading> = stmt
        .query_map([&cutoff], |row| {
            let ts: String = row.get(2)?;
            Ok(BrooderReading {
                temperature_f: row.get(0)?,
                humidity_percent: row.get(1)?,
                timestamp: ts.parse::<DateTime<Utc>>().unwrap_or_default(),
                brooder_id: row.get(3)?,
            })
        })
        .unwrap_or_else(|_| panic!("query_map failed"))
        .filter_map(|r| r.ok())
        .collect();

    Json(readings).into_response()
}

pub(crate) async fn system_latest(State(state): State<AppState>) -> impl IntoResponse {
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
pub(crate) struct StatusSummary {
    agent_connected: bool,
    last_brooder_reading: Option<String>,
    last_system_metric: Option<String>,
    last_detection_event: Option<String>,
}

pub(crate) async fn status(State(state): State<AppState>) -> Json<StatusSummary> {
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

pub(crate) async fn alerts(
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
        .expect("failed to prepare alerts query");

    let cutoff = format!("-{minutes} minutes");
    // TODO: filter_map silently drops row-mapping errors
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
