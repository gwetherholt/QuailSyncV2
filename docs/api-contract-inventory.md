# API Contract Inventory (KAN-3)

Investigation-only inventory of every backend API endpoint and how the two clients
consume it. Produced by reading the code; nothing was executed against a running
server and no code was changed.

**Sources**

| Layer | File | Notes |
|---|---|---|
| Router | `crates/quailsync-server/src/lib.rs:62-433` | `build_app()` — the **only** `Router::new()` in the crate (99 `.route(...)` calls). Dev routes are conditionally appended at `lib.rs:415-427` when `DEV_MODE=true`. |
| Shared DTOs | `crates/quailsync-common/src/lib.rs` | Most response/request shapes are `serde` structs here. |
| Handlers | `crates/quailsync-server/src/routes/*.rs` | Some handlers bypass the shared DTOs and emit hand-rolled `serde_json::json!` bodies. |
| Android | `android/app/src/main/java/com/quailsync/app/data/QuailSyncApi.kt` | Retrofit + Gson. Single interface at `:784-1122`; DTOs at `:18-781`. Plus raw OkHttp call sites (below). |
| Dashboard | `dashboard/index.html` | Vanilla JS. `API.get/post/put` helper at `:761-780`; some call sites use raw `fetch` to read error bodies. |

**Route-registration ambiguity check:** none. Every route is registered in one
`build_app()` chain. No nested routers, no `merge`, no `nest`. The only
conditional registration is the `DEV_MODE` block, which is explicit.

**Routing behaviour that hides bugs:** `lib.rs:430` installs
`.fallback(static_handler)`. A request to a path that matches **no** route falls
through to the SPA asset handler, which serves `index.html` with **HTTP 200** and
`Content-Type: text/html` (`lib.rs:26-45`). Clients therefore receive `200 OK` +
HTML instead of `404` when they call a path that no longer exists. A path that
exists with a different method still returns a proper `405`.

---

## 1. Summary table

Legend — **Consumed by:** `A` = Android, `D` = Dashboard, `both`, `none` (no client
in this repo; some are consumed by out-of-repo pollers/agents — see §4).
**Disc.** = discrepancy flagged in §3.

