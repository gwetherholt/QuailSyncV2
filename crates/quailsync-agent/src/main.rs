use quailsync_common::{BrooderReading, SystemMetrics, TelemetryPayload};

#[tokio::main]
async fn main() {
    println!("quailsync-agent starting...");

    let metrics = TelemetryPayload::System(SystemMetrics {
        cpu_usage_percent: 23.5,
        memory_used_bytes: 1_073_741_824,
        memory_total_bytes: 4_294_967_296,
        disk_used_bytes: 10_737_418_240,
        disk_total_bytes: 53_687_091_200,
        uptime_seconds: 86400,
    });

    let reading = TelemetryPayload::Brooder(BrooderReading {
        temperature_celsius: 37.5,
        humidity_percent: 55.0,
        timestamp: chrono::Utc::now(),
    });

    println!("{}", serde_json::to_string_pretty(&metrics).unwrap());
    println!("{}", serde_json::to_string_pretty(&reading).unwrap());
}
