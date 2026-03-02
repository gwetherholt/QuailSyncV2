use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use colored::Colorize;
use qrcode::QrCode;
use quailsync_common::{
    Alert, Bird, BirdStatus, Bloodline, BreedingGroup, Brooder, BrooderReading, CameraFeed,
    CameraStatus, Clutch, ClutchStatus, CreateBird, CreateBloodline, CreateBreedingGroup,
    CreateBrooder, CreateCameraFeed, CreateClutch, CreateProcessingRecord, CreateWeightRecord,
    CullReason, CullRecommendation, FrameCapture, InbreedingCoefficient, LifeStage,
    ProcessingRecord, ProcessingReason, ProcessingStatus, Sex, Severity, SystemMetrics,
    UpdateClutch, UpdateProcessingRecord, WeightRecord, COTURNIX_BUTCHER_WEIGHT_GRAMS,
};
use serde::Deserialize;

#[derive(Parser)]
#[command(name = "quailsync", about = "QuailSync CLI")]
struct Cli {
    /// Server URL
    #[arg(long, default_value = "http://localhost:3000")]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show agent connection status and health summary
    Status,
    /// Show brooder readings
    Brood {
        /// Show history for the last N minutes instead of latest reading
        #[arg(long)]
        history: Option<u64>,
    },
    /// Show system metrics
    System,
    /// Show recent alerts
    Alerts {
        /// Show alerts from the last N minutes
        #[arg(long, default_value = "60")]
        minutes: u64,
    },
    /// Manage bloodlines
    Bloodline {
        #[command(subcommand)]
        action: BloodlineAction,
    },
    /// Manage birds
    Bird {
        #[command(subcommand)]
        action: BirdAction,
    },
    /// Flock overview
    Flock {
        #[command(subcommand)]
        action: FlockAction,
    },
    /// Manage clutches and incubation
    Clutch {
        #[command(subcommand)]
        action: ClutchAction,
    },
    /// Breeding tools
    Breeding {
        #[command(subcommand)]
        action: BreedingAction,
    },
    /// Processing queue
    Processing {
        #[command(subcommand)]
        action: ProcessingAction,
    },
    /// Camera feed management
    Camera {
        #[command(subcommand)]
        action: CameraAction,
    },
    /// Manage brooders
    Brooder {
        #[command(subcommand)]
        action: BrooderAction,
    },
}

#[derive(Subcommand)]
enum BreedingAction {
    /// Suggest breeding pairs based on inbreeding coefficients
    Suggest,
    /// Manage breeding groups
    Group {
        #[command(subcommand)]
        action: BreedingGroupAction,
    },
}

