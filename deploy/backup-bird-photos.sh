#!/usr/bin/env bash
#
# QuailSync — overnight bird-photo backup (copy → verify → only-then-delete)
# =============================================================================
# Archives the Pi's bird photos to the PC share, and deletes the Pi-side copies
# ONLY after the archived copy is proven byte-identical on the share.
#
# Guiding rule: on ANY error or uncertainty, KEEP the Pi copy. Leaving stale
# photos on the Pi is harmless; deleting un-backed-up photos is catastrophic.
# Every branch below is biased toward NOT deleting.
#
# Delete is NOT performed here — it lives in prune-archived-photos.sh, which is
# only invoked after verification passes and which independently re-verifies
# the destination checksum before removing anything. See that script.
#
# Source path note: the Rust server's photo-upload handler writes JPEGs to
# `bird_photos/` under its `/data` workdir (host side of the ./data:/data
# docker volume), using history-keeping timestamped names like
# `bird_{id}_{YYYYMMDD-HHMMSS}.jpg`. PHOTO_SRC_DIR below points at that dir;
# the glob picks up the timestamped names unchanged. Adjust if your deployment
# differs. A missing/empty source is treated as "nothing to do" — no delete,
# no alert.
#
# Install: see quailsync-photo-backup.service / .timer in this directory.
# =============================================================================

set -uo pipefail   # NOT -e: each step is checked explicitly so we can emit a
                   # specific alert and exit without deleting.

# --- Configuration (edit these) ---------------------------------------------

# ntfy push target for failure alerts. Content transits ntfy's public server,
# so keep messages generic (this script never includes file contents/secrets).
NTFY_SERVER="${NTFY_SERVER:-https://ntfy.sh}"
NTFY_TOPIC="${NTFY_TOPIC:-quailsync-REPLACE-ME}"        # <-- set your topic

# Source: the Pi-side photo directory (host side of the ./data:/data volume).
PHOTO_SRC_DIR="${PHOTO_SRC_DIR:-/home/gwetherholt/quailsync/data/bird_photos}"

# Destination: the PC share. SHARE_MOUNT is the mount root used to detect an
# unreachable/unmounted share; DEST_DIR is where archives are written under it.
SHARE_MOUNT="${SHARE_MOUNT:-/mnt/pcshare}"
DEST_DIR="${DEST_DIR:-/mnt/pcshare/quailsync-photo-backups}"

# Local scratch space on the Pi for building the zip before it's copied.
WORK_DIR="${WORK_DIR:-/var/tmp/quailsync-photo-backup}"

# Run log (best-effort; also goes to journald when run via systemd).
LOG_DIR="${LOG_DIR:-/home/gwetherholt/quailsync/logs}"
LOGFILE="${LOGFILE:-$LOG_DIR/photo-backup.log}"

# Lock so two overnight runs never overlap.
LOCKFILE="${LOCKFILE:-/var/tmp/quailsync-photo-backup.lock}"

# The isolated delete step, resolved next to this script.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PRUNE_SCRIPT="${PRUNE_SCRIPT:-$SCRIPT_DIR/prune-archived-photos.sh}"

# --- Derived values ----------------------------------------------------------

STAMP="$(date +%Y-%m-%d)"
HOST="$(hostname 2>/dev/null || echo pi)"
SRC_ZIP="$WORK_DIR/bird_photos_${STAMP}.zip"
DEST_ZIP="$DEST_DIR/bird_photos_${STAMP}.zip"
MANIFEST="$WORK_DIR/manifest_${STAMP}.txt"

# --- Logging / alerting ------------------------------------------------------

mkdir -p "$LOG_DIR" 2>/dev/null || true

log() {
    # Timestamped line to stdout (journald) and the logfile.
    local line
    line="$(date '+%Y-%m-%d %H:%M:%S') [photo-backup] $*"
    echo "$line"
    echo "$line" >>"$LOGFILE" 2>/dev/null || true
}

notify() {
    # Send a failure push. If the push itself fails (e.g. no network), record
    # that locally so there's still a trace on the Pi.
    local msg="$1"
    if ! curl -fsS -m 15 \
            -H "Title: QuailSync photo backup FAILED ($HOST)" \
            -H "Priority: high" \
            -H "Tags: rotating_light" \
            -d "$msg" \
            "$NTFY_SERVER/$NTFY_TOPIC" >/dev/null 2>&1; then
        log "NTFY SEND FAILED — alert could not be delivered. Message was: $msg"
    fi
}

# fail <step> <message>: log, alert (step-specific), and exit non-zero WITHOUT
# deleting anything. This is the single abort path for every failure branch.
fail() {
    local step="$1" msg="$2"
    log "FAILURE at step '$step': $msg — Pi photos NOT deleted."
    notify "Step '$step' failed: $msg. Pi photos were NOT deleted; they remain safe on the Pi."
    exit 1
}

# --- Single-instance lock (re-exec under flock) ------------------------------
# Re-run ourselves wrapped in flock. `-E 0` makes a *contended* run exit 0
# cleanly without doing anything (a daily job almost never overlaps). A genuine
# flock error still surfaces non-zero, so a broken lock can never silently
# disable the backup forever. Hosts without flock simply proceed unlocked.
if [[ "${QS_PHOTO_BACKUP_LOCKED:-}" != "1" ]] && command -v flock >/dev/null 2>&1; then
    exec env QS_PHOTO_BACKUP_LOCKED=1 flock -n -E 0 "$LOCKFILE" "$0" "$@"
fi

log "===== Run start (stamp $STAMP) ====="

