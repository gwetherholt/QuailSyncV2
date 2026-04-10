pub mod alerts;
pub mod db;
pub mod routes;
pub mod state;
pub mod ws;

// Re-exports for main.rs
pub use db::init_db;
pub use routes::backup::auto_backup_if_needed;
pub use state::AppState;

use axum::extract::State;
use axum::http::{header, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use metrics::counter;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../dashboard/"]
struct Asset;

async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => match Asset::get("index.html") {
            Some(content) => Html(content.data.into_owned()).into_response(),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}

async fn request_counter(req: Request<axum::body::Body>, next: Next) -> impl IntoResponse {
    let path = req.uri().path().to_string();
    counter!("quailsync_http_requests_total", "endpoint" => path).increment(1);
    next.run(req).await
}

async fn metrics_handler(State(state): State<AppState>) -> impl IntoResponse {
    let body = state.metrics_handle.render();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}

pub fn build_app(state: AppState) -> Router {
    use routes::*;

    Router::new()
        .route("/health", get(telemetry::health))
        .route("/metrics", get(metrics_handler))
        .route("/ws", get(ws::ws_handler))
        .route("/ws/live", get(ws::ws_live_handler))
        .route("/api/brooder/latest", get(telemetry::brooder_latest))
        .route("/api/brooder/history", get(telemetry::brooder_history))
        .route("/api/system/latest", get(telemetry::system_latest))
        .route("/api/status", get(telemetry::status))
        .route("/api/alerts", get(telemetry::alerts))
        .route(
            "/api/readings",
            axum::routing::delete(telemetry::clear_readings),
        )
        .route(
            "/api/bloodlines",
            get(birds::list_bloodlines).post(birds::create_bloodline),
        )
        .route(
            "/api/birds",
            get(birds::list_birds).post(birds::create_bird),
        )
        .route(
            "/api/birds/{id}",
            axum::routing::put(birds::update_bird).delete(birds::delete_bird),
        )
        // Section 10: POST (not PUT) for weight creation; path matches GET route
        .route(
            "/api/birds/{id}/weights",
            get(birds::list_weights).post(birds::create_weight),
        )
        .route(
            "/api/birds/{id}/weights/{wid}",
            axum::routing::delete(birds::delete_weight),
        )
        .route(
            "/api/breeding-pairs",
            get(breeding::list_breeding_pairs).post(breeding::create_breeding_pair),
        )
        .route(
            "/api/clutches",
            get(clutches::list_clutches).post(clutches::create_clutch),
        )
        .route(
            "/api/clutches/{id}",
            axum::routing::put(clutches::update_clutch).delete(clutches::delete_clutch),
        )
        .route(
            "/api/processing",
            get(processing::list_processing).post(processing::create_processing),
        )
        .route(
            "/api/processing/queue",
            get(processing::list_processing_queue),
        )
        .route(
            "/api/processing/{id}",
            axum::routing::put(processing::update_processing),
        )
        .route(
            "/api/breeding-groups",
            get(breeding::list_breeding_groups).post(breeding::create_breeding_group),
        )
        .route(
            "/api/breeding-groups/{id}",
            get(breeding::get_breeding_group),
        )
        .route("/api/flock/summary", get(breeding::flock_summary))
        .route(
            "/api/flock/cull-recommendations",
            get(breeding::cull_recommendations),
        )
        .route(
            "/api/cull-batch",
            axum::routing::post(processing::cull_batch),
        )
        .route("/api/inbreeding-check", get(breeding::inbreeding_check))
        .route("/api/breeding/suggest", get(breeding::breeding_suggest))
        .route(
            "/api/brooders",
            get(brooders::list_brooders).post(brooders::create_brooder),
        )
        .route(
            "/api/brooders/{id}",
            axum::routing::put(brooders::update_brooder).delete(brooders::delete_brooder),
        )
        .route(
            "/api/brooders/{id}/readings",
            get(brooders::brooder_readings),
        )
        .route("/api/brooders/{id}/status", get(brooders::brooder_status))
        .route("/api/brooders/{id}/alerts", get(brooders::brooder_alerts))
        .route(
            "/api/brooders/{id}/headcount",
            axum::routing::post(brooders::post_headcount),
        )
        .route(
            "/api/brooders/{id}/headcount/latest",
            get(brooders::get_headcount_latest),
        )
        .route(
            "/api/brooders/{id}/target-temp",
            get(brooders::brooder_target_temp),
        )
        .route(
            "/api/brooders/{id}/assign-group",
            axum::routing::put(brooders::assign_group_to_brooder)
                .delete(brooders::unassign_brooder_group),
        )
        .route(
            "/api/brooders/{id}/residents",
            get(brooders::brooder_residents),
        )
        .route("/api/birds/{id}/move", axum::routing::put(birds::move_bird))
        .route(
            "/api/cameras",
            get(cameras::list_cameras).post(cameras::create_camera),
        )
        .route(
            "/api/cameras/{id}",
            axum::routing::delete(cameras::delete_camera),
        )
        .route(
            "/api/cameras/{id}/brooder",
            axum::routing::put(cameras::update_camera_brooder),
        )
        .route(
            "/api/cameras/{id}/detections/summary",
            get(cameras::camera_detection_summary),
        )
        .route(
            "/api/frames",
            get(cameras::list_frames).post(cameras::create_frame),
        )
        .route(
            "/api/frames/{id}/detections",
            axum::routing::post(cameras::create_frame_detections),
        )
        .route("/api/nfc/{tag_id}", get(birds::get_bird_by_nfc))
        .route(
            "/api/chick-groups",
            get(chick_groups::list_chick_groups).post(chick_groups::create_chick_group),
        )
        .route(
            "/api/chick-groups/{id}",
            get(chick_groups::get_chick_group)
                .put(chick_groups::update_chick_group)
                .delete(chick_groups::delete_chick_group),
        )
        // Section 10: POST for mortality (creates a log entry) and graduate (creates birds)
        .route(
            "/api/chick-groups/{id}/mortality",
            axum::routing::post(chick_groups::log_mortality),
        )
        .route(
            "/api/chick-groups/{id}/graduate",
            axum::routing::post(chick_groups::graduate_chick_group),
        )
        .route("/api/backup", axum::routing::post(backup::create_backup))
        .route("/api/backups", get(backup::list_backups))
        .route("/api/restore", axum::routing::post(backup::restore_backup))
        .fallback(static_handler)
        .layer(middleware::from_fn(request_counter))
        .with_state(state)
}
