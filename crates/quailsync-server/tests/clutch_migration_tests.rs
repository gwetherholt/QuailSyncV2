//! Verifies the additive clutches migration on a *pre-feature* database — the
//! exact path the live Pi DB takes on deploy: add `breeding_group_id`, drop the
//! dead `breeding_pair_id` column (via table rebuild), drop the empty
//! `breeding_pairs` table, and keep every existing clutch row intact.

use quailsync_server::init_db;
use rusqlite::Connection;

fn columns(conn: &Connection, table: &str) -> Vec<String> {
    conn.prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap()
        > 0
}

#[test]
fn old_clutches_migrate_to_group_and_drop_pair_keeping_rows() {
    let conn = Connection::open_in_memory().unwrap();

    // Simulate an OLD database: the dead breeding_pairs table + a clutches table
    // with breeding_pair_id and NO breeding_group_id, plus a couple of rows.
    conn.execute_batch(
        "CREATE TABLE breeding_pairs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            male_id INTEGER, female_id INTEGER, start_date TEXT, end_date TEXT, notes TEXT
        );
        CREATE TABLE clutches (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            breeding_pair_id    INTEGER REFERENCES breeding_pairs(id),
            lineage_id          INTEGER,
            eggs_set            INTEGER NOT NULL,
            eggs_fertile        INTEGER,
            eggs_hatched        INTEGER,
            set_date            TEXT NOT NULL,
            expected_hatch_date TEXT NOT NULL,
            status              TEXT NOT NULL DEFAULT 'Incubating',
            notes               TEXT,
            eggs_stillborn      INTEGER,
            eggs_quit           INTEGER,
            eggs_infertile      INTEGER,
            eggs_damaged        INTEGER,
            hatch_notes         TEXT
        );
        INSERT INTO clutches (id, breeding_pair_id, lineage_id, eggs_set, eggs_hatched, set_date, expected_hatch_date, status, notes)
            VALUES (7, NULL, 3, 24, 20, '2026-06-01', '2026-06-18', 'Hatched', 'legacy clutch');",
    )
    .unwrap();

    // Run the real schema init/migration.
    init_db(&conn);

    // Column swapped: breeding_group_id added, breeding_pair_id gone.
    let cols = columns(&conn, "clutches");
    assert!(
        cols.iter().any(|c| c == "breeding_group_id"),
        "breeding_group_id missing: {cols:?}"
    );
    assert!(
        !cols.iter().any(|c| c == "breeding_pair_id"),
        "breeding_pair_id should be dropped: {cols:?}"
    );

    // The dead breeding_pairs table is gone.
    assert!(!table_exists(&conn, "breeding_pairs"));

    // The pre-existing clutch row survived intact (same id + data), with a NULL group.
    let (id, eggs, hatched, lineage, status, group_id): (i64, i64, Option<i64>, Option<i64>, String, Option<i64>) =
        conn.query_row(
            "SELECT id, eggs_set, eggs_hatched, lineage_id, status, breeding_group_id FROM clutches WHERE id = 7",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .unwrap();
    assert_eq!(id, 7);
    assert_eq!(eggs, 24);
    assert_eq!(hatched, Some(20));
    assert_eq!(lineage, Some(3));
    assert_eq!(status, "Hatched");
    assert_eq!(group_id, None);

    // Migration is idempotent — a second run is a no-op, not an error.
    init_db(&conn);
    assert!(!table_exists(&conn, "breeding_pairs"));
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM clutches", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
