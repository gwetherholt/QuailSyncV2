pub mod helpers;

use quailsync_common::*;
use rusqlite::{params, Connection};

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

    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_stillborn INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_quit INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_infertile INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_damaged INTEGER", []).ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN hatch_notes TEXT", []).ok();
    conn.execute("ALTER TABLE birds ADD COLUMN nfc_tag_id TEXT UNIQUE", []).ok();
    conn.execute("ALTER TABLE brooders ADD COLUMN camera_url TEXT", []).ok();
    conn.execute("ALTER TABLE birds ADD COLUMN current_brooder_id INTEGER REFERENCES brooders(id)", []).ok();

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

    // --- Performance indexes ---
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_readings_brooder_received ON brooder_readings(brooder_id, received_at);
         CREATE INDEX IF NOT EXISTS idx_readings_received ON brooder_readings(received_at);
         CREATE INDEX IF NOT EXISTS idx_system_metrics_received ON system_metrics(received_at);
         CREATE INDEX IF NOT EXISTS idx_birds_status ON birds(status);
         CREATE INDEX IF NOT EXISTS idx_birds_bloodline ON birds(bloodline_id);
         CREATE INDEX IF NOT EXISTS idx_birds_nfc ON birds(nfc_tag_id);
         CREATE INDEX IF NOT EXISTS idx_birds_brooder ON birds(current_brooder_id);
         CREATE INDEX IF NOT EXISTS idx_weights_bird_date ON weight_records(bird_id, date);
         CREATE INDEX IF NOT EXISTS idx_processing_status ON processing_records(status);
         CREATE INDEX IF NOT EXISTS idx_chick_groups_brooder ON chick_groups(brooder_id, status);
         CREATE INDEX IF NOT EXISTS idx_alerts_timestamp ON alerts(timestamp);",
    )
    .expect("failed to create indexes");
}

// ---------------------------------------------------------------------------
// Telemetry storage
// ---------------------------------------------------------------------------

pub fn store_payload(conn: &Connection, payload: &TelemetryPayload) {
    match payload {
        TelemetryPayload::Brooder(r) => {
            if let Some(bid) = r.brooder_id {
                let exists: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM brooders WHERE id = ?1",
                        params![bid],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                if exists == 0 {
                    conn.execute(
                        "INSERT INTO brooders (id, name, life_stage) VALUES (?1, ?2, 'Chick')",
                        params![bid, format!("Brooder {bid}")],
                    )
                    .ok();
                    println!("[auto] Created brooder #{bid}");
                }
            }
            conn.execute(
                "INSERT INTO brooder_readings (temperature, humidity, timestamp, brooder_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![r.temperature_f, r.humidity_percent, r.timestamp.to_rfc3339(), r.brooder_id],
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
        TelemetryPayload::CameraAnnounce(ca) => {
            conn.execute(
                "UPDATE brooders SET camera_url = ?1 WHERE id = ?2",
                params![ca.stream_url, ca.brooder_id],
            )
            .ok();
            println!(
                "[camera] Auto-registered stream for brooder {}: {}",
                ca.brooder_id, ca.stream_url
            );
        }
    }
}

pub fn store_alert(conn: &Connection, severity: &Severity, message: &str) {
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