| Method | Path | Consumed by | Handler | Disc. |
|---|---|---|---|---|
| GET | `/health` | none | `telemetry::health` | no |
| GET | `/metrics` | none | `lib::metrics_handler` | no |
| GET | `/ws` | none (Pi agent) | `ws::ws_handler` | no |
| GET | `/ws/live` | A | `ws::ws_live_handler` | no |
| GET | `/api/brooder/latest` | none | `telemetry::brooder_latest` | no |
| GET | `/api/brooder/history` | D | `telemetry::brooder_history` | no |
| GET | `/api/system/latest` | none | `telemetry::system_latest` | no |
| GET | `/api/status` | D | `telemetry::status` | no |
| GET | `/api/alerts` | D | `telemetry::alerts` | no |
| POST | `/api/alerts` | none (Pi scripts) | `alerts::create_alert` | no |
| GET | `/api/alerts/active` | A | `alerts::list_active` | no |
| GET | `/api/alerts/recent` | A | `alerts::list_recent` | no |
| POST | `/api/alerts/resolve` | none (Pi scripts) | `alerts::resolve_alerts` | no |
| POST | `/api/alerts/{id}/dismiss` | A | `alerts::dismiss_alert` | no |
| DELETE | `/api/readings` | none | `telemetry::clear_readings` | no |
| GET | `/api/lineages` | both | `birds::list_lineages` | no |
| POST | `/api/lineages` | A | `birds::create_lineage` | no |
| GET | `/api/birds` | both | `birds::list_birds` | **yes** (D11–D15) |
| POST | `/api/birds` | both | `birds::create_bird` | **yes** (D4) |
| GET | `/api/birds/{id}` | none | `birds::get_bird` | **yes** (D29) |
| PUT | `/api/birds/{id}` | both | `birds::update_bird` | no |
| DELETE | `/api/birds/{id}` | A | `birds::delete_bird` | no |
| GET | `/api/birds/{id}/weights` | both | `birds::list_weights` | no |
| POST | `/api/birds/{id}/weights` | **none** | `birds::create_weight` | **yes** (D1) |
| DELETE | `/api/birds/{id}/weights/{wid}` | A | `birds::delete_weight` | no |
| GET | `/api/birds/{id}/photo` | none | `photos::serve_bird_photo` | no |
| POST | `/api/birds/{id}/photo` | A | `photos::upload_bird_photo` | **yes** (D20) |
| GET | `/api/birds/{id}/photos` | both | `photos::list_bird_photos` | no |
| GET | `/api/birds/{id}/photos/{filename}` | both (img src) | `photos::serve_bird_photo_file` | no |
| PUT | `/api/birds/{id}/lineages` | A | `birds::replace_bird_lineages_handler` | no |
| PUT | `/api/birds/{id}/move` | A | `birds::move_bird` | no |
| GET | `/api/nfc/{tag_id}` | D | `birds::get_bird_by_nfc` | **yes** (D2) |
| POST | `/api/trailcam/observation` | none (pipeline) | `trailcam::trailcam_observation` | no |
| GET | `/api/trailcam/cameras` | both | `trailcam::trailcam_cameras` | no |
| GET | `/api/trailcam/latest/{camera_id}` | both | `trailcam::trailcam_latest` | no |
| GET | `/api/trailcam/history/{camera_id}` | both | `trailcam::trailcam_history` | no |
| GET | `/api/trailcam/image/{camera_id}/{filename}` | both (img src) | `trailcam::trailcam_image` | no |
| POST | `/api/indoorcam/observation` | none (pipeline) | `indoorcam::indoorcam_observation` | no |
| PATCH | `/api/indoorcam/observation/{id}` | none (pipeline) | `indoorcam::clear_observation_image` | no |
| GET | `/api/indoorcam/cameras` | A | `indoorcam::indoorcam_cameras` | no |
| GET | `/api/indoorcam/latest/{camera_id}` | both | `indoorcam::indoorcam_latest` | no |
| GET | `/api/indoorcam/history/{camera_id}` | A | `indoorcam::indoorcam_history` | no |
| GET | `/api/indoorcam/image/{camera_id}/{filename}` | both (img src) | `indoorcam::indoorcam_image` | no |
| GET | `/api/incubation/events` | none | `incubation::list_events` | no |
| GET | `/api/incubation/summary` | D | `incubation::summary` | no |
| GET | `/api/clutches` | both | `clutches::list_clutches` | **yes** (D16, D17) |
| POST | `/api/clutches` | both | `clutches::create_clutch` | no |
| GET | `/api/clutches/{id}` | both | `clutches::get_clutch` | **yes** (D29) |
| PUT | `/api/clutches/{id}` | both | `clutches::update_clutch` | no |
| DELETE | `/api/clutches/{id}` | A | `clutches::delete_clutch` | no |
| GET | `/api/processing` | D | `processing::list_processing` | no |
| POST | `/api/processing` | D | `processing::create_processing` | **yes** (D22) |
| GET | `/api/processing/queue` | D | `processing::list_processing_queue` | no |
| PUT | `/api/processing/{id}` | D | `processing::update_processing` | no |
| POST | `/api/cull-batch` | A | `processing::cull_batch` | no |
| GET | `/api/breeding-groups` | both | `breeding::list_breeding_groups` | no |
| POST | `/api/breeding-groups` | A | `breeding::create_breeding_group` | no |
| GET | `/api/breeding-groups/{id}` | none | `breeding::get_breeding_group` | no |
| PUT | `/api/breeding-groups/{id}` | A | `breeding::update_breeding_group` | no |
| DELETE | `/api/breeding-groups/{id}` | A | `breeding::delete_breeding_group` | no |
| POST | `/api/groups/{id}/reconcile-tags` | A | `reconcile::reconcile_tags` | no |
| GET | `/api/flock/summary` | D | `breeding::flock_summary` | no |
| GET | `/api/flock/cull-recommendations` | both | `breeding::cull_recommendations` | no |
| GET | `/api/inbreeding-check` | A | `breeding::inbreeding_check` | no |
| GET | `/api/breeding/suggest` | both | `breeding::breeding_suggest` | no |
| GET | `/api/breeding/diversity` | both | `breeding::breeding_diversity` | no |
| GET | `/api/settings` | both | `settings::get_settings` | no |
| PUT | `/api/settings` | both | `settings::update_settings` | no |
| GET | `/api/settings/genetics` | both | `settings::get_genetics_settings` | no |
| PUT | `/api/settings/genetics` | both | `settings::update_genetics_settings` | no |
| GET | `/api/system-settings` | both | `system_settings::get_settings` | no |
| PUT | `/api/system-settings` | both | `system_settings::update_settings` | no |
| GET | `/api/brooders` | both | `brooders::list_brooders` | **yes** (D19) |
| POST | `/api/brooders` | both | `brooders::create_brooder` | no |
| PUT | `/api/brooders/{id}` | both | `brooders::update_brooder` | **yes** (D7) |
| DELETE | `/api/brooders/{id}` | A | `brooders::delete_brooder` | no |
| GET | `/api/brooders/{id}/readings` | both | `brooders::brooder_readings` | no |
| GET | `/api/brooders/{id}/status` | D | `brooders::brooder_status` | no |
| GET | `/api/brooders/{id}/alerts` | A | `brooders::brooder_alerts` | **yes** (D23) |
| POST | `/api/brooders/{id}/headcount` | none (Pi agent) | `brooders::post_headcount` | no |
| GET | `/api/brooders/{id}/headcount/latest` | both | `brooders::get_headcount_latest` | **yes** (D26) |
| GET | `/api/brooders/{id}/target-temp` | A | `brooders::brooder_target_temp` | no |
| PUT | `/api/brooders/{id}/assign-group` | A | `brooders::assign_group_to_brooder` | no |
| DELETE | `/api/brooders/{id}/assign-group` | A | `brooders::unassign_brooder_group` | no |
| GET | `/api/brooders/{id}/residents` | both | `brooders::brooder_residents` | no |
| POST | `/api/brooders/{id}/assign-birds` | both | `brooders::assign_birds` | no |
| POST | `/api/brooders/{id}/unassign-birds` | both | `brooders::unassign_birds` | no |
| POST | `/api/brooders/{id}/assign-graduated-group` | both | `brooders::assign_graduated_group` | no |
| GET | `/api/brooders/{id}/sensors` | none | `govee::brooder_sensors` | no |
| POST | `/api/govee/readings` | none (poller) | `govee::ingest_readings` | no |
| GET | `/api/govee/sensors` | both | `govee::list_sensors` | no |
| PUT | `/api/govee/sensors/{id}/assign` | both | `govee::assign_sensor` | no |
| DELETE | `/api/govee/sensors/{id}/assign` | both | `govee::unassign_sensor` | no |
| GET | `/api/brooders/{id}/cameras` | none | `trail_cameras::brooder_cameras` | no |
| GET | `/api/trail-cameras` | both | `trail_cameras::list_cameras` | no |
| POST | `/api/trail-cameras/register` | none (poller) | `trail_cameras::register_camera` | no |
| PUT | `/api/trail-cameras/{id}/assign` | both | `trail_cameras::assign_camera` | no |
| DELETE | `/api/trail-cameras/{id}/assign` | both | `trail_cameras::unassign_camera` | no |
| GET | `/api/brooders/{id}/indoor-cameras` | none | `indoor_cameras::brooder_indoor_cameras` | no |
| GET | `/api/indoor-cameras` | both | `indoor_cameras::list_cameras` | no |
| POST | `/api/indoor-cameras` | none | `indoor_cameras::create_camera` | no |
| GET | `/api/indoor-cameras/{id}` | none | `indoor_cameras::get_camera` | no |
| PUT | `/api/indoor-cameras/{id}` | none | `indoor_cameras::update_camera` | no |
| DELETE | `/api/indoor-cameras/{id}` | none | `indoor_cameras::delete_camera` | no |
| PUT | `/api/indoor-cameras/{id}/assign` | both | `indoor_cameras::assign_camera` | no |
| DELETE | `/api/indoor-cameras/{id}/assign` | both | `indoor_cameras::unassign_camera` | no |
| GET | `/api/cameras` | both | `cameras::list_cameras` | **yes** (D18) |
| POST | `/api/cameras` | both | `cameras::create_camera` | **yes** (D6) |
| DELETE | `/api/cameras/{id}` | A | `cameras::delete_camera` | no |
| PUT | `/api/cameras/{id}/brooder` | none | `cameras::update_camera_brooder` | **yes** (D9) |
| GET | `/api/cameras/{id}/detections/summary` | none | `cameras::camera_detection_summary` | no |
| GET | `/api/cameras/{id}/assignment` | both | `camera_assignment::get_assignment` | no |
| PUT | `/api/cameras/{id}/assignment` | both | `camera_assignment::set_assignment` | no |
| GET | `/api/frames` | none | `cameras::list_frames` | no |
| POST | `/api/frames` | none | `cameras::create_frame` | no |
| POST | `/api/frames/{id}/detections` | none | `cameras::create_frame_detections` | no |
| GET | `/api/chick-groups` | both | `chick_groups::list_chick_groups` | no |
| POST | `/api/chick-groups` | both | `chick_groups::create_chick_group` | **yes** (D5) |
| GET | `/api/chick-groups/{id}` | none | `chick_groups::get_chick_group` | no |
| PUT | `/api/chick-groups/{id}` | both | `chick_groups::update_chick_group` | **yes** (D8) |
| DELETE | `/api/chick-groups/{id}` | A | `chick_groups::delete_chick_group` | no |
| PUT | `/api/chick-groups/{id}/lineages` | A | `chick_groups::replace_chick_group_lineages_handler` | no |
| POST | `/api/chick-groups/{id}/mortality` | A | `chick_groups::log_mortality` | **yes** (D3, D10) |
| POST | `/api/chick-groups/{id}/graduate` | D | `chick_groups::graduate_chick_group` | no |
| POST | `/api/backup` | D | `backup::create_backup` | no |
| GET | `/api/backups` | D | `backup::list_backups` | no |
| POST | `/api/restore` | D | `backup::restore_backup` | **yes** (D21) |
| GET | `/api/dev/status` | A | `dev::status` | no |
| POST | `/api/dev/seed` | A | `dev::seed` | no |
| POST | `/api/dev/stress-seed` | A | `dev::stress_seed` | no |
| POST | `/api/dev/restore` | A | `dev::restore` | no |

### Client calls with NO matching backend route

These appear in the discrepancy list but have no row above, because there is no
route to list.

| Method | Path called | Client | Call site | Nearest real route |
|---|---|---|---|---|
| POST | `api/birds/{id}/weight` | Android | `QuailSyncApi.kt:841-842` | `POST /api/birds/{id}/weight`**s** |
| POST | `/api/birds/{id}/weight` | Dashboard | `index.html:2944` | `POST /api/birds/{id}/weight`**s** |
| GET | `api/birds/nfc/{tag_id}` | Android | `QuailSyncApi.kt:847-848` | `GET /api/nfc/{tag_id}` |

