use std::sync::atomic::Ordering;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use quailsync_common::TelemetryPayload;
use tokio::sync::broadcast;

use crate::alerts::check_brooder_alerts;
use crate::db::store_payload;
use crate::state::{acquire_db, touch_brooder, AppState};

pub async fn ws_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    println!("[ws] agent connected");
    state.agent_connected.store(true, Ordering::Relaxed);

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => match serde_json::from_str::<TelemetryPayload>(&text) {
                Ok(payload) => {
                    log_payload(&payload);
                    let conn = acquire_db(&state);
                    store_payload(&conn, &payload);
                    if let TelemetryPayload::Brooder(ref reading) = payload {
                        check_brooder_alerts(&conn, reading, &state.alert_config);
                        if let Some(bid) = reading.brooder_id {
                            touch_brooder(&state, bid);
                        }
                    }
                    let _ = state.live_tx.send(text.to_string());
                }
                Err(e) => eprintln!("[ws] bad payload: {e}"),
            },
            Message::Close(_) => {
                println!("[ws] agent disconnected");
                break;
            }
            _ => {}
        }
    }

    state.agent_connected.store(false, Ordering::Relaxed);
}

pub async fn ws_live_handler(State(state): State<AppState>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| handle_live_socket(socket, state))
}

async fn handle_live_socket(mut socket: WebSocket, state: AppState) {
    println!("[ws/live] dashboard client connected");
    let mut rx = state.live_tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        eprintln!("[ws/live] client lagged, skipped {n} messages");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    println!("[ws/live] dashboard client disconnected");
}

fn log_payload(payload: &TelemetryPayload) {
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
                "[telemetry] brooder | temp: {:.1}\u{00b0}F  humidity: {:.1}%  at {}",
                r.temperature_f, r.humidity_percent, r.timestamp,
            );
        }
        TelemetryPayload::Detection(d) => {
            println!(
                "[telemetry] detect  | {:?} ({:.1}% confidence) at {}",
                d.species,
                d.confidence * 100.0,
                d.timestamp,
            );
        }
        TelemetryPayload::CameraAnnounce(ca) => {
            println!(
                "[telemetry] camera  | brooder {} stream: {}",
                ca.brooder_id, ca.stream_url,
            );
        }
    }
}
