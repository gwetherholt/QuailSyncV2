use chrono::Utc;
use futures_util::SinkExt;
use quailsync_common::{BrooderReading, SystemMetrics, TelemetryPayload};
use rand::Rng;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn mock_brooder_readings() -> Vec<TelemetryPayload> {
    let mut rng = rand::rng();
    let configs: [(i64, f64, f64); 3] = [(1, 97.0, 100.0), (2, 95.0, 98.0), (3, 96.0, 99.0)];

    configs
        .iter()
        .map(|&(id, temp_min, temp_max)| {
            let (temp, humidity) = if rng.random_range(0..10) == 0 {
                // ~10% chance of out-of-range reading
                let temp = if rng.random_bool(0.5) {
                    rng.random_range(85.0..=92.0) // too cold
                } else {
                    rng.random_range(103.0..=110.0) // too hot
                };
                let humidity = if rng.random_bool(0.5) {
                    rng.random_range(20.0..=35.0) // too dry
                } else {
                    rng.random_range(65.0..=80.0) // too humid
                };
                (temp, humidity)
            } else {
                (
                    rng.random_range(temp_min..=temp_max),
                    rng.random_range(40.0..=60.0),
                )
            };

            TelemetryPayload::Brooder(BrooderReading {
                temperature_f: temp,
                humidity_percent: humidity,
                timestamp: Utc::now(),
                brooder_id: Some(id),
            })
        })
        .collect()
}

fn mock_system_metrics() -> TelemetryPayload {
    let mut rng = rand::rng();
    TelemetryPayload::System(SystemMetrics {
        cpu_usage_percent: rng.random_range(5.0..=85.0),
        memory_used_bytes: rng.random_range(512_000_000..=3_500_000_000),
        memory_total_bytes: 4_294_967_296,
        disk_used_bytes: rng.random_range(5_000_000_000..=40_000_000_000),
        disk_total_bytes: 53_687_091_200,
        uptime_seconds: rng.random_range(3600..=604800),
    })
}

#[tokio::main]
async fn main() {
    let host = std::env::var("QUAILSYNC_SERVER").unwrap_or_else(|_| "127.0.0.1:3000".to_string());
    let url = format!("ws://{host}/ws");
    println!("quailsync-agent connecting to {url}...");

    let (ws_stream, _) = connect_async(url).await.expect("failed to connect");
    println!("connected!");

    let (mut write, _read) = futures_util::StreamExt::split(ws_stream);
    let mut tick = 0u64;

    loop {
        if tick.is_multiple_of(2) {
            for payload in mock_brooder_readings() {
                let json = serde_json::to_string(&payload).unwrap();
                println!("[send] {json}");
                if write.send(Message::Text(json.into())).await.is_err() {
                    eprintln!("connection lost, exiting");
                    return;
                }
            }
        } else {
            let payload = mock_system_metrics();
            let json = serde_json::to_string(&payload).unwrap();
            println!("[send] {json}");
            if write.send(Message::Text(json.into())).await.is_err() {
                eprintln!("connection lost, exiting");
                return;
            }
        }

        tick += 1;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
