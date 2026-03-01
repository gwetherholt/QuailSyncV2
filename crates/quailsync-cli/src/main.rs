use clap::{Parser, Subcommand};
use colored::Colorize;
use quailsync_common::{Alert, BrooderReading, Severity, SystemMetrics};
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
    };

    if let Err(e) = result {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}
