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
        // Fallback defaults — canonical values live in the system_settings table
        // (keys alert_temp_min_f / alert_temp_max_f / alert_humidity_min /
        // alert_humidity_max). The server loads them into `Settings` at startup;
        // these literals only apply to a DB that somehow lacks the seeded rows.
        Self {
            brooder_temp_min: 68.0, // Fallback default — canonical value lives in system_settings table
            brooder_temp_max: 72.0, // Fallback default — canonical value lives in system_settings table
            humidity_min: 40.0, // Fallback default — canonical value lives in system_settings table
            humidity_max: 60.0, // Fallback default — canonical value lives in system_settings table
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
    /// When `photo_path` was last set by an upload (ISO-8601 string). `None`
    /// for birds with no uploaded photo. The DB is the source of truth for
    /// "current photo" now that filenames carry a timestamp and history is
    /// retained on disk — this surfaces the upload time to the UI.
    #[serde(default)]
    pub photo_uploaded_at: Option<String>,
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
    /// Probabilistic lineage distribution (Phase 3), derived from the
    /// `bird_genetic_profile` table. Empty for birds with no recorded profile.
    #[serde(default)]
    pub genetic_profile: GeneticProfile,
    /// Confidence in this bird's lineage: `min(max(paternal), max(maternal))` —
    /// the weakest inherited side's strongest single lineage. `0.0` when either
    /// side has no recorded distribution.
    #[serde(default)]
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Clutch {
    pub id: i64,
    /// The breeding group that produced the eggs (which male × which females).
    /// Nullable — lineage-only clutches (and pre-feature rows) have none.
    pub breeding_group_id: Option<i64>,
    /// The group's display name, JOINed in on read so clients don't need a
    /// separate lookup. `None` when the clutch has no group.
    pub breeding_group_name: Option<String>,
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
pub struct CreateClutch {
    /// Optional breeding group that produced the eggs. Lineage-only clutches
    /// (no group) are still valid.
    #[serde(default)]
    pub breeding_group_id: Option<i64>,
    pub lineage_id: Option<i64>,
    pub eggs_set: u32,
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub set_date: NaiveDate,
    pub status: ClutchStatus,
    pub notes: Option<String>,
}

// --- Clutch group snapshots (Phase 2: probabilistic genetics) --------------

/// One bird in a clutch's frozen group snapshot, with the lineage tags it had
/// at egg-collection time (a multi-lineage bird lists all of them).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SnapshotBird {
    pub bird_id: i64,
    pub lineage_ids: Vec<i64>,
}

/// A lineage with a probability weight in `0.0..=1.0`, plus its display name.
/// Reused for snapshot maternal/paternal distributions and bird profiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LineageProbability {
    pub lineage_id: i64,
    pub lineage_name: String,
    pub probability: f64,
}

/// A bird's probabilistic lineage (Phase 3), split by inherited side. The
/// probabilities on each side sum to 1.0 (subject to the 1% tracking floor).
/// Empty vecs mean the bird has no recorded genetic profile. Highest-probability
/// lineage first on each side. Supersedes the discrete `Bird::lineages` tags for
/// genetic reasoning; `lineages` stays populated for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GeneticProfile {
    pub paternal: Vec<LineageProbability>,
    pub maternal: Vec<LineageProbability>,
}

/// Frozen composition of the breeding group that produced a clutch, captured at
/// creation time. `maternal_distribution`/`paternal_distribution` are derived
/// from the members — each bird weighted equally and split across its lineages,
/// so the probabilities on each side sum to 1.0 (paternal is certain for a
/// single-male group). Drives the probabilistic lineage of the chicks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClutchSnapshot {
    pub males: Vec<SnapshotBird>,
    pub females: Vec<SnapshotBird>,
    pub paternal_distribution: Vec<LineageProbability>,
    pub maternal_distribution: Vec<LineageProbability>,
}

/// `GET /api/clutches/{id}`: the clutch plus its group snapshot. `snapshot` is
/// `None` for lineage-only clutches (no breeding group was recorded).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClutchDetail {
    #[serde(flatten)]
    pub clutch: Clutch,
    pub snapshot: Option<ClutchSnapshot>,
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

// Fallback default — canonical value lives in system_settings table (butcher_weight_grams).
pub const COTURNIX_BUTCHER_WEIGHT_GRAMS: f64 = 250.0;
// Fallback default — canonical value lives in system_settings table (min_breeding_weight_grams).
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
    /// All males in the group, from the `breeding_group_males` junction (the
    /// single source of truth). Empty when the group is `infertile`.
    pub male_ids: Vec<i64>,
    pub female_ids: Vec<i64>,
    pub start_date: NaiveDate,
    pub notes: Option<String>,
    /// `"active"` (has at least one male) or `"infertile"` (no males). The
    /// females stay assigned regardless — the group is birds cohabiting a hutch.
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBreedingGroup {
    pub name: String,
    /// Males are managed only through the junction table (this list). At least
    /// one is required. A scalar `male_id` is no longer accepted — any such
    /// field on the request is ignored.
    #[serde(default)]
    pub male_ids: Vec<i64>,
    pub female_ids: Vec<i64>,
    pub start_date: NaiveDate,
    pub notes: Option<String>,
}