#[derive(Subcommand)]
enum BreedingGroupAction {
    /// Create a breeding group
    Create {
        #[arg(long)]
        name: String,
        #[arg(long)]
        male: i64,
        /// Comma-separated list of female bird IDs
        #[arg(long)]
        females: String,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all breeding groups
    List,
}

#[derive(Subcommand)]
enum ProcessingAction {
    /// Schedule a bird for processing
    Schedule {
        #[arg(long)]
        bird: i64,
        /// Reason: excess-male, low-weight, poor-genetics, age, other
        #[arg(long)]
        reason: String,
        #[arg(long)]
        date: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Complete a processing record
    Complete {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        weight: Option<f64>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all processing records
    List,
    /// Show scheduled processing queue
    Queue,
}

#[derive(Subcommand)]
enum CameraAction {
    /// Register a new camera feed
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        location: String,
        #[arg(long)]
        url: String,
    },
    /// List all camera feeds
    List,
    /// Show camera status with last frame timestamps
    Status,
    /// Point a camera at a brooder
    Point {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        brooder: i64,
    },
}

#[derive(Subcommand)]
enum BrooderAction {
    /// Register a new brooder
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        bloodline: Option<i64>,
        /// Life stage: chick, adolescent, adult
        #[arg(long, default_value = "chick")]
        stage: String,
        #[arg(long)]
        qr: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all brooders
    List,
    /// Generate QR code SVG for a brooder
    Qr {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
enum BloodlineAction {
    /// Add a new bloodline
    Add {
        #[arg(long)]
        name: String,
        #[arg(long)]
        source: String,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all bloodlines
    List,
}

#[derive(Subcommand)]
enum BirdAction {
    /// Add a new bird
    Add {
        #[arg(long)]
        band: Option<String>,
        #[arg(long)]
        sex: String,
        #[arg(long)]
        bloodline: i64,
        #[arg(long)]
        hatch_date: Option<String>,
        #[arg(long)]
        mother: Option<i64>,
        #[arg(long)]
        father: Option<i64>,
        #[arg(long, default_value = "1")]
        generation: u32,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all birds
    List,
    /// Record a bird's weight
    Weigh {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        grams: f64,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Show weight history and growth trend for a bird
    Growth {
        #[arg(long)]
        id: i64,
    },
}

#[derive(Subcommand)]
enum FlockAction {
    /// Show flock summary
    Summary,
    /// Show cull recommendations
    CullReview,
}

#[derive(Subcommand)]
enum ClutchAction {
    /// Add a new clutch
    Add {
        #[arg(long)]
        bloodline: Option<i64>,
        #[arg(long)]
        eggs: u32,
        #[arg(long)]
        set_date: Option<String>,
        #[arg(long)]
        pair: Option<i64>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// List all clutches
    List,
    /// Update a clutch (after candling or hatch)
    Update {
        #[arg(long)]
        id: i64,
        #[arg(long)]
        fertile: Option<u32>,
        #[arg(long)]
        hatched: Option<u32>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Show incubation schedule for active clutches
    Schedule,
}

#[derive(Deserialize)]
struct StatusSummary {
    agent_connected: bool,
    last_brooder_reading: Option<String>,
    last_system_metric: Option<String>,
    last_detection_event: Option<String>,
}

fn status_dot(connected: bool) -> colored::ColoredString {
    if connected {
        "●".green()
    } else {
        "●".red()
    }
}

fn format_timestamp_age(ts: &str) -> colored::ColoredString {
    // Timestamps from the server are like "2026-03-01 20:03:58"
    // If we can't parse, just show raw
    let display = ts.to_string();
    display.normal()
}

async fn cmd_status(base: &str) -> anyhow::Result<()> {
    let url = format!("{base}/api/status");
    let resp = reqwest::get(&url).await?;

    if !resp.status().is_success() {
        eprintln!("{} Server returned {}", "error:".red().bold(), resp.status());
        std::process::exit(1);
    }

    let summary: StatusSummary = resp.json().await?;

    println!("{}", "QuailSync Status".bold().underline());
    println!();

    // Agent connection
    if summary.agent_connected {
        println!("  Agent:    {} {}", status_dot(true), "connected".green());
    } else {
        println!("  Agent:    {} {}", status_dot(false), "disconnected".red());
    }

    // Last seen timestamps
    println!();
    println!("  {}", "Last Seen".bold());

    match &summary.last_brooder_reading {
        Some(ts) => println!("    Brooder:   {}", format_timestamp_age(ts)),
        None => println!("    Brooder:   {}", "no data".dimmed()),
    }

    match &summary.last_system_metric {
        Some(ts) => println!("    System:    {}", format_timestamp_age(ts)),
        None => println!("    System:    {}", "no data".dimmed()),
    }

    match &summary.last_detection_event {
        Some(ts) => println!("    Detection: {}", format_timestamp_age(ts)),
        None => println!("    Detection: {}", "no data".dimmed()),
    }

    // Overall health
    println!();
    let has_data = summary.last_brooder_reading.is_some() || summary.last_system_metric.is_some();
    if summary.agent_connected && has_data {
        println!("  Health:   {}", "healthy".green().bold());
    } else if has_data {
        println!("  Health:   {}", "stale (agent disconnected)".yellow().bold());
    } else {
        println!("  Health:   {}", "no data".red().bold());
    }

    Ok(())
}

async fn cmd_brood_latest(base: &str) -> anyhow::Result<()> {
    let url = format!("{base}/api/brooder/latest");
    let resp = reqwest::get(&url).await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        println!("{}", "No brooder readings yet.".dimmed());
        return Ok(());
    }

    let reading: BrooderReading = resp.json().await?;

    println!("{}", "Brooder — Latest Reading".bold().underline());
    println!();
    println!("  Temperature:  {:.1}°F", reading.temperature_celsius);
    println!("  Humidity:     {:.1}%", reading.humidity_percent);
    println!("  Timestamp:    {}", reading.timestamp);

    Ok(())
}

async fn cmd_brood_history(base: &str, minutes: u64) -> anyhow::Result<()> {
    let url = format!("{base}/api/brooder/history?minutes={minutes}");
    let resp = reqwest::get(&url).await?;
    let readings: Vec<BrooderReading> = resp.json().await?;

    if readings.is_empty() {
        println!("{}", "No brooder readings in the selected window.".dimmed());
        return Ok(());
    }

    println!(
        "{}",
        format!("Brooder — Last {minutes} Minutes ({} readings)", readings.len())
            .bold()
            .underline()
    );
    println!();
    println!(
        "  {:<28} {:>10} {:>10}",
        "Timestamp".bold(),
        "Temp (°F)".bold(),
        "Humidity".bold(),
    );
    println!("  {}", "-".repeat(50));

    for r in &readings {
        println!(
            "  {:<28} {:>9.1}° {:>9.1}%",
            r.timestamp, r.temperature_celsius, r.humidity_percent,
        );
    }

    Ok(())
}

async fn cmd_system(base: &str) -> anyhow::Result<()> {
    let url = format!("{base}/api/system/latest");
    let resp = reqwest::get(&url).await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        println!("{}", "No system metrics yet.".dimmed());
        return Ok(());
    }

    let m: SystemMetrics = resp.json().await?;

    let mem_used_mb = m.memory_used_bytes / 1_048_576;
    let mem_total_mb = m.memory_total_bytes / 1_048_576;
    let mem_pct = (m.memory_used_bytes as f64 / m.memory_total_bytes as f64) * 100.0;

    let disk_used_gb = m.disk_used_bytes / 1_073_741_824;
    let disk_total_gb = m.disk_total_bytes / 1_073_741_824;
    let disk_pct = (m.disk_used_bytes as f64 / m.disk_total_bytes as f64) * 100.0;

    let hours = m.uptime_seconds / 3600;
    let mins = (m.uptime_seconds % 3600) / 60;

    println!("{}", "System Metrics".bold().underline());
    println!();
    println!("  CPU:     {:.1}%", m.cpu_usage_percent);
    println!(
        "  Memory:  {} / {} MB ({:.1}%)",
        mem_used_mb, mem_total_mb, mem_pct,
    );
    println!(
        "  Disk:    {} / {} GB ({:.1}%)",
        disk_used_gb, disk_total_gb, disk_pct,
    );
    println!("  Uptime:  {}h {}m", hours, mins);

    Ok(())
}

async fn cmd_alerts(base: &str, minutes: u64) -> anyhow::Result<()> {
    let url = format!("{base}/api/alerts?minutes={minutes}");
    let resp = reqwest::get(&url).await?;
    let alerts: Vec<Alert> = resp.json().await?;

    if alerts.is_empty() {
        println!("{}", format!("No alerts in the last {minutes} minutes.").dimmed());
        return Ok(());
    }

    println!(
        "{}",
        format!("Alerts — Last {minutes} Minutes ({} total)", alerts.len())
            .bold()
            .underline()
    );
    println!();

    for alert in &alerts {
        let sev_tag = match alert.severity {
            Severity::Critical => "[CRIT]".red().bold(),
            Severity::Warning => "[WARN]".yellow().bold(),
        };
        let msg = match alert.severity {
            Severity::Critical => alert.message.red().to_string(),
            Severity::Warning => alert.message.yellow().to_string(),
        };
        println!("  {} {} {}", alert.timestamp.dimmed(), sev_tag, msg);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Flock & Lineage commands
// ---------------------------------------------------------------------------

async fn cmd_bloodline_add(base: &str, name: String, source: String, notes: Option<String>) -> anyhow::Result<()> {
    let body = CreateBloodline { name, source, notes };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/bloodlines"))
        .json(&body)
        .send()
        .await?;
    let bl: Bloodline = resp.json().await?;
    println!("{} bloodline #{} \"{}\"", "Created".green().bold(), bl.id, bl.name);
    Ok(())
}

async fn cmd_bloodline_list(base: &str) -> anyhow::Result<()> {
    let resp = reqwest::get(format!("{base}/api/bloodlines")).await?;
    let list: Vec<Bloodline> = resp.json().await?;
    if list.is_empty() {
        println!("{}", "No bloodlines yet.".dimmed());
        return Ok(());
    }
    println!("{}", "Bloodlines".bold().underline());
    println!();
    println!("  {:<5} {:<20} {:<20} {}", "ID".bold(), "Name".bold(), "Source".bold(), "Notes".bold());
    println!("  {}", "-".repeat(60));
    for bl in &list {
        println!(
            "  {:<5} {:<20} {:<20} {}",
            bl.id,
            bl.name,
            bl.source,
            bl.notes.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

fn parse_sex(s: &str) -> Sex {
    match s.to_lowercase().as_str() {
        "male" | "m" => Sex::Male,
        "female" | "f" => Sex::Female,
        _ => Sex::Unknown,
    }
}

async fn cmd_bird_add(
    base: &str,
    band: Option<String>,
    sex: String,
    bloodline: i64,
    hatch_date: Option<String>,
    mother: Option<i64>,
    father: Option<i64>,
    generation: u32,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let hatch = match hatch_date {
        Some(s) => chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d")?,
        None => Local::now().date_naive(),
    };
    let body = CreateBird {
        band_color: band,
        sex: parse_sex(&sex),
        bloodline_id: bloodline,
        hatch_date: hatch,
        mother_id: mother,
        father_id: father,
        generation,
        status: BirdStatus::Active,
        notes,
    };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/birds"))
        .json(&body)
        .send()
        .await?;
    let bird: quailsync_common::Bird = resp.json().await?;
    println!(
        "{} bird #{} ({:?}, bloodline #{})",
        "Created".green().bold(),
        bird.id,
        bird.sex,
        bird.bloodline_id,
    );
    Ok(())
}

async fn cmd_bird_list(base: &str) -> anyhow::Result<()> {
    let resp = reqwest::get(format!("{base}/api/birds")).await?;
    let list: Vec<quailsync_common::Bird> = resp.json().await?;
    if list.is_empty() {
        println!("{}", "No birds yet.".dimmed());
        return Ok(());
    }
    println!("{}", "Birds".bold().underline());
    println!();
    println!(
        "  {:<5} {:<10} {:<8} {:<10} {:<12} {:<8} {}",
        "ID".bold(), "Band".bold(), "Sex".bold(), "Bloodline".bold(),
        "Hatch Date".bold(), "Gen".bold(), "Status".bold(),
    );
    println!("  {}", "-".repeat(70));
    for b in &list {
        println!(
            "  {:<5} {:<10} {:<8} {:<10} {:<12} {:<8} {:?}",
            b.id,
            b.band_color.as_deref().unwrap_or("-"),
            format!("{:?}", b.sex),
            b.bloodline_id,
            b.hatch_date,
            b.generation,
            b.status,
        );
    }
    Ok(())
}

#[derive(Deserialize)]
struct FlockSummaryResp {
    total_birds: i64,
    active_birds: i64,
    males: i64,
    females: i64,
    bloodlines: Vec<BloodlineCountResp>,
}

#[derive(Deserialize)]
struct BloodlineCountResp {
    name: String,
    count: i64,
}

async fn cmd_flock_summary(base: &str) -> anyhow::Result<()> {
    let resp = reqwest::get(format!("{base}/api/flock/summary")).await?;
    let s: FlockSummaryResp = resp.json().await?;

    println!("{}", "Flock Summary".bold().underline());
    println!();
    println!("  Total birds:   {}", s.total_birds);
    println!("  Active birds:  {}", s.active_birds);
    println!("  Males:         {}", s.males);
    println!("  Females:       {}", s.females);

    if !s.bloodlines.is_empty() {
        println!();
        println!("  {}", "By Bloodline".bold());
        for bl in &s.bloodlines {
            println!("    {:<20} {}", bl.name, bl.count);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Clutch & Incubation commands
// ---------------------------------------------------------------------------

async fn cmd_clutch_add(
    base: &str,
    bloodline: Option<i64>,
    eggs: u32,
    set_date: Option<String>,
    pair: Option<i64>,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let set = match set_date {
        Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")?,
        None => Local::now().date_naive(),
    };
    let body = CreateClutch {
        breeding_pair_id: pair,
        bloodline_id: bloodline,
        eggs_set: eggs,
        eggs_fertile: None,
        eggs_hatched: None,
        set_date: set,
        status: ClutchStatus::Incubating,
        notes,
    };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/clutches"))
        .json(&body)
        .send()
        .await?;
    let clutch: Clutch = resp.json().await?;
    println!(
        "{} clutch #{} — {} eggs set on {}, expected hatch {}",
        "Created".green().bold(),
        clutch.id,
        clutch.eggs_set,
        clutch.set_date,
        clutch.expected_hatch_date,
    );
    Ok(())
}

async fn cmd_clutch_list(base: &str) -> anyhow::Result<()> {
    let clutches: Vec<Clutch> = reqwest::get(format!("{base}/api/clutches"))
        .await?
        .json()
        .await?;
    if clutches.is_empty() {
        println!("{}", "No clutches yet.".dimmed());
        return Ok(());
    }

    // Fetch bloodlines for name lookup
    let bloodlines: Vec<Bloodline> = reqwest::get(format!("{base}/api/bloodlines"))
        .await?
        .json()
        .await?;

    let today = Local::now().date_naive();

    println!("{}", "Clutches".bold().underline());
    println!();
    println!(
        "  {:<4} {:<16} {:<6} {:<8} {:<8} {:<12} {:<12} {:<12} {}",
        "ID".bold(),
        "Bloodline".bold(),
        "Eggs".bold(),
        "Fertile".bold(),
        "Hatched".bold(),
        "Set Date".bold(),
        "Hatch Date".bold(),
        "Status".bold(),
        "Remaining".bold(),
    );
    println!("  {}", "-".repeat(95));

    for c in &clutches {
        let bl_name = bloodlines
            .iter()
            .find(|b| Some(b.id) == c.bloodline_id)
            .map(|b| b.name.as_str())
            .unwrap_or("-");

        let remaining = match c.status {
            ClutchStatus::Hatched => "Hatched".to_string(),
            ClutchStatus::Failed => "Failed".to_string(),
            ClutchStatus::Incubating => {
                let days = (c.expected_hatch_date - today).num_days();
                if days > 0 {
                    format!("{days}d")
                } else if days == 0 {
                    "Today!".to_string()
                } else {
                    format!("{}d overdue", -days)
                }
            }
        };

        let fertile_str = c
            .eggs_fertile
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".into());
        let hatched_str = c
            .eggs_hatched
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".into());

        println!(
            "  {:<4} {:<16} {:<6} {:<8} {:<8} {:<12} {:<12} {:<12} {}",
            c.id,
            bl_name,
            c.eggs_set,
            fertile_str,
            hatched_str,
            c.set_date,
            c.expected_hatch_date,
            format!("{:?}", c.status),
            remaining,
        );
    }
    Ok(())
}

async fn cmd_clutch_update(
    base: &str,
    id: i64,
    fertile: Option<u32>,
    hatched: Option<u32>,
    status_str: Option<String>,
    notes: Option<String>,
) -> anyhow::Result<()> {
    // If hatched count is provided and no explicit status, auto-set to Hatched
    let status = match (&status_str, &hatched) {
        (Some(s), _) => Some(match s.to_lowercase().as_str() {
            "failed" => ClutchStatus::Failed,
            "hatched" => ClutchStatus::Hatched,
            "incubating" => ClutchStatus::Incubating,
            _ => anyhow::bail!("unknown status: {s} (use incubating/hatched/failed)"),
        }),
        (None, Some(_)) => Some(ClutchStatus::Hatched),
        _ => None,
    };

    let body = UpdateClutch {
        eggs_fertile: fertile,
        eggs_hatched: hatched,
        status,
        notes,
    };

    let resp = reqwest::Client::new()
        .put(format!("{base}/api/clutches/{id}"))
        .json(&body)
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("clutch #{id} not found");
    }

    let clutch: Clutch = resp.json().await?;
    println!(
        "{} clutch #{} — {:?}, fertile: {}, hatched: {}",
        "Updated".green().bold(),
        clutch.id,
        clutch.status,
        clutch
            .eggs_fertile
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".into()),
        clutch
            .eggs_hatched
            .map(|n| n.to_string())
            .unwrap_or_else(|| "-".into()),
    );
    Ok(())
}

async fn cmd_clutch_schedule(base: &str) -> anyhow::Result<()> {
    let clutches: Vec<Clutch> = reqwest::get(format!("{base}/api/clutches"))
        .await?
        .json()
        .await?;

    let active: Vec<&Clutch> = clutches
        .iter()
        .filter(|c| c.status == ClutchStatus::Incubating)
        .collect();

    if active.is_empty() {
        println!("{}", "No active clutches.".dimmed());
        return Ok(());
    }

    let today = Local::now().date_naive();

    println!("{}", "Incubation Schedule".bold().underline());
    println!();

    for c in &active {
        let candle_1 = c.set_date + chrono::Duration::days(7);
        let candle_2 = c.set_date + chrono::Duration::days(14);
        let lockdown = c.set_date + chrono::Duration::days(14);
        let hatch = c.expected_hatch_date;

        println!(
            "  Clutch #{} — {} eggs, set {}",
            c.id, c.eggs_set, c.set_date,
        );

        let events: Vec<(&str, NaiveDate)> = vec![
            ("Candle (day 7)", candle_1),
            ("Candle + Lockdown (day 14)", candle_2),
            ("Expected Hatch (day 17)", hatch),
        ];

        for (label, date) in &events {
            let days_away = (*date - today).num_days();
            let line = format!("    {:<30} {}", label, date);
            if days_away > 0 {
                println!("{}", line.green());
            } else if days_away == 0 {
                println!("{}", line.yellow().bold());
            } else {
                println!("{}", line.dimmed());
            }
        }

        // Suppress unused variable warning — lockdown == candle_2 intentionally
        let _ = lockdown;

        println!();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Breeding commands
// ---------------------------------------------------------------------------

async fn cmd_breeding_suggest(base: &str) -> anyhow::Result<()> {
    let pairs: Vec<InbreedingCoefficient> =
        reqwest::get(format!("{base}/api/breeding/suggest"))
            .await?
            .json()
            .await?;

    if pairs.is_empty() {
        println!("{}", "No breeding pairs possible (need active males and females).".dimmed());
        return Ok(());
    }

    // Fetch birds + bloodlines for display info
    let birds: Vec<Bird> = reqwest::get(format!("{base}/api/birds"))
        .await?
        .json()
        .await?;
    let bloodlines: Vec<Bloodline> = reqwest::get(format!("{base}/api/bloodlines"))
        .await?
        .json()
        .await?;

    let bird_label = |id: i64| -> String {
        birds
            .iter()
            .find(|b| b.id == id)
            .map(|b| {
                let band = b.band_color.as_deref().unwrap_or("-");
                format!("#{} ({})", b.id, band)
            })
            .unwrap_or_else(|| format!("#{id}"))
    };

    let bird_bloodline_name = |id: i64| -> String {
        birds
            .iter()
            .find(|b| b.id == id)
            .and_then(|b| bloodlines.iter().find(|bl| bl.id == b.bloodline_id))
            .map(|bl| bl.name.clone())
            .unwrap_or_else(|| "?".into())
    };

    println!("{}", "Breeding Pair Suggestions".bold().underline());
    println!();
    println!(
        "  {:<14} {:<16} {:<14} {:<16} {:<8} {}",
        "Male".bold(),
        "Bloodline".bold(),
        "Female".bold(),
        "Bloodline".bold(),
        "Coeff".bold(),
        "Safe".bold(),
    );
    println!("  {}", "-".repeat(78));

    for p in &pairs {
        let male_lbl = bird_label(p.male_id);
        let male_bl = bird_bloodline_name(p.male_id);
        let female_lbl = bird_label(p.female_id);
        let female_bl = bird_bloodline_name(p.female_id);
        let coeff_str = format!("{:.3}", p.coefficient);

        let line = format!(
            "  {:<14} {:<16} {:<14} {:<16} {:<8} {}",
            male_lbl,
            male_bl,
            female_lbl,
            female_bl,
            coeff_str,
            if p.safe { "YES" } else { "NO" },
        );

        if p.safe {
            println!("{}", line.green());
        } else {
            println!("{}", line.red());
        }
    }

    let safe_count = pairs.iter().filter(|p| p.safe).count();
    println!();
    println!(
        "  {} safe pairs out of {} total",
        safe_count.to_string().green().bold(),
        pairs.len(),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Weight & Growth commands
// ---------------------------------------------------------------------------

async fn cmd_bird_weigh(base: &str, id: i64, grams: f64, notes: Option<String>) -> anyhow::Result<()> {
    let today = Local::now().date_naive();
    let body = CreateWeightRecord {
        weight_grams: grams,
        date: today,
        notes,
    };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/birds/{id}/weight"))
        .json(&body)
        .send()
        .await?;
    let rec: WeightRecord = resp.json().await?;
    println!(
        "{} weight record for bird #{}: {:.1}g on {}",
        "Recorded".green().bold(),
        rec.bird_id,
        rec.weight_grams,
        rec.date,
    );
    Ok(())
}

async fn cmd_bird_growth(base: &str, id: i64) -> anyhow::Result<()> {
    let weights: Vec<WeightRecord> = reqwest::get(format!("{base}/api/birds/{id}/weights"))
        .await?
        .json()
        .await?;

    if weights.is_empty() {
        println!("{}", "No weight records for this bird.".dimmed());
        return Ok(());
    }

    println!("{}", format!("Growth History — Bird #{id}").bold().underline());
    println!();
    println!(
        "  {:<12} {:>10} {}",
        "Date".bold(),
        "Weight (g)".bold(),
        "Notes".bold(),
    );
    println!("  {}", "-".repeat(50));

    for w in &weights {
        println!(
            "  {:<12} {:>10.1} {}",
            w.date,
            w.weight_grams,
            w.notes.as_deref().unwrap_or(""),
        );
    }

    // Trend from last 3 readings (weights are date DESC)
    if weights.len() >= 2 {
        println!();
        let latest = weights[0].weight_grams;
        let previous = weights[1].weight_grams;
        let diff = latest - previous;

        let trend = if diff > 5.0 {
            format!("^ Gaining (+{:.1}g)", diff).green().to_string()
        } else if diff < -5.0 {
            format!("v Losing ({:.1}g)", diff).red().to_string()
        } else {
            format!("- Stable ({:+.1}g)", diff).yellow().to_string()
        };

        println!("  Trend: {}", trend);
    }

    // Compare to butcher weight
    let latest = weights[0].weight_grams;
    let pct = (latest / COTURNIX_BUTCHER_WEIGHT_GRAMS) * 100.0;
    println!();
    if latest >= COTURNIX_BUTCHER_WEIGHT_GRAMS {
        println!(
            "  {} Butcher weight reached ({:.1}g / {:.0}g = {:.0}%)",
            ">>>".green().bold(),
            latest,
            COTURNIX_BUTCHER_WEIGHT_GRAMS,
            pct,
        );
    } else {
        let remaining = COTURNIX_BUTCHER_WEIGHT_GRAMS - latest;
        println!(
            "  Butcher weight: {:.1}g / {:.0}g ({:.0}%) — {:.1}g to go",
            latest, COTURNIX_BUTCHER_WEIGHT_GRAMS, pct, remaining,
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Processing commands
// ---------------------------------------------------------------------------

fn parse_processing_reason(s: &str) -> ProcessingReason {
    match s.to_lowercase().replace('_', "-").as_str() {
        "excess-male" | "excessmale" => ProcessingReason::ExcessMale,
        "low-weight" | "lowweight" => ProcessingReason::LowWeight,
        "poor-genetics" | "poorgenetics" => ProcessingReason::PoorGenetics,
        "age" => ProcessingReason::Age,
        _ => ProcessingReason::Other,
    }
}

async fn cmd_processing_schedule(
    base: &str,
    bird: i64,
    reason: String,
    date: Option<String>,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let scheduled_date = match date {
        Some(s) => NaiveDate::parse_from_str(&s, "%Y-%m-%d")?,
        None => Local::now().date_naive(),
    };
    let body = CreateProcessingRecord {
        bird_id: bird,
        reason: parse_processing_reason(&reason),
        scheduled_date,
        notes,
    };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/processing"))
        .json(&body)
        .send()
        .await?;
    let rec: ProcessingRecord = resp.json().await?;
    println!(
        "{} processing #{} — bird #{}, {:?}, scheduled {}",
        "Scheduled".green().bold(),
        rec.id,
        rec.bird_id,
        rec.reason,
        rec.scheduled_date,
    );
    Ok(())
}

async fn cmd_processing_complete(
    base: &str,
    id: i64,
    weight: Option<f64>,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let today = Local::now().date_naive();
    let body = UpdateProcessingRecord {
        processed_date: Some(today),
        final_weight_grams: weight,
        status: Some(ProcessingStatus::Completed),
        notes,
    };
    let resp = reqwest::Client::new()
        .put(format!("{base}/api/processing/{id}"))
        .json(&body)
        .send()
        .await?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!("processing record #{id} not found");
    }

    let rec: ProcessingRecord = resp.json().await?;

    // Also mark the bird as Culled
    let _ = reqwest::Client::new()
        .put(format!("{base}/api/birds/{}", rec.bird_id))
        .json(&quailsync_common::UpdateBird {
            status: Some(BirdStatus::Culled),
            notes: None,
        })
        .send()
        .await;

    println!(
        "{} processing #{} — bird #{} marked Culled{}",
        "Completed".green().bold(),
        rec.id,
        rec.bird_id,
        weight
            .map(|w| format!(", final weight {:.1}g", w))
            .unwrap_or_default(),
    );
    Ok(())
}

async fn cmd_processing_list(base: &str) -> anyhow::Result<()> {
    let records: Vec<ProcessingRecord> = reqwest::get(format!("{base}/api/processing"))
        .await?
        .json()
        .await?;

    if records.is_empty() {
        println!("{}", "No processing records.".dimmed());
        return Ok(());
    }

    println!("{}", "Processing Records".bold().underline());
    println!();
    println!(
        "  {:<5} {:<7} {:<14} {:<12} {:<12} {:<10} {}",
        "ID".bold(),
        "Bird#".bold(),
        "Reason".bold(),
        "Scheduled".bold(),
        "Processed".bold(),
        "Weight".bold(),
        "Status".bold(),
    );
    println!("  {}", "-".repeat(75));

    for r in &records {
        println!(
            "  {:<5} {:<7} {:<14} {:<12} {:<12} {:<10} {:?}",
            r.id,
            r.bird_id,
            format!("{:?}", r.reason),
            r.scheduled_date,
            r.processed_date
                .map(|d| d.to_string())
                .unwrap_or_else(|| "-".into()),
            r.final_weight_grams
                .map(|w| format!("{:.1}g", w))
                .unwrap_or_else(|| "-".into()),
            r.status,
        );
    }
    Ok(())
}

async fn cmd_processing_queue(base: &str) -> anyhow::Result<()> {
    let records: Vec<ProcessingRecord> = reqwest::get(format!("{base}/api/processing/queue"))
        .await?
        .json()
        .await?;

    if records.is_empty() {
        println!("{}", "Processing queue is empty.".dimmed());
        return Ok(());
    }

    println!("{}", "Processing Queue (Scheduled)".bold().underline());
    println!();
    println!(
        "  {:<5} {:<7} {:<14} {:<12} {}",
        "ID".bold(),
        "Bird#".bold(),
        "Reason".bold(),
        "Scheduled".bold(),
        "Notes".bold(),
    );
    println!("  {}", "-".repeat(55));

    for r in &records {
        println!(
            "  {:<5} {:<7} {:<14} {:<12} {}",
            r.id,
            r.bird_id,
            format!("{:?}", r.reason),
            r.scheduled_date,
            r.notes.as_deref().unwrap_or(""),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Breeding group commands
// ---------------------------------------------------------------------------

async fn cmd_breeding_group_create(
    base: &str,
    name: String,
    male: i64,
    females_str: String,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let female_ids: Vec<i64> = females_str
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if female_ids.is_empty() {
        anyhow::bail!("no valid female IDs provided");
    }

    let today = Local::now().date_naive();
    let body = CreateBreedingGroup {
        name,
        male_id: male,
        female_ids: female_ids.clone(),
        start_date: today,
        notes,
    };

    let resp = reqwest::Client::new()
        .post(format!("{base}/api/breeding-groups"))
        .json(&body)
        .send()
        .await?;

    #[derive(Deserialize)]
    struct GroupResp {
        id: i64,
        name: String,
        male_id: i64,
        female_ids: Vec<i64>,
        warning: Option<String>,
    }

    let g: GroupResp = resp.json().await?;
    println!(
        "{} breeding group #{} \"{}\" — male #{}, {} females {:?}",
        "Created".green().bold(),
        g.id,
        g.name,
        g.male_id,
        g.female_ids.len(),
        g.female_ids,
    );
    if let Some(w) = g.warning {
        println!("  {}", w.yellow());
    }
    Ok(())
}

async fn cmd_breeding_group_list(base: &str) -> anyhow::Result<()> {
    let groups: Vec<BreedingGroup> = reqwest::get(format!("{base}/api/breeding-groups"))
        .await?
        .json()
        .await?;

    if groups.is_empty() {
        println!("{}", "No breeding groups.".dimmed());
        return Ok(());
    }

    println!("{}", "Breeding Groups".bold().underline());
    println!();
    println!(
        "  {:<5} {:<16} {:<8} {:<20} {:<12} {}",
        "ID".bold(),
        "Name".bold(),
        "Male#".bold(),
        "Females".bold(),
        "Start Date".bold(),
        "Ratio".bold(),
    );
    println!("  {}", "-".repeat(70));

    for g in &groups {
        let females_str = g
            .female_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let ratio = format!("1:{}", g.female_ids.len());
        println!(
            "  {:<5} {:<16} {:<8} {:<20} {:<12} {}",
            g.id, g.name, g.male_id, females_str, g.start_date, ratio,
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Cull review command
// ---------------------------------------------------------------------------

async fn cmd_flock_cull_review(base: &str) -> anyhow::Result<()> {
    let recs: Vec<CullRecommendation> =
        reqwest::get(format!("{base}/api/flock/cull-recommendations"))
            .await?
            .json()
            .await?;

    if recs.is_empty() {
        println!("{}", "No cull recommendations — flock looks good!".green());
        return Ok(());
    }

    // Fetch birds and bloodlines for display
    let birds: Vec<Bird> = reqwest::get(format!("{base}/api/birds"))
        .await?
        .json()
        .await?;
    let bloodlines: Vec<Bloodline> = reqwest::get(format!("{base}/api/bloodlines"))
        .await?
        .json()
        .await?;

    let bird_label = |id: i64| -> String {
        birds
            .iter()
            .find(|b| b.id == id)
            .map(|b| {
                let band = b.band_color.as_deref().unwrap_or("-");
                let bl_name = bloodlines
                    .iter()
                    .find(|bl| bl.id == b.bloodline_id)
                    .map(|bl| bl.name.as_str())
                    .unwrap_or("?");
                format!("#{} ({}, {})", b.id, band, bl_name)
            })
            .unwrap_or_else(|| format!("#{id}"))
    };

    println!("{}", "Cull Recommendations".red().bold().underline());
    println!();

    // Group by reason type
    let excess: Vec<&CullRecommendation> = recs
        .iter()
        .filter(|r| matches!(r.reason, CullReason::ExcessMale))
        .collect();
    let low_weight: Vec<&CullRecommendation> = recs
        .iter()
        .filter(|r| matches!(r.reason, CullReason::LowWeight { .. }))
        .collect();
    let inbreeding: Vec<&CullRecommendation> = recs
        .iter()
        .filter(|r| matches!(r.reason, CullReason::HighInbreeding { .. }))
        .collect();

    if !excess.is_empty() {
        println!("  {}", "EXCESS MALES".red().bold());
        for r in &excess {
            println!("    {} — surplus male", bird_label(r.bird_id).red());
        }
        println!();
    }

    if !low_weight.is_empty() {
        println!("  {}", "LOW WEIGHT".red().bold());
        for r in &low_weight {
            if let CullReason::LowWeight { weight_grams } = &r.reason {
                println!(
                    "    {} — {:.1}g (min 200g)",
                    bird_label(r.bird_id).red(),
                    weight_grams,
                );
            }
        }
        println!();
    }

    if !inbreeding.is_empty() {
        println!("  {}", "HIGH INBREEDING RISK".red().bold());
        for r in &inbreeding {
            if let CullReason::HighInbreeding { coefficient } = &r.reason {
                println!(
                    "    {} — no safe pairings (worst coeff: {:.3})",
                    bird_label(r.bird_id).red(),
                    coefficient,
                );
            }
        }
        println!();
    }

    println!(
        "  {} birds flagged for review",
        recs.len().to_string().red().bold(),
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Camera commands
// ---------------------------------------------------------------------------

async fn cmd_camera_add(base: &str, name: String, location: String, url: String) -> anyhow::Result<()> {
    let body = CreateCameraFeed {
        name,
        location,
        feed_url: url,
        status: CameraStatus::Active,
        brooder_id: None,
    };
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/cameras"))
        .json(&body)
        .send()
        .await?;
    let cam: CameraFeed = resp.json().await?;
    println!(
        "{} camera #{} \"{}\" at {}",
        "Registered".green().bold(),
        cam.id,
        cam.name,
        cam.location,
    );
    Ok(())
}

async fn cmd_camera_list(base: &str) -> anyhow::Result<()> {
    let cameras: Vec<CameraFeed> = reqwest::get(format!("{base}/api/cameras"))
        .await?
        .json()
        .await?;

    if cameras.is_empty() {
        println!("{}", "No cameras registered.".dimmed());
        return Ok(());
    }

    println!("{}", "Camera Feeds".bold().underline());
    println!();
    println!(
        "  {:<5} {:<18} {:<16} {:<36} {}",
        "ID".bold(),
        "Name".bold(),
        "Location".bold(),
        "URL".bold(),
        "Status".bold(),
    );
    println!("  {}", "-".repeat(85));

    for c in &cameras {
        let status_str = match c.status {
            CameraStatus::Active => "Active".green().to_string(),
            CameraStatus::Offline => "Offline".red().to_string(),
        };
        println!(
            "  {:<5} {:<18} {:<16} {:<36} {}",
            c.id, c.name, c.location, c.feed_url, status_str,
        );
    }
    Ok(())
}

async fn cmd_camera_status(base: &str) -> anyhow::Result<()> {
    let cameras: Vec<CameraFeed> = reqwest::get(format!("{base}/api/cameras"))
        .await?
        .json()
        .await?;

    if cameras.is_empty() {
        println!("{}", "No cameras registered.".dimmed());
        return Ok(());
    }

    println!("{}", "Camera Status".bold().underline());
    println!();

    for c in &cameras {
        let dot = match c.status {
            CameraStatus::Active => "●".green(),
            CameraStatus::Offline => "●".red(),
        };

        // Fetch last frame for this camera
        let frames: Vec<FrameCapture> = reqwest::get(format!(
            "{base}/api/frames?camera_id={}&minutes=1440",
            c.id
        ))
        .await?
        .json()
        .await?;

        let last_frame = if let Some(f) = frames.first() {
            format!("{} ({:?})", f.timestamp.format("%Y-%m-%d %H:%M"), f.life_stage)
        } else {
            "no frames".dimmed().to_string()
        };

        println!(
            "  {} #{} {} — {} — last: {}",
            dot, c.id, c.name, c.location, last_frame,
        );
    }

    Ok(())
}

fn parse_life_stage(s: &str) -> LifeStage {
    match s.to_lowercase().as_str() {
        "chick" => LifeStage::Chick,
        "adolescent" => LifeStage::Adolescent,
        "adult" => LifeStage::Adult,
        _ => LifeStage::Chick,
    }
}

async fn cmd_brooder_add(
    base: &str,
    name: String,
    bloodline: Option<i64>,
    stage: String,
    qr: Option<String>,
    notes: Option<String>,
) -> anyhow::Result<()> {
    let body = CreateBrooder {
        name,
        bloodline_id: bloodline,
        life_stage: parse_life_stage(&stage),
        qr_code: qr.unwrap_or_default(),
        notes,
    };
    let client = reqwest::Client::new();
    let resp: Brooder = client
        .post(format!("{base}/api/brooders"))
        .json(&body)
        .send()
        .await?
        .json()
        .await?;
    println!(
        "{} Brooder #{} '{}' created ({:?})",
        "OK".green().bold(),
        resp.id,
        resp.name,
        resp.life_stage,
    );
    Ok(())
}

async fn cmd_brooder_list(base: &str) -> anyhow::Result<()> {
    let brooders: Vec<Brooder> = reqwest::get(format!("{base}/api/brooders"))
        .await?
        .json()
        .await?;

    if brooders.is_empty() {
        println!("{}", "No brooders registered.".dimmed());
        return Ok(());
    }

    println!("{}", "Brooders".bold().underline());
    println!();
    println!(
        "  {:<5} {:<20} {:<12} {:<14} {}",
        "ID".bold(),
        "Name".bold(),
        "Stage".bold(),
        "Bloodline".bold(),
        "QR Code".bold(),
    );
    println!("  {}", "-".repeat(65));

    for b in &brooders {
        let stage_str = format!("{:?}", b.life_stage);
        let bl = b
            .bloodline_id
            .map(|id| format!("#{id}"))
            .unwrap_or_else(|| "-".to_string());
        let qr = if b.qr_code.is_empty() {
            "-".to_string()
        } else {
            b.qr_code.clone()
        };
        println!(
            "  {:<5} {:<20} {:<12} {:<14} {}",
            b.id, b.name, stage_str, bl, qr,
        );
    }
    Ok(())
}

async fn cmd_brooder_qr(base: &str, id: i64) -> anyhow::Result<()> {
    let brooder: Brooder = reqwest::get(format!("{base}/api/brooders"))
        .await?
        .json::<Vec<Brooder>>()
        .await?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or_else(|| anyhow::anyhow!("Brooder #{id} not found"))?;

    let data = if brooder.qr_code.is_empty() {
        format!("brooder-{}", brooder.id)
    } else {
        brooder.qr_code.clone()
    };

    let code = QrCode::new(data.as_bytes())?;
    let svg = code.render::<qrcode::render::svg::Color>()
        .min_dimensions(200, 200)
        .build();

    let filename = format!("brooder-{}-qr.svg", brooder.id);
    std::fs::write(&filename, &svg)?;
    println!(
        "{} QR code saved to {} (data: \"{}\")",
        "OK".green().bold(),
        filename.bold(),
        data,
    );
    Ok(())
}

async fn cmd_camera_point(base: &str, id: i64, brooder: i64) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    client
        .put(format!("{base}/api/cameras/{id}/brooder"))
        .json(&serde_json::json!({ "brooder_id": brooder }))
        .send()
        .await?;
    println!(
        "{} Camera #{} now pointing at Brooder #{}",
        "OK".green().bold(),
        id,
        brooder,
    );
    Ok(())
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let base = cli.server.trim_end_matches('/');

    let result = match cli.command {
        Commands::Status => cmd_status(base).await,
        Commands::Brood { history: None } => cmd_brood_latest(base).await,
        Commands::Brood {
            history: Some(mins),
        } => cmd_brood_history(base, mins).await,
        Commands::System => cmd_system(base).await,
        Commands::Alerts { minutes } => cmd_alerts(base, minutes).await,
        Commands::Bloodline { action } => match action {
            BloodlineAction::Add { name, source, notes } => {
                cmd_bloodline_add(base, name, source, notes).await
            }
            BloodlineAction::List => cmd_bloodline_list(base).await,
        },
        Commands::Bird { action } => match action {
            BirdAction::Add {
                band, sex, bloodline, hatch_date, mother, father, generation, notes,
            } => {
                cmd_bird_add(base, band, sex, bloodline, hatch_date, mother, father, generation, notes).await
            }
            BirdAction::List => cmd_bird_list(base).await,
            BirdAction::Weigh { id, grams, notes } => cmd_bird_weigh(base, id, grams, notes).await,
            BirdAction::Growth { id } => cmd_bird_growth(base, id).await,
        },
        Commands::Flock { action } => match action {
            FlockAction::Summary => cmd_flock_summary(base).await,
            FlockAction::CullReview => cmd_flock_cull_review(base).await,
        },
        Commands::Clutch { action } => match action {
            ClutchAction::Add {
                bloodline, eggs, set_date, pair, notes,
            } => cmd_clutch_add(base, bloodline, eggs, set_date, pair, notes).await,
            ClutchAction::List => cmd_clutch_list(base).await,
            ClutchAction::Update {
                id, fertile, hatched, status, notes,
            } => cmd_clutch_update(base, id, fertile, hatched, status, notes).await,
            ClutchAction::Schedule => cmd_clutch_schedule(base).await,
        },
        Commands::Breeding { action } => match action {
            BreedingAction::Suggest => cmd_breeding_suggest(base).await,
            BreedingAction::Group { action: ga } => match ga {
                BreedingGroupAction::Create { name, male, females, notes } => {
                    cmd_breeding_group_create(base, name, male, females, notes).await
                }
                BreedingGroupAction::List => cmd_breeding_group_list(base).await,
            },
        },
        Commands::Processing { action } => match action {
            ProcessingAction::Schedule { bird, reason, date, notes } => {
                cmd_processing_schedule(base, bird, reason, date, notes).await
            }
            ProcessingAction::Complete { id, weight, notes } => {
                cmd_processing_complete(base, id, weight, notes).await
            }
            ProcessingAction::List => cmd_processing_list(base).await,
            ProcessingAction::Queue => cmd_processing_queue(base).await,
        },
        Commands::Camera { action } => match action {
            CameraAction::Add { name, location, url } => {
                cmd_camera_add(base, name, location, url).await
            }
            CameraAction::List => cmd_camera_list(base).await,
            CameraAction::Status => cmd_camera_status(base).await,
            CameraAction::Point { id, brooder } => cmd_camera_point(base, id, brooder).await,
        },
        Commands::Brooder { action } => match action {
            BrooderAction::Add { name, bloodline, stage, qr, notes } => {
                cmd_brooder_add(base, name, bloodline, stage, qr, notes).await
            }
            BrooderAction::List => cmd_brooder_list(base).await,
            BrooderAction::Qr { id } => cmd_brooder_qr(base, id).await,
        },
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}