# --- Step 0: source present? (empty/absent == nothing to do, no alert) -------

if [[ ! -d "$PHOTO_SRC_DIR" ]]; then
    log "Source dir '$PHOTO_SRC_DIR' does not exist — nothing to back up. Exiting 0."
    exit 0
fi

mkdir -p "$WORK_DIR" || fail "workspace" "could not create work dir '$WORK_DIR'"

# Snapshot the exact set of files to archive. Capturing the list up front means
# photos that arrive DURING the run are neither archived nor deleted this time —
# they're simply picked up on the next run (never deleted without a backup).
if ! ( cd "$PHOTO_SRC_DIR" && find . -type f -printf '%P\n' ) >"$MANIFEST" 2>>"$LOGFILE"; then
    fail "scan-source" "could not enumerate files under '$PHOTO_SRC_DIR'"
fi

if [[ ! -s "$MANIFEST" ]]; then
    log "Source dir is empty — nothing to back up. Exiting 0."
    rm -f "$MANIFEST"
    exit 0
fi
FILE_COUNT="$(wc -l <"$MANIFEST" | tr -d ' ')"
log "Found $FILE_COUNT file(s) to archive."

# --- Step: share reachable? (check BEFORE we bother zipping) ------------------
# A down CIFS/NFS mount can leave a writable empty dir behind, which would let a
# "successful" copy vanish. Require the mount to actually be mounted.

if command -v mountpoint >/dev/null 2>&1; then
    if ! mountpoint -q "$SHARE_MOUNT"; then
        fail "share-unreachable" "share mount '$SHARE_MOUNT' is not mounted"
    fi
fi
if ! mkdir -p "$DEST_DIR" 2>>"$LOGFILE"; then
    fail "share-unreachable" "cannot create/access destination '$DEST_DIR' on the share"
fi
# Prove the share is actually writable right now.
if ! ( : >"$DEST_DIR/.write_test_$$" ) 2>>"$LOGFILE"; then
    fail "share-unreachable" "destination '$DEST_DIR' is not writable"
fi
rm -f "$DEST_DIR/.write_test_$$" 2>/dev/null || true

# --- Step 1: zip the snapshot ------------------------------------------------

rm -f "$SRC_ZIP" 2>/dev/null || true
if ! ( cd "$PHOTO_SRC_DIR" && zip -q -@ "$SRC_ZIP" <"$MANIFEST" ) 2>>"$LOGFILE"; then
    fail "zip" "zip command returned an error while archiving the photos"
fi
if [[ ! -s "$SRC_ZIP" ]]; then
    fail "zip" "archive '$SRC_ZIP' is missing or zero-byte after zip"
fi
log "Step 1 OK: created local archive $SRC_ZIP"

# --- Step 2: checksum the source zip ----------------------------------------

SRC_SHA="$(sha256sum "$SRC_ZIP" 2>>"$LOGFILE" | awk '{print $1}')"
if [[ -z "$SRC_SHA" ]]; then
    fail "checksum-source" "could not compute SHA-256 of the local archive"
fi
log "Step 2 OK: source SHA-256 = $SRC_SHA"

# --- Step 3: copy to the share -----------------------------------------------

if ! cp -f "$SRC_ZIP" "$DEST_ZIP" 2>>"$LOGFILE"; then
    fail "copy-to-share" "copying the archive to '$DEST_ZIP' failed"
fi
# Flush to the share's backing store before we trust it.
sync 2>/dev/null || true
log "Step 3 OK: copied archive to $DEST_ZIP"

# --- Step 4: verify destination exists and is non-zero, then checksum --------

if [[ ! -e "$DEST_ZIP" ]]; then
    fail "dest-missing" "destination archive '$DEST_ZIP' is missing after copy"
fi
if [[ ! -s "$DEST_ZIP" ]]; then
    fail "dest-zero-byte" "destination archive '$DEST_ZIP' is zero-byte"
fi
DEST_SHA="$(sha256sum "$DEST_ZIP" 2>>"$LOGFILE" | awk '{print $1}')"
if [[ -z "$DEST_SHA" ]]; then
    fail "checksum-dest" "could not compute SHA-256 of the destination archive"
fi
log "Step 4 OK: destination SHA-256 = $DEST_SHA"

# --- Step 5: compare ---------------------------------------------------------

if [[ "$SRC_SHA" != "$DEST_SHA" ]]; then
    fail "checksum-mismatch" "copy corrupted — source and destination SHA-256 differ"
fi
log "Step 5 OK: checksums match — destination copy is provably intact."

# --- Step 6: delete (isolated + independently re-verified) --------------------
# Delegated to the prune script. It re-checks the destination archive before
# removing a single file; if its re-verification fails it deletes NOTHING and
# exits non-zero, and we alert here. Delete is unreachable unless we got here.

log "Verification passed — handing off to delete step: $PRUNE_SCRIPT"
if ! LOGFILE="$LOGFILE" "$PRUNE_SCRIPT" "$PHOTO_SRC_DIR" "$DEST_ZIP" "$SRC_SHA" "$MANIFEST"; then
    rc=$?
    fail "delete" "prune step refused/failed (exit $rc) — destination copy is safe, Pi copy left intact"
fi

# --- Cleanup -----------------------------------------------------------------
# Remove the local working zip (the whole point is to keep the Pi lean); the
# verified archive stays on the share. Keep the manifest for the log trail.
rm -f "$SRC_ZIP" 2>/dev/null || true

log "===== Run complete: archived $FILE_COUNT file(s) to $DEST_ZIP and pruned Pi copies ====="
exit 0
