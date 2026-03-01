use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "quailsync", about = "QuailSync CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Check server health
    Status,
    /// Show recent telemetry
    Telemetry,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Status => println!("Checking server status..."),
        Commands::Telemetry => println!("Fetching recent telemetry..."),
    }
}
