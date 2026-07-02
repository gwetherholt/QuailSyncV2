use chrono::NaiveDate;
use quailsync_common::*;
use rusqlite::{params, Connection};

// ---------------------------------------------------------------------------
// Enum conversion helpers
// ---------------------------------------------------------------------------

pub fn sex_to_str(s: &Sex) -> &'static str {
    match s {
        Sex::Male => "Male",
        Sex::Female => "Female",
        Sex::Unknown => "Unknown",
    }
}

pub fn str_to_sex(s: &str) -> Sex {
    match s {
        "Male" => Sex::Male,
        "Female" => Sex::Female,
        _ => Sex::Unknown,
    }
}

pub fn bird_status_to_str(s: &BirdStatus) -> &'static str {
    match s {
        BirdStatus::Active => "Active",
        BirdStatus::Culled => "Culled",
        BirdStatus::Deceased => "Deceased",
        BirdStatus::Sold => "Sold",
    }
}

pub fn str_to_bird_status(s: &str) -> BirdStatus {
    match s {
        "Culled" => BirdStatus::Culled,
        "Deceased" => BirdStatus::Deceased,
        "Sold" => BirdStatus::Sold,
        _ => BirdStatus::Active,
    }
}

pub fn clutch_status_to_str(s: &ClutchStatus) -> &'static str {
    match s {
        ClutchStatus::Incubating => "Incubating",
        ClutchStatus::Hatched => "Hatched",
        ClutchStatus::Failed => "Failed",
    }
}

pub fn str_to_clutch_status(s: &str) -> ClutchStatus {
    match s {
        "Hatched" => ClutchStatus::Hatched,
        "Failed" => ClutchStatus::Failed,
        _ => ClutchStatus::Incubating,
    }
}

pub fn processing_reason_to_str(r: &ProcessingReason) -> &'static str {
    match r {
        ProcessingReason::ExcessMale => "ExcessMale",
        ProcessingReason::LowWeight => "LowWeight",
        ProcessingReason::PoorGenetics => "PoorGenetics",
        ProcessingReason::Age => "Age",
        ProcessingReason::Other => "Other",
    }
}

pub fn str_to_processing_reason(s: &str) -> ProcessingReason {
    match s {
        "ExcessMale" => ProcessingReason::ExcessMale,
        "LowWeight" => ProcessingReason::LowWeight,
        "PoorGenetics" => ProcessingReason::PoorGenetics,
        "Age" => ProcessingReason::Age,
        _ => ProcessingReason::Other,
    }
}

pub fn processing_status_to_str(s: &ProcessingStatus) -> &'static str {
    match s {
        ProcessingStatus::Scheduled => "Scheduled",
        ProcessingStatus::Completed => "Completed",
        ProcessingStatus::Cancelled => "Cancelled",
    }
}

pub fn str_to_processing_status(s: &str) -> ProcessingStatus {
    match s {
        "Completed" => ProcessingStatus::Completed,
        "Cancelled" => ProcessingStatus::Cancelled,
        _ => ProcessingStatus::Scheduled,
    }
}

pub fn camera_status_to_str(s: &CameraStatus) -> &'static str {
    match s {
        CameraStatus::Active => "Active",
        CameraStatus::Offline => "Offline",
    }
}

pub fn str_to_camera_status(s: &str) -> CameraStatus {
    match s {
        "Offline" => CameraStatus::Offline,
        _ => CameraStatus::Active,
    }
}

pub fn life_stage_to_str(s: &LifeStage) -> &'static str {
    match s {
        LifeStage::Chick => "Chick",
        LifeStage::Adolescent => "Adolescent",
        LifeStage::Adult => "Adult",
    }
}

pub fn str_to_life_stage(s: &str) -> LifeStage {
    match s {
        "Chick" => LifeStage::Chick,
        "Adolescent" => LifeStage::Adolescent,
        _ => LifeStage::Adult,
    }
}

pub fn housing_type_to_str(h: &HousingType) -> &'static str {
    match h {
        HousingType::Incubator => "incubator",
        HousingType::Brooder => "brooder",
        HousingType::Hutch => "hutch",
    }
}

