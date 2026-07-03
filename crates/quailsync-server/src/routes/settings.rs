use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::{AppSettings, GeneticsSettings, UpdateAppSettings};
use rusqlite::{params, Connection};
use std::collections::{BTreeMap, HashMap};

use crate::state::{acquire_db, AppState};

/// Reads a single key from the settings table, or `None` if missing/unparseable.
///
/// Returning `None` rather than the default lets callers decide whether to fall
/// back at the per-key granularity or skip the row entirely.
fn read_u32(conn: &Connection, key: &str) -> Option<u32> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| v.parse::<u32>().ok())
}

/// Loads the current app settings, falling back to `AppSettings::default()`
/// for any key that's missing or malformed. Migrations seed both defaults at
/// init_db time, so the fallback only kicks in for fresh DBs that haven't
/// been touched by `init_db` (i.e. nowhere in practice).
pub fn load_settings(conn: &Connection) -> AppSettings {
    let defaults = AppSettings::default();
    AppSettings {
        desired_males_per_group: read_u32(conn, "desired_males_per_group")
            .unwrap_or(defaults.desired_males_per_group),
        max_females_per_male: read_u32(conn, "max_females_per_male")
            .unwrap_or(defaults.max_females_per_male),
    }
}

pub(crate) async fn get_settings(State(state): State<AppState>) -> Json<AppSettings> {
    let conn = acquire_db(&state);
    Json(load_settings(&conn))
}

pub(crate) async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<UpdateAppSettings>,
) -> impl IntoResponse {
    // Both fields are positive ints. Zero or absurdly large values would
    // either divide by zero downstream or produce nonsense math, so refuse
    // them at the boundary with a 400 rather than silently clamping.
    if let Some(n) = body.desired_males_per_group {
        if !(1..=100).contains(&n) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "desired_males_per_group must be between 1 and 100"
                })),
            )
                .into_response();
        }
    }
    if let Some(n) = body.max_females_per_male {
        if !(1..=100).contains(&n) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "max_females_per_male must be between 1 and 100"
                })),
            )
                .into_response();
        }
    }

    let conn = acquire_db(&state);
    if let Some(n) = body.desired_males_per_group {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('desired_males_per_group', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![n.to_string()],
        )
        .ok();
    }
    if let Some(n) = body.max_females_per_male {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('max_females_per_male', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![n.to_string()],
        )
        .ok();
    }
    Json(load_settings(&conn)).into_response()
}

// ---------------------------------------------------------------------------
// Phase 5: configurable genetics thresholds (GET/PUT /api/settings/genetics)
// ---------------------------------------------------------------------------

/// Loads the genetics thresholds, falling back to [`GeneticsSettings::default`]
/// for any key missing or unparseable in the `settings` table.
pub fn load_genetics_settings(conn: &Connection) -> GeneticsSettings {
    let mut s = GeneticsSettings::default();
    for (key, ..) in GeneticsSettings::SPEC {
        if let Some(v) = read_u32(conn, key) {
            s.set(key, v);
        }
    }
    s
}

/// `GET /api/settings/genetics` — the genetics settings as a flat
/// `{ "genetics.threshold.safe": "15", … }` string map.
pub(crate) async fn get_genetics_settings(
    State(state): State<AppState>,
) -> Json<BTreeMap<String, String>> {
    let conn = acquire_db(&state);
    Json(load_genetics_settings(&conn).to_map())
}

/// `PUT /api/settings/genetics` — apply a partial `{ key: value }` update. Every
/// key must be a known genetics key whose value is an integer within its range;
/// otherwise the whole request is rejected (400) with no writes. Values may be
/// JSON numbers or numeric strings. Returns the full updated settings map.
pub(crate) async fn update_genetics_settings(
    State(state): State<AppState>,
    Json(body): Json<HashMap<String, serde_json::Value>>,
) -> impl IntoResponse {
    let bad = |msg: String| {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response()
    };

    // Validate every entry up front (all-or-nothing).
    let mut parsed: Vec<(String, u32)> = Vec::new();
    for (key, raw) in &body {
        let Some((lo, hi)) = GeneticsSettings::valid_range(key) else {
            return bad(format!("unknown settings key: {key}"));
        };
        let value: Option<i64> = match raw {
            serde_json::Value::Number(n) => n.as_i64(),
            serde_json::Value::String(s) => s.trim().parse::<i64>().ok(),
            _ => None,
        };
        let Some(v) = value else {
            return bad(format!("{key} must be an integer"));
        };
        if v < lo as i64 || v > hi as i64 {
            return bad(format!("{key} must be between {lo} and {hi}"));
        }
        parsed.push((key.clone(), v as u32));
    }

    let conn = acquire_db(&state);
    for (key, v) in &parsed {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, v.to_string()],
        )
        .ok();
    }
    Json(load_genetics_settings(&conn).to_map()).into_response()
}
