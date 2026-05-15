use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

/// System-level resource metrics collected from an agent node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage_percent: f64,
    pub memory_used_bytes: u64,
    pub memory_total_bytes: u64,
    pub disk_used_bytes: u64,
    pub disk_total_bytes: u64,
    pub uptime_seconds: u64,
}

/// A single reading from a brooder's environmental sensors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrooderReading {
    pub temperature_f: f64,
    pub humidity_percent: f64,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub brooder_id: Option<i64>,
}

/// A species detected by the monitoring system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Species {
    BobwhiteQuail,
    CoturnixQuail,
    Unknown(String),
}

/// A wildlife detection event with classification confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionEvent {
    pub species: Species,
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
}

/// Camera auto-registration announcement from Pi agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraAnnounce {
    pub brooder_id: i64,
    pub stream_url: String,
    #[serde(default)]
    pub snapshot_url: Option<String>,
}

/// QR code detection event from Pi agent camera.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrDetected {
    pub brooder_id: i64,
    #[serde(alias = "bloodline")] // back-compat for already-printed QR labels
    pub lineage: String,
    pub qr_code: String,
}

/// Top-level telemetry payload sent from agent to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryPayload {
    System(SystemMetrics),
    Brooder(BrooderReading),
    Detection(DetectionEvent),
    CameraAnnounce(CameraAnnounce),
    QrDetected(QrDetected),
}

/// Configurable thresholds for brooder alerts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub brooder_temp_min: f64,
    pub brooder_temp_max: f64,
    pub humidity_min: f64,
    pub humidity_max: f64,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            brooder_temp_min: 68.0,
            brooder_temp_max: 72.0,
            humidity_min: 40.0,
            humidity_max: 60.0,
        }
    }
}

/// Severity level for an alert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// An alert generated when a reading is out of range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub severity: Severity,
    pub message: String,
    pub timestamp: String,
}