### Android raw-OkHttp call sites (outside the Retrofit interface)

| Endpoint | File:line |
|---|---|
| `GET /api/chick-groups` (raw, for debug logging) | `BrooderManageScreen.kt:142-143` |
| `DELETE /api/brooders/{id}/assign-group` | `BrooderManageScreen.kt:335-338` |
| `PUT /api/brooders/{id}/assign-group` | `BrooderManageScreen.kt:407-412` |
| `PUT /api/chick-groups/{id}` (explicit `{"housing_id": null}`) | `BrooderManageScreen.kt:515-518` |
| `PUT /api/chick-groups/{id}` (hand-built JSON) | `ClutchScreen.kt:1371-1382` |
| `GET /ws/live` (WebSocket) | `WebSocketService.kt:57-58`, `MonitoringService.kt:114-115` |

---

## 2. Per-endpoint detail

Only endpoints with at least one client are detailed. Server enum fields serialize
as their Rust variant name (`Sex::Male` → `"Male"`), except `HousingType`, which is
`#[serde(rename_all = "lowercase")]` (`quailsync-common/src/lib.rs:1002`).

### `GET /api/birds` — `birds::list_birds` (`routes/birds.rs:123`)

**Handler returns** `Vec<Bird>` (`quailsync-common/src/lib.rs:191-239`), hydrated
with lineages + genetic profile (`db/helpers.rs:317-330`):

```
id: i64, band_color: String?, sex: "Male"|"Female"|"Unknown",
hatch_date: "YYYY-MM-DD", mother_id: i64?, father_id: i64?, generation: u32,
status: "Active"|"Culled"|"Deceased"|"Sold", notes: String?, nfc_tag_id: String?,
current_brooder_id: i64?, photo_path: String?, photo_uploaded_at: String?,
housing_id: i64?, chick_group_id: i64?,
lineages: [{id, name, source, notes?}],
genetic_profile: {paternal: [LineageProbability], maternal: [...]}, confidence: f64
```

**Android** `Bird` (`QuailSyncApi.kt:67-112`), via `getBirds()` (`:826-827`).
Declares 6 fields the handler never sends: `band_id`, `species`, `sire_id`,
`dam_id`, `latest_weight`, `brooder_id`. All nullable, so they silently
deserialize to `null`. Matching fields: `band_color`, `sex`, `status`,
`hatch_date`, `notes`, `nfc_tag_id`, `housing_id`, `chick_group_id`, `lineages`,
`generation`, `genetic_profile`, `confidence`. → D11–D15.

**Dashboard** reads `id`, `band_color`, `sex`, `status`, `hatch_date`,
`mother_id`/`father_id` (`:2583-2584`, `:2715-2716`), `lineages`, `notes`. Correct.

### `POST /api/birds` — `birds::create_bird` (`routes/birds.rs:12`)

**Handler accepts** `CreateBird` (`quailsync-common/src/lib.rs:275-294`):
`band_color?`, `sex` (required), `hatch_date` (required), `mother_id?`,
`father_id?`, `generation` (required), `status` (required), `notes?`,
`nfc_tag_id?`, `chick_group_id?`, **`lineage_ids: Vec<i64>` (required, no
`#[serde(default)]`)**. Rejects an empty list with 400 (`birds.rs:16-22`).
Returns `201` + `Bird`.

**Android** `CreateBirdRequest` (`QuailSyncApi.kt:283-300`) sends `lineage_ids`. Correct.

**Dashboard** sends `lineage_id` (singular scalar) at `index.html:2642-2648`
(Flock → Add Bird) and `index.html:4409-4415` (`_buildBirdBody`, NFC write flow,
POSTed at `:4419`). `lineage_ids` is absent → serde rejects the body → **422**. → D4.

### `POST /api/birds/{id}/weights` — `birds::create_weight` (`routes/birds.rs:348`)

**Handler accepts** `CreateWeightRecord {weight_grams: f64, date: "YYYY-MM-DD",
notes: String?}`; returns `201` + `WeightRecord {id, bird_id, weight_grams, date, notes}`.

**No client calls this path.** Both clients POST to the singular
`/api/birds/{id}/weight`: Android `QuailSyncApi.kt:841-842` (`createBirdWeight`,
logged at `NfcScreen.kt:675`), dashboard `index.html:2944` (`submitWeight`). → D1.

### `GET /api/birds/{id}/weights` — `birds::list_weights` (`routes/birds.rs:379`)

Returns `Vec<WeightRecord>`. Android `BirdWeight` (`QuailSyncApi.kt:114-120`) and
dashboard `index.html:2679` both match.

### `POST /api/birds/{id}/photo` — `photos::upload_bird_photo` (`routes/photos.rs:45`)

**Handler returns** `200` + hand-rolled JSON `{"id", "photo_path",
"photo_uploaded_at"}` (`photos.rs:181-190`).

**Android** `PhotoUploadResponse` (`QuailSyncApi.kt:302-306`) declares
`{id, url, path}` — `url` and `path` are never sent. All nullable, and the caller
ignores the body (`BirdPhotoUpload.kt:39`), so the upload still succeeds. → D20.

### `GET /api/birds/{id}/photos` — `photos::list_bird_photos` (`routes/photos.rs:263`)

Returns `[{filename, uploaded_at, url}]` (`photos.rs:248-256`); `url` is
server-relative. Android `BirdPhoto` (`QuailSyncApi.kt:311-315`) matches exactly.
Dashboard reads `.url` at `:2701-2702`, `:2759-2760`.

### `GET /api/nfc/{tag_id}` — `birds::get_bird_by_nfc` (`routes/birds.rs:303`)

Returns `200` + `Bird`, or `404` + JSON `null`.
**Dashboard** calls it correctly at `index.html:4367`.
**Android** calls `api/birds/nfc/{tag_id}` (`QuailSyncApi.kt:847-848`) — no such
route. → D2.

### `GET /api/brooders` — `brooders::list_brooders` (`routes/brooders.rs:283`)

Returns `Vec<Brooder>` = `{id, name, lineage_id?, life_stage:
"Chick"|"Adolescent"|"Adult", qr_code, notes?, camera_url?, housing_type:
"incubator"|"brooder"|"hutch"}`. Optional `?type=` filter (`brooders.rs:288-292`).

**Android** `Brooder` (`QuailSyncApi.kt:18-36`) declares `location`, `capacity`,
`status`, `latest_temperature`, `latest_humidity`, `latest_temperature_f`,
`latest_humidity_percent` — none are sent. All nullable. → D19.

**Dashboard** reads `id`, `housing_type` (`:1368`), `life_stage` (`:1385`),
`camera_url` (`:3757`), `name`, `qr_code`. Correct.

### `POST /api/brooders` — `brooders::create_brooder` (`routes/brooders.rs:15`)

Accepts `CreateBrooder {name, lineage_id?, life_stage, qr_code, notes?,
camera_url?, housing_type?}`; returns `201` + `Brooder`. Validates `lineage_id`
exists (400 otherwise). Android sends a `Map` with exactly those keys
(`DashboardScreen.kt:178-186`); dashboard sends the same set
(`index.html:1584-1592`). Both match.

### `PUT /api/brooders/{id}` — `brooders::update_brooder` (`routes/brooders.rs:62`)

Accepts a free-form `serde_json::Value`; honours `camera_url`, `name`, `notes`,
`qr_code`, `lineage_id`, `housing_type` (400 on an unrecognized `housing_type`).
**Returns `StatusCode::OK` with an empty body** (`brooders.rs:147`).