impl CreateBreedingGroup {
    /// The male list, de-duplicated while preserving order.
    pub fn males(&self) -> Vec<i64> {
        let mut out: Vec<i64> = Vec::new();
        for &m in &self.male_ids {
            if !out.contains(&m) {
                out.push(m);
            }
        }
        out
    }
}

/// Partial update for a breeding group. Absent (`None`) fields are left
/// unchanged; present list fields fully replace the corresponding membership.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateBreedingGroup {
    #[serde(default)]
    pub name: Option<String>,
    /// Replacement male roster. `None` leaves males unchanged. A scalar
    /// `male_id` is no longer accepted — any such field is ignored.
    #[serde(default)]
    pub male_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub female_ids: Option<Vec<i64>>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl UpdateBreedingGroup {
    /// The new male list if `male_ids` was supplied (de-duplicated, order
    /// preserved), else `None` (meaning "leave males unchanged").
    pub fn males(&self) -> Option<Vec<i64>> {
        self.male_ids.as_ref().map(|ids| {
            let mut out: Vec<i64> = Vec::new();
            for &m in ids {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
            out
        })
    }
}

// =========================================================================
// Flock breeding stats (powers the cull-mode guardrail UI)
// =========================================================================

/// Per-male breeding utility. `safe_female_ids` lets clients answer the
/// "would culling this male leave any female with zero safe mates?"
/// question without another round-trip — they just remove the selected
/// males from each female's safe-mate set client-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerMaleSafePairings {
    pub bird_id: i64,
    pub safe_pairings: u32,
    pub safe_female_ids: Vec<i64>,
}

/// Server-computed snapshot of flock breeding capacity. Used by the
/// Flock screen's cull-mode guardrail: clients select birds, then
/// subtract them from `total_males` and compare against
/// `minimum_males_needed` to compute the green/yellow/red zone.
///
/// `safe_to_cull = max(0, total_males - minimum_males_needed)`, where
/// `minimum_males_needed = ceil(total_females / max_females_per_male)
///                         * desired_males_per_group` (settings-driven).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlockBreedingStats {
    pub total_males: u32,
    pub total_females: u32,
    pub minimum_males_needed: u32,
    pub safe_to_cull: u32,
    pub per_male_safe_pairings: Vec<PerMaleSafePairings>,
    /// Echoed back from the settings table so clients can recompute the
    /// required-males line themselves as the user toggles cull selections.
    pub desired_males_per_group: u32,
    pub max_females_per_male: u32,
}

// =========================================================================
// User-configurable app settings
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// How many males the user wants per breeding group. Multiplied into
    /// `minimum_males_needed` so a value of 2 doubles the required males.
    pub desired_males_per_group: u32,
    /// Cap on females per male before another male is required.
    pub max_females_per_male: u32,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            desired_males_per_group: 1,
            max_females_per_male: MAX_FEMALES_PER_MALE as u32,
        }
    }
}

/// Partial-update payload for PUT /api/settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateAppSettings {
    pub desired_males_per_group: Option<u32>,
    pub max_females_per_male: Option<u32>,
}

/// User-configurable genetics thresholds (Phase 5). All values are whole
/// percents except `display_cap` (a count). Persisted as string rows in the
/// `settings` table under the dotted keys below; the wire format on
/// `GET/PUT /api/settings/genetics` is a flat `{ "genetics.threshold.safe":
/// "15", … }` map. This is the parsed, typed view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneticsSettings {
    /// Overlap % below which a pairing is "safe"; at/above it is "caution".
    pub threshold_safe: u32,
    /// Overlap % above which a pairing should be "avoided".
    pub threshold_avoid: u32,
    /// Lineage components below this % are dropped from a bird's profile.
    pub tracking_floor: u32,
    /// Show at most this many lineages per side; the rest group as "trace".
    pub display_cap: u32,
    /// Flag new blood when a bird's / the flock's confidence drops below this %.
    pub new_blood_confidence: u32,
}

impl Default for GeneticsSettings {
    fn default() -> Self {
        Self {
            threshold_safe: 15,
            threshold_avoid: 35,
            tracking_floor: 1,
            display_cap: 4,
            new_blood_confidence: 50,
        }
    }
}

impl GeneticsSettings {
    pub const KEY_THRESHOLD_SAFE: &'static str = "genetics.threshold.safe";
    pub const KEY_THRESHOLD_AVOID: &'static str = "genetics.threshold.avoid";
    pub const KEY_TRACKING_FLOOR: &'static str = "genetics.tracking_floor";
    pub const KEY_DISPLAY_CAP: &'static str = "genetics.display_cap";
    pub const KEY_NEW_BLOOD_CONFIDENCE: &'static str = "genetics.new_blood_confidence";

