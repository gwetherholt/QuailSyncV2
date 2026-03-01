use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::Response,
    routing::get,
    Router,
};
use quailsync_common::TelemetryPayload;

async fn health() -> &'static str {
    "quailsync-server ok"
}

async fn ws_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    println!("[ws] agent connected");

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<TelemetryPayload>(&text) {
                Ok(payload) => print_payload(&payload),
                Err(e) => eprintln!("[ws] bad payload: {e}"),
            },
            Message::Close(_) => {
                println!("[ws] agent disconnected");
                break;
            }
            _ => {}
        }
    }
}

fn print_payload(payload: &TelemetryPayload) {
    match payload {
        TelemetryPayload::System(m) => {
            println!(
                "[telemetry] system  | cpu: {:.1}%  mem: {}/{}MB  disk: {}/{}GB  up: {}s",
                m.cpu_usage_percent,
                m.memory_used_bytes / 1_048_576,
                m.memory_total_bytes / 1_048_576,
                m.disk_used_bytes / 1_073_741_824,
                m.disk_total_bytes / 1_073_741_824,
                m.uptime_seconds,
            );
        }
        TelemetryPayload::Brooder(r) => {
            println!(
                "[telemetry] brooder | temp: {:.1}°F  humidity: {:.1}%  at {}",
                r.temperature_celsius, r.humidity_percent, r.timestamp,
            );
        }
        TelemetryPayload::Detection(d) => {
            println!(
                "[telemetry] detect  | {:?} ({:.1}% confidence) at {}",
                d.species, d.confidence * 100.0, d.timestamp,
            );
        }
    }
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("quailsync-server listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
