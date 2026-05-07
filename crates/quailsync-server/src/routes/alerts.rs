use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::Utc;
use quailsync_common::{
    CreateSystemAlert, ResolveSystemAlertRequest, ResolveSystemAlertResponse, SystemAlert,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::state::{acquire_db, db_error, AppState};

// ---------- internal row mapping --------------------------------------------

fn row_to_system_alert(row: &rusqlite::Row<'_>) -> rusqlite::Result<SystemAlert> {
    let resolved_at: Option<String> = row.get(7)?;
    let dismissed_at: Option<String> = row.get(8)?;
    let is_active = resolved_at.is_none() && dismissed_at.is_none();
    Ok(SystemAlert {
        id: row.get(0)?,
        alert_key: row.get(1)?,
        severity: row.get(2)?,
        title: row.get(3)?,
        message: row.get(4)?,
        source: row.get(5)?,
        created_at: row.get(6)?,
        resolved_at,
        dismissed_at,
        metadata_json: row.get(9)?,
        is_active,
    })
}

const SELECT_COLS: &str =
    "id, alert_key, severity, title, message, source, created_at, resolved_at, dismissed_at, metadata_json";

fn fetch_alert_by_id(conn: &Connection, id: i64) -> rusqlite::Result<Option<SystemAlert>> {
    let sql = format!("SELECT {SELECT_COLS} FROM system_alerts WHERE id = ?1");
    conn.query_row(&sql, params![id], row_to_system_alert)
        .optional()
}

/// Find an existing active row with the same alert_key (NULL resolved_at + NULL dismissed_at).
fn find_active_by_key(conn: &Connection, alert_key: &str) -> rusqlite::Result<Option<SystemAlert>> {
    let sql = format!(
        "SELECT {SELECT_COLS} FROM system_alerts
         WHERE alert_key = ?1 AND resolved_at IS NULL AND dismissed_at IS NULL
         ORDER BY id DESC LIMIT 1"
    );
    conn.query_row(&sql, params![alert_key], row_to_system_alert)
        .optional()
}

/// Increment the `occurrences` counter inside metadata_json (creating the
/// field if absent) and return the new JSON blob. If parsing fails we fall
/// back to a fresh single-key blob, which is fine — metadata is best-effort.
fn bump_occurrences(existing: Option<&str>) -> String {
    let mut value: serde_json::Value = existing
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_else(|| serde_json::json!({}));

    let next = value
        .get("occurrences")
        .and_then(|v| v.as_i64())
        .unwrap_or(1)
        + 1;

    if let Some(obj) = value.as_object_mut() {
        obj.insert("occurrences".to_string(), serde_json::json!(next));
    } else {
        value = serde_json::json!({ "occurrences": next });
    }

    value.to_string()
}

// ---------- POST /api/alerts ------------------------------------------------

pub(crate) async fn create_alert(
    State(state): State<AppState>,
    Json(body): Json<CreateSystemAlert>,
) -> impl IntoResponse {
    if body.alert_key.is_empty() || body.title.is_empty() || body.severity.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "alert_key, title, and severity are required",
        )
            .into_response();
    }

    let conn = acquire_db(&state);
    let now = Utc::now().to_rfc3339();

    // Collapse repeats: if there's already an active alert with this key,
    // refresh its title/message/created_at and bump the occurrences counter
    // in metadata_json instead of inserting a new row.
    match find_active_by_key(&conn, &body.alert_key) {
        Ok(Some(existing)) => {
            let new_meta = bump_occurrences(existing.metadata_json.as_deref());
            if let Err(e) = conn.execute(
                "UPDATE system_alerts
                    SET severity = ?1, title = ?2, message = ?3, source = ?4,
                        created_at = ?5, metadata_json = ?6
                  WHERE id = ?7",
                params![
                    body.severity,
                    body.title,
                    body.message,
                    body.source,
                    now,
                    new_meta,
                    existing.id,
                ],
            ) {
                return db_error(e);
            }
            match fetch_alert_by_id(&conn, existing.id) {
                Ok(Some(a)) => (StatusCode::OK, Json(a)).into_response(),
                Ok(None) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "alert vanished after update",
                )
                    .into_response(),
                Err(e) => db_error(e),
            }
        }
        Ok(None) => {
            if let Err(e) = conn.execute(
                "INSERT INTO system_alerts
                    (alert_key, severity, title, message, source, created_at, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    body.alert_key,
                    body.severity,
                    body.title,
                    body.message,
                    body.source,
                    now,
                    body.metadata_json,
                ],
            ) {
                return db_error(e);
            }
            let new_id = conn.last_insert_rowid();
            match fetch_alert_by_id(&conn, new_id) {
                Ok(Some(a)) => (StatusCode::CREATED, Json(a)).into_response(),
                Ok(None) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "alert vanished after insert",
                )
                    .into_response(),
                Err(e) => db_error(e),
            }
        }
        Err(e) => db_error(e),
    }
}

