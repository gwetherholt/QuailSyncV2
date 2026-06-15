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

    let router = Router::new()
        .route("/health", get(telemetry::health))
        .route("/metrics", get(metrics_handler))
        .route("/ws", get(ws::ws_handler))
        .route("/ws/live", get(ws::ws_live_handler))
        .route("/api/brooder/latest", get(telemetry::brooder_latest))
        .route("/api/brooder/history", get(telemetry::brooder_history))
        .route("/api/system/latest", get(telemetry::system_latest))
        .route("/api/status", get(telemetry::status))
        .route(
            "/api/alerts",
            get(telemetry::alerts).post(alerts::create_alert),
        )
        .route("/api/alerts/active", get(alerts::list_active))
        .route("/api/alerts/recent", get(alerts::list_recent))
        .route(
            "/api/alerts/resolve",
            axum::routing::post(alerts::resolve_alerts),
        )
        .route(
            "/api/alerts/{id}/dismiss",
            axum::routing::post(alerts::dismiss_alert),
        )
        .route(
            "/api/readings",
            axum::routing::delete(telemetry::clear_readings),
        )
        .route(
            "/api/lineages",
            get(birds::list_lineages).post(birds::create_lineage),
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
        // Bird-photo upload (POST, multipart) + serving (GET). The raised body
        // limit lets a marginally-oversized upload reach the handler for a
        // clean 413 + alert rather than Axum's generic rejection; it's harmless
        // on GET (no request body). See routes/photos.rs.
        .route(
            "/api/birds/{id}/photo",
            axum::routing::get(photos::serve_bird_photo)
                .post(photos::upload_bird_photo)
                .layer(axum::extract::DefaultBodyLimit::max(
                    photos::PHOTO_BODY_LIMIT,
                )),
        )
        // Photo history: list all timestamped photos for a bird, and serve a
        // specific historical file. See routes/photos.rs.
        .route("/api/birds/{id}/photos", get(photos::list_bird_photos))
        .route(
            "/api/birds/{id}/photos/{filename}",
            get(photos::serve_bird_photo_file),
        )
        // Trail-cam read endpoints: latest observation per camera + image
        // serving. Reads the pipeline's processed/observations.jsonl. See
        // routes/trailcam.rs.
        .route(
            "/api/trailcam/latest/{camera_id}",
            get(trailcam::trailcam_latest),
        )
        .route(
            "/api/trailcam/image/{camera_id}/{filename}",
            get(trailcam::trailcam_image),
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
            get(breeding::get_breeding_group)
                .put(breeding::update_breeding_group)
                .delete(breeding::delete_breeding_group),
        )
        .route(
            "/api/groups/{id}/reconcile-tags",
            axum::routing::post(reconcile::reconcile_tags),
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
            "/api/settings",
            get(settings::get_settings).put(settings::update_settings),
        )
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
        .route(
            "/api/brooders/{id}/assign-birds",
            axum::routing::post(brooders::assign_birds),
        )
        .route(
            "/api/brooders/{id}/unassign-birds",
            axum::routing::post(brooders::unassign_birds),
        )
        .route(
            "/api/brooders/{id}/assign-graduated-group",
            axum::routing::post(brooders::assign_graduated_group),
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
            "/api/chick-groups/{id}/lineages",
            axum::routing::put(chick_groups::replace_chick_group_lineages_handler),
        )
        .route(
            "/api/birds/{id}/lineages",
            axum::routing::put(birds::replace_bird_lineages_handler),
        )
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
        .route("/api/restore", axum::routing::post(backup::restore_backup));

    // Dev/test endpoints — only registered when DEV_MODE=true. When the env
    // var is absent these routes don't exist (fall through to the static
    // handler's 404 / index.html), keeping prod surface area unchanged.
    let router = if dev::dev_mode_enabled() {
        println!("[dev] DEV_MODE=true — registering /api/dev/* routes");
        router
            .route("/api/dev/status", get(dev::status))
            .route("/api/dev/seed", axum::routing::post(dev::seed))
            .route(
                "/api/dev/stress-seed",
                axum::routing::post(dev::stress_seed),
            )
            .route("/api/dev/restore", axum::routing::post(dev::restore))
    } else {
        router
    };

    router
        .fallback(static_handler)
        .layer(middleware::from_fn(request_counter))
        .with_state(state)
}
