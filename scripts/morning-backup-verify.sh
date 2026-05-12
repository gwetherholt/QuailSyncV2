#!/usr/bin/env bash
# morning-backup-verify.sh — Verify last night's QuailSync backup landed on Windows
# Cron: 0 7 * * * /home/gwetherholt/QuailSyncV2/scripts/morning-backup-verify.sh
#
# SSHes into Ironman and confirms today's (or yesterday's, if run before midnight)
# backup file exists and is non-empty.

set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────
LOGFILE="/var/log/quailsync-backup.log"
ALERT_URL="http://localhost:3000/api/alerts"

SSH_USER="Georgia"
SSH_HOST="192.168.0.228"
SSH_OPTS="-o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new"
REMOTE_DIR="/cygdrive/c/QuailSyncSnapshots/quailsync-nightly"
# NOTE: Adjust REMOTE_DIR path format to match nightly-backup.sh
# (see the note there about Cygwin vs Windows OpenSSH paths)

LOCAL_BACKUP_DIR="$HOME/quailsync-local-backups"

# ─── Helpers ──────────────────────────────────────────────────────────
DATE=$(date +%Y-%m-%d)
TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
EXPECTED_FILE="quailsync-${DATE}.db.gz"

log() {
    echo "${TIMESTAMP} [VERIFY] $1" | tee -a "$LOGFILE"
}

alert() {
    local level="$1"
    local message="$2"
    local alert_key="$3"
    curl -sf -X POST "$ALERT_URL" \
        -H "Content-Type: application/json" \
        -d "{\"level\":\"${level}\",\"message\":\"${message}\",\"alert_key\":\"${alert_key}\"}" \
        >> "$LOGFILE" 2>&1 || true
}

ERRORS=0

# ─── Check 1: SSH reachability ────────────────────────────────────────
log "Verifying SSH connectivity to ${SSH_HOST}..."
if ! ssh $SSH_OPTS "${SSH_USER}@${SSH_HOST}" "echo ok" > /dev/null 2>&1; then
    log "ERROR: Cannot reach ${SSH_HOST} via SSH"
    alert "error" "Backup verify failed: SSH to Ironman unreachable" "verify_ssh_unreachable"
    exit 1
fi

# ─── Check 2: Remote file exists and is non-empty ─────────────────────
log "Checking remote file: ${REMOTE_DIR}/${EXPECTED_FILE}"
REMOTE_SIZE=$(ssh $SSH_OPTS "${SSH_USER}@${SSH_HOST}" \
    "powershell -Command \"if (Test-Path 'C:\\QuailSyncSnapshots\\quailsync-nightly\\${EXPECTED_FILE}') { (Get-Item 'C:\\QuailSyncSnapshots\\quailsync-nightly\\${EXPECTED_FILE}').Length } else { Write-Output 0 }\"" \
    2>/dev/null | tr -d '\r')

if [[ "$REMOTE_SIZE" -eq 0 ]]; then
    log "ERROR: Remote backup missing or empty: ${EXPECTED_FILE}"
    alert "error" "Backup verify failed: ${EXPECTED_FILE} missing on Ironman" "verify_remote_missing"
    ERRORS=$((ERRORS + 1))
else
    log "Remote backup confirmed: ${EXPECTED_FILE} (${REMOTE_SIZE} bytes)"
fi

# ─── Check 3: Local copy exists ───────────────────────────────────────
LOCAL_FILE="${LOCAL_BACKUP_DIR}/${EXPECTED_FILE}"
if [[ -s "$LOCAL_FILE" ]]; then
    LOCAL_SIZE=$(stat -c%s "$LOCAL_FILE" 2>/dev/null || stat -f%z "$LOCAL_FILE")
    log "Local backup confirmed: ${LOCAL_FILE} (${LOCAL_SIZE} bytes)"
else
    log "WARNING: Local backup missing or empty: ${LOCAL_FILE}"
    alert "warning" "Local backup copy missing: ${EXPECTED_FILE}" "verify_local_missing"
    ERRORS=$((ERRORS + 1))
fi

# ─── Check 4: Size sanity (remote vs local should match) ─────────────
if [[ "$REMOTE_SIZE" -gt 0 ]] && [[ -s "$LOCAL_FILE" ]]; then
    if [[ "$REMOTE_SIZE" -ne "$LOCAL_SIZE" ]]; then
        log "WARNING: Size mismatch — remote=${REMOTE_SIZE} local=${LOCAL_SIZE}"
        alert "warning" "Backup size mismatch: remote=${REMOTE_SIZE} vs local=${LOCAL_SIZE}" "verify_size_mismatch"
        ERRORS=$((ERRORS + 1))
    else
        log "Size match confirmed: ${REMOTE_SIZE} bytes"
    fi
fi

# ─── Summary ──────────────────────────────────────────────────────────
if [[ "$ERRORS" -eq 0 ]]; then
    log "Morning verification passed."
    alert "info" "Backup verification passed: ${EXPECTED_FILE}" "verify_success"
else
    log "Morning verification completed with ${ERRORS} issue(s)."
fi

exit "$ERRORS"