// ---------- POST /api/alerts/resolve ----------------------------------------

pub(crate) async fn resolve_alerts(
    State(state): State<AppState>,
    Json(body): Json<ResolveSystemAlertRequest>,
) -> impl IntoResponse {
    if body.alert_key.is_empty() {
        return (StatusCode::BAD_REQUEST, "alert_key is required").into_response();
    }

    let conn = acquire_db(&state);
    let now = Utc::now().to_rfc3339();

    match conn.execute(
        "UPDATE system_alerts
            SET resolved_at = ?1
          WHERE alert_key = ?2
            AND resolved_at IS NULL
            AND dismissed_at IS NULL",
        params![now, body.alert_key],
    ) {
        Ok(rows) => (
            StatusCode::OK,
            Json(ResolveSystemAlertResponse {
                resolved: rows as i64,
            }),
        )
            .into_response(),
        Err(e) => db_error(e),
    }
}

// ---------- POST /api/alerts/{id}/dismiss -----------------------------------

pub(crate) async fn dismiss_alert(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let now = Utc::now().to_rfc3339();

    let updated = match conn.execute(
        "UPDATE system_alerts
            SET dismissed_at = ?1
          WHERE id = ?2 AND dismissed_at IS NULL",
        params![now, id],
    ) {
        Ok(n) => n,
        Err(e) => return db_error(e),
    };

    if updated == 0 {
        // Either the id doesn't exist or it was already dismissed.
        match fetch_alert_by_id(&conn, id) {
            Ok(Some(a)) => (StatusCode::OK, Json(a)).into_response(),
            Ok(None) => (StatusCode::NOT_FOUND, "alert not found").into_response(),
            Err(e) => db_error(e),
        }
    } else {
        match fetch_alert_by_id(&conn, id) {
            Ok(Some(a)) => (StatusCode::OK, Json(a)).into_response(),
            Ok(None) => (StatusCode::NOT_FOUND, "alert not found").into_response(),
            Err(e) => db_error(e),
        }
    }
}

// ---------- GET /api/alerts/active ------------------------------------------

pub(crate) async fn list_active(State(state): State<AppState>) -> impl IntoResponse {
    let conn = acquire_db(&state);
    let sql = format!(
        "SELECT {SELECT_COLS} FROM system_alerts
         WHERE resolved_at IS NULL AND dismissed_at IS NULL
         ORDER BY datetime(created_at) DESC, id DESC"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return db_error(e),
    };
    let rows: Vec<SystemAlert> = match stmt.query_map([], row_to_system_alert) {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(e) => return db_error(e),
    };
    Json(rows).into_response()
}

// ---------- GET /api/alerts/recent?limit=N ----------------------------------

#[derive(Deserialize)]
pub(crate) struct RecentParams {
    pub limit: Option<i64>,
}

pub(crate) async fn list_recent(
    State(state): State<AppState>,
    Query(params): Query<RecentParams>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    let conn = acquire_db(&state);
    let sql = format!(
        "SELECT {SELECT_COLS} FROM system_alerts
         ORDER BY datetime(created_at) DESC, id DESC
         LIMIT ?1"
    );
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return db_error(e),
    };
    let rows: Vec<SystemAlert> = match stmt.query_map(params![limit], row_to_system_alert) {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(e) => return db_error(e),
    };
    Json(rows).into_response()
}

// ---------- unit tests ------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_occurrences_from_none_starts_at_two() {
        // Reasoning: a pre-existing row already represents one occurrence;
        // a re-post is the second.
        let s = bump_occurrences(None);
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("occurrences").and_then(|x| x.as_i64()), Some(2));
    }

    #[test]
    fn bump_occurrences_increments_existing() {
        let s = bump_occurrences(Some(r#"{"occurrences": 4, "host": "pi"}"#));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("occurrences").and_then(|x| x.as_i64()), Some(5));
        assert_eq!(v.get("host").and_then(|x| x.as_str()), Some("pi"));
    }

    #[test]
    fn bump_occurrences_recovers_from_garbage() {
        let s = bump_occurrences(Some("not-json"));
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v.get("occurrences").and_then(|x| x.as_i64()), Some(2));
    }
}
