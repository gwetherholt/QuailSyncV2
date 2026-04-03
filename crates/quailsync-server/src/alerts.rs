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

/// Check brooder readings against age-based temperature thresholds and humidity limits.
///
/// Temperature ranges by chick age (from target_temp_for_age):
///   Week 1 (0-7d):  93-97°F
///   Week 2 (8-14d): 88-92°F
///   Week 3 (15-21d): 83-87°F
///   Week 4 (22-28d): 78-82°F
///   Week 5 (29-35d): 73-77°F
///   Week 6+ (36+d): 68-72°F
///
/// Alert severity by deviation:
///   >5°F outside range → CRITICAL
///   2-5°F outside range → WARNING
///   1-2°F outside range → INFO
///
/// Humidity:
///   <30% → CRITICAL
///   <40% → WARNING
pub fn check_brooder_alerts(conn: &Connection, reading: &BrooderReading, config: &AlertConfig) {
    let temp = reading.temperature_f;
    let hum = reading.humidity_percent;

    // Determine temperature range based on chick age in this brooder
    let (temp_min, temp_max, age_label) = if let Some(bid) = reading.brooder_id {
        if let Some((_group_id, age)) = youngest_chick_age_in_brooder(conn, bid) {
            let (target, tolerance) = target_temp_for_age(age);
            let week = (age / 7) + 1;
            (
                target - tolerance,
                target + tolerance,
                format!("brooder {} (week {}, day {})", bid, week, age),
            )
        } else {
            // No chick group assigned — use adult/unassigned range
            (
                config.brooder_temp_min,
                config.brooder_temp_max,
                format!("brooder {} (unassigned)", bid),
            )
        }
    } else {
        (
            config.brooder_temp_min,
            config.brooder_temp_max,
            "unknown brooder".to_string(),
        )
    };

    // Temperature alerts with graduated severity
    if temp < temp_min {
        let delta = temp_min - temp;
        let severity = if delta > 5.0 {
            Severity::Critical
        } else if delta > 2.0 {
            Severity::Warning
        } else if delta > 1.0 {
            Severity::Info
        } else {
            return; // within 1°F tolerance — no alert
        };
        let msg = format!(
            "Temperature LOW on {}: {:.1}\u{00b0}F (range {:.0}-{:.0}\u{00b0}F, {:.1}\u{00b0}F below)",
            age_label, temp, temp_min, temp_max, delta,
        );
        log_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    } else if temp > temp_max {
        let delta = temp - temp_max;
        let severity = if delta > 5.0 {
            Severity::Critical
        } else if delta > 2.0 {
            Severity::Warning
        } else if delta > 1.0 {
            Severity::Info
        } else {
            return;
        };
        let msg = format!(
            "Temperature HIGH on {}: {:.1}\u{00b0}F (range {:.0}-{:.0}\u{00b0}F, {:.1}\u{00b0}F above)",
            age_label, temp, temp_min, temp_max, delta,
        );
        log_alert(&severity, &msg);
        store_alert(conn, &severity, &msg);
    }

    // Humidity alerts
    if hum < 30.0 {
        let msg = format!(
            "Humidity CRITICAL on {}: {:.1}% (below 30%)",
            age_label, hum,
        );
        log_alert(&Severity::Critical, &msg);
        store_alert(conn, &Severity::Critical, &msg);
    } else if hum < 40.0 {
        let msg = format!(
            "Humidity LOW on {}: {:.1}% (below 40%)",
            age_label, hum,
        );
        log_alert(&Severity::Warning, &msg);
        store_alert(conn, &Severity::Warning, &msg);
    }
}

fn log_alert(severity: &Severity, message: &str) {
    match severity {
        Severity::Info => eprintln!("[INFO] {message}"),
        Severity::Warning => eprintln!("[WARN] {message}"),
        Severity::Critical => eprintln!("[CRIT] {message}"),
    }
}
