use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct BackupInfo {
    filename: String,
    size_bytes: u64,
    created: String,
}

pub(crate) async fn create_backup() -> impl IntoResponse {
    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        std::fs::create_dir_all(backup_dir).ok();
    }
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let filename = format!("quailsync_{}.db", timestamp);
    let dest = backup_dir.join(&filename);

    match std::fs::copy("quailsync.db", &dest) {
        Ok(_) => {
            let meta = std::fs::metadata(&dest).ok();
            let size = meta.map(|m| m.len()).unwrap_or(0);
            (
                StatusCode::CREATED,
                Json(BackupInfo {
                    filename,
                    size_bytes: size,
                    created: chrono::Local::now().to_rfc3339(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Backup failed: {e}"),
        )
            .into_response(),
    }
}

pub(crate) async fn list_backups() -> Json<Vec<BackupInfo>> {
    let backup_dir = std::path::Path::new("backups");
    let mut backups = Vec::new();
    if let Ok(entries) = std::fs::read_dir(backup_dir) {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.metadata() {
                let fname = entry.file_name().to_string_lossy().to_string();
                if fname.ends_with(".db") {
                    let created = meta
                        .modified()
                        .ok()
                        .map(|t| {
                            let dt: chrono::DateTime<chrono::Local> = t.into();
                            dt.to_rfc3339()
                        })
                        .unwrap_or_default();
                    backups.push(BackupInfo {
                        filename: fname,
                        size_bytes: meta.len(),
                        created,
                    });
                }
            }
        }
    }
    backups.sort_by(|a, b| b.created.cmp(&a.created));
    Json(backups)
}

#[derive(Deserialize)]
pub(crate) struct RestoreRequest {
    filename: String,
}

pub(crate) async fn restore_backup(Json(body): Json<RestoreRequest>) -> impl IntoResponse {
    let backup_dir = std::path::Path::new("backups");
    if body.filename.contains('/')
        || body.filename.contains('\\')
        || body.filename.contains("..")
        || body.filename.contains('\0')
    {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }
    let source = backup_dir.join(&body.filename);
    if !source.exists() {
        return (StatusCode::NOT_FOUND, "Backup file not found").into_response();
    }
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let pre_restore = backup_dir.join(format!("quailsync_pre_restore_{}.db", timestamp));
    std::fs::copy("quailsync.db", &pre_restore).ok();

    match std::fs::copy(&source, "quailsync.db") {
        Ok(_) => (
            StatusCode::OK,
            "Database restored. Restart server to apply.",
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Restore failed: {e}"),
        )
            .into_response(),
    }
}

pub fn auto_backup_if_needed() {
    let backup_dir = std::path::Path::new("backups");
    if !backup_dir.exists() {
        std::fs::create_dir_all(backup_dir).ok();
    }
    let db_path = std::path::Path::new("quailsync.db");
    if !db_path.exists() {
        return;
    }
    let should_backup = match std::fs::read_dir(backup_dir) {
        Ok(entries) => {
            let latest = entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".db"))
                .filter_map(|e| e.metadata().ok()?.modified().ok())
                .max();
            match latest {
                Some(t) => {
                    std::time::SystemTime::now()
                        .duration_since(t)
                        .unwrap_or_default()
                        .as_secs()
                        > 86400
                }
                None => true,
            }
        }
        Err(_) => true,
    };
    if should_backup {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let dest = backup_dir.join(format!("quailsync_auto_{}.db", timestamp));
        match std::fs::copy("quailsync.db", &dest) {
            Ok(_) => println!("[backup] Auto-backup created: {}", dest.display()),
            Err(e) => eprintln!("[backup] Auto-backup failed: {e}"),
        }
    }
}
