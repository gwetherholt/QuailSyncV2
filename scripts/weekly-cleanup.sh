#!/bin/bash
#
# weekly-cleanup.sh — keep the Pi from filling up.
#
# Cron: 0 3 * * 0  (3 AM Sunday, run as gwetherholt)
#
# Steps:
#   1. Truncate Docker container json logs > MAX_DOCKER_LOG_BYTES (in place).
#   2. Delete app-generated backups in APP_BACKUP_DIR older than RETAIN_DAYS.
#   3. docker image prune -f.
#   4. sudo journalctl --vacuum-time=14d.
#   5. sudo apt clean.
#
# Posts a system alert to the QuailSync server on failure (visible via
# the Android app's bell icon). A successful run posts a resolve so any
# prior cleanup_failed alert auto-clears.
#
# Requires passwordless sudo for journalctl/apt — see sudoers.d snippet
# installed by install-cron.sh.
#

set -euo pipefail

# ---------- configuration -----------------------------------------------------
APP_BACKUP_DIR="${HOME}/QuailSyncV2/backups"
DOCKER_LOG_GLOB="/var/lib/docker/containers/*/*-json.log"
MAX_DOCKER_LOG_BYTES=$((100 * 1024 * 1024))   # 100 MB
RETAIN_DAYS=7
LOG_FILE="/var/log/quailsync-cleanup.log"
SERVER_URL="http://localhost:3000"
ALERT_KEY="cleanup_failed"
ALERT_SOURCE="cleanup"
ALERT_SEVERITY="warning"
# ------------------------------------------------------------------------------

OVERALL_FAILED=0

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
        "$(json_escape "{\"host\":\"$(hostname)\"}")"
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

on_error() {
    local line="$1"
    local msg="weekly-cleanup.sh failed at line ${line} on $(hostname)"
    log_event FAIL "$msg"
    notify_failure "Weekly cleanup failed" "$msg"
}
trap 'on_error $LINENO' ERR

# ---------- 1. truncate large Docker logs -------------------------------------
truncated_count=0
while IFS= read -r f; do
    [[ -z "$f" ]] && continue
    sudo truncate -s 0 "$f"
    truncated_count=$((truncated_count + 1))
done < <(sudo find /var/lib/docker/containers -maxdepth 2 -type f -name '*-json.log' -size +"${MAX_DOCKER_LOG_BYTES}c" 2>/dev/null || true)
log_event INFO "truncated ${truncated_count} Docker log file(s) > $((MAX_DOCKER_LOG_BYTES / 1024 / 1024))MB"

# ---------- 2. prune old app-generated backups --------------------------------
deleted_count=0
if [[ -d "$APP_BACKUP_DIR" ]]; then
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        rm -f "$f"
        deleted_count=$((deleted_count + 1))
    done < <(find "$APP_BACKUP_DIR" -maxdepth 1 -type f -mtime "+${RETAIN_DAYS}" -print 2>/dev/null || true)
    log_event INFO "deleted ${deleted_count} app backup(s) older than ${RETAIN_DAYS}d in ${APP_BACKUP_DIR}"
else
    log_event INFO "app backup dir not present, skipping: ${APP_BACKUP_DIR}"
fi

# ---------- 3. docker image prune --------------------------------------------
if command -v docker >/dev/null 2>&1; then
    if docker image prune -f >/dev/null 2>&1; then
        log_event INFO "docker image prune -f complete"
    else
        log_event INFO "docker image prune -f returned non-zero, continuing"
    fi
else
    log_event INFO "docker binary not found, skipping image prune"
fi

# ---------- 4. journalctl vacuum ---------------------------------------------
if sudo journalctl --vacuum-time=14d >/dev/null 2>&1; then
    log_event INFO "journalctl --vacuum-time=14d complete"
else
    log_event FAIL "journalctl --vacuum-time=14d failed"
    notify_failure "Weekly cleanup partial failure" "journalctl --vacuum-time=14d failed on $(hostname)"
    OVERALL_FAILED=1
fi

# ---------- 5. apt clean -----------------------------------------------------
if sudo apt clean >/dev/null 2>&1; then
    log_event INFO "apt clean complete"
else
    log_event FAIL "apt clean failed"
    notify_failure "Weekly cleanup partial failure" "apt clean failed on $(hostname)"
    OVERALL_FAILED=1
fi

if (( OVERALL_FAILED == 0 )); then
    log_event OK "weekly-cleanup finished truncated=${truncated_count} deleted=${deleted_count}"
    notify_resolve
else
    log_event FAIL "weekly-cleanup finished with errors truncated=${truncated_count} deleted=${deleted_count}"
fi

exit $OVERALL_FAILED
