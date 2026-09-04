//! API contract tests (KAN-2).
//!
//! One data-driven table of every route the Android app or the dashboard
//! consumes, walked by a handful of property tests. This suite deliberately
//! knows nothing about business logic — it asserts *shape and reachability*
//! only. Validation rules, permissions and behaviour live in the per-feature
//! test files.
//!
//! It exists because four separate breaks shipped in one week, and every one of
//! them was a shape or reachability regression that no test was watching:
//!
//! | Break  | What changed                                    | Property that catches it |
//! |--------|-------------------------------------------------|--------------------------|
//! | KAN-26 | unmatched `/api/*` fell through to the SPA      | `content_type_matches_contract` — the SPA answers `text/html`, not `application/json` |
//! | KAN-27 | `lineage_id` → `lineage_ids` on two creators    | `creators_return_the_entity` — the old body 422s, so no `id` comes back |
//! | KAN-28 | `/weight` → `/weights`, `/birds/nfc` → `/nfc`   | `route_is_reachable` — a renamed path 404s, a wrong verb 405s |
//! | KAN-29 | updaters answered a bare 200 with no body       | `updaters_round_trip` — an empty PUT body cannot equal its GET |
//!
//! **Adding a route:** if a client starts calling an endpoint, add a row to
//! `CONTRACT`. If you change a handler's response shape, update its row in the
//! same commit. See `docs/CONTRACT_TESTS.md`.

use std::sync::atomic::{AtomicU32, Ordering};

use quailsync_common::{HousingType, Sex};
use serde_json::{json, Value};

mod common;
use common::*;

// ===========================================================================
// The table
// ===========================================================================

/// Which fixture rows a route needs before it can be exercised. The resolver
/// seeds them and substitutes the ids into the path template.
#[derive(Clone, Copy, PartialEq)]
enum Seed {
    /// No fixture needed.
    None,
    /// A lineage. Substitutes `{lineage}`.
    Lineage,
    /// A lineage + a bird on it. Substitutes `{lineage}` and `{bird}`.
    Bird,
    /// A lineage + a chick group. Substitutes `{lineage}` and `{group}`.
    ChickGroup,
    /// A brooder. Substitutes `{brooder}`.
    Brooder,
    /// A lineage + a clutch on it. Substitutes `{lineage}` and `{clutch}`.
    Clutch,
    /// A brooder *and* a bird, for the assignment endpoints.
    BrooderAndBird,
    /// A brooder *and* a chick group, for assign-group.
    BrooderAndGroup,
}

/// What the response body should look like. Not the full schema — just enough
/// that a client parsing it cannot be silently broken.
#[derive(Clone, Copy, PartialEq)]
enum CType {
    /// `application/json`, body parses as JSON.
    Json,
    /// No body at all (204, or a bodyless 404).
    Empty,
    /// `text/plain`. Only legitimate where no client calls `.json()` on it.
    Text,
}

/// How the route participates in the deeper property tests.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Shape and reachability only.
    Plain,
    /// An updater whose response must equal a GET on the same path.
    Updater,
    /// A creator whose response must carry an `id` that is then GET-able at
    /// `collection/{id}`.
    Creator,
    /// A creator whose collection has no single-resource GET route, so only the
    /// `id` can be checked. `/api/brooders/{id}` is PUT + DELETE only
    /// (lib.rs:283-285) and no client asks for a GET, so this is a real gap in
    /// the API rather than a defect.
    CreatorNoRead,
}

struct Route {
    method: &'static str,
    /// Path template; `{bird}` etc. are replaced by seeded ids.
    path: &'static str,
    seed: Seed,
    /// Request body, if the handler takes one.
    body: Option<fn(&Ids) -> Value>,
    status: u16,
    ctype: CType,
    kind: Kind,
}

/// Ids produced by the seeding step, for path and body substitution.
#[derive(Default, Clone, Copy)]
struct Ids {
    lineage: i64,
    bird: i64,
    group: i64,
    brooder: i64,
    clutch: i64,
}

const fn r(
    method: &'static str,
    path: &'static str,
    seed: Seed,
    body: Option<fn(&Ids) -> Value>,
    status: u16,
    ctype: CType,
    kind: Kind,
) -> Route {
    Route {
        method,
        path,
        seed,
        body,
        status,
        ctype,
        kind,
    }
}

