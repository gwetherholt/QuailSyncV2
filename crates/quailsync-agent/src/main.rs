use chrono::Utc;
use futures_util::SinkExt;
use quailsync_common::{BrooderReading, SystemMetrics, TelemetryPayload};
use rand::Rng;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn mock_brooder_reading() -> TelemetryPayload {
    let mut rng = rand::rng();
    TelemetryPayload::Brooder(BrooderReading {
        // 95-100°F
        temperature_celsius: rng.random_range(95.0..=100.0),
        // 40-60%
        humidity_percent: rng.random_range(40.0..=60.0),
        timestamp: Utc::now(),
    })
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
    let url = "ws://127.0.0.1:3000/ws";
    println!("quailsync-agent connecting to {url}...");

    let (ws_stream, _) = connect_async(url).await.expect("failed to connect");
    println!("connected!");

    let (mut write, _read) = futures_util::StreamExt::split(ws_stream);
    let mut tick = 0u64;

    loop {
        let payload = if tick % 2 == 0 {
            mock_brooder_reading()
        } else {
            mock_system_metrics()
        };

        let json = serde_json::to_string(&payload).unwrap();
        println!("[send] {json}");

        if write.send(Message::Text(json.into())).await.is_err() {
            eprintln!("connection lost, exiting");
            break;
        }

        tick += 1;
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
}
