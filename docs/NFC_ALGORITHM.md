# QuailSync NFC Algorithm — Complete Walkthrough

Source files referenced throughout:

- `android/app/src/main/AndroidManifest.xml`
- `android/app/src/main/res/xml/nfc_tech_filter.xml`
- `android/app/src/main/java/com/quailsync/app/MainActivity.kt`
- `android/app/src/main/java/com/quailsync/app/data/NfcService.kt`
- `android/app/src/main/java/com/quailsync/app/ui/screens/NfcScreen.kt`

---

## 1. How NFC intents arrive

### 1a. The three Android NFC actions

Android dispatches one of three actions when a tag enters the field, in priority order:

1. **`ACTION_NDEF_DISCOVERED`** — tag is NDEF‑formatted and contains a record matching a MIME or URI filter. Highest priority.
2. **`ACTION_TECH_DISCOVERED`** — tag matches a registered tech‑list (e.g. `Ndef`, `NdefFormatable`). Fires for blank/unformatted tags or NDEF tags without a matching MIME type.
3. **`ACTION_TAG_DISCOVERED`** — fallback. Fires only if no app claimed the tag via the two higher‑priority actions.

### 1b. Manifest declarations (`AndroidManifest.xml:42`–`74`)

The `MainActivity` is declared `singleTop` and `exported="true"`, with three NFC intent filters:

```xml
<intent-filter>
  <action android:name="android.nfc.action.NDEF_DISCOVERED" />
  <category android:name="android.intent.category.DEFAULT" />
  <data android:mimeType="text/plain" />
</intent-filter>
<intent-filter>
  <action android:name="android.nfc.action.TAG_DISCOVERED" />
  <category android:name="android.intent.category.DEFAULT" />
</intent-filter>
<intent-filter>
  <action android:name="android.nfc.action.TECH_DISCOVERED" />
  <category android:name="android.intent.category.DEFAULT" />
</intent-filter>
<meta-data
  android:name="android.nfc.action.TECH_DISCOVERED"
  android:resource="@xml/nfc_tech_filter" />
```

`@xml/nfc_tech_filter.xml` lists two `<tech-list>` blocks — `android.nfc.tech.Ndef` and `android.nfc.tech.NdefFormatable`. Each `<tech-list>` is an AND group; Android OR’s across the lists. So a tag matches if it supports `Ndef` OR `NdefFormatable`.

**Why the TECH filter matters specifically**: factory‑fresh blank NTAG/Mifare tags have no NDEF records, so they never fire `NDEF_DISCOVERED`. Without the `TECH_DISCOVERED` filter + tech‑list, blank tags would either fall through to `TAG_DISCOVERED` (lowest priority, gets pre‑empted by other NFC apps installed on the phone) or wouldn’t be delivered when the activity isn’t already foregrounded.

`singleTop` launch mode is what allows `onNewIntent` to be reused instead of relaunching `MainActivity` on every tap.

`<uses-permission android:name="android.permission.NFC" />` is granted at install time. `<uses-feature android:name="android.hardware.nfc" android:required="false" />` lets the app be installed on devices without NFC hardware (the UI just shows a banner when `isAvailable` is false).

### 1c. Foreground dispatch (runtime override)

The manifest filters route taps when the activity is **not on top**. When the activity *is* foregrounded, foreground dispatch takes precedence and bypasses all other apps’ filters.

`NfcService.enableForegroundDispatch` (`NfcService.kt:99`) is called from `MainActivity.onResume` (`MainActivity.kt:148`) and disabled in `onPause` (`MainActivity.kt:154`). It builds:

- A `PendingIntent` targeting `MainActivity` itself with `FLAG_ACTIVITY_SINGLE_TOP` so the tap arrives in `onNewIntent` rather than `onCreate`.
- Three `IntentFilter`s for `NDEF_DISCOVERED` (with `text/plain`), `TECH_DISCOVERED`, `TAG_DISCOVERED`.
- Two tech‑lists, mirroring `nfc_tech_filter.xml`: `Ndef` and `NdefFormatable`.

