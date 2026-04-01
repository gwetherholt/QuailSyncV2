# QuailSync V2 — Test Coverage Analysis

## Current Test Inventory

| Test File | Type | Count | Scope |
|-----------|------|-------|-------|
| `crates/quailsync-server/tests/api_tests.rs` | Integration | 6 | Health, bloodlines CRUD, birds CRUD, breeding suggest (3 scenarios) |
| `crates/quailsync-server/tests/boundary_tests.rs` | Boundary/Stress | 62 | Input validation, SQL injection, XSS, WebSocket protocol, alerts, backup security, edge cases |
| `pi-agent/tests/test_pi_agent.py` | Unit | 66 | Sensor reading, payload builders, backoff, system metrics, multi-sensor, serde compat |

**Total: 134 tests** across 3 test files.

---

## Coverage Map — What's Tested vs. What's Not

### Rust Server Routes (9 modules, ~2,360 LOC)

| Route Module | LOC | Tests? | Coverage Level | Notes |
|-------------|-----|--------|----------------|-------|
| `brooders.rs` | 386 | Partial | ~30% | Create/list tested via boundary tests; no tests for PATCH, DELETE, status, target-temp, assign/unassign group, residents |
| `birds.rs` | 314 | Partial | ~35% | Create/list tested; no tests for PATCH, DELETE, move, weight CRUD (except boundary weight edge cases) |
| `breeding.rs` | 479 | Partial | ~25% | `breeding/suggest` well-tested (3 scenarios); no tests for breeding pairs CRUD, breeding groups CRUD, `cull-recommendations`, `flock-summary`, `inbreeding-check` |
| `clutches.rs` | 173 | **None** | 0% | Zero tests for create/list/update/delete clutches. Hatch date auto-calculation (set_date + 17 days) untested. |
| `chick_groups.rs` | 275 | **Minimal** | ~5% | One boundary test for mortality > count. No tests for create, list, get, update, delete, graduation workflow. |
| `processing.rs` | 167 | **Minimal** | ~5% | One boundary test for nonexistent bird. No tests for create, list, update, queue, cull-batch. |
| `cameras.rs` | 226 | **None** | 0% | Zero tests for camera CRUD, frame captures, detection results, detection summary. |
| `telemetry.rs` | 188 | Partial | ~40% | Health, brooder-latest, readings tested; no tests for system-latest, status, alerts query, clear-readings. |
| `backup.rs` | 146 | Partial | ~30% | Path traversal and restore validation tested; no tests for create backup or list backups. |

### Core Logic Modules

| Module | LOC | Tests? | Coverage Level | Notes |
|--------|-----|--------|----------------|-------|
| `alerts.rs` | 93 | Partial | ~40% | Boundary tests cover alert thresholds via integration; no **unit tests** for `youngest_chick_age_in_brooder` or `check_brooder_alerts` directly. Age-based temperature scheduling untested. |
| `ws.rs` | 131 | Partial | ~30% | Boundary tests cover connect/disconnect, malformed messages; no tests for payload routing (CameraAnnounce, QrDetected), broadcast to live clients, agent_connected flag lifecycle. |
| `db/helpers.rs` | 236 | **None** | 0% | Zero unit tests for 8 enum converter pairs (sex, bird_status, clutch_status, processing_reason, etc.) or 6 row mapper functions. |
| `db/mod.rs` | 333 | **None** | 0% | Schema initialization tested implicitly; no tests for individual query functions or cascade delete behavior. |
| `quailsync-common` | 797 | **None** | 0% | Shared types/models have no dedicated tests (serde round-trip, Default impls, validation). |

### Python Agents

| Module | LOC | Tests? | Coverage Level | Notes |
|--------|-----|--------|----------------|-------|
| `pi_agent.py` | 313 | **Good** | ~80% | Well-tested: sensor reading, payloads, backoff, multi-sensor, edge cases. |
| `camera_stream.py` | 469 | **None** | 0% | Zero tests for MJPEG streaming, QR code detection, 3-frame confirmation logic, camera registration, snapshot collection. |

### Android App & Dashboard

| Component | Files | Tests? | Notes |
|-----------|-------|--------|-------|
| Android (Kotlin) | 19 files | **None** | No unit or instrumentation tests for API client, WebSocket manager, NFC service, or UI screens. |
| Dashboard (JS) | 1 file (3,109 LOC) | **None** | No JS testing framework. No tests for API calls, WebSocket handling, or UI logic. |

---

## Priority Recommendations

### Priority 1 — Critical Business Logic (High Impact, Moderate Effort)