/// Every route the Android app or dashboard consumes that can be exercised
/// with the shared seeds. Statuses and content-types here were observed
/// against the real router, not assumed.
///
/// Deliberately excluded (see `docs/CONTRACT_TESTS.md` for the full list):
/// websocket upgrades, `/api/dev/*`, backup/restore *success* paths, and the
/// pipeline-fed trailcam / indoorcam / govee per-entity routes, which need
/// fixtures these helpers cannot produce without inventing a schema.
#[rustfmt::skip]
static CONTRACT: &[Route] = &[
    // --- status / alerts ---------------------------------------------------
    r("GET",  "/api/status",                    Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/alerts",                    Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/alerts/active",             Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/alerts/recent",             Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooder/history",           Seed::None, None, 200, CType::Json, Kind::Plain),

    // --- lineages ----------------------------------------------------------
    r("GET",  "/api/lineages",                  Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/lineages",                  Seed::None,
      Some(|_| json!({"name":"CT","source":"S","notes":null})), 201, CType::Json, Kind::Plain),

    // --- birds -------------------------------------------------------------
    r("GET",  "/api/birds",                     Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/birds",                     Seed::Lineage,
      Some(|i| json!({"band_color":null,"sex":"Male","lineage_ids":[i.lineage],
                      "hatch_date":"2026-01-01","mother_id":null,"father_id":null,
                      "generation":1,"status":"Active","notes":null})),
      201, CType::Json, Kind::Creator),
    r("GET",  "/api/birds/{bird}",              Seed::Bird, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/birds/{bird}",              Seed::Bird, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("DELETE", "/api/birds/{bird}",            Seed::Bird, None, 204, CType::Empty, Kind::Plain),
    r("GET",  "/api/birds/{bird}/weights",      Seed::Bird, None, 200, CType::Json, Kind::Plain),
    // KAN-28: this path was `/weight` in both clients and 404'd.
    r("POST", "/api/birds/{bird}/weights",      Seed::Bird,
      Some(|_| json!({"weight_grams":100.0,"date":"2026-01-01","notes":null})),
      201, CType::Json, Kind::Plain),
    r("GET",  "/api/birds/{bird}/photos",       Seed::Bird, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/birds/{bird}/lineages",     Seed::Bird,
      Some(|i| json!({"lineage_ids":[i.lineage]})), 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/birds/{bird}/move",         Seed::BrooderAndBird,
      Some(|i| json!({"brooder_id":i.brooder})), 200, CType::Json, Kind::Plain),

    // KAN-28: Android called `/api/birds/nfc/{tag}`; the route is `/api/nfc/{tag}`.
    // A miss is a *handler* 404 with a JSON null body, not the router's 404.
    r("GET",  "/api/nfc/NO-SUCH-TAG",           Seed::None, None, 404, CType::Json, Kind::Plain),

    // --- camera / sensor collections ---------------------------------------
    r("GET",  "/api/trailcam/cameras",          Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/indoorcam/cameras",         Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/govee/sensors",             Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/trail-cameras",             Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/indoor-cameras",            Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/cameras",                   Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/cameras",                   Seed::None,
      Some(|_| json!({"name":"C","location":"L","feed_url":"http://x/","status":"Active","brooder_id":null})),
      201, CType::Json, Kind::Plain),
    r("GET",  "/api/cameras/1/assignment",      Seed::None, None, 404, CType::Json, Kind::Plain),

    // --- incubation / clutches ---------------------------------------------
    r("GET",  "/api/incubation/summary",        Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/clutches",                  Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/clutches",                  Seed::Lineage,
      Some(|i| json!({"breeding_group_id":null,"lineage_id":i.lineage,"eggs_set":12,
                      "eggs_fertile":null,"eggs_hatched":null,"set_date":"2026-01-01",
                      "status":"Incubating","notes":null})),
      201, CType::Json, Kind::Creator),
    r("GET",  "/api/clutches/{clutch}",         Seed::Clutch, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/clutches/{clutch}",         Seed::Clutch, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("DELETE", "/api/clutches/{clutch}",       Seed::Clutch, None, 204, CType::Empty, Kind::Plain),

    // --- processing --------------------------------------------------------
    r("GET",  "/api/processing",                Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/processing/queue",          Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/processing",                Seed::Bird,
      Some(|i| json!({"bird_id":i.bird,"reason":"Other","scheduled_date":"2026-01-01","notes":null})),
      201, CType::Json, Kind::Plain),

    // --- breeding / flock ---------------------------------------------------
    r("GET",  "/api/breeding-groups",           Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/breeding-groups",           Seed::Bird,
      Some(|i| json!({"name":"G","male_ids":[i.bird],"female_ids":[],
                      "start_date":"2026-01-01","notes":null})),
      201, CType::Json, Kind::Plain),
    r("GET",  "/api/flock/summary",             Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/flock/cull-recommendations", Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/breeding/suggest",          Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/breeding/diversity",        Seed::None, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/inbreeding-check?male_id={bird}&female_id={bird}", Seed::Bird,
      None, 200, CType::Json, Kind::Plain),

    // --- settings ----------------------------------------------------------
    r("GET",  "/api/settings",                  Seed::None, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/settings",                  Seed::None, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("GET",  "/api/settings/genetics",         Seed::None, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/settings/genetics",         Seed::None, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("GET",  "/api/system-settings",           Seed::None, None, 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/system-settings",           Seed::None, Some(|_| json!({})), 200, CType::Json, Kind::Updater),

    // --- brooders ----------------------------------------------------------
    r("GET",  "/api/brooders",                  Seed::None, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/brooders",                  Seed::None,
      Some(|_| json!({"name":"B2","lineage_id":null,"life_stage":"Chick","qr_code":"ct-qr",
                      "notes":null,"camera_url":null,"housing_type":"brooder"})),
      201, CType::Json, Kind::CreatorNoRead),
    // KAN-29: this answered a bare 200 with no body.
    r("PUT",  "/api/brooders/{brooder}",        Seed::Brooder, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("DELETE", "/api/brooders/{brooder}",      Seed::Brooder, None, 204, CType::Empty, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/readings",         Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/status",           Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/alerts",           Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/headcount/latest", Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/target-temp",      Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/residents",        Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/sensors",          Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/cameras",          Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("GET",  "/api/brooders/{brooder}/indoor-cameras",   Seed::Brooder, None, 200, CType::Json, Kind::Plain),
    r("POST", "/api/brooders/{brooder}/assign-birds",     Seed::BrooderAndBird,
      Some(|i| json!({"bird_ids":[i.bird]})), 200, CType::Json, Kind::Plain),
    r("POST", "/api/brooders/{brooder}/unassign-birds",   Seed::BrooderAndBird,
      Some(|i| json!({"bird_ids":[i.bird]})), 200, CType::Json, Kind::Plain),
    r("PUT",  "/api/brooders/{brooder}/assign-group",     Seed::BrooderAndGroup,
      Some(|i| json!({"group_id":i.group})), 200, CType::Json, Kind::Plain),
    r("DELETE", "/api/brooders/{brooder}/assign-group",   Seed::Brooder, None, 204, CType::Empty, Kind::Plain),

    // --- chick groups ------------------------------------------------------
    r("GET",  "/api/chick-groups",              Seed::None, None, 200, CType::Json, Kind::Plain),
    // KAN-27: this body used to send scalar `lineage_id` and 422'd.
    r("POST", "/api/chick-groups",              Seed::Lineage,
      Some(|i| json!({"clutch_id":null,"lineage_ids":[i.lineage],"brooder_id":null,
                      "initial_count":5,"hatch_date":"2026-01-01","notes":null})),
      201, CType::Json, Kind::Creator),
    r("GET",  "/api/chick-groups/{group}",      Seed::ChickGroup, None, 200, CType::Json, Kind::Plain),
    // KAN-29: this answered a bare 200 with no body.
    r("PUT",  "/api/chick-groups/{group}",      Seed::ChickGroup, Some(|_| json!({})), 200, CType::Json, Kind::Updater),
    r("DELETE", "/api/chick-groups/{group}",    Seed::ChickGroup, None, 204, CType::Empty, Kind::Plain),
    r("PUT",  "/api/chick-groups/{group}/lineages", Seed::ChickGroup,
      Some(|i| json!({"lineage_ids":[i.lineage]})), 200, CType::Json, Kind::Plain),
    // KAN-28: the dashboard used PUT here; the route is POST, so it 405'd.
    r("POST", "/api/chick-groups/{group}/mortality", Seed::ChickGroup,
      Some(|_| json!({"count":1,"reason":"contract"})), 200, CType::Json, Kind::Plain),

    // --- backup / restore ---------------------------------------------------
    r("GET",  "/api/backups",                   Seed::None, None, 200, CType::Json, Kind::Plain),
    // KAN-29 mismatch 4: this handler answers text/plain, and the dashboard's
    // API.post used to call r.json() on it. The traversal guard is the one
    // branch that can be exercised without actually restoring the database,
    // and it is served by the same `(StatusCode, &str)` return, so it pins the
    // content-type class the client has to cope with.
    r("POST", "/api/restore",                   Seed::None,
      Some(|_| json!({"filename":"../etc/passwd"})), 400, CType::Text, Kind::Plain),
];

// ===========================================================================
// Harness
// ===========================================================================

static PHOTO_DIR_COUNTER: AtomicU32 = AtomicU32::new(0);

/// Each case gets its own server so destructive rows (the DELETEs) cannot
/// affect their neighbours, and its own photo dir so the shared temp dir from
/// other suites cannot leak in.
async fn spawn() -> String {
    let n = PHOTO_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("qs-contract-{}-{}", std::process::id(), n));
    let _ = std::fs::remove_dir_all(&dir);
    spawn_test_server_with_photos(quailsync_server::state::PhotoConfig::for_dir(dir)).await
}

async fn seed_for(route: &Route, base: &str, c: &reqwest::Client) -> Ids {
    let mut ids = Ids::default();
    match route.seed {
        Seed::None => {}
        Seed::Lineage => {
            ids.lineage = seed_lineage(base, c, "CT").await;
        }
        Seed::Bird => {
            ids.lineage = seed_lineage(base, c, "CT").await;
            ids.bird = seed_bird(base, c, vec![ids.lineage], Sex::Male).await;
        }
        Seed::ChickGroup => {
            ids.lineage = seed_lineage(base, c, "CT").await;
            ids.group = seed_chick_group(base, c, vec![ids.lineage]).await;
        }
        Seed::Brooder => {
            ids.brooder = seed_brooder(base, c, "CT-B", "ct-qr-1", HousingType::Brooder).await;
        }
        Seed::Clutch => {
            ids.lineage = seed_lineage(base, c, "CT").await;
            ids.clutch = seed_clutch(base, c, Some(ids.lineage)).await;
        }
        Seed::BrooderAndBird => {
            ids.lineage = seed_lineage(base, c, "CT").await;
            ids.bird = seed_bird(base, c, vec![ids.lineage], Sex::Male).await;
            ids.brooder = seed_brooder(base, c, "CT-B", "ct-qr-1", HousingType::Brooder).await;
        }
        Seed::BrooderAndGroup => {
            ids.lineage = seed_lineage(base, c, "CT").await;
            ids.group = seed_chick_group(base, c, vec![ids.lineage]).await;
            ids.brooder = seed_brooder(base, c, "CT-B", "ct-qr-1", HousingType::Brooder).await;
        }
    }
    ids
}

fn resolve(path: &str, ids: &Ids) -> String {
    path.replace("{lineage}", &ids.lineage.to_string())
        .replace("{bird}", &ids.bird.to_string())
        .replace("{group}", &ids.group.to_string())
        .replace("{brooder}", &ids.brooder.to_string())
        .replace("{clutch}", &ids.clutch.to_string())
}

fn label(route: &Route) -> String {
    format!("{} {}", route.method, route.path)
}

async fn send(
    c: &reqwest::Client,
    method: &str,
    url: &str,
    body: Option<Value>,
) -> reqwest::Response {
    let req = match method {
        "GET" => c.get(url),
        "POST" => c.post(url),
        "PUT" => c.put(url),
        "PATCH" => c.patch(url),
        "DELETE" => c.delete(url),
        other => panic!("unsupported method {other}"),
    };
    let req = match body {
        Some(b) => req.json(&b),
        None => req,
    };
    req.send()
        .await
        .unwrap_or_else(|e| panic!("{method} {url}: request failed: {e}"))
}

/// Run `f` for every row, collecting failures so one run reports every broken
/// endpoint rather than stopping at the first.
macro_rules! for_each_route {
    (|$route:ident, $base:ident, $c:ident, $ids:ident| $body:block) => {{
        let mut failures: Vec<String> = Vec::new();
        for $route in CONTRACT {
            let $base = spawn().await;
            let $c = client();
            let $ids = seed_for($route, &$base, &$c).await;
            let outcome = std::panic::AssertUnwindSafe(async { $body });
            if let Err(msg) = outcome.0.await {
                failures.push(msg);
            }
        }
        assert!(
            failures.is_empty(),
            "\n{} contract failure(s):\n{}\n",
            failures.len(),
            failures.join("\n")
        );
    }};
}

/// Name the keys that differ between two JSON objects. Dumping both bodies
/// buries a one-field difference in 300 characters of identical text, and a
/// one-field difference is what a divergence usually is.
fn describe_divergence(put: &Value, get: &Value) -> String {
    let (Some(p), Some(g)) = (put.as_object(), get.as_object()) else {
        return format!(
            "     PUT: {put}
     GET: {get}"
        );
    };
    let mut lines = Vec::new();
    for k in g.keys() {
        if !p.contains_key(k) {
            lines.push(format!("     missing from PUT: `{k}` (GET has {})", g[k]));
        }
    }
    for k in p.keys() {
        if !g.contains_key(k) {
            lines.push(format!("     only in PUT: `{k}` ({})", p[k]));
        } else if p[k] != g[k] {
            lines.push(format!("     differs: `{k}` PUT={} GET={}", p[k], g[k]));
        }
    }
    if lines.is_empty() {
        lines.push("     bodies differ but no top-level key does".into());
    }
    lines.join(
        "
",
    )
}

// ===========================================================================
// Properties
// ===========================================================================

/// The table is not empty and every row is addressable. Guards against a
/// refactor silently emptying the suite.
#[tokio::test]
async fn table_is_populated() {
    assert!(
        CONTRACT.len() >= 60,
        "contract table shrank to {} rows — did a refactor drop entries?",
        CONTRACT.len()
    );
    for route in CONTRACT {
        assert!(
            route.path.starts_with("/api/") || route.path.starts_with("/health"),
            "{}: path should be absolute",
            label(route)
        );
    }
}

/// **KAN-28.** Every route in the table answers. A renamed path gives the
/// router's JSON 404 (`{"error":"not_found"}`); a wrong verb gives 405. Both
/// are contract breaks, and both are what KAN-28's three stale client calls
/// actually hit.
#[tokio::test]
async fn route_is_reachable() {
    for_each_route!(|route, base, c, ids| {
        let path = resolve(route.path, &ids);
        let url = format!("{base}{path}");
        let resp = send(&c, route.method, &url, route.body.map(|f| f(&ids))).await;
        let status = resp.status().as_u16();
        let mime = media_type(&resp);
        let text = resp.text().await.unwrap_or_default();

        if status == 405 {
            return Err(format!(
                "  {} -> 405 Method Not Allowed. The path exists but not for this verb.",
                label(route)
            ));
        }
        // Discriminate the router's catch-all 404 from a handler's own "row not
        // found" 404, which is legitimate. Both carry `"error":"not_found"`, so
        // key off the catch-all's sentinel message (lib.rs:67) instead.
        if status == 404 && text.contains("No such API endpoint.") {
            return Err(format!(
                "  {} -> router 404 (no such endpoint). Path renamed or removed?\n     body: {text}",
                label(route)
            ));
        }
        // KAN-26: the SPA fallback used to answer unmatched /api/* with
        // 200 + index.html.
        if mime == "text/html" {
            return Err(format!(
                "  {} -> text/html. This is the SPA fallback answering an API path.",
                label(route)
            ));
        }
        Ok(())
    });
}

/// **KAN-26 / KAN-29 #4.** The response carries the content-type its clients
/// parse. A status-only check passes on an endpoint returning `index.html`, and
/// a client calling `.json()` on `text/plain` fails at runtime, not in CI.
#[tokio::test]
async fn content_type_matches_contract() {
    for_each_route!(|route, base, c, ids| {
        let path = resolve(route.path, &ids);
        let url = format!("{base}{path}");
        let resp = send(&c, route.method, &url, route.body.map(|f| f(&ids))).await;
        let status = resp.status().as_u16();
        let mime = media_type(&resp);
        let text = resp.text().await.unwrap_or_default();

        if status != route.status {
            return Err(format!(
                "  {} -> expected status {}, got {status}\n     body: {}",
                label(route),
                route.status,
                text.chars().take(160).collect::<String>()
            ));
        }
        match route.ctype {
            CType::Json => {
                if mime != "application/json" {
                    return Err(format!(
                        "  {} -> expected application/json, got '{mime}'\n     body: {}",
                        label(route),
                        text.chars().take(160).collect::<String>()
                    ));
                }
                if serde_json::from_str::<Value>(&text).is_err() {
                    return Err(format!(
                        "  {} -> content-type is JSON but the body does not parse\n     body: {}",
                        label(route),
                        text.chars().take(160).collect::<String>()
                    ));
                }
            }
            CType::Empty => {
                if !text.is_empty() {
                    return Err(format!(
                        "  {} -> expected an empty body, got {} bytes: {}",
                        label(route),
                        text.len(),
                        text.chars().take(160).collect::<String>()
                    ));
                }
            }
            CType::Text => {
                if mime != "text/plain" {
                    return Err(format!(
                        "  {} -> expected text/plain, got '{mime}'",
                        label(route)
                    ));
                }
            }
        }
        Ok(())
    });
}

/// **KAN-29.** An updater answers with the same entity its GET does. This is
/// the property `update_brooder` and `update_chick_group` violated by returning
/// a bare 200 with no body — clients could not tell success from failure.
#[tokio::test]
async fn updaters_round_trip() {
    for_each_route!(|route, base, c, ids| {
        if route.kind != Kind::Updater {
            return Ok(());
        }
        let path = resolve(route.path, &ids);
        let body = route.body.map(|f| f(&ids)).unwrap_or_else(|| json!({}));

        let put = send(&c, route.method, &format!("{base}{path}"), Some(body)).await;
        let put_status = put.status().as_u16();
        let put_mime = media_type(&put);
        let put_text = put.text().await.unwrap_or_default();
        if put_status != 200 || put_mime != "application/json" {
            return Err(format!(
                "  {} -> updater should answer 200 + application/json, got {put_status} '{put_mime}'",
                label(route)
            ));
        }
        if put_text.is_empty() {
            return Err(format!(
                "  {} -> updater answered an empty body; a client cannot tell this from a failure",
                label(route)
            ));
        }

        let get = send(&c, "GET", &format!("{base}{path}"), None).await;
        if get.status().as_u16() != 200 {
            // Not every updater has a GET on the same path (e.g. /api/settings
            // does, /api/birds/{id}/move does not). Only compare when it does.
            return Ok(());
        }
        let get_text = get.text().await.unwrap_or_default();
        let put_json: Value = serde_json::from_str(&put_text).unwrap_or(Value::Null);
        let get_json: Value = serde_json::from_str(&get_text).unwrap_or(Value::Null);
        if put_json != get_json {
            return Err(format!(
                "  {} -> PUT response diverges from GET on the same path
{}",
                label(route),
                describe_divergence(&put_json, &get_json)
            ));
        }
        Ok(())
    });
}

/// **KAN-27.** A creator returns the created entity, with an `id` that is then
/// GET-able. The scalar-`lineage_id` bodies KAN-27 fixed produced a 422 with no
/// `id` at all, so this is the property that would have caught them.
#[tokio::test]
async fn creators_return_the_entity() {
    for_each_route!(|route, base, c, ids| {
        if route.kind != Kind::Creator && route.kind != Kind::CreatorNoRead {
            return Ok(());
        }
        let path = resolve(route.path, &ids);
        let resp = send(
            &c,
            route.method,
            &format!("{base}{path}"),
            route.body.map(|f| f(&ids)),
        )
        .await;
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        if status != route.status {
            return Err(format!(
                "  {} -> expected {}, got {status}\n     body: {}",
                label(route),
                route.status,
                text.chars().take(200).collect::<String>()
            ));
        }
        let body: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                return Err(format!(
                    "  {} -> body is not JSON ({e}): {text}",
                    label(route)
                ))
            }
        };
        let Some(new_id) = body["id"].as_i64() else {
            return Err(format!(
                "  {} -> created entity has no `id`\n     body: {}",
                label(route),
                text.chars().take(200).collect::<String>()
            ));
        };

        if route.kind == Kind::CreatorNoRead {
            return Ok(());
        }
        // ...and the thing it says it created is really there.
        let get = send(&c, "GET", &format!("{base}{path}/{new_id}"), None).await;
        if get.status().as_u16() != 200 {
            return Err(format!(
                "  {} -> created id {new_id} but GET {path}/{new_id} returned {}",
                label(route),
                get.status().as_u16()
            ));
        }
        Ok(())
    });
}