So while the app is in the foreground, **every** tag tap routes to `MainActivity` regardless of payload, formatted/unformatted state, or what other NFC apps are installed.

### 1d. Entry points into MainActivity

- **Cold launch with a tap**: `onCreate` → `handleNfcIntent(intent)` (`MainActivity.kt:129`).
- **Warm tap while activity foregrounded**: `onNewIntent` → `handleNfcIntent(intent)` (`MainActivity.kt:159`).

Both funnel through the same routing function.

---

## 2. The routing decision tree — `MainActivity.handleNfcIntent` (`MainActivity.kt:182`)

```
NFC intent arrives
       │
       ▼
Read current batchState (snapshot at entry)
       │
       ├── batchState is AwaitingTagScan or AwaitingTagWrite?
       │       │
       │       ▼  YES — batch flow owns the tap
       │   Compute writeData:
       │     • pendingWriteData if already set, else
       │     • "QS-L<lineageId>" if AwaitingTagScan
       │     • "BIRD-<pendingBird.id>" if AwaitingTagWrite
       │       │
       │       ▼
       │   nfcService.handleBatchIntent(intent, writeData)
       │   (NO conflict detection, NO bird lookup — see §5)
       │       │
       │       ├── returns tagId (success)
       │       │     ├─ Toast "Tag written"
       │       │     ├─ AwaitingTagScan → onBatchTagScanned(tagId)
       │       │     └─ AwaitingTagWrite → onBatchTagWritten(tagId, true)
       │       │
       │       └── returns null (failure)
       │             ├─ writeResult populated with retry-friendly msg
       │             ├─ Banner on BatchAwaitingScanScreen renders it
       │             └─ Write mode stays armed; same bird; next tap retries
       │       │
       │       ▼
       │   navController.navigate(Screen.Nfc)   (return)
       │
       ├── Not in batch ──────────────────────────────────────────────
       │
       ▼
nfcService.handleIntent(intent)
returns Pair(NfcScanResult, WriteAttemptResult?)
       │
       ├── writeAttempt != null  → app was in standalone WRITE mode
       │       │
       │       ├── Written       → Toast "Wrote …", navigate to NFC
       │       ├── Conflict      → viewModel.lookupConflictBird(conflict)
       │       │                   (then TagConflictDialog renders)
       │       └── Failed        → Toast failure message
       │
       └── writeAttempt == null  → standalone READ
               │
               ├── viewModel.lookupBirdByNfc(tagId, payload)
               │     • If payload starts "BIRD-" → look up by payload (BIRD-id)
               │       — server miss + local miss → OrphanTag dialog
               │     • Otherwise look up by hardware tagId
               ├── Toast "Scanned: BIRD-N" or "NFC tag: <id>"
               └── navigate to NFC
```

Key invariant: the **batch state at entry** (snapshot taken on line 183) determines ownership for the entire intent. The batch flow never falls through to `handleIntent` — it either succeeds and advances state, or fails and leaves state untouched so the same tap can be retried.

---

## 3. The batch state machine — `BatchState` (`NfcScreen.kt:138`–`236`)

### 3a. State definitions

| State | Trigger to enter | NFC behavior |
|---|---|---|
| `Idle` | Initial / `dismissBatchSummary()` | None — `NfcMainScreen` (standalone scanner) renders |
| `Setup` | `openBatchSetup()` from main NFC screen | None |
| `AwaitingTagScan(currentIndex, totalCount, lineageId, …)` | `startBatchTagging()` or `advanceFromConfirm()` for next bird | **Write mode armed** with payload `QS-L<lineageId>`. Every tap is overwritten via `handleBatchIntent`. |
| `PerBirdEntry(…, capturedTagId, draftSex…draftPhotoBitmap)` | `onBatchTagScanned(tagId)` or `skipNfcScan()` | None — user fills form |
| `CreatingBird(…)` | `createAndTagBird(...)` invoked | None — API POST in flight |
| `AwaitingTagWrite(…, pendingBird)` | **Legacy path** — present in sealed class but not entered in current NFC‑first flow. The current path goes `PerBirdEntry → CreatingBird → PostTagConfirm`. The state is retained for backward compatibility and the second write‑mode arm path (`BIRD-<id>` payload) is still wired through `handleBatchIntent`. |
| `PostTagConfirm(…, justTaggedBird, justTaggedTagId, photoSaved, weightLogged)` | API succeeds | None — confirmation screen |
| `Complete(graduated, lineageName)` | Last bird advances from `PostTagConfirm` | None — summary screen, optionally flips source chick‑group status to `Graduated` |