**Android** `updateBrooder` (`QuailSyncApi.kt:970-971`) declares a non-null
`Brooder` return → Gson gets an empty body → converter throws. → D7.
**Dashboard** `index.html:3806` calls `API.put`, whose `r.json()` throws on the
empty body and returns `null` from the catch (`:773-778`); the call site then
checks `resp !== false`, so `null` still reports success. The write does land.

### `GET /api/brooders/{id}/status` — `brooders::brooder_status` (`routes/brooders.rs:356`)

Returns `{brooder: Brooder, latest_temp: f64?, latest_humidity: f64?, has_alert:
bool, alert_message: String?, sensor_status: "online"|"offline"}`
(`brooders.rs:346-354`). Dashboard reads all of these at `:1386-1397`. Match.

### `GET /api/brooders/{id}/alerts` — `brooders::brooder_alerts` (`routes/brooders.rs:202`)

**Hardcoded `Json(vec![])`** — always an empty array, per the handler's own comment.
Android `getBrooderAlerts` (`QuailSyncApi.kt:823-824`) deserializes into
`List<BrooderAlert>` (`:57-65`: `brooder_id`, `alert_type`, `severity`, `message`,
`acknowledged`, `created_at`). The model can never be populated. → D23.

### `GET /api/brooders/{id}/headcount/latest` — `brooders::get_headcount_latest` (`routes/brooders.rs:248`)

Two shapes on one route: the found path returns the typed `HeadcountResponse` with
non-`Option` `count: i64` / `timestamp: String` (`brooders.rs:215-220`); the
not-found path returns `200` + `{"brooder_id": N, "count": null, "timestamp": null}`
(`brooders.rs:263-271`). Android `HeadcountResponse` (`QuailSyncApi.kt:490-494`)
makes all three nullable and the dashboard null-checks at `:1410`, so both cope. → D26.

### `GET /api/brooders/{id}/target-temp` — `brooders::brooder_target_temp` (`routes/brooders.rs:423`)

Returns `TargetTempResponse {brooder_id, target_temp_f, min_temp_f, max_temp_f,
week, age_days?, chick_group_id?, schedule_label, status}` where `status` is
`heat_required|weaning|ambient|unassigned` (`quailsync-common/src/lib.rs:1218-1229`).
Android `TargetTempResponse` (`QuailSyncApi.kt:475-488`) widens several non-null
server fields to nullable, which is safe. Match.

### `GET /api/brooders/{id}/residents` — `brooders::brooder_residents` (`routes/brooders.rs:514`)

Returns `BrooderResidentsResponse {brooder_id, chick_groups: [ChickGroup],
individual_birds: [Bird], active_bird_count: i64}`
(`quailsync-common/src/lib.rs:1242-1250`). Android
(`QuailSyncApi.kt:539-543`) omits `active_bird_count` — extra server field,
ignored by Gson. Dashboard `:2076`, `:2174`, `:2351`.

### `PUT` / `DELETE /api/brooders/{id}/assign-group`

`PUT` (`brooders.rs:470`) takes `{group_id}` and returns `200` + `ChickGroup`;
`DELETE` (`brooders.rs:500`) returns `204`. Android drives both through raw OkHttp
(`BrooderManageScreen.kt:407-412`, `:335-338`) and also declares the PUT in
Retrofit (`QuailSyncApi.kt:979-980`). Match.

### `POST /api/brooders/{id}/assign-birds` and `/unassign-birds`

Accept `{bird_ids: [i64]}`, return `{updated: i64}` (`brooders.rs:574`, `:636`).
Android `BirdAssignmentRequest`/`Response` (`QuailSyncApi.kt:243-249`, calls
`:797-809`) and dashboard `:2122`, `:2261`, `:2364` all match.

### `POST /api/brooders/{id}/assign-graduated-group` — `brooders.rs:677`

Accepts `{group_id}`, returns `{group_id, housing_id, birds_updated}`. Validates
that the target is a hutch, the group is `Graduated`, and the group has banded
birds (400 otherwise). Android `QuailSyncApi.kt:814-818`; dashboard reads
`resp.group_id` and `resp.birds_updated` at `index.html:2331`. Match.

### `GET /api/clutches` — `clutches::list_clutches` (`routes/clutches.rs:188`)

Returns `Vec<Clutch>`: `{id, breeding_group_id?, breeding_group_name?, lineage_id?,
eggs_set, eggs_fertile?, eggs_hatched?, set_date, expected_hatch_date, status:
"Incubating"|"Hatched"|"Failed", notes?, eggs_stillborn?, eggs_quit?,
eggs_infertile?, eggs_damaged?, hatch_notes?}`.

**Android** `Clutch` (`QuailSyncApi.kt:176-203`) additionally declares
`lineage_name`, `egg_count`, `fertile_count`, `hatch_count` — none are sent. → D16, D17.
**Dashboard** `:2995` reads `eggs_set`, `eggs_fertile`, `lineage_id`, `status`. Match.

### `POST /api/clutches` — `clutches::create_clutch` (`routes/clutches.rs:12`)

Accepts `CreateClutch`; returns `201` + **`ClutchDetail`** (clutch fields flattened
plus `snapshot`), not a bare `Clutch` (`clutches.rs:42`). Android's `createClutch`
declares `Clutch`; the extra `snapshot` key is ignored by Gson. Harmless.

### `GET /api/clutches/{id}` — `clutches::get_clutch` (`routes/clutches.rs:163`)

`200` + `ClutchDetail` (flattened clutch + `snapshot?` with `males`, `females`,
`paternal_distribution`, `maternal_distribution`), or **`404` with a JSON `null`
body** (`clutches.rs:174`). Android `ClutchDetail` (`QuailSyncApi.kt:721-723`)
reads only `snapshot`; dashboard `:3003` reads the distributions. → D29.

### `POST /api/processing` — `processing::create_processing` (`routes/processing.rs:384`)

Accepts `CreateProcessingRecord {bird_id, reason, scheduled_date, notes?}` where
`reason` must be one of `ExcessMale|LowWeight|PoorGenetics|Age|Other`
(`quailsync-common/src/lib.rs:446-453`).

**Dashboard** `index.html:2961-2966` (Bird Detail → "Schedule processing") sends
`reason: 'Manual'` → serde rejects → **422**. → D22.
`index.html:3630-3641` (`scheduleFromRec`) forwards a caller-supplied `reason` but
has no callers — dead code (§5).

### `PUT /api/processing/{id}` — `processing::update_processing` (`routes/processing.rs:412`)

Accepts `{processed_date?, final_weight_grams?, status?, notes?}`; returns
`ProcessingRecord`. Dashboard `:3645-3648` sends `status: 'Completed'` +
`processed_date`. Match.

### `POST /api/cull-batch` — `processing::cull_batch` (`routes/processing.rs:511`)

Accepts `{bird_ids, reason, method, notes?, processed_date}`; `method` must be
`Butchered|Culled|Deceased|Sold` (400 otherwise, with `{error, message}`).
Returns `{"updated": i64}`. Android `CullBatchRequest`/`Response`
(`QuailSyncApi.kt:730-740`, call `:1091-1092`) match.

### `GET /api/breeding-groups` — `breeding::list_breeding_groups` (`routes/breeding.rs:157`)

Returns `Vec<BreedingGroup> {id, name, male_ids: [i64], female_ids: [i64],
start_date, notes?, status: "active"|"infertile"}`. Android `BreedingGroupDto`
(`QuailSyncApi.kt:551-564`) matches. Dashboard reads `male_ids`/`female_ids` at
`:3342`, `:3430-3432`, `:3467-3468`. Match.

### `POST /api/breeding-groups` — `breeding::create_breeding_group` (`routes/breeding.rs:62`)