    /// `(key, default, min, max)` for every genetics setting — the single source
    /// of truth driving DB seeding, GET serialization, and PUT validation. The
    /// `(min, max)` bounds are inclusive.
    pub const SPEC: [(&'static str, u32, u32, u32); 5] = [
        (Self::KEY_THRESHOLD_SAFE, 15, 0, 100),
        (Self::KEY_THRESHOLD_AVOID, 35, 0, 100),
        (Self::KEY_TRACKING_FLOOR, 1, 1, 50),
        (Self::KEY_DISPLAY_CAP, 4, 1, 10),
        (Self::KEY_NEW_BLOOD_CONFIDENCE, 50, 0, 100),
    ];

    /// Inclusive `(min, max)` valid range for a key, or `None` if unknown.
    pub fn valid_range(key: &str) -> Option<(u32, u32)> {
        Self::SPEC
            .iter()
            .find(|(k, _, _, _)| *k == key)
            .map(|(_, _, lo, hi)| (*lo, *hi))
    }

    /// Set a value by its dotted key. Returns `false` for an unknown key.
    pub fn set(&mut self, key: &str, value: u32) -> bool {
        match key {
            Self::KEY_THRESHOLD_SAFE => self.threshold_safe = value,
            Self::KEY_THRESHOLD_AVOID => self.threshold_avoid = value,
            Self::KEY_TRACKING_FLOOR => self.tracking_floor = value,
            Self::KEY_DISPLAY_CAP => self.display_cap = value,
            Self::KEY_NEW_BLOOD_CONFIDENCE => self.new_blood_confidence = value,
            _ => return false,
        }
        true
    }

    /// The flat `{ dotted-key: string-value }` map used on the wire.
    pub fn to_map(&self) -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([
            (
                Self::KEY_THRESHOLD_SAFE.to_string(),
                self.threshold_safe.to_string(),
            ),
            (
                Self::KEY_THRESHOLD_AVOID.to_string(),
                self.threshold_avoid.to_string(),
            ),
            (
                Self::KEY_TRACKING_FLOOR.to_string(),
                self.tracking_floor.to_string(),
            ),
            (
                Self::KEY_DISPLAY_CAP.to_string(),
                self.display_cap.to_string(),
            ),
            (
                Self::KEY_NEW_BLOOD_CONFIDENCE.to_string(),
                self.new_blood_confidence.to_string(),
            ),
        ])
    }
}

// =========================================================================
// System settings — server-owned lifecycle + alert thresholds.
//
// The canonical values live in the `system_settings` table (one key/value row
// each). `Settings` is the typed view; the server loads it at startup and the
// GET/PUT /api/system-settings routes read/write it. This is the foundation for
// per-user settings — today it's a single system-level set of rows.
//
// Each field has a corresponding hardcoded constant elsewhere in this module
// that serves only as a fallback default (see `Settings::default`).
// =========================================================================

/// Fallback default — canonical value lives in system_settings table (incubation_days).
pub const DEFAULT_INCUBATION_DAYS: i64 = 17;
/// Fallback default — canonical value lives in system_settings table (sensor_stale_seconds).
pub const DEFAULT_SENSOR_STALE_SECONDS: i64 = 15;
/// Fallback default — canonical value lives in system_settings table (brooder_week_temps_f).
pub const DEFAULT_BROODER_WEEK_TEMPS_F: [i64; 6] = [97, 92, 87, 82, 77, 72];

/// Typed, server-owned settings. Built from the `system_settings` rows via
/// [`Settings::from_rows`], falling back to [`Settings::default`] per key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub alert_temp_min_f: f64,
    pub alert_temp_max_f: f64,
    pub alert_humidity_min: f64,
    pub alert_humidity_max: f64,
    pub adult_temp_min_f: f64,
    pub adult_temp_max_f: f64,
    pub incubation_days: i64,
    pub ready_to_transition_age_days: i64,
    pub butcher_weight_grams: f64,
    pub min_breeding_weight_grams: f64,
    pub sensor_stale_seconds: i64,
    /// Per-week brooder target temps (°F), week 1..=6, stored as a JSON array.
    pub brooder_week_temps_f: Vec<i64>,
    /// Indoor-cam: upload notable frames to Roboflow for active learning.
    pub indoor_cam_roboflow_upload_enabled: bool,
    /// Indoor-cam: save notable frames to disk (the PC running the pipeline).
    pub indoor_cam_image_save_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        // Pull from the existing hardcoded constants so there's a single source
        // of truth for the fallback values.
        let alerts = AlertConfig::default();
        Self {
            alert_temp_min_f: alerts.brooder_temp_min,
            alert_temp_max_f: alerts.brooder_temp_max,
            alert_humidity_min: alerts.humidity_min,
            alert_humidity_max: alerts.humidity_max,
            adult_temp_min_f: ADULT_TEMP_MIN,
            adult_temp_max_f: ADULT_TEMP_MAX,
            incubation_days: DEFAULT_INCUBATION_DAYS,
            ready_to_transition_age_days: READY_TO_TRANSITION_AGE_DAYS,
            butcher_weight_grams: COTURNIX_BUTCHER_WEIGHT_GRAMS,
            min_breeding_weight_grams: COTURNIX_MIN_BREEDING_WEIGHT_GRAMS,
            sensor_stale_seconds: DEFAULT_SENSOR_STALE_SECONDS,
            brooder_week_temps_f: DEFAULT_BROODER_WEEK_TEMPS_F.to_vec(),
            // Both default ON so the indoor-cam pipeline keeps its current
            // behavior until a user explicitly turns a toggle off.
            indoor_cam_roboflow_upload_enabled: true,
            indoor_cam_image_save_enabled: true,
        }
    }
}