### 3b. Transitions in detail

#### Entry into the batch

Two entry points:

1. **From the NFC screen**: user taps "Graduate Batch" → `openBatchSetup()` → `Setup`. User picks lineage + count, taps "Start Tagging N Birds" → `startBatchTagging(count, lineageId, chickGroupId = null)`.
2. **From Hatchery / "Band Group"** (`MainActivity.kt:344`–`368`): clicking a chick group's "Band Group" passes the group's current count and *first* lineage id directly to `startBatchTagging(count, lineageId, chickGroupId = group.id)`, skipping `Setup`. The `chickGroupId` is carried through every subsequent state so the source group can be marked `Graduated` on completion (see §3c).

`startBatchTagging` (`NfcScreen.kt:412`) does two things, in this order:

```kotlin
nfcService.enterWriteMode("QS-L$lineageId")           // arm write mode FIRST
_batchState.value = BatchState.AwaitingTagScan(/*…*/) // then transition
```

The order matters: Compose recomposition triggers off the state flow. If state changed before write mode armed, there would be a microsecond window where `BatchAwaitingScanScreen` is on screen but write mode is off, and a fast user tap would fail.

#### `AwaitingTagScan` — first per‑bird step

The screen (`BatchAwaitingScanScreen`, `NfcScreen.kt:1159`) has a `LaunchedEffect(state.currentIndex)` that calls `viewModel.armNfcForBatchScan()` every time `currentIndex` changes. This is the **belt‑and‑suspenders** re‑arm: it ensures bird 2, 3, 4, … all get write mode engaged even if the synchronous arm in `advanceFromConfirm` was somehow undone by an intermediate state change. The arm is idempotent — calling `enterWriteMode` resets pending data and clears stale `writeResult`/`pendingConflict`, so re‑arming on an already‑armed bird is harmless.

Two outcomes:

- **Tag tap succeeds** → `onBatchTagScanned(tagId)` (`NfcScreen.kt:452`) → `advanceFromScanToEntry(state, capturedTagId = tagId)` → state becomes `PerBirdEntry(capturedTagId = tagId, …)`. The hardware uniqueId rides through state until the bird is POST‑ed.
- **User taps "Skip NFC for this bird"** → `skipNfcScan()` (`NfcScreen.kt:459`) → `nfcService.cancelWriteMode()` + `advanceFromScanToEntry(state, capturedTagId = null)`. Bird is created without an `nfc_tag_id`; user can attach one later from the standalone scanner.

If the tap fails (locked tag, password‑protected with custom password, etc.) `handleBatchIntent` returns null and `_writeResult` is populated. `BatchAwaitingScanScreen` shows the red error card; state doesn't advance; write mode is still armed; the next tap retries.

#### `PerBirdEntry` — fill form

The user picks sex, band color, optional weight, optional photo, optional notes. Form fields are `remember(state.currentIndex)` so they reset for each bird — UNLESS the state's `draftXxx` fields are populated, in which case the prior input is replayed (this is the "Bug 3" restore path described in the source: on a server failure the form state is preserved across the failed `CreatingBird` round‑trip).

Band color auto‑fills from `lastMaleBandColor` / `lastFemaleBandColor` of the current batch when the user picks a sex matching a previously tagged bird's sex.

On "Create & Tag Bird" → `createAndTagBird(sex, bandColor, notes, weightText, photoBitmap, context)`.

#### `CreatingBird` — API call

`createAndTagBird` (`NfcScreen.kt:484`) transitions immediately to `CreatingBird`, then in a coroutine:

1. Build `CreateBirdRequest` with `lineageIds`, `sex`, `status="Active"`, `hatchDate=today`, `bandColor`, `notes`, `chickGroupId`, **`nfcTagId = state.capturedTagId`** (key NFC‑first optimization: collapses the old POST+PUT into a single call).
2. POST to `api.createBird`.
3. On success: optionally POST weight (`api.createBirdWeight`), optionally save photo locally to `filesDir/bird_photos/bird_<id>.jpg`. Update `lastMaleBandColor` / `lastFemaleBandColor` from this bird's pick.
4. Transition to `PostTagConfirm` with `justTaggedTagId = state.capturedTagId ?: ""` (empty string signals "NFC was skipped" so the confirmation screen can hide tag‑specific UI).
5. On HTTP failure or network error: pull a friendly message via `friendlyErrorMessage(e)`, set `writeResult` (so the banner on `PerBirdEntry` shows it), and **transition back to `PerBirdEntry` with every `draftXxx` field populated** so the user retries without retyping.

#### `PostTagConfirm` — optional photo/weight

The user can take/retake a photo or log a weight here too. Confirmation screen. On "Next" → `advanceFromConfirm()` (`NfcScreen.kt:688`).

#### `advanceFromConfirm` — decision point

```kotlin
if (state.currentIndex >= state.totalCount) {
    _batchState.value = BatchState.Complete(state.graduated, lineageName)
    // Optionally flip source chick group's status to 'Graduated'
} else {
    nfcService.enterWriteMode("QS-L${state.lineageId}")      // re-arm FIRST
    _batchState.value = BatchState.AwaitingTagScan(/*…*/)    // then transition
}
```

Same arm‑before‑transition order as `startBatchTagging`. This is the documented root cause of the historical bird‑2+ "Failed to write to tag" bug: doing it in the opposite order opened a race where the screen was visible with write mode off, and the user's fast next tap would hit `handleIntent` with `_writeMode == false`, fall through to the standalone read path, and not write.

#### `Complete` — finalize

`BatchCompleteScreen` shows the summary. If the batch was started from a chick group, `advanceFromConfirm` already kicked off `api.updateChickGroup(groupId, mapOf("status" to "Graduated"))` in a coroutine so the source group disappears from the active Hatchery list. Birds are reloaded from the server.

### 3c. Cancel path

`viewModel.cancelBatch()` (`NfcScreen.kt:402`) is available from every batch screen's red "Cancel" button. It calls `nfcService.cancelWriteMode()` (clears write mode, pending data, conflict state) and sets `_batchState = Idle`. Returns to the standalone scanner.

---

## 4. The standalone NFC flow (`NfcMainScreen`, `NfcScreen.kt:802`)

This is what renders when `batchState == Idle`. Conceptually independent from the batch state machine — uses `NfcService` directly with full conflict detection.

### 4a. Read flow

1. User taps a tag while on the NFC screen (or anywhere — foreground dispatch is global; the routing navigates back to NFC).
2. `handleNfcIntent` sees no batch state, calls `nfcService.handleIntent(intent)`.
3. With `_writeMode == false`, `handleIntent` skips the write branch and just records the scan: extracts hardware tagId (`tag.id` as hex), reads NDEF payload, updates `_lastScan` and prepends to `_scanHistory` (capped at 20).
4. `MainActivity` then calls `viewModel.lookupBirdByNfc(scanResult.tagId, scanResult.payload)` (`NfcScreen.kt:304`):
   - If payload starts with `"BIRD-"`, look up by payload via `api.getBirdByNfcTag("BIRD-N")` (server side: matches by either payload or tagId).
   - Otherwise, look up by hardware `tagId`.
   - On API miss + `"BIRD-N"` payload: try local cache. Still miss → set `_orphanTag = OrphanTag(tagId, orphanBirdId)`. This fires `OrphanTagDialog` ("This tag references bird #N which no longer exists. Reuse for a new bird?"). "Reuse" → `_batchState = Setup`; "Cancel" → just dismiss.