Returns `201` + `BreedingGroup` **flattened with an extra `warning: String?`**
(`breeding.rs:133-141`). Android's `createBreedingGroup` declares
`BreedingGroupDto`, which drops `warning` silently — the inbreeding warning the
server computes never reaches the user. Noted; not counted as a break.

### `POST /api/groups/{id}/reconcile-tags` — `reconcile::reconcile_tags` (`routes/reconcile.rs:422`)

Request `{orphan_tag_ids: [String], observed_birds: [{ref_id, sex?, bloodline?,
traits: {band_color?}}]}`. Response `{results: [{ref_id, outcome}], unmatched_tags}`,
where `outcome` is internally tagged `#[serde(tag = "kind", rename_all =
"snake_case")]` → `{"kind":"resolved","tag_id","confidence":"sole"|"forced"}` /
`{"kind":"ambiguous","candidates":[{tag_id, score}]}` / `{"kind":"no_candidate"}`
(`reconcile.rs:85-113`). Android's flattened `ReconcileOutcome`
(`QuailSyncApi.kt:623-629`) mirrors this exactly. Match.

### `GET /api/flock/cull-recommendations` — `breeding::cull_recommendations` (`routes/breeding.rs:703`)

Returns `FlockBreedingStats {total_males, total_females, minimum_males_needed,
safe_to_cull, per_male_safe_pairings: [{bird_id, safe_pairings, safe_female_ids}],
desired_males_per_group, max_females_per_male}`. Android
(`QuailSyncApi.kt:638-656`) matches; dashboard reads the four scalars at
`:3576-3579`. Match. The path name is legacy — noted in `QuailSyncApi.kt:1036-1039`.

### `GET /api/flock/summary` — `breeding::flock_summary` (`routes/breeding.rs:789`)

Returns `{total_birds, active_birds, males, females, lineages: [{name, count}]}`.
Dashboard-only (`:2395`). Match.

### `GET /api/breeding/suggest` and `/diversity` — `routes/breeding.rs:532`, `:581`

`suggest` → `Vec<PairingSuggestion> {bird_a_id, bird_b_id, paternal_overlap,
maternal_overlap, risk_percent, risk_level}`; `diversity` → `FlockDiversity
{flock_confidence, min_confidence, best_pairing_risk, needs_new_blood,
active_lineage_count}`. Android `:696-712`; dashboard `:3407-3408`, `:1235`. Match.

### `GET /api/inbreeding-check` — `breeding::inbreeding_check` (`routes/breeding.rs:649`)

Query `?male_id&female_id`. Returns hand-rolled JSON `{male_id, female_id,
coefficient, safe, warning}` where `warning` is `""` (never `null`) when safe
(`breeding.rs:115-119`); `404` + `{"error": ...}` when a bird is missing.
Android `InbreedingCheckResult` (`QuailSyncApi.kt:676-682`) matches.

### `GET`/`PUT /api/settings` — `routes/settings.rs:39`, `:44`

`{desired_males_per_group: u32, max_females_per_male: u32}`; PUT is a partial
update validated to 1..=100 (400 + `{"error"}` otherwise). Android `AppSettings`/
`UpdateAppSettings` (`QuailSyncApi.kt:658-667`); dashboard `:4579`, `:4640`. Match.

### `GET`/`PUT /api/settings/genetics` — `routes/settings.rs:112`, `:123`

Flat `{ "genetics.threshold.safe": "15", … }` string map with 5 known keys
(`quailsync-common/src/lib.rs:664-679`). PUT is all-or-nothing validated, accepts
numbers or numeric strings, and returns the full map. Android
`Map<String,String>` (`QuailSyncApi.kt:1067-1071`); dashboard `:4581`, `:4687`,
`:4697`, with defaults documented at `:4542`. Match.

### `GET`/`PUT /api/system-settings` — `routes/system_settings.rs:48`, `:56`

Returns the full typed `Settings` — 14 fields
(`quailsync-common/src/lib.rs:751-769`). Android `SystemSettings`
(`QuailSyncApi.kt:671-674`) intentionally declares only the two indoor-cam
booleans; the dashboard reads only the same two (`:4614-4615`). Deliberate partial
consumption, not a discrepancy.

### `GET`/`POST /api/cameras` — `routes/cameras.rs:39`, `:13`

`CameraFeed {id, name, location: String, feed_url, status: "Active"|"Offline",
brooder_id: i64?}`. POST accepts `CreateCameraFeed` with **`location` and `status`
both required, non-`Option`** (`quailsync-common/src/lib.rs:946-952`).

**Android** `Camera` (`QuailSyncApi.kt:262-271`) declares `url` and `brooder_name`,
which are never sent (→ D18). `CreateCameraRequest` (`:43-47`) sends only
`{name, feed_url, location?}` — no `status`, and `location` may be `null` → **422**. → D6.
**Dashboard** `:3832-3834` sends `{name, location, feed_url, status:'Active'}`. Correct.

### `GET`/`PUT /api/cameras/{id}/assignment` — `routes/camera_assignment.rs:68`, `:82`

`{camera_id, assignment: "incubator"|"brooder", active_model:
"incubation"|"chick", updated_at}`; PUT body `{assignment}`, 400 on anything else,
404 on an unknown camera. Android `CameraAssignment`/`SetCameraAssignmentRequest`
(`QuailSyncApi.kt:463-473`); dashboard `:1832`, `:1859` with a hardcoded
`_camModeId: 'indoor_tapo'` (`:1827`). Match.

### `GET`/`POST /api/chick-groups` — `routes/chick_groups.rs:54`, `:11`

`ChickGroup {id, clutch_id?, brooder_id?, initial_count, current_count, hatch_date,
status: "Active"|"Graduated"|"Lost", notes?, housing_id?, is_ready_to_transition,
lineages: [Lineage]}`. `is_ready_to_transition` is computed server-side
(`db/helpers.rs:233`). POST accepts `CreateChickGroup` with **`lineage_ids:
Vec<i64>` required** (`quailsync-common/src/lib.rs:1103-1111`); 400 on empty.

**Android** `ChickGroupDto` (`QuailSyncApi.kt:496-532`) and
`CreateChickGroupRequest` (`:228-236`) both use `lineage_ids`. Correct.
**Dashboard** sends `lineage_id` (scalar) at `index.html:3287-3294` (post-hatch
prompt) and `index.html:3963-3966` (Nursery → Create group) → **422**. → D5.
Dashboard reads `is_ready_to_transition` at `:1375`.

### `PUT /api/chick-groups/{id}` — `chick_groups::update_chick_group` (`routes/chick_groups.rs:88`)

Free-form JSON; honours `current_count`, `brooder_id`, `notes`, `status`,
`housing_id`. Presence-checked, so an explicit `null` clears the field.
**Returns `StatusCode::OK` with an empty body** (`chick_groups.rs:141`).
Android correctly uses `retrofit2.Response<Unit>` (`QuailSyncApi.kt:952-956`) plus
raw OkHttp (`ClutchScreen.kt:1380`, `BrooderManageScreen.kt:515-518`).
Dashboard `index.html:2357` awaits `API.put`, gets `null`, and treats it as
failure. → D8.

### `POST /api/chick-groups/{id}/mortality` — `chick_groups::log_mortality` (`routes/chick_groups.rs:210`)

Accepts `{count: u32, reason: String}`. **Returns `ChickMortalityLog {id, group_id,
count, reason, date}`** (`chick_groups.rs:268-274`).

**Android** `logMortality` (`QuailSyncApi.kt:988-989`) declares a `ChickGroupDto`
return. → D10.
**Dashboard** uses **PUT** at `index.html:3989`; the route is POST-only → **405**. → D3.