impl Settings {
    /// Build `Settings` from raw `(key, value)` rows out of `system_settings`,
    /// falling back to [`Settings::default`] for any key that's missing or
    /// unparseable. `brooder_week_temps_f` is stored as a JSON array string
    /// (e.g. `"[97,92,87,82,77,72]"`).
    pub fn from_rows<I, K, V>(rows: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        use std::collections::HashMap;
        let map: HashMap<String, String> = rows
            .into_iter()
            .map(|(k, v)| (k.as_ref().to_string(), v.as_ref().to_string()))
            .collect();
        let d = Settings::default();

        fn parse_f64(map: &HashMap<String, String>, key: &str, fallback: f64) -> f64 {
            map.get(key)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(fallback)
        }
        fn parse_i64(map: &HashMap<String, String>, key: &str, fallback: i64) -> i64 {
            map.get(key)
                .and_then(|v| v.trim().parse::<i64>().ok())
                .unwrap_or(fallback)
        }
        fn parse_bool(map: &HashMap<String, String>, key: &str, fallback: bool) -> bool {
            match map.get(key) {
                Some(v) => matches!(
                    v.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                ),
                None => fallback,
            }
        }

        let brooder_week_temps_f = map
            .get("brooder_week_temps_f")
            .and_then(|v| serde_json::from_str::<Vec<i64>>(v).ok())
            .unwrap_or(d.brooder_week_temps_f);

        Settings {
            alert_temp_min_f: parse_f64(&map, "alert_temp_min_f", d.alert_temp_min_f),
            alert_temp_max_f: parse_f64(&map, "alert_temp_max_f", d.alert_temp_max_f),
            alert_humidity_min: parse_f64(&map, "alert_humidity_min", d.alert_humidity_min),
            alert_humidity_max: parse_f64(&map, "alert_humidity_max", d.alert_humidity_max),
            adult_temp_min_f: parse_f64(&map, "adult_temp_min_f", d.adult_temp_min_f),
            adult_temp_max_f: parse_f64(&map, "adult_temp_max_f", d.adult_temp_max_f),
            incubation_days: parse_i64(&map, "incubation_days", d.incubation_days),
            ready_to_transition_age_days: parse_i64(
                &map,
                "ready_to_transition_age_days",
                d.ready_to_transition_age_days,
            ),
            butcher_weight_grams: parse_f64(&map, "butcher_weight_grams", d.butcher_weight_grams),
            min_breeding_weight_grams: parse_f64(
                &map,
                "min_breeding_weight_grams",
                d.min_breeding_weight_grams,
            ),
            sensor_stale_seconds: parse_i64(&map, "sensor_stale_seconds", d.sensor_stale_seconds),
            brooder_week_temps_f,
            indoor_cam_roboflow_upload_enabled: parse_bool(
                &map,
                "indoor_cam_roboflow_upload_enabled",
                d.indoor_cam_roboflow_upload_enabled,
            ),
            indoor_cam_image_save_enabled: parse_bool(
                &map,
                "indoor_cam_image_save_enabled",
                d.indoor_cam_image_save_enabled,
            ),
        }
    }

    /// View the alert thresholds as an [`AlertConfig`] for the alert engine.
    pub fn alert_config(&self) -> AlertConfig {
        AlertConfig {
            brooder_temp_min: self.alert_temp_min_f,
            brooder_temp_max: self.alert_temp_max_f,
            humidity_min: self.alert_humidity_min,
            humidity_max: self.alert_humidity_max,
        }
    }
}