5. A toast confirms the scan. `lastScan` / `scanHistory` are displayed via `NfcResultCard`.

### 4b. Write flow

1. User picks a bird from the "Write Tag" dropdown → `viewModel.startWriteMode(birdId)` → `nfcService.enterWriteMode("BIRD-$birdId")`.
2. The `NfcScanArea` switches to the dusty‑rose pulsing affordance, showing pending payload.
3. User taps tag. `handleNfcIntent` → `nfcService.handleIntent(intent)` with `_writeMode == true`, so the write branch runs:
   - Read existing payload via `readNdefPayload(intent)`.
   - Call `attemptWrite(tag, tagId, existingPayload, writeData)` (`NfcService.kt:396`).
4. `attemptWrite` decides:
   - **Existing payload starts with `"BIRD-"` AND payload != writeData** → return `Conflict(TagConflict)`, store `conflictTag = tag` for later, set `_pendingConflict`. Routes back to NFC screen; `TagConflictDialog` (`NfcScreen.kt:1689`) renders showing the existing bird's name and the pending write.
     - User taps "Overwrite" → `confirmOverwrite()` → `nfcService.confirmOverwrite()` → `writeNdefText(tag, conflict.pendingWriteData)` using the stale `conflictTag` reference. If the tag is no longer in the field, write fails and message is "Tag lost — tap the tag again"; write mode stays armed.
     - User taps "Cancel" → `cancelOverwrite()` → conflict is cleared but write mode stays active so the user can tap a different tag.
   - **Existing payload starts with `"BIRD-"` AND payload == writeData** → return `Written` immediately (no actual write needed; tag already correct).
   - **Otherwise** (blank tag, non‑QuailSync data like a `QS-L...` lineage tag, random text, etc.) → call `writeNdefText` immediately, no conflict prompt.
5. On `Written` → clear write mode, set success `writeResult`, append to scan history.

The standalone flow specifically protects against silently clobbering another bird's tag — every `BIRD-X` overwrite requires explicit user confirmation. The batch flow deliberately skips this protection (§5).

---

## 5. The actual write operation

Two distinct entry points share most of the underlying helpers but differ in conflict handling.

### 5a. Batch path — `handleBatchIntent` (`NfcService.kt:213`)

Permissive. No conflict detection at all. Server‑side `clear_nfc_tag_from_others` handles DB uniqueness when the create‑bird POST eventually runs, so the client never needs to inspect existing tag data.

```
handleBatchIntent(intent, writeData):
  • Validate action is one of NDEF/TECH/TAG_DISCOVERED
  • Extract Tag (Tiramisu+ typed overload, legacy fallback for <33)
  • tagId = tag.id as uppercase hex
  • tryWriteAndRecordSuccess(tag, tagId, writeData)
       ├── success → return tagId
       └── fail   → continue to password-removal fallback
  • If tag.techList contains MifareUltralight:
      tryRemoveNtagPassword(tag, tagId):
         ├── Removed         → retry tryWriteAndRecordSuccess
         │                     · success → return tagId
         │                     · fail    → fall through to generic msg
         ├── CustomPassword  → writeResult = "Custom password — remove via NFC Tools"
         │                     · return null
         └── NotApplicable   → fall through
  • Set writeResult = "This tag can't be written to — it may be locked. Try a different tag."
  • Leave write mode active; return null
```

### 5b. Standalone path — `handleIntent` → `attemptWrite` → `writeNdefText` (`NfcService.kt:136`, `:396`, `:556`)

`writeNdefText` is the single shared write primitive used by both paths:

```
writeNdefText(tag, text):
  • Build NdefRecord (TNF_WELL_KNOWN / RTD_TEXT, lang "en")
  • Wrap in NdefMessage
  • Try formatted-NDEF path (writeToFormattedNdef):
       ndef = Ndef.get(tag)
       if ndef != null:
          • ndef.connect()
          • Log diagnostics: isWritable, maxSize, type, isConnected
          • if !ndef.isWritable → close + return false (fall through to NdefFormatable)
          • ndef.writeNdefMessage(message)
          • close, return true
       on IOException / any exception → close (best-effort), return false
  • If still not written, try blank-tag path (writeToBlankTag):
       formatable = NdefFormatable.get(tag)
       if formatable == null → log unsupported, return false
       • formatable.connect()
       • formatable.format(message)   ← writes capability container + payload atomically
       • close, return true
       on exception → close, return false
  • Both paths failed → log "not writable by any method", return false
```

Why two paths in sequence:

- **`Ndef` path** handles tags already carrying an NDEF capability container — the common state once a tag has been written once.
- **`NdefFormatable` fallback** handles two cases:
  1. Truly blank factory‑state tags (Ndef.get returns null).
  2. Edge cases where `Ndef.get(tag)` succeeds but `writeNdefMessage` throws or `isWritable == false` — some NTAG variants have OTP/lock bits that gate the data area but not the capability container, so `NdefFormatable.format` can rewrite both atomically and succeed where `Ndef` reported read‑only.

### 5c. NTAG password‑removal fallback — `tryRemoveNtagPassword` (`NfcService.kt:310`)

Some NTAG213/215/216 tags ship with factory password protection still enabled. They report `isWritable=true` via `Ndef` but the underlying `writeNdefMessage` throws IOException. This is the batch path's one‑shot recovery (standalone path never tries this — see why below):

1. `mu = MifareUltralight.get(tag)` — bail if not MU (returns `NotApplicable`).
2. `mu.connect()`.
3. `detectNtagVariant(mu)`: issue `GET_VERSION` (0x60), inspect the 7th response byte (`storage_size`):
   - `0x0F` → NTAG213 (CFG0 at page 0x29)
   - `0x11` → NTAG215 (CFG0 at page 0x83)
   - `0x13` → NTAG216 (CFG0 at page 0xE3)
   - Anything else → `NotApplicable`.
4. For each of the two factory‑default passwords `[0xFFFFFFFF]`, `[0x00000000]`:
   - Send `PWD_AUTH` (0x1B) + 4 password bytes via `mu.transceive`.
   - Success = response is exactly 2 bytes (PACK). Failure = IOException (Android surfaces NAK as `TagLostException`).
   - On success: `mu.readPages(cfg0Page)` returns 16 bytes (4 pages). Build a new CFG0: preserve bytes 0–2 (`MIRROR_CONF`, `MIRROR_BYTE`, `RFUI`), set byte 3 (`AUTH0`) to `0xFF`. `AUTH0 = 0xFF` means "first page requiring auth is beyond end‑of‑memory" → effectively password protection disabled. `mu.writePage(cfg0Page, newCfg0)`.
   - Return `Removed` → caller retries the NDEF write.
5. If both default passwords return NAK → `CustomPassword` → user must use a desktop NFC tool to clear it.
6. Any IOException at the MU layer → `NotApplicable`.
7. `mu.close()` always runs via `finally`.

The standalone path skips this because the user can read the conflict dialog, choose to cancel, and decide. The batch user has their hands full and needs the one‑tap recovery.

---

## 6. Tag state handling — every possible state

### 6a. Blank / unformatted tag