### `POST /api/chick-groups/{id}/graduate` — `chick_groups::graduate_chick_group` (`routes/chick_groups.rs:278`)

Accepts `GraduateRequest {birds: [{sex, band_color?, nfc_tag_id?, notes?,
weight_grams?, photo_path?}], target_housing_id?}`; returns `Vec<Bird>`
(`chick_groups.rs:450`). Dashboard-only (`index.html:4089`; body built at
`:4071-4077`). Match.

### `POST /api/backup`, `GET /api/backups`, `POST /api/restore` — `routes/backup.rs`

`create_backup` → `201` + `{filename, size_bytes, created}`; `list_backups` → an
array of the same. `restore_backup` accepts `{filename}` (path-traversal guarded)
and returns **`200` + `text/plain`** `"Database restored. Restart server to apply."`
(`backup.rs:95-99`). Dashboard `:4724`, `:4585`, `:4743`. → D21.

### `GET /api/alerts/active`, `/recent`, `POST /api/alerts/{id}/dismiss` — `routes/alerts.rs:229`, `:254`, `:194`

All return `SystemAlert {id, alert_key, severity, title, message, source,
created_at, resolved_at?, dismissed_at?, metadata_json?, is_active}`
(`quailsync-common/src/lib.rs:119-131`). `is_active` is derived, not stored
(`alerts.rs:19`). Android `SystemAlertDto` (`QuailSyncApi.kt:747-762`) matches
field-for-field. Android-only.

### `GET /api/alerts` (telemetry) — `telemetry::alerts` (`routes/telemetry.rs:141`)

A **different resource** from `/api/alerts/active` despite the shared prefix:
`Vec<Alert> {severity: "Info"|"Warning"|"Critical", message, timestamp}`
(`quailsync-common/src/lib.rs:103-107`). Dashboard-only (`index.html:2375`,
`?minutes=120`).

### `GET /api/status`, `GET /api/brooder/history` — `routes/telemetry.rs:108`, `:43`

`status` → `{agent_connected, last_brooder_reading?, last_system_metric?,
last_detection_event?}`. `brooder/history?minutes=N` → `Vec<BrooderReading>
{temperature_f, humidity_percent, timestamp, brooder_id?}`. Dashboard `:1031`,
`:1338` (reads `temperature_f`/`humidity_percent` at `:1341-1344`). Match.

### `GET /api/trailcam/latest/{id}` and `/history/{id}` — `routes/trailcam.rs:174`, `:248`

Hand-rolled JSON. `latest` → `{camera_id, bird_count, timestamp, confidence_avg,
ambient_temperature_f, detections, image_url, annotated_image_url}`
(`trailcam.rs:219-232`); `history?hours=N` adds `min_confidence`,
`inference_time_ms`, `created_at` (`trailcam.rs:303-320`). `404` when the camera
has no observation. Android `TrailcamLatest` (`QuailSyncApi.kt:321-329`) matches
but omits `ambient_temperature_f` (extra server field, ignored).
`detections` elements carry `class_name`/`confidence`/`bbox` from the Python
detector (`indoor-pipeline/detector.py:35`, `:140`), matching `TrailcamDetection`
(`:331-335`). Dashboard `:1937`, `:4795`. Image URLs are server-relative and made
absolute client-side (`CameraScreen.kt:403`, `:639`).

### `GET /api/indoorcam/latest/{id}` and `/history/{id}` — `routes/indoorcam.rs:258`, `:330`

`latest` → `{camera_id, detection_count, timestamp, confidence_avg, detections,
class_counts, detection_label, image_url, annotated_image_url}`
(`indoorcam.rs:297-314`). `detection_label` is **`null`** when there are no
detections and `class_counts` is `{}` (`indoorcam.rs:105-107`). Android
`IndoorcamLatest` (`QuailSyncApi.kt:418-428`) matches. Dashboard `:1705`.

### `GET /api/indoorcam/cameras` and `/api/trailcam/cameras` — `indoorcam.rs:232`, `trailcam.rs:148`

Both return `[{camera_id, label}]` with labels `"Indoor Cam N"` / `"Outdoor Cam N"`.
Android reuses `TrailcamCamera` for both (`QuailSyncApi.kt:339-342`, `:860`, `:901`).
Dashboard `:1929`. Match.

### `GET /api/govee/sensors` + assign/unassign — `routes/govee.rs:162`, `:180`, `:235`

`GoveeSensor {id, govee_device_id, name?, model?, first_seen, last_seen,
assignment?: {brooder_id, brooder_name, assigned_at}, latest_reading?:
{temperature_f, humidity, recorded_at}}`. `PUT .../assign` takes `{brooder_id}` and
returns the sensor; `DELETE` returns no content. Android `:349-380`, `:875-885`;
dashboard `:1616`, `:4785`, `:4911`, `:4922` via `_put`/`_del` (`:4933-4951`). Match.

### `GET /api/trail-cameras` + assign/unassign — `routes/trail_cameras.rs:186`, `:204`, `:259`

`TrailCamera {id, spypoint_camera_id, name?, model?, first_seen, last_seen,
assignment?: {brooder_id, brooder_name, assigned_at}}`. Android `:384-407`,
`:888-898`; dashboard `:1654`, `:4786`, `:4795` (reads `spypoint_camera_id`),
`:4916`, `:4928`. Match.

### `GET /api/indoor-cameras` + assign/unassign — `routes/indoor_cameras.rs:205`, `:320`, `:385`

`IndoorCamera {id, camera_id, name?, rtsp_url?, model?, first_seen, last_seen,
created_at, assignment?: {brooder_id, brooder_name, housing_type, assigned_at}}`.
Assign rejects hutches. Android `:432-456`, `:907-917`; dashboard `:1689`,
`:1802-1816`. Match.

### `GET /api/incubation/summary` — `routes/incubation.rs:103`

`{window_hours, total_events, slots: [{slot_id, event_count, last_event_at,
last_diff_score}], clutches: [{clutch_id, event_count, last_event_at}]}`.
Dashboard-only (`index.html:1871`). `clutches` is always empty today because
`clutch_id` is static-null (`incubation.rs:171-178`). Match.

### `GET /ws/live` — `ws::ws_live_handler` (`src/ws.rs:72`)

Broadcasts the raw agent telemetry text verbatim (`ws.rs:56`, `:86`). Payloads are
the externally-tagged `TelemetryPayload` enum, e.g. `{"Brooder": {...}}`. Android
`WebSocketService.parseMessage` (`WebSocketService.kt:116-169`) unwraps
`Brooder`/`brooder` and tolerates both `temperature_f`/`temperature` and
`humidity_percent`/`humidity`. `MonitoringService.kt:176` additionally reads
`brooder_name`, which is not a field of `BrooderReading`
(`quailsync-common/src/lib.rs:17-23`); it is guarded by `json.has(...)` and
degrades to `"Brooder #N"` (`MonitoringService.kt:210`). Noted, not flagged.

### `/api/dev/*` — `routes/dev.rs` (registered only when `DEV_MODE=true`)

`status` → `{dev_mode, has_backup}` (`dev.rs:35-48`); `seed`/`stress-seed` →
`{status, backup}` (`:56-59`); `restore` → `{status}` or `404` `{error, message}`
(`:113-123`). Android `:769-781`, `:1111-1121`, all using `Response<T>` so a 404 in
prod hides the dev card (`MainActivity.kt:926-952`). Match.

---

## 3. Discrepancy list

**24 discrepancies. Flagged only — nothing was changed.**

### Hard breaks — the request fails or the feature is dead (10)