/// Partial-update payload for `PUT /api/system-settings` — any subset of fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSettings {
    #[serde(default)]
    pub alert_temp_min_f: Option<f64>,
    #[serde(default)]
    pub alert_temp_max_f: Option<f64>,
    #[serde(default)]
    pub alert_humidity_min: Option<f64>,
    #[serde(default)]
    pub alert_humidity_max: Option<f64>,
    #[serde(default)]
    pub adult_temp_min_f: Option<f64>,
    #[serde(default)]
    pub adult_temp_max_f: Option<f64>,
    #[serde(default)]
    pub incubation_days: Option<i64>,
    #[serde(default)]
    pub ready_to_transition_age_days: Option<i64>,
    #[serde(default)]
    pub butcher_weight_grams: Option<f64>,
    #[serde(default)]
    pub min_breeding_weight_grams: Option<f64>,
    #[serde(default)]
    pub sensor_stale_seconds: Option<i64>,
    #[serde(default)]
    pub brooder_week_temps_f: Option<Vec<i64>>,
    #[serde(default)]
    pub indoor_cam_roboflow_upload_enabled: Option<bool>,
    #[serde(default)]
    pub indoor_cam_image_save_enabled: Option<bool>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HousingType {
    Incubator,
    #[default]
    Brooder,
    Hutch,
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
// Fallback default — canonical value lives in system_settings table (ready_to_transition_age_days).
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
// Fallback default — canonical value lives in system_settings table (adult_temp_min_f).
pub const ADULT_TEMP_MIN: f64 = 65.0;
// Fallback default — canonical value lives in system_settings table (adult_temp_max_f).
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
    /// The headcount: how many Active birds have `housing_id` pointing at this
    /// unit right now. This is the source of truth for the resident count —
    /// graduated chick groups are provenance only and never feed this number.
    pub active_bird_count: i64,
}

/// Inbreeding coefficient for a potential male-female pairing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InbreedingCoefficient {
    pub male_id: i64,
    pub female_id: i64,
    pub coefficient: f64,
    pub safe: bool,
}

/// A suggested breeding pairing scored by probability-weighted lineage overlap
/// (Phase 4). `bird_a_id` is the male, `bird_b_id` the female. Each side's
/// overlap is `Σ A.side[lineage] × B.side[lineage]`; the pairing's risk is the
/// larger of the two, surfaced as a rounded percent and a `risk_level` of
/// `"safe"` (<15%), `"caution"` (15–35%), or `"avoid"` (>35%).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PairingSuggestion {
    pub bird_a_id: i64,
    pub bird_b_id: i64,
    pub paternal_overlap: f64,
    pub maternal_overlap: f64,
    pub risk_percent: i64,
    pub risk_level: String,
}

/// Flock-wide genetic-diversity snapshot (Phase 4), powering the "new blood"
/// alert. Overlap risk is `max(paternal_overlap, maternal_overlap)` across
/// candidate male×female pairings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlockDiversity {
    /// Mean lineage confidence across all active birds (`0.0` for an empty flock).
    pub flock_confidence: f64,
    /// Lowest confidence of any active bird (`0.0` for an empty flock).
    pub min_confidence: f64,
    /// Lowest overlap risk achievable among active male×female pairings
    /// (`1.0` when no such pairing exists).
    pub best_pairing_risk: f64,
    /// `true` when the flock needs fresh genetics: `best_pairing_risk > 0.35`
    /// or `min_confidence < 0.50`.
    pub needs_new_blood: bool,
    /// Distinct lineages appearing in active birds' genetic profiles.
    pub active_lineage_count: i64,
}

// ---------------------------------------------------------------------------
// Govee H5179 WiFi temp/humidity sensors
//
// A separate Python poller hits the Govee cloud API and POSTs batches of
// readings. Sensors auto-register on first sight and are dynamically
// assignable to brooders/hutches (one active assignment per sensor at a time).
// ---------------------------------------------------------------------------

/// One reading in a `POST /api/govee/readings` batch. `model`/`name` are
/// optional so the poller can omit metadata it doesn't have; they backfill the
/// sensor's columns when it auto-registers. `recorded_at` is the timestamp the
/// Govee API reported the reading at (ISO-8601), kept verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeReadingInput {
    pub device_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub temperature_f: f64,
    pub humidity: f64,
    pub recorded_at: String,
}

/// Body of `POST /api/govee/readings`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeReadingsRequest {
    pub readings: Vec<GoveeReadingInput>,
}

/// Response for `POST /api/govee/readings` — how many rows were stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeReadingsResponse {
    pub stored: i64,
}

/// A sensor's current (open) assignment to a housing unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeAssignment {
    pub brooder_id: i64,
    pub brooder_name: String,
    pub assigned_at: String,
}

/// The most recent reading recorded for a sensor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeLatestReading {
    pub temperature_f: f64,
    pub humidity: f64,
    pub recorded_at: String,
}

/// A registered Govee sensor with its current assignment (if any) and latest
/// reading (if any). Returned by `GET /api/govee/sensors`,
/// `GET /api/brooders/{id}/sensors`, and the assign endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoveeSensor {
    pub id: i64,
    pub govee_device_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub assignment: Option<GoveeAssignment>,
    pub latest_reading: Option<GoveeLatestReading>,
}

