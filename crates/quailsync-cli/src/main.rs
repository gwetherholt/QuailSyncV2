use chrono::{Local, NaiveDate};
use clap::{Parser, Subcommand};
use colored::Colorize;
use quailsync_common::{
    Alert, BirdStatus, Bloodline, BrooderReading, Clutch, ClutchStatus, CreateBird,
    CreateBloodline, CreateClutch, Sex, Severity, SystemMetrics, UpdateClutch,
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
}

#[derive(Subcommand)]
enum FlockAction {
    /// Show flock summary
    Summary,
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
        },
        Commands::Flock { action } => match action {
            FlockAction::Summary => cmd_flock_summary(base).await,
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
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}