**D1 — `POST /api/birds/{id}/weight` does not exist; both clients call it.**
The route is `POST /api/birds/{id}/weight`**s** (`lib.rs:107-110`).
Callers: Android `QuailSyncApi.kt:841-842` (`createBirdWeight`, used by the NFC
weigh-in flow, logged at `NfcScreen.kt:675`); dashboard `index.html:2944`
(`Page.BirdDetail.submitWeight`). Because the path matches no route, the SPA
fallback returns `200 OK` + `index.html`, so the dashboard's `r.json()` throws and
reports "Failed to log weight", and Gson throws on the Android side.
**No weight can be recorded from either client.**
History: the singular route existed as `POST /api/birds/{id}/weight` in commit
`d0943d9` and was collapsed onto `/weights` in the modularization commit `9053b0d`
(the surviving comment at `lib.rs:106` reads "path matches GET route"). Neither
client was updated.

**D2 — Android calls `GET api/birds/nfc/{tag_id}`; the route is `GET /api/nfc/{tag_id}`.**
`QuailSyncApi.kt:847-848` vs `lib.rs:380`. Falls through to the SPA handler and
returns HTML with a 200. The dashboard uses the correct path (`index.html:4367`).

**D3 — Dashboard uses `PUT` for chick-group mortality; the route is `POST`.**
`index.html:3989` (`Page.Nursery.submitMortality`) vs `lib.rs:400-403`. The path
exists, so Axum returns **405 Method Not Allowed**; `API.put` swallows it and the
UI shows "Failed to log mortality". Android uses POST correctly
(`QuailSyncApi.kt:988`).

**D4 — Dashboard `POST /api/birds` sends `lineage_id` (scalar); the handler requires `lineage_ids` (array).**
`CreateBird.lineage_ids: Vec<i64>` has no `#[serde(default)]`
(`quailsync-common/src/lib.rs:293`), so the missing field is a hard serde rejection
→ **422**. Two call sites: `index.html:2642-2648` (Flock → Add Bird) and
`index.html:4409-4415`, POSTed at `:4419` (NFC tag-write flow). Android already
migrated (`QuailSyncApi.kt:287`).

**D5 — Dashboard `POST /api/chick-groups` sends `lineage_id` (scalar); the handler requires `lineage_ids` (array).**
Same mechanism as D4 (`quailsync-common/src/lib.rs:1110`) → **422**. Two call
sites: `index.html:3287-3294` (post-hatch "create chick group" prompt) and
`index.html:3963-3966` (Nursery → Create group). Android already migrated
(`QuailSyncApi.kt:231`).

**D6 — Android `POST /api/cameras` omits the required `status` field and may send `location: null`.**
`CreateCameraRequest` (`QuailSyncApi.kt:43-47`) sends `{name, feed_url, location?}`.
`CreateCameraFeed` (`quailsync-common/src/lib.rs:946-952`) requires
`location: String` and `status: CameraStatus`, neither optional → **422**.
The dashboard sends both correctly (`index.html:3832-3834`).

**D7 — `PUT /api/brooders/{id}` returns an empty 200 body; Android declares a `Brooder` return.**
`brooders.rs:147` returns `StatusCode::OK` alone; `QuailSyncApi.kt:970-971`
declares `suspend fun updateBrooder(...): Brooder`, so Gson's converter hits EOF on
the empty body and throws. The write itself succeeds, so the app reports failure on
a successful edit.

**D10 — `POST /api/chick-groups/{id}/mortality` returns `ChickMortalityLog`; Android declares `ChickGroupDto`.**
Handler: `{id, group_id, count, reason, date}` (`chick_groups.rs:268-274`).
Client: `QuailSyncApi.kt:988-989` → `ChickGroupDto` (`:496-515`), whose non-null
`currentCount: Int` and `hatchDate: String` are absent from the payload. Gson's
reflective construction leaves them `0` and `null`, violating Kotlin null-safety on
`hatchDate` and reporting a stale count.

**D21 — `POST /api/restore` returns `text/plain`; the dashboard parses it as JSON.**
`backup.rs:95-99` vs `index.html:4743`. `API.post` calls `r.json()`, throws, and
returns `null` — the UI reports failure on every successful restore.

**D22 — Dashboard sends `reason: 'Manual'` to `POST /api/processing`.**
`index.html:2961-2966`. Valid `ProcessingReason` values are `ExcessMale`,
`LowWeight`, `PoorGenetics`, `Age`, `Other` (`quailsync-common/src/lib.rs:446-453`)
→ **422**. "Schedule for processing" from the Bird Detail page never creates a record.

### Silent field-level drift — deserializes, but the field is always empty (10)

**D11 — Android `Bird.sire_id` / `Bird.dam_id` never populated.**
`QuailSyncApi.kt:77-78`; the handler sends `mother_id`/`father_id`
(`quailsync-common/src/lib.rs:196-197`). No in-tree Kotlin reads them today, so
this is dormant — but the model advertises parentage the app can never show. The
dashboard uses the correct names (`index.html:2583-2584`, `:2715-2716`).

**D12 — Android `Bird.brooder_id` never populated.** `QuailSyncApi.kt:75`; the
handler sends `current_brooder_id` (`quailsync-common/src/lib.rs:204`).

**D13 — Android `Bird.band_id` never populated, and it drives visible UI text.**
`QuailSyncApi.kt:69`, read at `BrooderManageScreen.kt:556` and `:632`
(`Text(b.bandId ?: "Bird #${b.id}")`). The resident lists on the brooder-manage
screen therefore always show `"Bird #N"` instead of a band label. The handler never
sends `band_id`; the closest real field is `band_color`.

**D14 — Android `Bird.latest_weight` never populated.** `QuailSyncApi.kt:79`.
No handler emits it; weights come from `GET /api/birds/{id}/weights`.

**D15 — Android `Bird.species` never populated.** `QuailSyncApi.kt:71`. Not a
column on `birds` and not a field on the `Bird` DTO.

**D16 — Android `Clutch.lineage_name` never populated, and it drives visible UI text.**
`QuailSyncApi.kt:183`. Read at `HatchCountdownWorker.kt:65`
(`clutch.lineageName ?: "Clutch #${clutch.id}"` — the hatch notification always
shows the fallback), `ClutchScreen.kt:911`, `:1281`. Two other sites
(`ClutchScreen.kt:348`, `:381`) have a local `lineageMap` fallback and recover.
The handler sends `breeding_group_name` but never `lineage_name`
(`quailsync-common/src/lib.rs:242-263`).

**D17 — Android `Clutch.egg_count` / `fertile_count` / `hatch_count` never populated.**
`QuailSyncApi.kt:184-188`. They exist only as `?:` fallbacks behind
`eggs_set`/`eggs_fertile`/`eggs_hatched` (`:200-202`), which the handler does send.
Dead compatibility shims.

**D18 — Android `Camera.url` and `Camera.brooder_name` never populated.**
`QuailSyncApi.kt:265`, `:270`. `CameraFeed` sends `feed_url` and `brooder_id`
(`quailsync-common/src/lib.rs:936-943`); nothing joins the brooder name in.

**D19 — Android `Brooder` declares 7 fields the handler never sends.**
`location`, `capacity`, `status`, `latest_temperature`, `latest_humidity`,
`latest_temperature_f`, `latest_humidity_percent` (`QuailSyncApi.kt:21-28`).
The comment at `:26` ("Alternative field names the server might use") is
speculative — none of these exist on `Brooder`
(`quailsync-common/src/lib.rs:1011-1023`). Temperature comes from
`/api/brooders/{id}/status` or `/readings` instead.

**D20 — Android `PhotoUploadResponse.url` / `.path` never populated.**
`QuailSyncApi.kt:304-305` vs the handler's `{id, photo_path, photo_uploaded_at}`
(`photos.rs:183-187`).