/// Body of `PUT /api/govee/sensors/{id}/assign`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignSensorRequest {
    pub brooder_id: i64,
}

// ---------------------------------------------------------------------------
// SPYPOINT trail cameras
//
// Trail cameras auto-register when the poller's photos are first seen (the
// `spypoint_camera_id` is the natural key) or can be seeded manually via
// `POST /api/trail-cameras/register`. They're assignable to brooders/hutches
// exactly like Govee sensors — one active assignment per camera at a time.
// ---------------------------------------------------------------------------

/// A camera's current (open) assignment to a housing unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraAssignment {
    pub brooder_id: i64,
    pub brooder_name: String,
    pub assigned_at: String,
}

/// A registered trail camera with its current assignment (if any). Returned by
/// `GET /api/trail-cameras`, `GET /api/brooders/{id}/cameras`, and the register
/// + assign endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrailCamera {
    pub id: i64,
    pub spypoint_camera_id: String,
    pub name: Option<String>,
    pub model: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub assignment: Option<CameraAssignment>,
}

/// Body of `POST /api/trail-cameras/register`. Idempotent upsert keyed on
/// `spypoint_camera_id`; `name`/`model` update the row when provided.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterCameraRequest {
    pub spypoint_camera_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of `PUT /api/trail-cameras/{id}/assign`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignCameraRequest {
    pub brooder_id: i64,
}

// ---------------------------------------------------------------------------
// Indoor cameras (RTSP, e.g. Tapo C100). The same decoupled pattern as trail
// cameras — a Python poller (see `indoor-cam/`) grabs RTSP frames, runs YOLO,
// and POSTs observations — but scoped to brooders/incubators only (never
// hutches). Cameras auto-register on first observation (keyed by `camera_id`)
// or are created/managed via `/api/indoor-cameras` CRUD, carry an `rtsp_url`
// for the management UI, and are assignable to a brooder/incubator with one
// active assignment at a time (mirrors `TrailCamera`/`CameraAssignment`).
// ---------------------------------------------------------------------------

/// An indoor camera's current (open) assignment to a brooder or incubator.
/// Distinct from [`CameraAssignment`] in that it also carries the unit's
/// `housing_type` ("brooder" | "incubator") so the UI can label it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndoorCameraAssignment {
    pub brooder_id: i64,
    pub brooder_name: String,
    pub housing_type: String,
    pub assigned_at: String,
}

/// A registered indoor camera with its current assignment (if any). Returned by
/// `GET /api/indoor-cameras`, `GET /api/brooders/{id}/indoor-cameras`, and the
/// create/update/assign endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndoorCamera {
    pub id: i64,
    /// Stable slug the poller posts observations with (e.g. "indoor-1").
    pub camera_id: String,
    pub name: Option<String>,
    /// RTSP stream address for the management UI. Store without embedded
    /// credentials where possible — the poller reads its own credentialed URL
    /// from its out-of-repo secrets file, not from here.
    pub rtsp_url: Option<String>,
    pub model: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub created_at: String,
    pub assignment: Option<IndoorCameraAssignment>,
}

/// Body of `POST /api/indoor-cameras`. Idempotent upsert keyed on `camera_id`;
/// `name`/`rtsp_url`/`model` update the row when provided.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterIndoorCameraRequest {
    pub camera_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rtsp_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of `PUT /api/indoor-cameras/{id}`. Each present field overwrites the
/// stored value; omitted fields are left unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateIndoorCameraRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub rtsp_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

/// Body of `PUT /api/indoor-cameras/{id}/assign`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssignIndoorCameraRequest {
    pub brooder_id: i64,
}

// ---------------------------------------------------------------------------
// Incubation events (stage-1 incubator capture pipeline; see `incubator/`).
// A Python sidecar watches per-slot ROIs over the incubator tray, runs
// frame-difference detection, and writes one `change_detected` row per event to
// the `incubation_events` table (the sidecar is the only writer). The backend
// owns the schema and exposes read-only aggregates over it. `clutch_id` is
// nullable and static-null today; the per-clutch breakdown populates for free
// once slots carry clutch ids — no code change needed then.
//
// Null-safety: `clutch_id` and `frame_path` are nullable in the schema, so they
// are `Option`; every NOT NULL column is non-optional.
// ---------------------------------------------------------------------------

/// One change-detection event for an incubator-tray slot. Returned by
/// `GET /api/incubation/events`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncubationEventDto {
    pub id: i64,
    pub slot_id: String,
    pub event_type: String,
    pub diff_score: f64,
    pub high_threshold: f64,
    /// Nullable per schema — populated later once slots carry a clutch id.
    pub clutch_id: Option<i64>,
    /// Saved ROI crop path (the stage-2 labeling dataset); absent when the
    /// sidecar didn't save a crop for the event.
    pub frame_path: Option<String>,
    pub created_at: String,
}