pub fn str_to_housing_type(s: &str) -> HousingType {
    match s {
        "incubator" => HousingType::Incubator,
        "hutch" => HousingType::Hutch,
        _ => HousingType::Brooder,
    }
}

pub fn str_to_chick_group_status(s: &str) -> ChickGroupStatus {
    match s {
        "Graduated" => ChickGroupStatus::Graduated,
        "Lost" => ChickGroupStatus::Lost,
        _ => ChickGroupStatus::Active,
    }
}

// ---------------------------------------------------------------------------
// Row-mapping helpers (DRY — replaces 7+ duplicated Bird-from-row blocks, etc.)
// ---------------------------------------------------------------------------

/// Maps a `birds` row produced by `BIRD_SELECT` (id, band_color, sex,
/// hatch_date, mother_id, father_id, generation, status, notes, nfc_tag_id,
/// current_brooder_id, photo_path, photo_uploaded_at, housing_id,
/// chick_group_id). The `lineages` field is left empty — callers populate it
/// via `fetch_bird_lineages`.
pub fn row_to_bird(row: &rusqlite::Row) -> rusqlite::Result<Bird> {
    let sex_str: String = row.get(2)?;
    let hatch_str: String = row.get(3)?;
    let status_str: String = row.get(7)?;
    Ok(Bird {
        id: row.get(0)?,
        band_color: row.get(1)?,
        sex: str_to_sex(&sex_str),
        hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
        mother_id: row.get(4)?,
        father_id: row.get(5)?,
        generation: row.get(6)?,
        status: str_to_bird_status(&status_str),
        notes: row.get(8)?,
        nfc_tag_id: row.get(9)?,
        current_brooder_id: row.get(10)?,
        photo_path: row.get(11)?,
        photo_uploaded_at: row.get(12)?,
        housing_id: row.get(13)?,
        chick_group_id: row.get(14)?,
        // Derived fields left empty here; callers fill them via `hydrate_bird`.
        lineages: Vec::new(),
        genetic_profile: Default::default(),
        confidence: 0.0,
    })
}

/// Maps a `clutches` row produced by `CLUTCH_SELECT` (columns in this exact
/// order): id, breeding_group_id, breeding_group_name (JOINed), lineage_id,
/// eggs_set, eggs_fertile, eggs_hatched, set_date, expected_hatch_date, status,
/// notes, eggs_stillborn, eggs_quit, eggs_infertile, eggs_damaged, hatch_notes.
pub fn row_to_clutch(row: &rusqlite::Row) -> rusqlite::Result<Clutch> {
    let set_str: String = row.get(7)?;
    let exp_str: String = row.get(8)?;
    let status_str: String = row.get(9)?;
    Ok(Clutch {
        id: row.get(0)?,
        breeding_group_id: row.get(1)?,
        breeding_group_name: row.get(2)?,
        lineage_id: row.get(3)?,
        eggs_set: row.get(4)?,
        eggs_fertile: row.get(5)?,
        eggs_hatched: row.get(6)?,
        set_date: NaiveDate::parse_from_str(&set_str, "%Y-%m-%d").unwrap_or_default(),
        expected_hatch_date: NaiveDate::parse_from_str(&exp_str, "%Y-%m-%d").unwrap_or_default(),
        status: str_to_clutch_status(&status_str),
        notes: row.get(10)?,
        eggs_stillborn: row.get(11)?,
        eggs_quit: row.get(12)?,
        eggs_infertile: row.get(13)?,
        eggs_damaged: row.get(14)?,
        hatch_notes: row.get(15)?,
    })
}

