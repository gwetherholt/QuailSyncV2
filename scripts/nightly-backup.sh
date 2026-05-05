#!/bin/bash
#
# nightly-backup.sh — hot SQLite backup of the QuailSync database to SMB.
#
# Cron: 0 2 * * *  (run as gwetherholt)
#
# Uses sqlite3's .backup command (safe while the server writes), gzips
# the result to /mnt/pc-snapshots/quailsync-nightly/, verifies integrity,
# prunes files older than RETAIN_DAYS, logs every run, and posts to the
# QuailSync server's /api/alerts endpoint on failure (the bell icon in
# the Android app surfaces it). A successful run posts to
# /api/alerts/resolve so a recovered run auto-clears yesterday's failure.
#
# Usage:
#   ./nightly-backup.sh           # normal run
#   ./nightly-backup.sh --dry-run # walk through every step, skip writes/deletes
#

set -euo pipefail

# ---------- configuration (edit these) ----------------------------------------
DB_PATH="/home/gwetherholt/QuailSyncV2/data/quailsync.db"
BACKUP_DIR="/mnt/pc-snapshots/quailsync-nightly"
LOG_FILE="/var/log/quailsync-backup.log"
RETAIN_DAYS=7
MIN_BYTES=$((1 * 1024 * 1024))            # 1 MB sanity threshold
SERVER_URL="http://localhost:3000"        # script runs on the server itself
ALERT_KEY="backup_failed"
ALERT_SOURCE="nightly-backup"
ALERT_SEVERITY="critical"
# ------------------------------------------------------------------------------

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=1
fi

DATE_STAMP="$(date -u +%Y-%m-%d)"
DEST_GZ="${BACKUP_DIR}/quailsync-${DATE_STAMP}.db.gz"
TMP_DB=""

# Log format: ISO-8601-UTC <STATUS> <reason...>   (grep-friendly, single line)
log_event() {
    local status="$1"
    shift
    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '%s %s %s\n' "$ts" "$status" "$*" >> "$LOG_FILE" 2>/dev/null || true
    printf '%s %s %s\n' "$ts" "$status" "$*"
}

# Escape a string for safe inclusion as a JSON string value.
json_escape() {
    python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1" 2>/dev/null \
        || printf '"%s"' "${1//\"/\\\"}"
}

# POST a failure alert. Best-effort: never re-fails the script.
notify_failure() {
    local title="$1"
    local message="$2"
    local payload
    payload=$(printf '{"alert_key":%s,"severity":%s,"title":%s,"message":%s,"source":%s,"metadata_json":%s}' \
        "$(json_escape "$ALERT_KEY")" \
        "$(json_escape "$ALERT_SEVERITY")" \
        "$(json_escape "$title")" \
        "$(json_escape "$message")" \
        "$(json_escape "$ALERT_SOURCE")" \
        "$(json_escape "{\"host\":\"$(hostname)\",\"date\":\"${DATE_STAMP}\"}")"
    )
    if ! curl --max-time 10 -fsS -H 'Content-Type: application/json' \
              -X POST -d "$payload" "${SERVER_URL}/api/alerts" >/dev/null 2>&1; then
        log_event INFO "alert POST failed (server unreachable?), continuing"
    fi
}

# POST a resolve. Best-effort.
notify_resolve() {
    local payload
    payload=$(printf '{"alert_key":%s}' "$(json_escape "$ALERT_KEY")")
    if ! curl --max-time 10 -fsS -H 'Content-Type: application/json' \
              -X POST -d "$payload" "${SERVER_URL}/api/alerts/resolve" >/dev/null 2>&1; then
        log_event INFO "resolve POST failed (server unreachable?), continuing"
    fi
}

cleanup() {
    if [[ -n "$TMP_DB" && -f "$TMP_DB" ]]; then
        rm -f "$TMP_DB"
    fi
}
trap cleanup EXIT

on_error() {
    local line="$1"
    local msg="nightly-backup.sh failed at line ${line} (host=$(hostname) date=${DATE_STAMP})"
    log_event FAIL "$msg"
    notify_failure "Backup failed" "$msg"
}
trap 'on_error $LINENO' ERR

# ---------- preflight ---------------------------------------------------------
if [[ ! -r "$DB_PATH" ]]; then
    log_event FAIL "source DB not readable: $DB_PATH"
    notify_failure "Backup failed" "Source DB missing or unreadable: $DB_PATH"
    exit 1
fi

if (( DRY_RUN )); then
    log_event INFO "DRY RUN start: would write $DEST_GZ"
else
    mkdir -p "$BACKUP_DIR"
fi

# ---------- backup ------------------------------------------------------------
TMP_DB="$(mktemp --suffix=.db /tmp/quailsync-backup.XXXXXX)"

if (( DRY_RUN )); then
    log_event INFO "would run: sqlite3 $DB_PATH .backup $TMP_DB"
    : > "$TMP_DB"
else
    sqlite3 "$DB_PATH" ".backup '${TMP_DB}'"
fi

# ---------- compress ----------------------------------------------------------
if (( DRY_RUN )); then
    log_event INFO "would gzip $TMP_DB -> $DEST_GZ"
else
    gzip -c "$TMP_DB" > "$DEST_GZ"
fi

# ---------- verify ------------------------------------------------------------
if (( DRY_RUN )); then
    log_event INFO "would verify file existence, size>${MIN_BYTES}B, gunzip -t"
else
    if [[ ! -f "$DEST_GZ" ]]; then
        log_event FAIL "verify: destination missing $DEST_GZ"
        notify_failure "Backup failed" "Backup file missing after write: $DEST_GZ"
        exit 1
    fi

    actual_bytes="$(stat -c%s "$DEST_GZ")"
    if (( actual_bytes < MIN_BYTES )); then
        log_event FAIL "verify: backup too small (${actual_bytes}B < ${MIN_BYTES}B) at $DEST_GZ"
        notify_failure "Backup failed" "Backup suspiciously small: ${actual_bytes} bytes at $DEST_GZ"
        exit 1
    fi

    if ! gunzip -t "$DEST_GZ" >/dev/null 2>&1; then
        log_event FAIL "verify: gunzip -t failed for $DEST_GZ"
        notify_failure "Backup failed" "Backup gzip integrity check failed: $DEST_GZ"
        exit 1
    fi
fi

# ---------- prune -------------------------------------------------------------
if (( DRY_RUN )); then
    log_event INFO "would prune files older than ${RETAIN_DAYS} days in $BACKUP_DIR"
    find "$BACKUP_DIR" -maxdepth 1 -type f -name 'quailsync-*.db.gz' -mtime "+${RETAIN_DAYS}" -print 2>/dev/null || true
else
    find "$BACKUP_DIR" -maxdepth 1 -type f -name 'quailsync-*.db.gz' -mtime "+${RETAIN_DAYS}" -delete
fi

# ---------- success -----------------------------------------------------------
if (( DRY_RUN )); then
    log_event OK "dry-run complete (no files written or deleted, no API call)"
else
    size_human="$(du -h "$DEST_GZ" | awk '{print $1}')"
    log_event OK "backup_size=${size_human} location=${DEST_GZ}"
    notify_resolve
fi

exit 0
