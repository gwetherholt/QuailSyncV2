#!/usr/bin/env bash
# nightly-backup.sh — QuailSync nightly SQLite backup via rsync-over-SSH
# Cron: 0 2 * * * /home/gwetherholt/QuailSyncV2/scripts/nightly-backup.sh
#
# Replaces the previous SMB/CIFS-based backup. SSH key auth eliminates
# Windows password-rotation breakage.

set -euo pipefail

# ─── Configuration ────────────────────────────────────────────────────
QUAILSYNC_DIR="$HOME/QuailSyncV2"
DB_PATH="$QUAILSYNC_DIR/data/quailsync.db"
LOGFILE="/var/log/quailsync-backup.log"
ALERT_URL="http://localhost:3000/api/alerts"

# SSH target (Windows machine "Ironman")
SSH_USER="Georgia"
SSH_HOST="192.168.0.228"
SSH_OPTS="-o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new"
REMOTE_DIR="/cygdrive/c/QuailSyncSnapshots/quailsync-nightly"
# NOTE: If using Windows OpenSSH (not Cygwin), the path format is:
# REMOTE_DIR="C:/QuailSyncSnapshots/quailsync-nightly"
# Test with: ssh Georgia@192.168.0.228 "ls C:/QuailSyncSnapshots/"
# and adjust REMOTE_DIR to whichever format works.

# Local backup (cheap insurance against SSH/network failures)
LOCAL_BACKUP_DIR="$HOME/quailsync-local-backups"
LOCAL_RETENTION_DAYS=3

# Remote retention
REMOTE_RETENTION_DAYS=7

# ─── Helpers ──────────────────────────────────────────────────────────
DATE=$(date +%Y-%m-%d)
TIMESTAMP=$(date '+%Y-%m-%d %H:%M:%S')
BACKUP_FILENAME="quailsync-${DATE}.db.gz"

log() {
    echo "${TIMESTAMP} [BACKUP] $1" | tee -a "$LOGFILE"
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

cleanup_local() {
    log "Pruning local backups older than ${LOCAL_RETENTION_DAYS} days..."
    find "$LOCAL_BACKUP_DIR" -name "quailsync-*.db.gz" -mtime +${LOCAL_RETENTION_DAYS} -delete 2>/dev/null || true
}

cleanup_remote() {
    log "Pruning remote backups older than ${REMOTE_RETENTION_DAYS} days..."
    # Run prune via PowerShell on Windows OpenSSH — `find -mtime -delete`
    # doesn't exist on the Windows side (no GNU coreutils, no Cygwin in this
    # path). `$_` is escaped so bash doesn't expand it locally; the inner
    # double quotes around the -Command argument are also escaped.
    ssh $SSH_OPTS "${SSH_USER}@${SSH_HOST}" "powershell -Command \"Get-ChildItem 'C:\\QuailSyncSnapshots\\quailsync-nightly\\quailsync-*.db.gz' | Where-Object { \$_.LastWriteTime -lt (Get-Date).AddDays(-${REMOTE_RETENTION_DAYS}) } | Remove-Item -Force\"" 2>/dev/null || {
        log "WARNING: Remote cleanup failed (non-critical)"
    }
}

# ─── Pre-flight checks ───────────────────────────────────────────────
if [[ ! -f "$DB_PATH" ]]; then
    log "ERROR: Database not found at ${DB_PATH}"
    alert "error" "Nightly backup failed: database file missing" "backup_db_missing"
    exit 1
fi

mkdir -p "$LOCAL_BACKUP_DIR"

# SSH reachability check (replaces old `mountpoint -q` SMB check)
log "Checking SSH connectivity to ${SSH_HOST}..."
if ! ssh $SSH_OPTS "${SSH_USER}@${SSH_HOST}" "echo ok" > /dev/null 2>&1; then
    log "ERROR: Cannot reach ${SSH_HOST} via SSH"
    alert "error" "Nightly backup failed: SSH connection to Ironman refused" "backup_ssh_unreachable"
    exit 1
fi
log "SSH connectivity confirmed."

# ─── Step 1: SQLite safe backup ──────────────────────────────────────
STAGING_DIR=$(mktemp -d)
STAGING_DB="${STAGING_DIR}/quailsync-${DATE}.db"
STAGING_GZ="${STAGING_DIR}/${BACKUP_FILENAME}"

log "Starting SQLite backup..."
sqlite3 "$DB_PATH" ".backup '${STAGING_DB}'"

if [[ ! -s "$STAGING_DB" ]]; then
    log "ERROR: SQLite .backup produced empty file"
    alert "error" "Nightly backup failed: sqlite3 .backup produced empty output" "backup_sqlite_empty"
    rm -rf "$STAGING_DIR"
    exit 1
fi

gzip -9 "$STAGING_DB"
FILESIZE=$(stat -c%s "$STAGING_GZ" 2>/dev/null || stat -f%z "$STAGING_GZ")
log "Backup compressed: ${BACKUP_FILENAME} (${FILESIZE} bytes)"

# ─── Step 2: Local copy ──────────────────────────────────────────────
cp "$STAGING_GZ" "$LOCAL_BACKUP_DIR/"
log "Local copy saved to ${LOCAL_BACKUP_DIR}/${BACKUP_FILENAME}"

# ─── Step 3: rsync to Windows via SSH ────────────────────────────────
log "Syncing to ${SSH_HOST}:${REMOTE_DIR}/ ..."
if rsync -avz -e "ssh ${SSH_OPTS}" "$STAGING_GZ" "${SSH_USER}@${SSH_HOST}:${REMOTE_DIR}/"; then
    log "Remote sync complete."
    alert "info" "Nightly backup succeeded: ${BACKUP_FILENAME} (${FILESIZE} bytes)" "backup_success"
else
    log "ERROR: rsync to ${SSH_HOST} failed (exit code $?)"
    alert "error" "Nightly backup failed: rsync to Ironman failed" "backup_rsync_failed"
    rm -rf "$STAGING_DIR"
    exit 1
fi

# ─── Step 4: Retention cleanup ────────────────────────────────────────
cleanup_local
cleanup_remote

# ─── Cleanup staging ─────────────────────────────────────────────────
rm -rf "$STAGING_DIR"
log "Nightly backup complete."
