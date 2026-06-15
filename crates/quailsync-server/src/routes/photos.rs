//! Bird-photo upload handler.
//!
//! The Android app POSTs `multipart/form-data` to
//! `POST /api/birds/{id}/photo` with a single part named `photo`
//! (filename `bird_{id}.jpg`, Content-Type `image/jpeg`). This module receives
//! that exact shape, validates it, writes it to disk under a **timestamped,
//! never-overwritten** name (so we keep a per-bird history), and only then
//! records the file's path + upload time on the bird row.
//!
//! Ordering is deliberately copy-then-commit, mirroring the backup pipeline:
//! the DB is updated **after** the file is fully on disk, so `photo_path` can
//! never point at a file that isn't there.

use std::io::Write;
use std::path::PathBuf;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;

use crate::state::{acquire_db, internal_error_response, AppState, PhotoConfig};

/// Hard cap on an accepted photo. Larger uploads are rejected with 413 — the
/// intent is to stop accidental videos / wrong-files, not to be precise.
pub const MAX_PHOTO_BYTES: usize = 10 * 1024 * 1024; // 10 MB

/// Body limit applied to the upload route. Must exceed `MAX_PHOTO_BYTES` (plus
/// multipart framing overhead) so a marginally-oversized upload still reaches
/// this handler and gets a clean, specific 413 + alert — rather than being cut
/// off by Axum's generic body-limit rejection. Anything past this is so far
/// beyond a photo that Axum's blanket 413 is the right response.
pub const PHOTO_BODY_LIMIT: usize = MAX_PHOTO_BYTES + 10 * 1024 * 1024; // 20 MB

/// Build a JSON error body. The synchronous HTTP error is the PRIMARY feedback
/// path — the app shows `message` to the person uploading, inline.
fn err(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({ "error": code, "message": message }))).into_response()
}

/// `POST /api/birds/{id}/photo` — receive, validate, store a bird photo.
pub(crate) async fn upload_bird_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> Response {
    // --- Bird must exist (404 before we touch the filesystem) ----------------
    if !bird_exists(&state, id) {
        return err(
            StatusCode::NOT_FOUND,
            "bird_not_found",
            "No bird with that id exists.",
        );
    }

    // --- Locate the `photo` part ---------------------------------------------
    let mut field = loop {
        match multipart.next_field().await {
            Ok(Some(f)) if f.name() == Some("photo") => break f,
            Ok(Some(_)) => continue, // ignore any other parts
            Ok(None) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "missing_photo",
                    "Upload is missing the 'photo' part.",
                )
            }
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "malformed_upload",
                    "Could not parse the uploaded form data.",
                )
            }
        }
    };

    // Declared content-type is cheap to check and a quick reject for obviously
    // wrong files — but it's client-supplied and spoofable, so it is NOT
    // sufficient on its own (magic bytes are checked after the read).
    let declared_ct = field.content_type().map(|s| s.to_ascii_lowercase());
    if let Some(ct) = &declared_ct {
        // Tolerate parameters like "image/jpeg; charset=binary".
        let base = ct.split(';').next().unwrap_or("").trim();
        if base != "image/jpeg" && base != "image/jpg" {
            return err(
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "not_jpeg",
                "Photo must be a JPEG image.",
            );
        }
    }

    // --- Stream the bytes, enforcing the size cap as we go -------------------
    // Reading incrementally means a too-large upload is rejected without ever
    // buffering the whole thing in memory.
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match field.chunk().await {
            Ok(Some(chunk)) => {
                if buf.len() + chunk.len() > MAX_PHOTO_BYTES {
                    // Genuinely anomalous (suggests a video / wrong file) — this
                    // is the ONE rejection that also fires an ntfy alert.
                    send_oversized_alert(state.photos.clone(), id).await;
                    return err(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "too_large",
                        &format!(
                            "Photo exceeds the {} MB limit.",
                            MAX_PHOTO_BYTES / (1024 * 1024)
                        ),
                    );
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => {
                return err(
                    StatusCode::BAD_REQUEST,
                    "read_error",
                    "The upload stream ended unexpectedly.",
                )
            }
        }
    }

    // --- Validate it is actually a JPEG (magic bytes: SOI = FF D8 FF) --------
    if buf.len() < 3 || buf[0] != 0xFF || buf[1] != 0xD8 || buf[2] != 0xFF {
        return err(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "not_jpeg",
            "Uploaded file is not a valid JPEG image.",
        );
    }

    // --- Write to disk under a unique, timestamped name ----------------------
    // Create the directory if needed. A failure here means the file is NOT
    // written, so we return 500 and leave the DB untouched.
    let dir: PathBuf = (*state.photos.dir).clone();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "[photo-upload] could not create photo dir {}: {e}",
            dir.display()
        );
        return internal_error_response();
    }

    let stamp = Utc::now().format("%Y%m%d-%H%M%S").to_string();
    // `create_new` makes each candidate an atomic claim: if the name already
    // exists we bump a suffix and retry, so two uploads in the same second
    // never collide and an existing file is NEVER overwritten.
    let stored_path = match write_unique(&dir, id, &stamp, &buf) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[photo-upload] write failed for bird {id}: {e}");
            return internal_error_response();
        }
    };
    let stored_str = stored_path.to_string_lossy().to_string();

    // --- Commit to the DB — ONLY now that the file is safely on disk ---------
    let uploaded_at = Utc::now().to_rfc3339();
    {
        let conn = acquire_db(&state);
        if let Err(e) = conn.execute(
            "UPDATE birds SET photo_path = ?1, photo_uploaded_at = ?2 WHERE id = ?3",
            params![stored_str, uploaded_at, id],
        ) {
            // The file is on disk but we couldn't record it. Don't leave an
            // orphan that the next upload would shadow — remove it and report
            // a server error so the client can retry cleanly.
            eprintln!("[photo-upload] DB update failed for bird {id}: {e}");
            let _ = std::fs::remove_file(&stored_path);
            return internal_error_response();
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "id": id,
            "photo_path": stored_str,
            "photo_uploaded_at": uploaded_at,
        })),
    )
        .into_response()
}