/// Maps a `chick_groups` row produced by `GROUP_SELECT` (id, clutch_id,
/// brooder_id, initial_count, current_count, hatch_date, status, notes,
/// housing_id). The `lineages` field is left empty — callers populate it via
/// `fetch_chick_group_lineages`.
pub fn row_to_chick_group(row: &rusqlite::Row) -> rusqlite::Result<ChickGroup> {
    let hatch_str: String = row.get(5)?;
    let status_str: String = row.get(6)?;
    let mut group = ChickGroup {
        id: row.get(0)?,
        clutch_id: row.get(1)?,
        brooder_id: row.get(2)?,
        initial_count: row.get(3)?,
        current_count: row.get(4)?,
        hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
        status: str_to_chick_group_status(&status_str),
        notes: row.get(7)?,
        housing_id: row.get(8)?,
        is_ready_to_transition: false,
        lineages: Vec::new(),
    };
    group.is_ready_to_transition = group.compute_is_ready_to_transition();
    Ok(group)
}

/// Maps a `brooders` row produced by `BROODER_SELECT` (id, name, lineage_id,
/// life_stage, qr_code, notes, camera_url, housing_type). The 8th column is
/// always present after the housing-type migration.
pub fn row_to_brooder(row: &rusqlite::Row) -> rusqlite::Result<Brooder> {
    let stage_str: String = row.get(3)?;
    let housing_str: String = row.get(7)?;
    Ok(Brooder {
        id: row.get(0)?,
        name: row.get(1)?,
        lineage_id: row.get(2)?,
        life_stage: str_to_life_stage(&stage_str),
        qr_code: row.get(4)?,
        notes: row.get(5)?,
        camera_url: row.get(6)?,
        housing_type: str_to_housing_type(&housing_str),
    })
}

// ---------------------------------------------------------------------------
// Lineage junction lookups
// ---------------------------------------------------------------------------

/// Load the lineages attached to a single chick group via the junction table.
pub fn fetch_chick_group_lineages(conn: &Connection, group_id: i64) -> Vec<Lineage> {
    let mut stmt = match conn.prepare(
        "SELECT l.id, l.name, l.source, l.notes
         FROM lineages l
         JOIN chick_group_lineages cgl ON cgl.lineage_id = l.id
         WHERE cgl.chick_group_id = ?1
         ORDER BY l.id",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![group_id], |row| {
        Ok(Lineage {
            id: row.get(0)?,
            name: row.get(1)?,
            source: row.get(2)?,
            notes: row.get(3)?,
        })
    })
    .map(|it| it.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Load the lineages attached to a single bird via the junction table.
pub fn fetch_bird_lineages(conn: &Connection, bird_id: i64) -> Vec<Lineage> {
    let mut stmt = match conn.prepare(
        "SELECT l.id, l.name, l.source, l.notes
         FROM lineages l
         JOIN bird_lineages bl ON bl.lineage_id = l.id
         WHERE bl.bird_id = ?1
         ORDER BY l.id",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map(params![bird_id], |row| {
        Ok(Lineage {
            id: row.get(0)?,
            name: row.get(1)?,
            source: row.get(2)?,
            notes: row.get(3)?,
        })
    })
    .map(|it| it.filter_map(|r| r.ok()).collect())
    .unwrap_or_default()
}

/// Populate the `lineages` field on every chick group in the slice.
pub fn populate_chick_group_lineages(conn: &Connection, groups: &mut [ChickGroup]) {
    for g in groups.iter_mut() {
        g.lineages = fetch_chick_group_lineages(conn, g.id);
    }
}

/// Fill a single `Bird`'s response-only derived fields: its discrete lineage
/// tags plus its probabilistic genetic profile + confidence (Phase 3). Use this
/// instead of setting `.lineages` alone so every bird response carries genetics.
pub fn hydrate_bird(conn: &Connection, bird: &mut Bird) {
    bird.lineages = fetch_bird_lineages(conn, bird.id);
    let profile = crate::genetics::read_profile(conn, bird.id);
    bird.confidence = crate::genetics::confidence(&profile);
    bird.genetic_profile = profile;
}

/// Populate the derived fields (lineages + genetic profile + confidence) on
/// every bird in the slice.
pub fn populate_bird_lineages(conn: &Connection, birds: &mut [Bird]) {
    for b in birds.iter_mut() {
        hydrate_bird(conn, b);
    }
}