#### 1. Chick Group Graduation Tests (`chick_groups.rs`)
**Why:** Graduation creates permanent Bird records with inherited genetics. Bugs here corrupt lineage data.
- Test that graduated birds inherit correct `bloodline_id`, `hatch_date`, `mother_id`, `father_id`
- Test `generation = max(parent_generations) + 1`
- Test group status transitions to "Graduated"
- Test graduation with zero current_count
- Test graduation when breeding pair parents have been deleted

#### 2. Breeding Groups & Cull Recommendations (`breeding.rs`)
**Why:** Breeding decisions directly affect flock genetics. Cull recommendations lead to irreversible actions.
- Test breeding group creation with female count outside 2-5 range (warning path)
- Test `cull-recommendations` endpoint: excess male detection, low weight filter (<170g), no-safe-mate detection
- Test `flock-summary` returns correct counts and sex ratios
- Test `inbreeding-check` for specific male/female pair

#### 3. Alert Engine Unit Tests (`alerts.rs`)
**Why:** Temperature alerts protect live animals. False negatives could be lethal.
- Unit test `youngest_chick_age_in_brooder` with multiple chick groups at different ages
- Unit test age-based temperature thresholds (day 0-7: 95°F, ramping down to room temp by week 6)
- Test severity classification: >3°F deviation = Critical, else Warning
- Test with no chick groups assigned (should use adult defaults)

### Priority 2 — Untested CRUD Routes (Medium Impact, Low Effort)

#### 4. Clutch CRUD Tests (`clutches.rs`)
- Test create: verify `expected_hatch_date = set_date + 17 days`
- Test update: fertility counts, hatch outcome fields
- Test delete: verify cleanup

#### 5. Camera & Detection Tests (`cameras.rs`)
- Test camera CRUD and brooder linking
- Test frame creation with detection results
- Test `detection-summary` aggregation (label counts, avg confidence)

#### 6. Processing Pipeline Tests (`processing.rs`)
- Test create/list/update processing records
- Test `processing/queue` returns only scheduled items
- Test `cull-batch` with valid and invalid status values

### Priority 3 — Data Integrity & Helpers (Medium Impact, Low Effort)

#### 7. Database Helpers Unit Tests (`db/helpers.rs`)
- Round-trip tests for all 8 enum converter pairs
- Test `str_to_*` functions with invalid/unknown strings
- Row mapper tests with mock data

#### 8. Cascade Delete Tests
- Test bird deletion cascades to: weight_records, breeding_pairs, breeding_group_memberships, processing_records
- Test brooder deletion cascades to: readings, unassigns chick groups, clears bird.brooder_id, unlinks cameras
- Test chick group deletion cascades to mortality logs

#### 9. Common Types Tests (`quailsync-common`)
- Serde round-trip tests for all payload types (serialize → deserialize)
- Test `AlertConfig::default()` values match documented thresholds
- Test enum serialization matches expected JSON strings

### Priority 4 — Cross-Component & Integration (High Impact, High Effort)

#### 10. Camera Stream Tests (`camera_stream.py`)
- Test QR code 3-frame confirmation logic
- Test camera registration HTTP call to server
- Test MJPEG frame encoding
- Test graceful degradation when camera hardware is unavailable

#### 11. WebSocket End-to-End Tests
- Test full flow: agent sends BrooderReading → stored in DB → broadcast to live client → alert generated
- Test CameraAnnounce and QrDetected payload handling
- Test multiple live clients receive broadcasts
- Test agent disconnect sets `agent_connected = false`

#### 12. Bird Lifecycle Integration Tests
- Test full lifecycle: hatch (clutch) → chick group → graduation → breeding → processing
- Test NFC tag assignment and lookup
- Test bird move between brooders

---

## Quick Wins (can be done in a single session)

1. **`db/helpers.rs` unit tests** — Pure functions, no async, no server needed. ~20 tests in 30 min.
2. **Clutch CRUD integration tests** — Follow existing `api_tests.rs` pattern. ~5 tests.
3. **Serde round-trip tests for `quailsync-common`** — Pure Rust, fast to write. ~10 tests.
4. **Processing CRUD tests** — Simple REST endpoints. ~5 tests.
5. **Camera CRUD tests** — Simple REST endpoints. ~5 tests.

---

## Test Infrastructure Gaps

| Gap | Impact | Recommendation |
|-----|--------|----------------|
| No code coverage tool configured | Can't measure actual line/branch coverage | Add `cargo-llvm-cov` or `cargo-tarpaulin` to CI |
| No CI pipeline running tests | Tests may break without detection | Add GitHub Actions workflow for `cargo test` + `pytest` |
| No test for database migrations | Schema changes could break production | Add migration test that applies schema to existing DB |
| No load/performance tests | Unknown server capacity limits | Add basic load test (e.g., 100 concurrent telemetry writes) |
| Android has zero test infrastructure | UI and data layer completely untested | Add JUnit + Compose test dependencies to `build.gradle.kts` |