/// `GET /api/birds/{id}/photo` — serve the bird's current photo from disk.
///
/// "Current photo" is whatever `birds.photo_path` points at — i.e. the most
/// recent upload (the upload handler keeps timestamped history on disk but only
/// the newest one is referenced by `photo_path`). Returns 404 if the bird has
/// no photo recorded, or if the recorded file is missing on disk.
pub(crate) async fn serve_bird_photo(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    // Look up the recorded path for this bird's current photo. A missing bird,
    // a NULL/empty photo_path, or a query error all collapse to "no photo".
    let photo_path: Option<String> = {
        let conn = acquire_db(&state);
        conn.query_row(
            "SELECT photo_path FROM birds WHERE id = ?1",
            params![id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
        .filter(|p| !p.is_empty())
    };

    let photo_path = match photo_path {
        Some(p) => p,
        None => return err(StatusCode::NOT_FOUND, "no_photo", "This bird has no photo."),
    };

    // `photo_path` is server-generated by the upload handler (a sanitized
    // `bird_{id}_{stamp}.jpg` under the photos dir) and is resolved relative to
    // the server's working directory (`/data` in the container), so reading it
    // as-is lands inside the volume.
    let bytes = match std::fs::read(&photo_path) {
        Ok(b) => b,
        Err(_) => {
            return err(
                StatusCode::NOT_FOUND,
                "photo_missing",
                "The bird's photo file is no longer on disk.",
            )
        }
    };

    // Content-type from the stored filename (these are always .jpg today, but
    // derive it rather than hardcode).
    let mime = mime_guess::from_path(&photo_path).first_or_octet_stream();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
        bytes,
    )
        .into_response()
}

/// One entry in a bird's photo history.
#[derive(serde::Serialize)]
struct PhotoHistoryEntry {
    filename: String,
    /// ISO-8601 local timestamp parsed from the filename (no zone).
    uploaded_at: String,
    /// Served via `serve_bird_photo_file`.
    url: String,
}

/// `GET /api/birds/{id}/photos` — list a bird's full photo history.
///
/// Scans the photos directory for `bird_{id}_*.jpg`, parses the
/// `YYYYMMDD-HHMMSS` timestamp out of each filename, and returns the entries
/// newest-first. Returns `[]` (not 404) when the bird has no photos, so the
/// dashboard can simply skip the section.
pub(crate) async fn list_bird_photos(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    // Trailing underscore makes the prefix unambiguous: `bird_7_` never matches
    // `bird_70_...`.
    let prefix = format!("bird_{id}_");
    let dir = state.photos.dir.clone();

    let mut entries: Vec<PhotoHistoryEntry> = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(&*dir) {
        for entry in read_dir.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if !fname.starts_with(&prefix) || !fname.ends_with(".jpg") {
                continue;
            }
            // The 15 chars right after the prefix are the `YYYYMMDD-HHMMSS`
            // stamp (a possible `-N` collision suffix follows and is ignored).
            let uploaded_at = fname
                .get(prefix.len()..prefix.len() + 15)
                .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y%m%d-%H%M%S").ok())
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default();
            entries.push(PhotoHistoryEntry {
                url: format!("/api/birds/{id}/photos/{fname}"),
                filename: fname,
                uploaded_at,
            });
        }
    }

    // Newest first by timestamp; filename as a stable tiebreak (collision
    // suffixes, unparseable names).
    entries.sort_by(|a, b| {
        b.uploaded_at
            .cmp(&a.uploaded_at)
            .then_with(|| b.filename.cmp(&a.filename))
    });

    Json(entries).into_response()
}

