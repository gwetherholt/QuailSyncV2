use chrono::{NaiveDate, Utc};
use quailsync_common::*;
use rusqlite::{params, Connection};

use crate::db::store_alert;

/// Look up the youngest active chick group in a brooder.
/// Returns (group_id, age_in_days) or None.
pub fn youngest_chick_age_in_brooder(conn: &Connection, brooder_id: i64) -> Option<(i64, i64)> {
    conn.query_row(
        "SELECT id, hatch_date FROM chick_groups
         WHERE brooder_id = ?1 AND status = 'Active'
         ORDER BY hatch_date DESC LIMIT 1",
        params![brooder_id],
        |row| {
            let id: i64 = row.get(0)?;
            let hatch_str: String = row.get(1)?;
            let hatch = NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d")
                .unwrap_or_else(|_| Utc::now().date_naive());
            let age = (Utc::now().date_naive() - hatch).num_days();
            Ok((id, age))
        },
    )
    .ok()
}

/// Check brooder readings against alert thresholds and store alerts.
pub fn check_brooder_alerts(conn: &Connection, reading: &BrooderReading, config: &AlertConfig) {
    let temp = reading.temperature_f;
    let hum = reading.humidity_percent;

    let (temp_min, temp_max) = if let Some(bid) = reading.brooder_id {
        if let Some((_group_id, age)) = youngest_chick_age_in_brooder(conn, bid) {
            let (target, tolerance) = target_temp_for_age(age);
            (target - tolerance, target + tolerance)
        } else {
            (config.brooder_temp_min, config.brooder_temp_max)
        }
    } else {
        (config.brooder_temp_min, config.brooder_temp_max)
    };

    if temp < temp_min {
        let delta = temp_min - temp;
        let severity = if delta > 3.0 { Severity::Critical } else { Severity::Warning };
        let msg = format!(
            "Temperature LOW: {:.1}\u{00b0}F (min {:.1}\u{00b0}F, {:.1}\u{00b0}F below)",
            temp, temp_min, delta,
        );
        log_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    } else if temp > temp_max {
        let delta = temp - temp_max;
        let severity = if delta > 3.0 { Severity::Critical } else { Severity::Warning };
        let msg = format!(
            "Temperature HIGH: {:.1}\u{00b0}F (max {:.1}\u{00b0}F, {:.1}\u{00b0}F above)",
            temp, temp_max, delta,
        );
        log_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    }

    if hum < config.humidity_min {
        let msg = format!("Humidity LOW: {:.1}% (min {:.1}%)", hum, config.humidity_min);
        log_alert(&Severity::Warning, &msg);
        store_alert(conn, &Severity::Warning, &msg);
    } else if hum > config.humidity_max {
        let msg = format!("Humidity HIGH: {:.1}% (max {:.1}%)", hum, config.humidity_max);
        log_alert(&Severity::Warning, &msg);
        store_alert(conn, &Severity::Warning, &msg);
    }
}

fn log_alert(severity: &Severity, message: &str) {
    match severity {
        Severity::Warning => eprintln!("[WARN] {message}"),
        Severity::Critical => eprintln!("[CRIT] {message}"),
    }
}
