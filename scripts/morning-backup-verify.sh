#!/bin/bash
#
# morning-backup-verify.sh — deadman switch for nightly-backup.sh.
#
# Cron: 0 8 * * *  (run as gwetherholt)
#
# Confirms that:
#   1. A file named quailsync-<today-utc>.db.gz exists in BACKUP_DIR.
#   2. The most recent quailsync-*.db.gz was created within the last
#      MAX_AGE_HOURS hours.
#
# On failure: POST a system alert to the QuailSync server (surfaced
# via the Android app's bell icon). On success: POST a resolve so any
# previous failure auto-clears.
#

set -euo pipefail

# ---------- configuration -----------------------------------------------------
BACKUP_DIR="/mnt/pc-snapshots/quailsync-nightly"
LOG_FILE="/var/log/quailsync-backup.log"
MAX_AGE_HOURS=12
SERVER_URL="http://localhost:3000"
ALERT_KEY="deadman_no_recent_backup"
ALERT_SOURCE="morning-verify"
ALERT_SEVERITY="critical"
# ------------------------------------------------------------------------------

DATE_STAMP="$(date -u +%Y-%m-%d)"
EXPECTED="${BACKUP_DIR}/quailsync-${DATE_STAMP}.db.gz"

log_event() {
    local status="$1"
    shift
    local ts
    ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf '%s %s %s\n' "$ts" "$status" "$*" >> "$LOG_FILE" 2>/dev/null || true
    printf '%s %s %s\n' "$ts" "$status" "$*"
}

json_escape() {
    python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1" 2>/dev/null \
        || printf '"%s"' "${1//\"/\\\"}"
}

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

notify_resolve() {
    local payload
    payload=$(printf '{"alert_key":%s}' "$(json_escape "$ALERT_KEY")")
    if ! curl --max-time 10 -fsS -H 'Content-Type: application/json' \
              -X POST -d "$payload" "${SERVER_URL}/api/alerts/resolve" >/dev/null 2>&1; then
        log_event INFO "resolve POST failed (server unreachable?), continuing"
    fi
}

# ---------- check 1: today's file exists --------------------------------------
if [[ ! -f "$EXPECTED" ]]; then
    msg="Backup verification failed: expected ${EXPECTED} not found"
    log_event FAIL "$msg"
    notify_failure "Backup verification failed" "$msg"
    exit 1
fi

# ---------- check 2: most recent backup younger than MAX_AGE_HOURS ------------
WINDOW_MIN=$(( MAX_AGE_HOURS * 60 ))
RECENT="$(find "$BACKUP_DIR" -maxdepth 1 -type f -name 'quailsync-*.db.gz' -mmin "-${WINDOW_MIN}" -print -quit 2>/dev/null || true)"

if [[ -z "$RECENT" ]]; then
    age_h=""
    if [[ -f "$EXPECTED" ]]; then
        mtime_epoch="$(stat -c%Y "$EXPECTED" 2>/dev/null || echo 0)"
        now_epoch="$(date -u +%s)"
        age_h=$(( (now_epoch - mtime_epoch) / 3600 ))
    fi
    msg="Backup verification failed: no backup within last ${MAX_AGE_HOURS}h (today's file age=${age_h:-?}h) in ${BACKUP_DIR}"
    log_event FAIL "$msg"
    notify_failure "Backup verification failed" "$msg"
    exit 1
fi

# ---------- success -----------------------------------------------------------
size_human="$(du -h "$EXPECTED" | awk '{print $1}')"
log_event OK "verify backup_size=${size_human} location=${EXPECTED}"
notify_resolve
exit 0
