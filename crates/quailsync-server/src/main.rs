use std::sync::{atomic::AtomicBool, Arc, Mutex};

use quailsync_common::AlertConfig;
use quailsync_server::{auto_backup_if_needed, build_app, init_db, AppState};
use rusqlite::Connection;

#[tokio::main]
async fn main() {
    auto_backup_if_needed();

    let conn = Connection::open("quailsync.db").expect("failed to open database");
    init_db(&conn);
    println!("[db] SQLite initialized (quailsync.db)");

    let alert_config = AlertConfig::default();
    println!(
        "[alerts] thresholds: temp {:.0}-{:.0}\u{00b0}F, humidity {:.0}-{:.0}%",
        alert_config.brooder_temp_min,
        alert_config.brooder_temp_max,
        alert_config.humidity_min,
        alert_config.humidity_max,
    );

    let state = AppState {
        db: Arc::new(Mutex::new(conn)),
        agent_connected: Arc::new(AtomicBool::new(false)),
        alert_config,
    };

    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    println!("quailsync-server listening on 0.0.0.0:3000");
    axum::serve(listener, app).await.unwrap();
}