**D23 — `GET /api/brooders/{id}/alerts` is a hardcoded empty array.**
`brooders.rs:202-207`. Android's `getBrooderAlerts` (`QuailSyncApi.kt:823-824`)
and its `BrooderAlert` model (`:57-65`) can never receive data. The handler's own
comment says a migration is pending.

### Response-shape inconsistencies — cope today, will break a contract test (4)

**D8 — `PUT /api/chick-groups/{id}` returns an empty 200 body.**
`chick_groups.rs:141`. Dashboard `index.html:2357` awaits `API.put`, which returns
`null`, and the caller treats `null` as failure even though the write landed.
Android correctly uses `Response<Unit>` (`QuailSyncApi.kt:952-956`) and raw OkHttp.

**D9 — `PUT /api/cameras/{id}/brooder` returns an empty 200 body.**
`cameras.rs:92`. No client consumes it today; flagged so a contract test does not
assert a JSON body.

**D26 — `GET /api/brooders/{id}/headcount/latest` has two different 200 shapes.**
Typed `HeadcountResponse` with non-nullable `count`/`timestamp` on the found path
(`brooders.rs:256-260`) versus hand-rolled `{brooder_id, count: null,
timestamp: null}` on the not-found path (`brooders.rs:263-271`). Both clients
null-check, so nothing breaks now, but the contract is ambiguous.

**D29 — Several handlers return `404` with a JSON `null` body.**
`Json(None::<T>)` at `birds.rs:117`, `birds.rs:154`, `birds.rs:317`,
`clutches.rs:174`, `clutches.rs:216`, `processing.rs:427`, `breeding.rs:232`,
`breeding.rs:385`. Every other error path returns either plain text or
`{"error", "message"}` — three different error encodings across the surface.

---

## 4. Uncertain

Endpoints where I could not determine from this repo alone whether they are live or
dead. **None were dropped and none were guessed at.**

| Endpoint | Reasoning |
|---|---|
| `POST /api/trailcam/observation`, `POST /api/indoorcam/observation`, `PATCH /api/indoorcam/observation/{id}` | Almost certainly live, but the writers are the Python pipelines (`indoor-pipeline/`, `trailcam/`), not the two clients in scope. I confirmed `indoor-pipeline/detector.py:35,140` produces the `class_name` key the read handlers depend on (`indoorcam.rs:97`), but did not audit the posters end-to-end. |
| `POST /api/govee/readings`, `POST /api/trail-cameras/register` | Same: written by `govee-poller/` and the SPYPOINT poller. |
| `POST /api/brooders/{id}/headcount` | Written by the Pi agent (`pi-agent/`, `crates/quailsync-agent`). Both clients only read `/headcount/latest`. |
| `POST /api/alerts`, `POST /api/alerts/resolve` | Written by Pi cron/maintenance scripts per the comment at `quailsync-common/src/lib.rs:110-115`. Android only reads `/active` and `/recent`. |
| `GET /api/frames`, `POST /api/frames`, `POST /api/frames/{id}/detections` | No reference anywhere in `dashboard/index.html` or `android/`. The write side is likely the Pi agent's camera pipeline; the read side (`GET /api/frames`) has no consumer I could find. Possibly dead. |
| `GET /api/cameras/{id}/detections/summary` | **No consumer found in either client.** The project memory file describes a Cameras page with "detection summaries", but the current dashboard Cameras page (`index.html:3684-3690`) fetches only `/api/brooders` and `/api/cameras`. Either the feature was removed or the endpoint is now dead. Needs a product-level answer. |
| `PUT /api/cameras/{id}/brooder` | No consumer found. The dashboard associates cameras with brooders through `PUT /api/brooders/{id}` (`camera_url`) instead. Probably superseded. |
| `GET /api/brooder/latest`, `GET /api/system/latest` | No consumer found. The dashboard uses `/api/brooder/history` and `/api/brooders/{id}/status`. `/api/system/latest` has no reader at all — ingest exists and `/api/status` reads the `system_metrics` timestamp, but nothing renders the metrics themselves. |
| `DELETE /api/readings` | No consumer found. Looks like an operator/debug endpoint. |
| `GET /api/incubation/events` | No consumer found; only `/api/incubation/summary` is used (`index.html:1871`). May be intentional as a raw-event API for future tooling. |
| `GET /api/birds/{id}` | No consumer found. Both clients fetch the full `/api/birds` list and filter client-side (`index.html:2675-2677`, `FlockScreen`). Not dead exactly — it is the natural single-resource read a contract test would want — but currently unused. |
| `GET /api/breeding-groups/{id}` | Same as above: both clients use the list endpoint. |
| `POST /api/indoor-cameras`, `GET`/`PUT`/`DELETE /api/indoor-cameras/{id}` | Only the list and the `/assign` sub-routes are consumed. The CRUD may be intended for a management UI that does not exist yet, or may be exercised by a script I did not find. |
| `GET /api/brooders/{id}/sensors`, `GET /api/brooders/{id}/cameras`, `GET /api/brooders/{id}/indoor-cameras` | Per-brooder scoped variants of the three global list endpoints. Neither client uses them — both fetch the global list and filter by `assignment.brooder_id` client-side (`index.html:1616-1620`, `DashboardScreen.kt:277`, `:286`). Possibly superseded. |
| `GET /api/birds/{id}/photo` (singular serve) | No consumer found. Both clients render photos from the `url` values returned by `/api/birds/{id}/photos` (`index.html:2701`, `FlockScreen.kt:917`). The singular route may still be used by an external tool or a bookmark. |
| `GET /health`, `GET /metrics` | Infrastructure endpoints (`docker-compose.yml`, `prometheus.yml`). Not client-facing; listed for completeness. |

---

## 5. Refactors noticed but deliberately not made

Recorded here per the "note them, do not fix" instruction.

1. **`Page.Processing.scheduleFromRec` is dead code** (`index.html:3630-3641`).
   It was the handler for the old "Recommended" kanban column, replaced by the
   breeding-capacity banner when `/api/flock/cull-recommendations` changed shape
   (`index.html:3568-3570`). Grep finds no callers. It is also the only remaining
   caller-parameterized `reason` on `POST /api/processing`.
2. **Two unrelated resources share the `/api/alerts` prefix** — brooder telemetry
   alerts (`GET /api/alerts`) and system/maintenance alerts (`/api/alerts/active`,
   `/recent`, `/{id}/dismiss`, `POST /api/alerts`), with completely different
   shapes. A contract suite must treat these as two resources.
3. **Three error encodings coexist**: plain text (`brooders.rs:76`, the
   `assign_birds` 400s), `{"error", "message"}` JSON (`camera_assignment.rs:30`,
   `trailcam.rs:24`, `indoorcam.rs:32`, `processing.rs:526-531`), and a bare JSON
   `null` with a 404 status (D29).
4. **Several handlers bypass the shared DTOs** and hand-roll `serde_json::json!`
   bodies: `photos::upload_bird_photo`, `breeding::inbreeding_check`, all the
   `trailcam.rs`/`indoorcam.rs` reads, `processing::cull_batch`, the empty branch
   of `brooders::get_headcount_latest`, `telemetry::clear_readings`. These are
   exactly the endpoints where field names have drifted from the clients, because
   nothing type-checks them against `quailsync-common`.
5. **`filter_map(|r| r.ok())` silently drops row-mapping errors** in ~20 list
   handlers (the codebase already carries `// TODO` markers at `telemetry.rs:59`
   and `:156`). A malformed row disappears from the response instead of surfacing
   an error — another way a response can change shape with nothing noticing.
