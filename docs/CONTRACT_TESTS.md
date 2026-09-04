# API contract tests

`crates/quailsync-server/tests/contract_tests.rs` holds one table, `CONTRACT`, listing every
route the Android app or the dashboard consumes, and four property tests that walk it. The
properties are deliberately shallow — a route answers, it answers with the content-type its
clients parse, an updater returns the same entity its GET does, and a creator returns an `id`
that is really there. Business logic, validation and permissions are **not** tested here;
they belong in the per-feature files (`api_tests.rs`, `photo_upload_tests.rs`, and friends).
The suite exists because four breaks shipped in one week — KAN-26, 27, 28 and 29 — and every
one of them was a shape or reachability regression that no test was watching. Shared harness,
seeds and assertions live in `tests/common/mod.rs`.

**Two rules.** First: *any change to a handler's response shape updates `CONTRACT` in the same
commit.* If you change a status code, a content-type, or whether a handler returns a body, the
matching row is part of that change — not a follow-up. A red contract test is the suite doing
its job, so read the failure before you edit the table: it either means you changed a contract
a client depends on, or the row is now stale. Only the second is a reason to edit the row.
Second: *any new client-consumed route gets a row.* When the dashboard or the Android app
starts calling an endpoint, add it to `CONTRACT` with its seed requirement, minimal body,
expected status and content-type class, and mark it `Updater` or `Creator` if it is one. A
route with no row is a route with no protection, which is precisely how the KAN-28 renames
went unnoticed.

The table is intentionally not exhaustive. Excluded today: websocket upgrades (`/ws`,
`/ws/live`), the `/api/dev/*` endpoints, the *success* paths of `POST /api/backup` and
`POST /api/restore` (both mutate real files on disk — restore's error path is covered instead,
which is what pins its `text/plain` class), the image-serving routes, and the per-entity
trailcam / indoorcam / govee routes, which need pipeline-produced fixtures the shared seeds
cannot build without inventing a schema. Those last ones are worth adding when a seed helper
for a registered camera or sensor exists; the rest are deliberate. Each row runs against its
own freshly spawned in-memory server, so destructive rows cannot disturb their neighbours and
the table can be reordered freely.

CI needs no special configuration: `.github/workflows/ci.yml` runs `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`, and the
last of those picks up anything under `tests/` automatically.
