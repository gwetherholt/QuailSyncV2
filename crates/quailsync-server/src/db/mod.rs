pub mod helpers;

use quailsync_common::*;
use rusqlite::{params, Connection};

// ---------------------------------------------------------------------------
// Database setup
// ---------------------------------------------------------------------------

/// Returns true if `table` has a column named `column`.
/// Used by the lineage migration to check whether legacy bloodline_id columns
/// still exist before backfilling junction tables and dropping them.
fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
    let sql = format!("PRAGMA table_info(\"{table}\")");
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let cols: Vec<String> = match stmt.query_map([], |row| row.get::<_, String>(1)) {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(_) => return false,
    };
    cols.iter().any(|c| c == column)
}

/// Returns true if a table named `table` exists.
fn table_exists(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?1",
        [table],
        |_| Ok(()),
    )
    .is_ok()
}

pub fn init_db(conn: &Connection) {
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("failed to enable foreign keys");

    // -------------------------------------------------------------------------
    // PRE-MIGRATION: bloodline -> lineage rename.
    //
    // This MUST run before the main CREATE TABLE batch below — otherwise
    // `CREATE TABLE IF NOT EXISTS lineages` would create an empty new table
    // alongside the legacy `bloodlines` table, blocking the rename.
    //
    // Idempotent: errors are swallowed when the source name no longer exists.
    // -------------------------------------------------------------------------
    if table_exists(conn, "bloodlines") && !table_exists(conn, "lineages") {
        conn.execute("ALTER TABLE bloodlines RENAME TO lineages", [])
            .ok();
    }
    if column_exists(conn, "brooders", "bloodline_id") {
        conn.execute(
            "ALTER TABLE brooders RENAME COLUMN bloodline_id TO lineage_id",
            [],
        )
        .ok();
    }
    if column_exists(conn, "clutches", "bloodline_id") {
        conn.execute(
            "ALTER TABLE clutches RENAME COLUMN bloodline_id TO lineage_id",
            [],
        )
        .ok();
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS brooders (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            name         TEXT NOT NULL,
            lineage_id   INTEGER REFERENCES lineages(id),
            life_stage   TEXT NOT NULL DEFAULT 'Chick',
            qr_code      TEXT NOT NULL DEFAULT '',
            notes        TEXT,
            housing_type TEXT NOT NULL DEFAULT 'brooder'
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

        CREATE TABLE IF NOT EXISTS lineages (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            name            TEXT    NOT NULL,
            source          TEXT    NOT NULL,
            notes           TEXT
        );

        CREATE TABLE IF NOT EXISTS birds (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            band_color      TEXT,
            sex             TEXT    NOT NULL,
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
            lineage_id          INTEGER REFERENCES lineages(id),
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

        -- Males live in the breeding_group_males junction (single source of
        -- truth). `status` is 'active' (>=1 male) or 'infertile' (no males);
        -- the group still represents birds cohabiting a hutch even with no male.
        CREATE TABLE IF NOT EXISTS breeding_groups (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            name       TEXT    NOT NULL,
            start_date TEXT    NOT NULL,
            notes      TEXT,
            status     TEXT    NOT NULL DEFAULT 'active'
        );

        CREATE TABLE IF NOT EXISTS breeding_group_members (
            group_id  INTEGER NOT NULL REFERENCES breeding_groups(id),
            female_id INTEGER NOT NULL REFERENCES birds(id),
            PRIMARY KEY (group_id, female_id)
        );

        -- Males assigned to a group — the SINGLE source of truth for a group's
        -- males. Most groups have one; the UI allows extra males behind a
        -- confirmation step. A group with zero rows here is 'infertile'.
        CREATE TABLE IF NOT EXISTS breeding_group_males (
            group_id INTEGER NOT NULL REFERENCES breeding_groups(id),
            male_id  INTEGER NOT NULL REFERENCES birds(id),
            PRIMARY KEY (group_id, male_id)
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

    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_stillborn INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_quit INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_infertile INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN eggs_damaged INTEGER", [])
        .ok();
    conn.execute("ALTER TABLE clutches ADD COLUMN hatch_notes TEXT", [])
        .ok();
    conn.execute("ALTER TABLE birds ADD COLUMN nfc_tag_id TEXT UNIQUE", [])
        .ok();
    conn.execute("ALTER TABLE brooders ADD COLUMN camera_url TEXT", [])
        .ok();
    // Housing-type axis (issue #11). Existing rows get 'brooder' so behaviour
    // is unchanged until the user explicitly changes a unit's type.
    if !column_exists(conn, "brooders", "housing_type") {
        conn.execute(
            "ALTER TABLE brooders ADD COLUMN housing_type TEXT NOT NULL DEFAULT 'brooder'",
            [],
        )
        .expect("ALTER TABLE brooders ADD COLUMN housing_type failed");
        println!("[migration] added brooders.housing_type (default 'brooder')");
    }
    conn.execute(
        "ALTER TABLE birds ADD COLUMN current_brooder_id INTEGER REFERENCES brooders(id)",
        [],
    )
    .ok();
    conn.execute("ALTER TABLE birds ADD COLUMN photo_path TEXT", [])
        .ok();
    // Bird-photo upload timestamp (ISO-8601). Filenames are now history-keyed
    // (`bird_{id}_{stamp}.jpg`) so they're no longer derivable from the id
    // alone — `photo_path` points at the most-recent upload and this records
    // when it landed. Both are written together, only after the file is safely
    // on disk. See routes/photos.rs.
    conn.execute("ALTER TABLE birds ADD COLUMN photo_uploaded_at TEXT", [])
        .ok();

    // Breeding-group male normalization: `breeding_group_males` junction table
    // is the single source of truth for males. If the old `male_id` column
    // still exists, backfill the junction, rebuild the table without it,
    // and add the `status` column ('active' / 'infertile').
    //
    // SQLite can't DROP a column that's part of a FOREIGN KEY constraint
    // (male_id REFERENCES birds), so we rebuild the table. Only runs on legacy
    // DBs — fresh ones are already created in the new shape above.
    if column_exists(conn, "breeding_groups", "male_id") {
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;

             -- Make sure every legacy male_id is represented in the junction
             -- before we drop the column (older rows may predate the junction).
             INSERT OR IGNORE INTO breeding_group_males (group_id, male_id)
                 SELECT id, male_id FROM breeding_groups WHERE male_id IS NOT NULL;

             CREATE TABLE breeding_groups_new (
                 id         INTEGER PRIMARY KEY AUTOINCREMENT,
                 name       TEXT    NOT NULL,
                 start_date TEXT    NOT NULL,
                 notes      TEXT,
                 status     TEXT    NOT NULL DEFAULT 'active'
             );

             -- Carry ids over verbatim; derive status from junction membership.
             INSERT INTO breeding_groups_new (id, name, start_date, notes, status)
                 SELECT bg.id, bg.name, bg.start_date, bg.notes,
                        CASE WHEN EXISTS (
                            SELECT 1 FROM breeding_group_males m WHERE m.group_id = bg.id
                        ) THEN 'active' ELSE 'infertile' END
                 FROM breeding_groups bg;

             DROP TABLE breeding_groups;
             ALTER TABLE breeding_groups_new RENAME TO breeding_groups;

             PRAGMA foreign_keys = ON;",
        )
        .expect("breeding_groups male_id->junction migration failed");
        println!("[migration] normalized breeding_groups (dropped male_id, added status)");
    }

    // Issue #13: permanent housing assignment for adult birds. Distinct from
    // current_brooder_id (live location). Nullable — unhoused birds have NULL.
    if !column_exists(conn, "birds", "housing_id") {
        conn.execute(
            "ALTER TABLE birds ADD COLUMN housing_id INTEGER REFERENCES brooders(id)",
            [],
        )
        .expect("ALTER TABLE birds ADD COLUMN housing_id failed");
        println!("[migration] added birds.housing_id");
    }
    // (birds.chick_group_id is added below, after the chick_groups table
    //  exists — its FK refers to chick_groups, which is created further
    //  down in `init_db`.)

    // System alerts (backup/maintenance scripts -> dashboard bell icon)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS system_alerts (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            alert_key     TEXT    NOT NULL,
            severity      TEXT    NOT NULL,
            title         TEXT    NOT NULL,
            message       TEXT    NOT NULL,
            source        TEXT    NOT NULL,
            created_at    TEXT    NOT NULL,
            resolved_at   TEXT,
            dismissed_at  TEXT,
            metadata_json TEXT
        );
         CREATE INDEX IF NOT EXISTS idx_system_alerts_active
             ON system_alerts(resolved_at, dismissed_at)
             WHERE resolved_at IS NULL AND dismissed_at IS NULL;
         CREATE INDEX IF NOT EXISTS idx_system_alerts_key
             ON system_alerts(alert_key);",
    )
    .expect("failed to create system_alerts table");

    // Chick groups (nursery)
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chick_groups (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            clutch_id     INTEGER REFERENCES clutches(id),
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

    // Issue #14: track which hutch a graduated chick group has moved into.
    // Distinct from brooder_id (the nursery brooder during the chick stage).
    // Nullable — Active groups have NULL; graduated groups stay NULL until
    // assigned to a hutch.
    if !column_exists(conn, "chick_groups", "housing_id") {
        conn.execute(
            "ALTER TABLE chick_groups ADD COLUMN housing_id INTEGER REFERENCES brooders(id)",
            [],
        )
        .expect("ALTER TABLE chick_groups ADD COLUMN housing_id failed");
        println!("[migration] added chick_groups.housing_id");
    }
    // Issue #14: link birds back to the chick group they graduated from, so
    // "assign graduated group → hutch" can find a group's birds. Nullable —
    // existing birds (pre-issue-#14) and birds that weren't created via the
    // graduate flow have NULL. Placed here (after `chick_groups` is created)
    // so the REFERENCES target exists.
    if !column_exists(conn, "birds", "chick_group_id") {
        conn.execute(
            "ALTER TABLE birds ADD COLUMN chick_group_id INTEGER REFERENCES chick_groups(id)",
            [],
        )
        .expect("ALTER TABLE birds ADD COLUMN chick_group_id failed");
        println!("[migration] added birds.chick_group_id");
    }

    // -------------------------------------------------------------------------
    // Many-to-many lineage migration.
    //
    // 1. Create the junction tables (idempotent via IF NOT EXISTS).
    // 2. Backfill from any legacy bloodline_id columns that still exist.
    // 3. Drop the now-redundant bloodline_id columns (SQLite ≥ 3.35).
    //
    // Order matters: the junctions must exist before we INSERT into them, and
    // the source columns must exist when we read from them, so the DROPs happen
    // last.
    // -------------------------------------------------------------------------
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS chick_group_lineages (
            chick_group_id INTEGER NOT NULL REFERENCES chick_groups(id) ON DELETE CASCADE,
            lineage_id     INTEGER NOT NULL REFERENCES lineages(id) ON DELETE RESTRICT,
            PRIMARY KEY (chick_group_id, lineage_id)
        );
         CREATE TABLE IF NOT EXISTS bird_lineages (
            bird_id    INTEGER NOT NULL REFERENCES birds(id) ON DELETE CASCADE,
            lineage_id INTEGER NOT NULL REFERENCES lineages(id) ON DELETE RESTRICT,
            PRIMARY KEY (bird_id, lineage_id)
        );
         CREATE INDEX IF NOT EXISTS idx_chick_group_lineages_lineage
            ON chick_group_lineages(lineage_id);
         CREATE INDEX IF NOT EXISTS idx_bird_lineages_lineage
            ON bird_lineages(lineage_id);",
    )
    .expect("failed to create lineage junction tables");

    // Corrective migration. The original drop-column block used `.ok()` to
    // swallow errors, which hid a SQLite restriction: DROP COLUMN refuses
    // to run when the column is referenced by a secondary index. The
    // chick_groups column had no such index so it dropped cleanly, but
    // birds had `idx_birds_bloodline` from the pre-refactor schema. The
    // result on every live DB created before commit 479e37f: birds still
    // has an orphaned NOT NULL `bloodline_id` column, breaking every
    // INSERT into birds. Fix: drop the blocking index first, then the
    // column, and never swallow the error.
    if column_exists(conn, "chick_groups", "bloodline_id") {
        println!("[migration] chick_groups.bloodline_id column present — backfilling junction + dropping");
        let backfilled = conn
            .execute(
                "INSERT OR IGNORE INTO chick_group_lineages (chick_group_id, lineage_id)
                 SELECT id, bloodline_id FROM chick_groups WHERE bloodline_id IS NOT NULL",
                [],
            )
            .expect("backfill chick_group_lineages from orphan column failed");
        println!("[migration]   backfilled {backfilled} chick_group_lineages row(s)");
        // Defensive: chick_groups didn't ship with a bloodline_id index in
        // the original schema, but drop one if a fork or older build added it.
        conn.execute("DROP INDEX IF EXISTS idx_chick_groups_bloodline", [])
            .expect("drop idx_chick_groups_bloodline failed");
        conn.execute("ALTER TABLE chick_groups DROP COLUMN bloodline_id", [])
            .expect("ALTER TABLE chick_groups DROP COLUMN bloodline_id failed — SQLite >= 3.35 required");
        println!("[migration]   dropped chick_groups.bloodline_id");
    }

    if column_exists(conn, "birds", "bloodline_id") {
        println!("[migration] birds.bloodline_id column present — backfilling junction + dropping");
        let backfilled = conn
            .execute(
                "INSERT OR IGNORE INTO bird_lineages (bird_id, lineage_id)
                 SELECT id, bloodline_id FROM birds WHERE bloodline_id IS NOT NULL",
                [],
            )
            .expect("backfill bird_lineages from orphan column failed");
        println!("[migration]   backfilled {backfilled} bird_lineages row(s)");
        // The blocker — must be dropped before DROP COLUMN can succeed.
        conn.execute("DROP INDEX IF EXISTS idx_birds_bloodline", [])
            .expect("drop idx_birds_bloodline failed");
        println!("[migration]   dropped idx_birds_bloodline (was blocking column drop)");
        conn.execute("ALTER TABLE birds DROP COLUMN bloodline_id", [])
            .expect("ALTER TABLE birds DROP COLUMN bloodline_id failed — SQLite >= 3.35 required");
        println!("[migration]   dropped birds.bloodline_id");
    }

    // --- App settings (key/value config). The cull-mode guardrail reads
    //     `desired_males_per_group` and `max_females_per_male` from here;
    //     defaults are seeded on first run and editable via PUT /api/settings.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )
    .expect("failed to create settings table");
    // INSERT OR IGNORE so existing rows (user-edited values) aren't clobbered
    // on every server restart, but missing keys still get a sane default.
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value) VALUES ('desired_males_per_group', '1')",
        [],
    )
    .ok();
    conn.execute(
        &format!(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('max_females_per_male', '{}')",
            MAX_FEMALES_PER_MALE
        ),
        [],
    )
    .ok();

    // --- System settings (server-owned lifecycle + alert thresholds) ---
    // Key/value rows; the typed view + parsing live in quailsync_common::Settings.
    // Seeded once with INSERT OR IGNORE so user edits survive restarts and only
    // missing keys ever get a default. brooder_week_temps_f is a JSON array.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS system_settings (
            key        TEXT PRIMARY KEY,
            value      TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .expect("failed to create system_settings table");
    for (key, value) in [
        ("alert_temp_min_f", "68.0"),
        ("alert_temp_max_f", "72.0"),
        ("alert_humidity_min", "40.0"),
        ("alert_humidity_max", "60.0"),
        ("adult_temp_min_f", "65.0"),
        ("adult_temp_max_f", "75.0"),
        ("incubation_days", "17"),
        ("ready_to_transition_age_days", "35"),
        ("butcher_weight_grams", "250.0"),
        ("min_breeding_weight_grams", "200.0"),
        ("sensor_stale_seconds", "15"),
        ("brooder_week_temps_f", "[97,92,87,82,77,72]"),
    ] {
        conn.execute(
            "INSERT OR IGNORE INTO system_settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .ok();
    }

    // --- Headcount inference results ---
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS headcounts (
            id         INTEGER PRIMARY KEY AUTOINCREMENT,
            brooder_id INTEGER NOT NULL REFERENCES brooders(id),
            count      INTEGER NOT NULL,
            timestamp  TEXT    NOT NULL DEFAULT (datetime('now')),
            received_at TEXT   NOT NULL DEFAULT (datetime('now'))
        );",
    )
    .expect("failed to create headcounts table");

    // --- Govee H5179 WiFi sensors ---
    // Commercial replacement for the DIY ESP32 sensors. Sensors auto-register
    // on first reading (govee_device_id is the natural key) and are movable
    // between brooders/hutches via sensor_assignments. The partial unique index
    // enforces "at most one active (unassigned_at IS NULL) assignment per
    // sensor" — re-assigning closes the old row before opening a new one.
    // govee_sensors is created first so the FKs below resolve.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS govee_sensors (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            govee_device_id TEXT    UNIQUE NOT NULL,
            name            TEXT,
            model           TEXT,
            first_seen      TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_seen       TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS sensor_assignments (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            govee_sensor_id INTEGER NOT NULL REFERENCES govee_sensors(id),
            brooder_id      INTEGER NOT NULL REFERENCES brooders(id),
            assigned_at     TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
            unassigned_at   TEXT
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_sensor_active_assignment
            ON sensor_assignments(govee_sensor_id)
            WHERE unassigned_at IS NULL;

        CREATE TABLE IF NOT EXISTS govee_readings (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            govee_sensor_id INTEGER NOT NULL REFERENCES govee_sensors(id),
            temperature_f   REAL    NOT NULL,
            humidity        REAL    NOT NULL,
            recorded_at     TEXT    NOT NULL,
            created_at      TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_govee_readings_sensor
            ON govee_readings(govee_sensor_id, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_sensor_assignments_brooder
            ON sensor_assignments(brooder_id, unassigned_at);",
    )
    .expect("failed to create govee sensor tables");

    // --- SPYPOINT trail cameras ---
    // Mirrors the Govee sensor tables. Cameras auto-register on first sight
    // (spypoint_camera_id is the natural key) and are movable between
    // brooders/hutches via camera_assignments. The partial unique index enforces
    // "at most one active (unassigned_at IS NULL) assignment per camera" —
    // re-assigning closes the old row before opening a new one. trail_cameras is
    // created first so the FK below resolves.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trail_cameras (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            spypoint_camera_id TEXT    UNIQUE NOT NULL,
            name               TEXT,
            model              TEXT,
            first_seen         TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
            last_seen          TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS camera_assignments (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            trail_camera_id INTEGER NOT NULL REFERENCES trail_cameras(id),
            brooder_id      INTEGER NOT NULL REFERENCES brooders(id),
            assigned_at     TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP,
            unassigned_at   TEXT
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_camera_active_assignment
            ON camera_assignments(trail_camera_id)
            WHERE unassigned_at IS NULL;

        CREATE INDEX IF NOT EXISTS idx_camera_assignments_brooder
            ON camera_assignments(brooder_id, unassigned_at);",
    )
    .expect("failed to create trail camera tables");

    // --- Trail-cam observations ---
    // One row per processed photo, moved off the legacy
    // processed/observations.jsonl into SQLite. The bridge POSTs to
    // /api/trailcam/observation (with a JSONL write-ahead log fallback if the
    // API is down). The (camera_id, timestamp) index makes latest-per-camera
    // and history-window queries fast.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trail_cam_observations (
            id                       INTEGER PRIMARY KEY AUTOINCREMENT,
            camera_id                TEXT    NOT NULL,
            timestamp                TEXT,
            bird_count               INTEGER NOT NULL DEFAULT 0,
            average_confidence       REAL,
            min_confidence           REAL,
            detections               TEXT,
            inference_time_ms        REAL,
            image_filename           TEXT,
            annotated_image_filename TEXT,
            created_at               TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_trail_cam_obs_camera_ts
            ON trail_cam_observations(camera_id, timestamp);",
    )
    .expect("failed to create trail_cam_observations table");

    // --- Performance indexes ---
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_readings_brooder_received ON brooder_readings(brooder_id, received_at);
         CREATE INDEX IF NOT EXISTS idx_readings_received ON brooder_readings(received_at);
         CREATE INDEX IF NOT EXISTS idx_system_metrics_received ON system_metrics(received_at);
         CREATE INDEX IF NOT EXISTS idx_birds_status ON birds(status);
         CREATE INDEX IF NOT EXISTS idx_birds_nfc ON birds(nfc_tag_id);
         CREATE INDEX IF NOT EXISTS idx_birds_brooder ON birds(current_brooder_id);
         CREATE INDEX IF NOT EXISTS idx_birds_housing ON birds(housing_id);
         CREATE INDEX IF NOT EXISTS idx_weights_bird_date ON weight_records(bird_id, date);
         CREATE INDEX IF NOT EXISTS idx_processing_status ON processing_records(status);
         CREATE INDEX IF NOT EXISTS idx_chick_groups_brooder ON chick_groups(brooder_id, status);
         CREATE INDEX IF NOT EXISTS idx_chick_groups_housing ON chick_groups(housing_id);
         CREATE INDEX IF NOT EXISTS idx_birds_chick_group ON birds(chick_group_id);
         CREATE INDEX IF NOT EXISTS idx_alerts_timestamp ON alerts(timestamp);
         CREATE INDEX IF NOT EXISTS idx_headcounts_brooder ON headcounts(brooder_id, received_at);",
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
                params![
                    r.temperature_f,
                    r.humidity_percent,
                    r.timestamp.to_rfc3339(),
                    r.brooder_id
                ],
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
        TelemetryPayload::QrDetected(qr) => {
            conn.execute(
                "UPDATE brooders SET qr_code = ?1 WHERE id = ?2",
                params![qr.qr_code, qr.brooder_id],
            )
            .ok();
            println!(
                "[qr] Updated brooder {} qr_code={} lineage={}",
                qr.brooder_id, qr.qr_code, qr.lineage
            );
        }
    }
}

pub fn store_alert(conn: &Connection, severity: &Severity, message: &str) {
    let sev_str = match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Critical => "critical",
    };
    conn.execute(
        "INSERT INTO alerts (severity, message) VALUES (?1, ?2)",
        (sev_str, message),
    )
    .ok();
}
