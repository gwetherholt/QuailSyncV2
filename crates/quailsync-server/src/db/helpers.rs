use chrono::NaiveDate;
use quailsync_common::*;

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

pub fn row_to_bird(row: &rusqlite::Row) -> rusqlite::Result<Bird> {
    let sex_str: String = row.get(2)?;
    let hatch_str: String = row.get(4)?;
    let status_str: String = row.get(8)?;
    Ok(Bird {
        id: row.get(0)?,
        band_color: row.get(1)?,
        sex: str_to_sex(&sex_str),
        bloodline_id: row.get(3)?,
        hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
        mother_id: row.get(5)?,
        father_id: row.get(6)?,
        generation: row.get(7)?,
        status: str_to_bird_status(&status_str),
        notes: row.get(9)?,
        nfc_tag_id: row.get(10)?,
        current_brooder_id: row.get(11)?,
    })
}

pub fn row_to_clutch(row: &rusqlite::Row) -> rusqlite::Result<Clutch> {
    let set_str: String = row.get(6)?;
    let exp_str: String = row.get(7)?;
    let status_str: String = row.get(8)?;
    Ok(Clutch {
        id: row.get(0)?,
        breeding_pair_id: row.get(1)?,
        bloodline_id: row.get(2)?,
        eggs_set: row.get(3)?,
        eggs_fertile: row.get(4)?,
        eggs_hatched: row.get(5)?,
        set_date: NaiveDate::parse_from_str(&set_str, "%Y-%m-%d").unwrap_or_default(),
        expected_hatch_date: NaiveDate::parse_from_str(&exp_str, "%Y-%m-%d").unwrap_or_default(),
        status: str_to_clutch_status(&status_str),
        notes: row.get(9)?,
        eggs_stillborn: row.get(10)?,
        eggs_quit: row.get(11)?,
        eggs_infertile: row.get(12)?,
        eggs_damaged: row.get(13)?,
        hatch_notes: row.get(14)?,
    })
}

pub fn row_to_chick_group(row: &rusqlite::Row) -> rusqlite::Result<ChickGroup> {
    let hatch_str: String = row.get(6)?;
    let status_str: String = row.get(7)?;
    Ok(ChickGroup {
        id: row.get(0)?,
        clutch_id: row.get(1)?,
        bloodline_id: row.get(2)?,
        brooder_id: row.get(3)?,
        initial_count: row.get(4)?,
        current_count: row.get(5)?,
        hatch_date: NaiveDate::parse_from_str(&hatch_str, "%Y-%m-%d").unwrap_or_default(),
        status: str_to_chick_group_status(&status_str),
        notes: row.get(8)?,
    })
}

pub fn row_to_brooder(row: &rusqlite::Row) -> rusqlite::Result<Brooder> {
    let stage_str: String = row.get(3)?;
    Ok(Brooder {
        id: row.get(0)?,
        name: row.get(1)?,
        bloodline_id: row.get(2)?,
        life_stage: str_to_life_stage(&stage_str),
        qr_code: row.get(4)?,
        notes: row.get(5)?,
        camera_url: row.get(6)?,
    })
}

pub fn row_to_breeding_pair(row: &rusqlite::Row) -> rusqlite::Result<BreedingPair> {
    let start_str: String = row.get(3)?;
    let end_str: Option<String> = row.get(4)?;
    Ok(BreedingPair {
        id: row.get(0)?,
        male_id: row.get(1)?,
        female_id: row.get(2)?,
        start_date: NaiveDate::parse_from_str(&start_str, "%Y-%m-%d").unwrap_or_default(),
        end_date: end_str.and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok()),
        notes: row.get(5)?,
    })
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