/// Per-slot activity within the summary window. Nested in
/// [`IncubationSummaryDto`], one entry per `slot_id` seen, ordered by
/// `last_event_at` descending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotActivityDto {
    pub slot_id: String,
    pub event_count: i64,
    pub last_event_at: String,
    pub last_diff_score: f64,
}

/// Per-clutch activity within the summary window, derived purely from rows with
/// a non-null `clutch_id`. Empty today (config leaves `clutch_id` null); it
/// populates automatically once slots get clutch ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClutchActivityDto {
    pub clutch_id: i64,
    pub event_count: i64,
    pub last_event_at: String,
}

/// Aggregate incubation activity over a rolling window. Returned by
/// `GET /api/incubation/summary`. Reflects `change_detected` events only — a
/// slot's active/quiet state lives in the sidecar detector, not this table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncubationSummaryDto {
    pub window_hours: u32,
    pub total_events: i64,
    /// One per `slot_id` seen in the window, ordered by `last_event_at` desc.
    pub slots: Vec<SlotActivityDto>,
    /// One per DISTINCT non-null `clutch_id` in the window; empty when none.
    pub clutches: Vec<ClutchActivityDto>,
}

// ---------------------------------------------------------------------------
// Indoor-camera assignment (storage + API + UIs). The single indoor Tapo camera
// is assigned to an "incubator" or a "brooder"; that assignment selects which
// vision model stage 3 will eventually run. The vision pipeline does NOT consume
// it yet. `active_model` is DERIVED from `assignment` via [`active_model_for`]
// (never stored) so there's a single source of truth for the mapping that
// stage 3 will reuse.
//
// This is a flat mode field for the one indoor camera — NOT a general location
// system, and distinct from the housing-unit attachment in `IndoorCamera`.
// ---------------------------------------------------------------------------

/// The two valid camera assignments. Kept as `&str` (not an enum) because the
/// column is a flat TEXT field the value round-trips through JSON as-is.
pub const CAMERA_ASSIGNMENT_INCUBATOR: &str = "incubator";
pub const CAMERA_ASSIGNMENT_BROODER: &str = "brooder";

/// Map an assignment to its derived vision-model name — the single source of
/// truth stage 3 will reuse. Returns `None` for anything that isn't a valid
/// assignment, so callers validate and derive in one step.
///
/// * `incubator` → `"incubation"`
/// * `brooder`   → `"chick"`
pub fn active_model_for(assignment: &str) -> Option<&'static str> {
    match assignment {
        CAMERA_ASSIGNMENT_INCUBATOR => Some("incubation"),
        CAMERA_ASSIGNMENT_BROODER => Some("chick"),
        _ => None,
    }
}

/// A camera's current assignment plus the derived, read-only model it selects.
/// Returned by `GET`/`PUT /api/cameras/{camera_id}/assignment`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraAssignmentDto {
    pub camera_id: String,
    /// `"incubator"` | `"brooder"`.
    pub assignment: String,
    /// DERIVED, read-only: `incubator` → `"incubation"`, `brooder` → `"chick"`.
    /// Computed from `assignment` via [`active_model_for`]; never stored.
    pub active_model: String,
    pub updated_at: String,
}