- **Read mode**: `readNdefPayload` returns `null`. `_lastScan` stores tagId with `payload=null`. `lookupBirdByNfc` looks up by hardware tagId (no `"BIRD-"` prefix). Server miss → silently no result (no orphan dialog, since there's no orphan birdId to surface).
- **Write mode (batch or standalone)**: `Ndef.get(tag)` returns null, so the formatted path skips. `NdefFormatable.get(tag)` returns non‑null. `format(message)` writes the capability container + message atomically in one round trip — no separate "format then write" second tap.

### 6b. Tag with existing NDEF data

The NDEF reader (`readNdefPayload`, `NfcService.kt:512`) prefers `TNF_WELL_KNOWN` or `TNF_MIME_MEDIA` records. For well‑known text records, it strips the 1‑byte language‑code length header per the NDEF text record spec.

Three subcases based on what the payload says:

- **`"BIRD-<id>"`** (the canonical QuailSync payload):
  - **Read**: lookup goes through `getBirdByNfcTag("BIRD-N")`. Hit → scan augmented with bird. Miss (deleted bird) → `OrphanTag` dialog.
  - **Standalone write**: `attemptWrite` parses out `existingBirdId`, returns `Conflict` if it differs from the pending write, else returns `Written` immediately (already correct).
  - **Batch write**: silently overwritten.

- **`"QS-L<lineageId>"`** (intermediate state — written during `AwaitingTagScan`, never gets `BIRD-` because the bird id isn't known yet):
  - **Read**: payload doesn't start with `"BIRD-"`, so lookup uses hardware tagId. No bird → no result; no orphan dialog.
  - **Standalone write**: not a `BIRD-` prefix → falls into the "non‑QuailSync data" branch in `attemptWrite` and is written immediately, no conflict prompt.
  - **Batch write**: silently overwritten.

- **Random / foreign data** (some other app wrote a URL, vCard, etc.):
  - **Read**: payload extracted as text, doesn't start with `"BIRD-"`, lookup by hardware tagId, server miss → silent.
  - **Standalone write**: same as `QS-L*` — no conflict prompt, immediate write.
  - **Batch write**: silently overwritten.

### 6c. Password‑protected tag

- **Standalone write**: `Ndef.get` succeeds, `connect()` succeeds, `isWritable` may report `true`, but `writeNdefMessage` throws IOException. Falls through to `NdefFormatable` path, which also fails on a password‑gated data area. `writeNdefText` returns false → `WriteAttemptResult.Failed("Failed to write to tag")` → toast. Write mode stays active so the user can retry or pick a different bird/tag.
- **Batch write**: same first failure, then `handleBatchIntent` checks `tag.techList.contains(MifareUltralight)` and runs `tryRemoveNtagPassword`:
  - Factory password (0xFFFFFFFF or 0x00000000) accepted → AUTH0 cleared → retry succeeds.
  - Custom password → user sees "This tag is password‑protected with a custom password. Remove the password using NFC Tools first." and is steered away.
  - MU connection drops mid‑recovery → `NotApplicable` → user sees the generic "tag can't be written to — try a different tag" message.

### 6d. Read‑only / locked tag

- `Ndef.get(tag)` returns non‑null. `connect()` works. The diagnostic log fires: `writable=false, maxSize=…, type=…, connected=true`. The path explicitly closes and returns false on `!ndef.isWritable` to fall through to the `NdefFormatable` path (some NTAG variants gate the data area via lock bits but not the capability container — the comment in `writeToFormattedNdef` calls this out specifically).
- `NdefFormatable.get(tag)` may return null on truly locked tags (no formatable channel) → both paths exhaust → returns false.
- For NTAGs with OTP/lock bits actually set (irreversible), even `format` will throw IOException → batch path proceeds to the MU password‑removal fallback, which will either complete a successful auth (no help — the lock bits still gate writes) or return `NotApplicable`, and either way the user sees "This tag can't be written to — try a different tag."

### 6e. Defective tag / tag leaves field mid‑write

Three subcases:

- **Tag leaves the field between scan and write** (rare on a single tap, but possible if the user pulls the phone away during the batch‑path retry). `connect()` throws IOException → write returns false → batch generic "can't be written to" message; standalone "Failed to write to tag" toast.
- **Tag leaves the field between conflict detection and `confirmOverwrite`** (standalone only). The cached `conflictTag` reference is stale → `writeNdefText` throws on `connect()` → `writeResult = "Tag lost — tap the tag again"`. Write mode stays active.
- **Tag with unsupported tech** (no `Ndef`, no `NdefFormatable`): `Ndef.get` returns null AND `NdefFormatable.get` returns null. Both paths log and return false. `writeNdefText` logs `Tag <id> is not writable by any method — tag may be locked or defective. techs=<...>` with the full tech list for forensics.

---

## 7. Cross‑cutting concerns

### 7a. NDEF payload format

QuailSync writes well‑known text records with language code `"en"` (`NdefRecord.createTextRecord("en", text)`). Reading mirrors this: strips the language‑code length header for `TNF_WELL_KNOWN` + `RTD_TEXT`, otherwise UTF‑8 decodes the raw payload bytes. This means a tag written by another app with `TNF_WELL_KNOWN`/`RTD_URI` would have its URI byte‑identifier (first byte) leak into the payload — not currently a bug, just a known limitation.

### 7b. API version handling

`getParcelableExtra` and `getParcelableArrayExtra` were deprecated on Android 13 (Tiramisu, API 33). `NfcService.handleIntent`, `handleBatchIntent`, and `readNdefPayload` all branch on `Build.VERSION.SDK_INT >= TIRAMISU` to use the typed overloads on new devices and `@Suppress("DEPRECATION")` legacy calls on older ones. `minSdk` is 26.

### 7c. Race conditions the code explicitly guards against

1. **Arm‑before‑transition** (`startBatchTagging`, `advanceFromConfirm`): always `enterWriteMode(...)` *then* update `_batchState`. Documented as the root cause of the bird‑2+ "Failed to write to tag" bug.
2. **Belt‑and‑suspenders re‑arm**: `BatchAwaitingScanScreen`'s `LaunchedEffect(state.currentIndex)` calls `armNfcForBatchScan` on every bird, logging a warning if write mode was unexpectedly off at entry — surfaces any path that cancels write mode between `PostTagConfirm` and `AwaitingTagScan`.
3. **Stale `conflictTag` after dialog**: the `Tag` object cached during a standalone conflict may be invalid by the time the user confirms; `confirmOverwrite` handles a null tag and an IOException‑during‑write with the same "Tag lost — tap again" message and keeps write mode armed.
4. **`Pair<NfcScanResult, WriteAttemptResult?>` return**: `handleIntent` returns both so `MainActivity` can run a Toast plus update the ViewModel in one go without race‑prone re‑reads of `_lastScan`.

### 7d. Why the batch path skips conflict detection (by design)

Documented in the comment block on `handleBatchIntent`: the user is holding a chick in one hand and a tag in the other. Pausing for "overwrite?" confirmation, looking up a previous bird, or surfacing a generic "Failed to write" toast all break the flow. Uniqueness across previously‑tagged birds is enforced server‑side via `clear_nfc_tag_from_others` on the create‑bird POST — when the next POST sets `nfc_tag_id` on the new bird, the server clears that tagId from any other bird that had it. The result: every tap "just works" from the user's perspective, and the DB stays consistent regardless of what was on the tag before.

---

## 8. End‑to‑end summary

1. **Foreground**: every tap arrives in `MainActivity.onNewIntent` via foreground dispatch (manifest filters are backup for cold launches).
2. **Route by batch state**:
   - In batch flow (`AwaitingTagScan` / `AwaitingTagWrite`) → permissive write via `handleBatchIntent` → advance state on success, retry‑on‑same‑bird on failure.
   - Otherwise → conflict‑aware `handleIntent` → either read‑and‑lookup or conflict‑gated write.
3. **Write primitive** (`writeNdefText`): try `Ndef` first, fall back to `NdefFormatable`. Returns true/false.
4. **Batch‑only recovery**: on first‑write failure for MifareUltralight tags, try `tryRemoveNtagPassword` with factory defaults, then retry. Custom‑password and unrecognized tags surface clear user‑facing messages.
5. **State machine** (batch): `Idle → Setup → AwaitingTagScan → PerBirdEntry → CreatingBird → PostTagConfirm → AwaitingTagScan or Complete`. Tag uniqueId captured during `AwaitingTagScan` rides through `PerBirdEntry` as `capturedTagId` and is stamped into the create‑bird POST body — a single API call, no follow‑up PUT.
6. **Standalone fallback**: `NfcMainScreen` handles single reads, single writes, conflict dialogs (`TagConflictDialog`), and orphan‑tag recovery (`OrphanTagDialog`) when batch flow is `Idle`.
