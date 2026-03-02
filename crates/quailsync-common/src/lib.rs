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
    pub temperature_celsius: f64,
    pub humidity_percent: f64,
    pub timestamp: DateTime<Utc>,
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

/// Top-level telemetry payload sent from agent to server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TelemetryPayload {
    System(SystemMetrics),
    Brooder(BrooderReading),
    Detection(DetectionEvent),
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
            brooder_temp_min: 95.0,
            brooder_temp_max: 100.0,
            humidity_min: 40.0,
            humidity_max: 60.0,
        }
    }
}

/// Severity level for an alert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
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
pub struct Bloodline {
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
    pub bloodline_id: i64,
    pub hatch_date: NaiveDate,
    pub mother_id: Option<i64>,
    pub father_id: Option<i64>,
    pub generation: u32,
    pub status: BirdStatus,
    pub notes: Option<String>,
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
    pub bloodline_id: Option<i64>,
    pub eggs_set: u32,
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub set_date: NaiveDate,
    pub expected_hatch_date: NaiveDate,
    pub status: ClutchStatus,
    pub notes: Option<String>,
}

// --- Create structs (POST bodies, no id) ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBloodline {
    pub name: String,
    pub source: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBird {
    pub band_color: Option<String>,
    pub sex: Sex,
    pub bloodline_id: i64,
    pub hatch_date: NaiveDate,
    pub mother_id: Option<i64>,
    pub father_id: Option<i64>,
    pub generation: u32,
    pub status: BirdStatus,
    pub notes: Option<String>,
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
    pub bloodline_id: Option<i64>,
    pub eggs_set: u32,
    pub eggs_fertile: Option<u32>,
    pub eggs_hatched: Option<u32>,
    pub set_date: NaiveDate,
    pub status: ClutchStatus,
    pub notes: Option<String>,
}