/// Body of `PUT /api/cameras/{camera_id}/assignment`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetCameraAssignmentRequest {
    /// `"incubator"` | `"brooder"`; anything else is rejected with 400.
    pub assignment: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    // --- Camera assignment → model mapping (single source of truth) ---

    #[test]
    fn active_model_for_maps_valid_assignments() {
        assert_eq!(
            active_model_for(CAMERA_ASSIGNMENT_INCUBATOR),
            Some("incubation")
        );
        assert_eq!(active_model_for(CAMERA_ASSIGNMENT_BROODER), Some("chick"));
        assert_eq!(active_model_for("incubator"), Some("incubation"));
        assert_eq!(active_model_for("brooder"), Some("chick"));
    }

    #[test]
    fn active_model_for_rejects_unknown_assignments() {
        assert_eq!(active_model_for("hutch"), None);
        assert_eq!(active_model_for(""), None);
        assert_eq!(active_model_for("Incubator"), None); // case-sensitive
    }

    // --- GeneticsSettings (Phase 5) ---

    #[test]
    fn genetics_settings_default_matches_spec() {
        // The DB seeds from SPEC while load-fallback uses Default — they must agree.
        let map = GeneticsSettings::default().to_map();
        for (key, default, _lo, _hi) in GeneticsSettings::SPEC {
            assert_eq!(
                map.get(key),
                Some(&default.to_string()),
                "default drift for {key}"
            );
        }
    }

    #[test]
    fn genetics_settings_rejects_unknown_key() {
        assert!(GeneticsSettings::valid_range("genetics.bogus").is_none());
        assert!(!GeneticsSettings::default().set("genetics.bogus", 1));
    }

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

    // --- Settings (system-settings) ---

    #[test]
    fn settings_default_matches_seed_values() {
        let s = Settings::default();
        assert!((s.alert_temp_min_f - 68.0).abs() < f64::EPSILON);
        assert!((s.alert_temp_max_f - 72.0).abs() < f64::EPSILON);
        assert!((s.alert_humidity_min - 40.0).abs() < f64::EPSILON);
        assert!((s.alert_humidity_max - 60.0).abs() < f64::EPSILON);
        assert!((s.adult_temp_min_f - 65.0).abs() < f64::EPSILON);
        assert!((s.adult_temp_max_f - 75.0).abs() < f64::EPSILON);
        assert_eq!(s.incubation_days, 17);
        assert_eq!(s.ready_to_transition_age_days, 35);
        assert!((s.butcher_weight_grams - 250.0).abs() < f64::EPSILON);
        assert!((s.min_breeding_weight_grams - 200.0).abs() < f64::EPSILON);
        assert_eq!(s.sensor_stale_seconds, 15);
        assert_eq!(s.brooder_week_temps_f, vec![97, 92, 87, 82, 77, 72]);
        // Indoor-cam toggles default ON.
        assert!(s.indoor_cam_roboflow_upload_enabled);
        assert!(s.indoor_cam_image_save_enabled);
    }

    #[test]
    fn settings_from_rows_parses_indoor_cam_bools() {
        // Explicit values override the ON defaults; case/format tolerant.
        let s = Settings::from_rows(vec![
            ("indoor_cam_roboflow_upload_enabled", "false"),
            ("indoor_cam_image_save_enabled", "TRUE"),
        ]);
        assert!(!s.indoor_cam_roboflow_upload_enabled);
        assert!(s.indoor_cam_image_save_enabled);

        // Missing keys fall back to the ON defaults.
        let d = Settings::from_rows(Vec::<(&str, &str)>::new());
        assert!(d.indoor_cam_roboflow_upload_enabled);
        assert!(d.indoor_cam_image_save_enabled);
    }

    #[test]
    fn settings_from_rows_parses_all_keys() {
        let rows = vec![
            ("alert_temp_min_f", "70.5"),
            ("alert_temp_max_f", "74.0"),
            ("alert_humidity_min", "30.0"),
            ("alert_humidity_max", "55.0"),
            ("adult_temp_min_f", "60.0"),
            ("adult_temp_max_f", "80.0"),
            ("incubation_days", "18"),
            ("ready_to_transition_age_days", "42"),
            ("butcher_weight_grams", "260.0"),
            ("min_breeding_weight_grams", "210.0"),
            ("sensor_stale_seconds", "30"),
            ("brooder_week_temps_f", "[95,90,85,80,75,70]"),
        ];
        let s = Settings::from_rows(rows);
        assert!((s.alert_temp_min_f - 70.5).abs() < f64::EPSILON);
        assert_eq!(s.incubation_days, 18);
        assert_eq!(s.ready_to_transition_age_days, 42);
        assert_eq!(s.sensor_stale_seconds, 30);
        assert_eq!(s.brooder_week_temps_f, vec![95, 90, 85, 80, 75, 70]);
    }

    #[test]
    fn settings_from_rows_falls_back_for_missing_or_malformed() {
        // Only one valid key; one malformed; the rest absent -> all default.
        let rows = vec![
            ("incubation_days", "21"),
            ("alert_temp_min_f", "not-a-number"),
            ("brooder_week_temps_f", "{bad json"),
        ];
        let s = Settings::from_rows(rows);
        assert_eq!(s.incubation_days, 21); // provided
        let d = Settings::default();
        assert!((s.alert_temp_min_f - d.alert_temp_min_f).abs() < f64::EPSILON); // malformed -> default
        assert_eq!(s.brooder_week_temps_f, d.brooder_week_temps_f); // malformed -> default
        assert_eq!(s.sensor_stale_seconds, d.sensor_stale_seconds); // absent -> default
    }

    #[test]
    fn settings_empty_rows_is_all_defaults() {
        let empty: Vec<(String, String)> = Vec::new();
        assert_eq!(Settings::from_rows(empty), Settings::default());
    }

    #[test]
    fn settings_week_temps_json_round_trip() {
        let original = Settings::default();
        let encoded = serde_json::to_string(&original.brooder_week_temps_f).unwrap();
        assert_eq!(encoded, "[97,92,87,82,77,72]");
        let s = Settings::from_rows(vec![("brooder_week_temps_f", encoded.as_str())]);
        assert_eq!(s.brooder_week_temps_f, original.brooder_week_temps_f);
    }

    #[test]
    fn settings_alert_config_view() {
        let s = Settings::default();
        let cfg = s.alert_config();
        assert!((cfg.brooder_temp_min - s.alert_temp_min_f).abs() < f64::EPSILON);
        assert!((cfg.humidity_max - s.alert_humidity_max).abs() < f64::EPSILON);
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