/// Replace a chick group's lineage set atomically. Returns Err on any DB error
/// or if the group does not exist. Validates non-empty input at the call site.
pub fn replace_chick_group_lineages(
    conn: &Connection,
    group_id: i64,
    lineage_ids: &[i64],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM chick_group_lineages WHERE chick_group_id = ?1",
        params![group_id],
    )?;
    for lid in lineage_ids {
        conn.execute(
            "INSERT INTO chick_group_lineages (chick_group_id, lineage_id) VALUES (?1, ?2)",
            params![group_id, lid],
        )?;
    }
    Ok(())
}

/// Replace a bird's lineage set atomically.
pub fn replace_bird_lineages(
    conn: &Connection,
    bird_id: i64,
    lineage_ids: &[i64],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM bird_lineages WHERE bird_id = ?1",
        params![bird_id],
    )?;
    for lid in lineage_ids {
        conn.execute(
            "INSERT INTO bird_lineages (bird_id, lineage_id) VALUES (?1, ?2)",
            params![bird_id, lid],
        )?;
    }
    Ok(())
}

/// Clear `nfc_tag_id` from every bird (optionally except `except_id`) that
/// currently has the given tag. Used by `create_bird`, `update_bird`, and
/// the graduate handler to make NFC tag reassignment seamless during batch
/// graduation — without this, the `UNIQUE` constraint on `birds.nfc_tag_id`
/// would 500 on every re-program of a tag that already belonged to another
/// bird.
///
/// Logs the reassignment so re-pairings are auditable from the server log.
/// Errors during the query/update are swallowed: we'd rather let the
/// subsequent INSERT/UPDATE surface a constraint error than blow up the
/// caller on a transient SELECT failure.
pub fn clear_nfc_tag_from_others(conn: &Connection, tag_id: &str, except_id: Option<i64>) {
    // Look up the prior owner (if any) so we can name it in the log line.
    // The query is also the existence check — if no other bird holds the
    // tag, we have nothing to do and skip the UPDATE entirely.
    let prior_owner: Option<i64> = match except_id {
        Some(id) => conn
            .query_row(
                "SELECT id FROM birds WHERE nfc_tag_id = ?1 AND id != ?2",
                params![tag_id, id],
                |row| row.get(0),
            )
            .ok(),
        None => conn
            .query_row(
                "SELECT id FROM birds WHERE nfc_tag_id = ?1",
                params![tag_id],
                |row| row.get(0),
            )
            .ok(),
    };
    let Some(old_id) = prior_owner else { return };
    let cleared = match except_id {
        Some(id) => conn.execute(
            "UPDATE birds SET nfc_tag_id = NULL WHERE nfc_tag_id = ?1 AND id != ?2",
            params![tag_id, id],
        ),
        None => conn.execute(
            "UPDATE birds SET nfc_tag_id = NULL WHERE nfc_tag_id = ?1",
            params![tag_id],
        ),
    };
    if cleared.is_ok() {
        match except_id {
            Some(new_id) => {
                println!("[nfc] tag {tag_id} reassigned from bird {old_id} to bird {new_id}")
            }
            None => println!("[nfc] tag {tag_id} reassigned from bird {old_id} to new bird"),
        }
    }
}

pub fn row_to_processing_record(row: &rusqlite::Row) -> rusqlite::Result<ProcessingRecord> {
    let reason_str: String = row.get(2)?;
    let sched_str: String = row.get(3)?;
    let proc_str: Option<String> = row.get(4)?;
    let status_str: String = row.get(6)?;
    Ok(ProcessingRecord {
        id: row.get(0)?,
        bird_id: row.get(1)?,
        reason: str_to_processing_reason(&reason_str),
        scheduled_date: NaiveDate::parse_from_str(&sched_str, "%Y-%m-%d").unwrap_or_default(),
        processed_date: proc_str.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        final_weight_grams: row.get(5)?,
        status: str_to_processing_status(&status_str),
        notes: row.get(7)?,
    })
}