// =========================================================================
// System alerts (backup / maintenance / pi-script failures)
//
// Distinct from the brooder `Alert` above — these flow from cron/maintenance
// scripts on the Pi into the QuailSync server and surface in the Android app.
// `severity` is a raw lowercase string ("critical"|"warning"|"info") rather
// than the `Severity` enum because the script-side payload uses that form.
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemAlert {
    pub id: i64,
    pub alert_key: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub source: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
    pub dismissed_at: Option<String>,
    pub metadata_json: Option<String>,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSystemAlert {
    pub alert_key: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    pub source: String,
    #[serde(default)]
    pub metadata_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSystemAlertRequest {
    pub alert_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSystemAlertResponse {
    pub resolved: i64,
}

// =========================================================================
// Flock & Lineage types
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sex {
    Male,
    Female,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BirdStatus {
    Active,
    Culled,
    Deceased,
    Sold,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClutchStatus {
    Incubating,
    Hatched,
    Failed,
}

// --- Model structs (server responses, include id) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lineage {
    pub id: i64,
    pub name: String,
    pub source: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bird {
    pub id: i64,
    pub band_color: Option<String>,
    pub sex: Sex,
    pub hatch_date: NaiveDate,
    pub mother_id: Option<i64>,
    pub father_id: Option<i64>,
    pub generation: u32,
    pub status: BirdStatus,
    pub notes: Option<String>,
    #[serde(default)]
    pub nfc_tag_id: Option<String>,
    #[serde(default)]
    pub current_brooder_id: Option<i64>,
    #[serde(default)]
    pub photo_path: Option<String>,
    /// Permanent housing assignment for adult birds (issue #13). Distinct
    /// from `current_brooder_id`, which tracks the bird's *current* physical
    /// location during the chick/adolescent stages. `None` for unhoused
    /// birds — chick-stage birds remain unhoused; their location is derived
    /// from the chick group's `brooder_id`.
    #[serde(default)]
    pub housing_id: Option<i64>,
    /// The chick group this bird graduated from (issue #14). Populated by
    /// the graduate handler; lets "assign graduated group → hutch" find all
    /// birds of a group later. `None` for legacy birds and any bird not
    /// produced via the graduate flow.
    #[serde(default)]
    pub chick_group_id: Option<i64>,
    /// Many-to-many lineage tags. Populated from the `bird_lineages`
    /// junction table; empty Vec is allowed (legacy migration only).
    #[serde(default)]
    pub lineages: Vec<Lineage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreedingPair {
    pub id: i64,
    pub male_id: i64,
    pub female_id: i64,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clutch {
    pub id: i64,
    pub breeding_pair_id: Option<i64>,
    pub lineage_id: Option<i64>,
    pub eggs_set: u32,
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub set_date: NaiveDate,
    pub expected_hatch_date: NaiveDate,
    pub status: ClutchStatus,
    pub notes: Option<String>,
    pub eggs_stillborn: Option<u32>,
    pub eggs_quit: Option<u32>,
    pub eggs_infertile: Option<u32>,
    pub eggs_damaged: Option<u32>,
    pub hatch_notes: Option<String>,
}

// --- Create structs (POST bodies, no id) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLineage {
    pub name: String,
    pub source: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBird {
    pub band_color: Option<String>,
    pub sex: Sex,
    pub hatch_date: NaiveDate,
    pub mother_id: Option<i64>,
    pub father_id: Option<i64>,
    pub generation: u32,
    pub status: BirdStatus,
    pub notes: Option<String>,
    #[serde(default)]
    pub nfc_tag_id: Option<String>,
    /// Optional back-link to the chick group this bird came from (issue #14).
    /// The Android batch-banding flow creates birds via POST /api/birds (not
    /// /graduate) — passing chick_group_id here keeps the relationship intact
    /// so "Assign Graduated Group → hutch" can later find the group's birds.
    #[serde(default)]
    pub chick_group_id: Option<i64>,
    /// One or more lineage IDs; must be non-empty (validated at handler level).
    pub lineage_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBreedingPair {
    pub male_id: i64,
    pub female_id: i64,
    pub start_date: NaiveDate,
    pub end_date: Option<NaiveDate>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateClutch {
    pub breeding_pair_id: Option<i64>,
    pub lineage_id: Option<i64>,
    pub eggs_set: u32,
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub set_date: NaiveDate,
    pub status: ClutchStatus,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBird {
    pub status: Option<BirdStatus>,
    pub notes: Option<String>,
    pub nfc_tag_id: Option<String>,
    /// Newly-editable post-banding fields. Each field is independently
    /// optional — missing means "leave unchanged"; `Some("")` is treated
    /// as the literal empty value (caller decides whether that's allowed).
    #[serde(default)]
    pub band_color: Option<String>,
    #[serde(default)]
    pub sex: Option<Sex>,
    #[serde(default)]
    pub hatch_date: Option<NaiveDate>,
    /// Issue #13: set a permanent housing assignment for an adult bird. `None`
    /// here means "leave unchanged" (NOT "clear"); use the dedicated
    /// `POST /api/brooders/{id}/unassign-birds` endpoint to clear a housing
    /// assignment. This avoids the JSON serde double-Option mess for a single
    /// rarely-used semantic.
    #[serde(default)]
    pub housing_id: Option<i64>,
}

/// Body for `POST /api/brooders/{id}/assign-birds` and `/unassign-birds`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirdAssignmentRequest {
    pub bird_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BirdAssignmentResponse {
    pub updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateClutch {
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub status: Option<ClutchStatus>,
    pub notes: Option<String>,
    pub set_date: Option<NaiveDate>,
    pub eggs_stillborn: Option<u32>,
    pub eggs_quit: Option<u32>,
    pub eggs_infertile: Option<u32>,
    pub eggs_damaged: Option<u32>,
    pub hatch_notes: Option<String>,
}

// =========================================================================
// Lifecycle constants
// =========================================================================

pub const COTURNIX_BUTCHER_WEIGHT_GRAMS: f64 = 250.0;
pub const COTURNIX_MIN_BREEDING_WEIGHT_GRAMS: f64 = 200.0;
pub const MIN_FEMALES_PER_MALE: usize = 3;
pub const MAX_FEMALES_PER_MALE: usize = 5;

// =========================================================================
// Weight tracking
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeightRecord {
    pub id: i64,
    pub bird_id: i64,
    pub weight_grams: f64,
    pub date: NaiveDate,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWeightRecord {
    pub weight_grams: f64,
    pub date: NaiveDate,
    pub notes: Option<String>,
}

// =========================================================================
// Processing queue
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessingReason {
    ExcessMale,
    LowWeight,
    PoorGenetics,
    Age,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessingStatus {
    Scheduled,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingRecord {
    pub id: i64,
    pub bird_id: i64,
    pub reason: ProcessingReason,
    pub scheduled_date: NaiveDate,
    pub processed_date: Option<NaiveDate>,
    pub final_weight_grams: Option<f64>,
    pub status: ProcessingStatus,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProcessingRecord {
    pub bird_id: i64,
    pub reason: ProcessingReason,
    pub scheduled_date: NaiveDate,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProcessingRecord {
    pub processed_date: Option<NaiveDate>,
    pub final_weight_grams: Option<f64>,
    pub status: Option<ProcessingStatus>,
    pub notes: Option<String>,
}

// =========================================================================
// Breeding groups
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreedingGroup {
    pub id: i64,
    pub name: String,
    pub male_id: i64,
    pub female_ids: Vec<i64>,
    pub start_date: NaiveDate,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBreedingGroup {
    pub name: String,
    pub male_id: i64,
    pub female_ids: Vec<i64>,
    pub start_date: NaiveDate,
    pub notes: Option<String>,
}

// =========================================================================
// Cull recommendations
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CullReason {
    ExcessMale,
    LowWeight { weight_grams: f64 },
    HighInbreeding { coefficient: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CullRecommendation {
    pub bird_id: i64,
    pub reason: CullReason,
}

// =========================================================================
// Camera feed infrastructure
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraStatus {
    Active,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifeStage {
    Chick,
    Adolescent,
    Adult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraFeed {
    pub id: i64,
    pub name: String,
    pub location: String,
    pub feed_url: String,
    pub status: CameraStatus,
    pub brooder_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCameraFeed {
    pub name: String,
    pub location: String,
    pub feed_url: String,
    pub status: CameraStatus,
    pub brooder_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameCapture {
    pub id: i64,
    pub camera_id: i64,
    pub timestamp: DateTime<Utc>,
    pub image_path: String,
    pub life_stage: LifeStage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFrameCapture {
    pub camera_id: i64,
    pub image_path: String,
    pub life_stage: LifeStage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub id: i64,
    pub frame_id: i64,
    pub label: String,
    pub confidence: f64,
    pub bounding_box_x: f64,
    pub bounding_box_y: f64,
    pub bounding_box_w: f64,
    pub bounding_box_h: f64,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDetectionResult {
    pub label: String,
    pub confidence: f64,
    pub bounding_box_x: f64,
    pub bounding_box_y: f64,
    pub bounding_box_w: f64,
    pub bounding_box_h: f64,
    pub notes: Option<String>,
}

// =========================================================================
// Brooder management
// =========================================================================

/// What the housing unit is used for. Separate axis from `LifeStage` (which
/// describes the residents). A single physical pen can change role across
/// its lifetime (e.g. an incubator becoming a brooder for a hatched clutch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HousingType {
    Incubator,
    Brooder,
    Hutch,
}

impl Default for HousingType {
    fn default() -> Self { Self::Brooder }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brooder {
    pub id: i64,
    pub name: String,
    pub lineage_id: Option<i64>,
    pub life_stage: LifeStage,
    pub qr_code: String,
    pub notes: Option<String>,
    #[serde(default)]
    pub camera_url: Option<String>,
    /// Defaults to Brooder for back-compat with rows created before issue #11.
    #[serde(default)]
    pub housing_type: HousingType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBrooder {
    pub name: String,
    pub lineage_id: Option<i64>,
    pub life_stage: LifeStage,
    pub qr_code: String,
    pub notes: Option<String>,
    #[serde(default)]
    pub camera_url: Option<String>,
    /// Optional on create — server falls back to "brooder" if omitted.
    #[serde(default)]
    pub housing_type: Option<HousingType>,
}

// =========================================================================
// Chick groups (nursery stage)
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChickGroupStatus {
    Active,
    Graduated,
    Lost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChickGroup {
    pub id: i64,
    pub clutch_id: Option<i64>,
    pub brooder_id: Option<i64>,
    pub initial_count: u32,
    pub current_count: u32,
    pub hatch_date: NaiveDate,
    pub status: ChickGroupStatus,
    pub notes: Option<String>,
    /// The hutch the group lives in after graduation (issue #14). `None` for
    /// Active groups (still in their nursery `brooder_id`) and for graduated
    /// groups that haven't been assigned to a hutch yet.
    #[serde(default)]
    pub housing_id: Option<i64>,
    #[serde(default)]
    pub is_ready_to_transition: bool,
    /// Many-to-many lineage tags. Populated from the `chick_group_lineages`
    /// junction table; must be non-empty for new groups (validated at handler level).
    #[serde(default)]
    pub lineages: Vec<Lineage>,
}

/// Coturnix maturity threshold — fully feathered, sexable, ready to band.
/// 35 days = start of the 6th week under the 1-indexed "we are IN week N"
/// convention used by the UI (week = floor(age_days / 7) + 1).
pub const READY_TO_TRANSITION_AGE_DAYS: i64 = 35;

impl ChickGroup {
    pub fn age_days_at(&self, today: NaiveDate) -> i64 {
        (today - self.hatch_date).num_days()
    }

    pub fn age_weeks_at(&self, today: NaiveDate) -> i64 {
        self.age_days_at(today) / 7
    }

    pub fn age_weeks(&self) -> i64 {
        self.age_weeks_at(chrono::Local::now().date_naive())
    }

    pub fn compute_is_ready_to_transition_at(&self, today: NaiveDate) -> bool {
        self.age_days_at(today) >= READY_TO_TRANSITION_AGE_DAYS
            && self.status == ChickGroupStatus::Active
    }

    pub fn compute_is_ready_to_transition(&self) -> bool {
        self.compute_is_ready_to_transition_at(chrono::Local::now().date_naive())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateChickGroup {
    pub clutch_id: Option<i64>,
    pub brooder_id: Option<i64>,
    pub initial_count: u32,
    pub hatch_date: NaiveDate,
    pub notes: Option<String>,
    /// One or more lineage IDs; must be non-empty (validated at handler level).
    pub lineage_ids: Vec<i64>,
}

/// Body for PUT /api/chick-groups/{id}/lineages — replaces the group's
/// lineage set atomically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaceLineagesRequest {
    pub lineage_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChickMortalityLog {
    pub id: i64,
    pub group_id: i64,
    pub count: u32,
    pub reason: String,
    pub date: NaiveDate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MortalityRequest {
    pub count: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraduateRequest {
    pub birds: Vec<GraduateBird>,
    /// Issue #14: optional hutch destination. When provided the server
    /// validates the target is a `Hutch`, stamps `housing_id` on every
    /// graduated bird, and writes the group's `housing_id` so the group
    /// shows up under that hutch's residents. Omitting leaves both NULL
    /// (graduated, unhoused) — the dashboard's "Assign Graduated Group"
    /// flow can place them later.
    #[serde(default)]
    pub target_housing_id: Option<i64>,
}

/// Body for `POST /api/brooders/{id}/assign-graduated-group` (issue #14).
/// Moves an already-graduated group + every bird produced from it into the
/// target hutch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignGraduatedGroupRequest {
    pub group_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignGraduatedGroupResponse {
    pub group_id: i64,
    pub housing_id: i64,
    /// Number of bird rows whose housing_id was set by this call.
    pub birds_updated: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraduateBird {
    pub sex: Sex,
    pub band_color: Option<String>,
    pub nfc_tag_id: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub weight_grams: Option<f64>,
    #[serde(default)]
    pub photo_path: Option<String>,
}

// =========================================================================
// Brooder temperature schedule (coturnix quail)
// =========================================================================

/// Target temperature (°F) by chick age. Returns (target, tolerance).
pub fn target_temp_for_age(age_days: i64) -> (f64, f64) {
    // Returns (target_temp_f, tolerance_f).
    // Week 1: 93-97 (target 95, ±2)
    // Week 2: 88-92 (target 90, ±2)
    // Week 3: 83-87 (target 85, ±2)
    // Week 4: 78-82 (target 80, ±2)
    // Week 5: 73-77 (target 75, ±2)
    // Week 6+: 68-72 (target 70, ±2)
    let tolerance = 2.0;
    let target = match age_days {
        0..=7 => 95.0,
        8..=14 => 90.0,
        15..=21 => 85.0,
        22..=28 => 80.0,
        29..=35 => 75.0,
        _ => 70.0, // feathered out
    };
    (target, tolerance)
}

/// Week label for the temperature schedule.
pub fn temp_schedule_label(age_days: i64) -> String {
    let week = (age_days / 7) + 1;
    let (target, _) = target_temp_for_age(age_days);
    if age_days >= 35 {
        format!("Week {}+ — {:.0}°F (feathered)", week, target)
    } else {
        format!("Week {} — {:.0}°F", week, target)
    }
}

/// Default adult/unassigned brooder temperature range.
pub const ADULT_TEMP_MIN: f64 = 65.0;
pub const ADULT_TEMP_MAX: f64 = 75.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetTempResponse {
    pub brooder_id: i64,
    pub target_temp_f: f64,
    pub min_temp_f: f64,
    pub max_temp_f: f64,
    pub week: i64,
    pub age_days: Option<i64>,
    pub chick_group_id: Option<i64>,
    pub schedule_label: String,
    pub status: String, // "heat_required", "weaning", "ambient", "unassigned"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignGroupRequest {
    pub group_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveBirdRequest {
    pub target_brooder_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrooderResidentsResponse {
    pub brooder_id: i64,
    pub chick_groups: Vec<ChickGroup>,
    pub individual_birds: Vec<Bird>,
}

/// Inbreeding coefficient for a potential male-female pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InbreedingCoefficient {
    pub male_id: i64,
    pub female_id: i64,
    pub coefficient: f64,
    pub safe: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // --- TelemetryPayload serde roundtrips ---

    #[test]
    fn telemetry_system_roundtrip() {
        let payload = TelemetryPayload::System(SystemMetrics {
            cpu_usage_percent: 42.5,
            memory_used_bytes: 1024,
            memory_total_bytes: 2048,
            disk_used_bytes: 500,
            disk_total_bytes: 1000,
            uptime_seconds: 3600,
        });
        let json = serde_json::to_string(&payload).unwrap();
        let back: TelemetryPayload = serde_json::from_str(&json).unwrap();
        match back {
            TelemetryPayload::System(m) => {
                assert!((m.cpu_usage_percent - 42.5).abs() < f64::EPSILON);
                assert_eq!(m.memory_used_bytes, 1024);
                assert_eq!(m.uptime_seconds, 3600);
            }
            _ => panic!("expected System variant"),
        }
    }

    #[test]
    fn telemetry_brooder_roundtrip() {
        let payload = TelemetryPayload::Brooder(BrooderReading {
            temperature_f: 98.6,
            humidity_percent: 55.0,
            timestamp: Utc::now(),
            brooder_id: Some(1),
        });
        let json = serde_json::to_string(&payload).unwrap();
        let back: TelemetryPayload = serde_json::from_str(&json).unwrap();
        match back {
            TelemetryPayload::Brooder(r) => {
                assert!((r.temperature_f - 98.6).abs() < f64::EPSILON);
                assert!((r.humidity_percent - 55.0).abs() < f64::EPSILON);
            }
            _ => panic!("expected Brooder variant"),
        }
    }

    #[test]
    fn telemetry_detection_roundtrip() {
        let payload = TelemetryPayload::Detection(DetectionEvent {
            species: Species::CoturnixQuail,
            confidence: 0.95,
            timestamp: Utc::now(),
        });
        let json = serde_json::to_string(&payload).unwrap();
        let back: TelemetryPayload = serde_json::from_str(&json).unwrap();
        match back {
            TelemetryPayload::Detection(d) => {
                assert_eq!(d.species, Species::CoturnixQuail);
                assert!((d.confidence - 0.95).abs() < f64::EPSILON);
            }
            _ => panic!("expected Detection variant"),
        }
    }

    #[test]
    fn telemetry_detection_unknown_species_roundtrip() {
        let payload = TelemetryPayload::Detection(DetectionEvent {
            species: Species::Unknown("Sparrow".into()),
            confidence: 0.3,
            timestamp: Utc::now(),
        });
        let json = serde_json::to_string(&payload).unwrap();
        let back: TelemetryPayload = serde_json::from_str(&json).unwrap();
        match back {
            TelemetryPayload::Detection(d) => {
                assert_eq!(d.species, Species::Unknown("Sparrow".into()));
            }
            _ => panic!("expected Detection variant"),
        }
    }

    // --- AlertConfig defaults ---

    #[test]
    fn alert_config_defaults() {
        // Default is the adult/unassigned range (68-72°F)
        let config = AlertConfig::default();
        assert!((config.brooder_temp_min - 68.0).abs() < f64::EPSILON);
        assert!((config.brooder_temp_max - 72.0).abs() < f64::EPSILON);
        assert!((config.humidity_min - 40.0).abs() < f64::EPSILON);
        assert!((config.humidity_max - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn alert_config_serde_roundtrip() {
        let config = AlertConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let back: AlertConfig = serde_json::from_str(&json).unwrap();
        assert!((back.brooder_temp_min - 68.0).abs() < f64::EPSILON);
        assert!((back.brooder_temp_max - 72.0).abs() < f64::EPSILON);
    }

    #[test]
    fn age_based_temp_week1() {
        let (target, tolerance) = target_temp_for_age(3); // day 3 = week 1
        assert!((target - 95.0).abs() < f64::EPSILON);
        assert!((tolerance - 2.0).abs() < f64::EPSILON);
        // Range: 93-97°F
    }

    #[test]
    fn age_based_temp_week3() {
        let (target, tolerance) = target_temp_for_age(18); // day 18 = week 3
        assert!((target - 85.0).abs() < f64::EPSILON);
        assert!((tolerance - 2.0).abs() < f64::EPSILON);
        // Range: 83-87°F
    }

    #[test]
    fn age_based_temp_week6_plus() {
        let (target, tolerance) = target_temp_for_age(42); // day 42 = week 6+
        assert!((target - 70.0).abs() < f64::EPSILON);
        assert!((tolerance - 2.0).abs() < f64::EPSILON);
        // Range: 68-72°F
    }

    // --- InbreedingCoefficient safe threshold ---

    #[test]
    fn inbreeding_safe_when_below_threshold() {
        let ic = InbreedingCoefficient {
            male_id: 1,
            female_id: 2,
            coefficient: 0.0,
            safe: 0.0 < 0.0625,
        };
        assert!(ic.safe);
    }

    #[test]
    fn inbreeding_unsafe_at_threshold() {
        let ic = InbreedingCoefficient {
            male_id: 1,
            female_id: 2,
            coefficient: 0.0625,
            safe: 0.0625 < 0.0625,
        };
        assert!(!ic.safe);
    }

    #[test]
    fn inbreeding_unsafe_above_threshold() {
        let ic = InbreedingCoefficient {
            male_id: 1,
            female_id: 2,
            coefficient: 0.25,
            safe: 0.25 < 0.0625,
        };
        assert!(!ic.safe);
    }

    #[test]
    fn inbreeding_serde_roundtrip() {
        let ic = InbreedingCoefficient {
            male_id: 10,
            female_id: 20,
            coefficient: 0.125,
            safe: false,
        };
        let json = serde_json::to_string(&ic).unwrap();
        let back: InbreedingCoefficient = serde_json::from_str(&json).unwrap();
        assert_eq!(back.male_id, 10);
        assert_eq!(back.female_id, 20);
        assert!((back.coefficient - 0.125).abs() < f64::EPSILON);
        assert!(!back.safe);
    }

    // --- ClutchStatus enum ---

    #[test]
    fn clutch_status_serde_roundtrip() {
        for status in [
            ClutchStatus::Incubating,
            ClutchStatus::Hatched,
            ClutchStatus::Failed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let back: ClutchStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    #[test]
    fn clutch_status_equality() {
        assert_eq!(ClutchStatus::Incubating, ClutchStatus::Incubating);
        assert_ne!(ClutchStatus::Incubating, ClutchStatus::Hatched);
        assert_ne!(ClutchStatus::Hatched, ClutchStatus::Failed);
    }

    #[test]
    fn clutch_status_json_values() {
        assert_eq!(
            serde_json::to_string(&ClutchStatus::Incubating).unwrap(),
            "\"Incubating\""
        );
        assert_eq!(
            serde_json::to_string(&ClutchStatus::Hatched).unwrap(),
            "\"Hatched\""
        );
        assert_eq!(
            serde_json::to_string(&ClutchStatus::Failed).unwrap(),
            "\"Failed\""
        );
    }
}