/// `GET /api/birds/{id}/photos/{filename}` — serve a specific historical photo.
///
/// The filename comes from the URL, so it is validated strictly: it must be a
/// bare `bird_{id}_*.jpg` name (no path separators, no `..`). This both scopes
/// access to *this* bird's files and blocks path traversal.
pub(crate) async fn serve_bird_photo_file(
    State(state): State<AppState>,
    Path((id, filename)): Path<(i64, String)>,
) -> Response {
    let prefix = format!("bird_{id}_");
    let safe = filename.starts_with(&prefix)
        && filename.ends_with(".jpg")
        && !filename.contains('/')
        && !filename.contains('\\')
        && !filename.contains("..");
    if !safe {
        return err(
            StatusCode::NOT_FOUND,
            "photo_not_found",
            "No such photo for this bird.",
        );
    }

    let path = state.photos.dir.join(&filename);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(_) => {
            return err(
                StatusCode::NOT_FOUND,
                "photo_not_found",
                "No such photo for this bird.",
            )
        }
    };

    let mime = mime_guess::from_path(&filename).first_or_octet_stream();
    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, mime.as_ref())],
        bytes,
    )
        .into_response()
}

/// Does a bird with this id exist?
fn bird_exists(state: &AppState, id: i64) -> bool {
    let conn = acquire_db(state);
    conn.query_row(
        "SELECT COUNT(*) FROM birds WHERE id = ?1",
        params![id],
        |row| row.get::<_, i64>(0),
    )
    .map(|c| c > 0)
    .unwrap_or(false)
}

/// Write `buf` to `dir/bird_{id}_{stamp}.jpg`, bumping a `-N` suffix until an
/// unused name is found. Uses `create_new` so the existence check and the
/// create are a single atomic step (no TOCTOU, no overwrite). Returns the
/// path actually written.
fn write_unique(
    dir: &std::path::Path,
    id: i64,
    stamp: &str,
    buf: &[u8],
) -> std::io::Result<PathBuf> {
    let mut attempt: u32 = 0;
    loop {
        let name = if attempt == 0 {
            format!("bird_{id}_{stamp}.jpg")
        } else {
            format!("bird_{id}_{stamp}-{attempt}.jpg")
        };
        let candidate = dir.join(&name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut f) => {
                f.write_all(buf)?;
                f.flush()?;
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt += 1;
                if attempt > 10_000 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "exhausted unique filename attempts",
                    ));
                }
                continue;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Push an ntfy alert for an oversized (anomalous) upload. Reuses the backup
/// script's mechanism — `NTFY_SERVER`/`NTFY_TOPIC` from env, surfaced via
/// `PhotoConfig`. The message is intentionally generic (no file contents). A
/// failure to alert is logged and swallowed — it must NEVER break the HTTP
/// response the user is waiting on.
async fn send_oversized_alert(photos: PhotoConfig, bird_id: i64) {
    if !photos.ntfy_enabled() {
        return;
    }
    let topic = match &photos.ntfy_topic {
        Some(t) => t,
        None => return,
    };
    let url = format!("{}/{}", photos.ntfy_server.trim_end_matches('/'), topic);
    let body = format!("oversized upload rejected for bird {bird_id}");

    let request = reqwest::Client::new()
        .post(&url)
        .header("Title", "QuailSync photo upload rejected")
        .header("Priority", "high")
        .header("Tags", "warning")
        .body(body)
        .send();

    match tokio::time::timeout(std::time::Duration::from_secs(5), request).await {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => eprintln!("[photo-upload] ntfy send failed: {e}"),
        Err(_) => eprintln!("[photo-upload] ntfy send timed out"),
    }
}
