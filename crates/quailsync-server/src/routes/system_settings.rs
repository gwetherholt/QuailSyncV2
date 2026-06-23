//! Server-owned system settings (lifecycle + alert thresholds).
//!
//! Backed by the `system_settings` key/value table (seeded in `init_db`). The
//! typed view and per-key default fallback live in `quailsync_common::Settings`.
//! This is the foundation for multi-user settings — today it's a single
//! system-level set of rows.
//!
//! Routes: `GET /api/system-settings` (read all) and `PUT /api/system-settings`
//! (partial update — any subset of fields). Note: the older `/api/settings`
//! endpoint is a *separate*, pre-existing thing (breeding-group cull guardrails,
//! see `routes/settings.rs`); these system settings are intentionally mounted
//! under a distinct path so neither clobbers the other.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use quailsync_common::{Settings, UpdateSettings};
use rusqlite::{params, Connection};

use crate::state::{acquire_db, AppState};

/// Read every row from `system_settings` and build the typed `Settings`,
/// falling back to defaults per missing/malformed key.
pub fn load_system_settings(conn: &Connection) -> Settings {
    let rows: Vec<(String, String)> = conn
        .prepare("SELECT key, value FROM system_settings")
        .and_then(|mut stmt| {
            let mapped = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            Ok(mapped.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default();
    Settings::from_rows(rows)
}

/// Upsert a single setting, stamping `updated_at`.
fn upsert(conn: &Connection, key: &str, value: &str) {
    conn.execute(
        "INSERT INTO system_settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        params![key, value],
    )
    .ok();
}

/// `GET /api/system-settings` — the full current settings (always DB-fresh).
pub(crate) async fn get_settings(State(state): State<AppState>) -> Json<Settings> {
    let conn = acquire_db(&state);
    Json(load_system_settings(&conn))
}

/// `PUT /api/system-settings` — update only the keys present in the body, then
/// return the full updated settings. Also refreshes the in-memory copy in
/// `AppState` so alert routes see the new thresholds without a restart.
pub(crate) async fn update_settings(
    State(state): State<AppState>,
    Json(body): Json<UpdateSettings>,
) -> impl IntoResponse {
    let updated = {
        let conn = acquire_db(&state);
        if let Some(v) = body.alert_temp_min_f {
            upsert(&conn, "alert_temp_min_f", &v.to_string());
        }
        if let Some(v) = body.alert_temp_max_f {
            upsert(&conn, "alert_temp_max_f", &v.to_string());
        }
        if let Some(v) = body.alert_humidity_min {
            upsert(&conn, "alert_humidity_min", &v.to_string());
        }
        if let Some(v) = body.alert_humidity_max {
            upsert(&conn, "alert_humidity_max", &v.to_string());
        }
        if let Some(v) = body.adult_temp_min_f {
            upsert(&conn, "adult_temp_min_f", &v.to_string());
        }
        if let Some(v) = body.adult_temp_max_f {
            upsert(&conn, "adult_temp_max_f", &v.to_string());
        }
        if let Some(v) = body.incubation_days {
            upsert(&conn, "incubation_days", &v.to_string());
        }
        if let Some(v) = body.ready_to_transition_age_days {
            upsert(&conn, "ready_to_transition_age_days", &v.to_string());
        }
        if let Some(v) = body.butcher_weight_grams {
            upsert(&conn, "butcher_weight_grams", &v.to_string());
        }
        if let Some(v) = body.min_breeding_weight_grams {
            upsert(&conn, "min_breeding_weight_grams", &v.to_string());
        }
        if let Some(v) = body.sensor_stale_seconds {
            upsert(&conn, "sensor_stale_seconds", &v.to_string());
        }
        if let Some(ref v) = body.brooder_week_temps_f {
            let json = serde_json::to_string(v).unwrap_or_else(|_| "[]".to_string());
            upsert(&conn, "brooder_week_temps_f", &json);
        }
        if let Some(v) = body.indoor_cam_roboflow_upload_enabled {
            upsert(&conn, "indoor_cam_roboflow_upload_enabled", &v.to_string());
        }
        if let Some(v) = body.indoor_cam_image_save_enabled {
            upsert(&conn, "indoor_cam_image_save_enabled", &v.to_string());
        }
        load_system_settings(&conn)
    };

    // Keep the cached settings (used by the alert engine) in sync with the DB.
    if let Ok(mut guard) = state.settings.write() {
        *guard = updated.clone();
    }

    Json(updated)
}
