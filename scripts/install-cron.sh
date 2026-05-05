#!/bin/bash
#
# install-cron.sh — idempotent installer for QuailSync backup/cleanup cron jobs.
#
# Run as the gwetherholt user on the Pi. Will use sudo for steps that
# need root (creating /var/log files with correct ownership, installing
# the sudoers.d snippet for weekly-cleanup).
#
# Re-running this script is safe: existing QSYNC-managed crontab lines
# are removed before fresh ones are added (matched by the trailing
# "# QSYNC-MANAGED" marker), and the sudoers fragment is overwritten.
#

set -euo pipefail

# ---------- configuration -----------------------------------------------------
TARGET_USER="gwetherholt"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
NIGHTLY="${SCRIPT_DIR}/nightly-backup.sh"
VERIFY="${SCRIPT_DIR}/morning-backup-verify.sh"
WEEKLY="${SCRIPT_DIR}/weekly-cleanup.sh"

BACKUP_LOG="/var/log/quailsync-backup.log"
CLEANUP_LOG="/var/log/quailsync-cleanup.log"

SUDOERS_FILE="/etc/sudoers.d/quailsync-cleanup"
CRON_MARKER="# QSYNC-MANAGED"
# ------------------------------------------------------------------------------

current_user="$(id -un)"
if [[ "$current_user" != "$TARGET_USER" ]]; then
    echo "ERROR: run this as the ${TARGET_USER} user (you are ${current_user})." >&2
    echo "  sudo -iu ${TARGET_USER} $0" >&2
    exit 2
fi

for f in "$NIGHTLY" "$VERIFY" "$WEEKLY"; do
    if [[ ! -f "$f" ]]; then
        echo "ERROR: missing script: $f" >&2
        exit 1
    fi
done

# ---------- 1. ensure scripts are executable ----------------------------------
chmod +x "$NIGHTLY" "$VERIFY" "$WEEKLY"
echo "[ok] chmod +x on the three scripts"

# ---------- 2. create log files with correct ownership ------------------------
echo "[..] creating log files (sudo required)"
sudo touch "$BACKUP_LOG" "$CLEANUP_LOG"
sudo chown "${TARGET_USER}:${TARGET_USER}" "$BACKUP_LOG" "$CLEANUP_LOG"
sudo chmod 644 "$BACKUP_LOG" "$CLEANUP_LOG"
echo "[ok] $BACKUP_LOG and $CLEANUP_LOG owned by ${TARGET_USER}"

# ---------- 3. install sudoers.d snippet for weekly-cleanup -------------------
echo "[..] installing sudoers fragment at ${SUDOERS_FILE}"
SUDOERS_TMP="$(mktemp)"
cat > "$SUDOERS_TMP" <<EOF
# Installed by QuailSync install-cron.sh — allows weekly-cleanup.sh
# to vacuum the systemd journal, clean the apt cache, and read/truncate
# Docker container logs without a password prompt.
${TARGET_USER} ALL=(root) NOPASSWD: /usr/bin/journalctl, /usr/bin/apt, /usr/bin/apt-get, /usr/bin/find /var/lib/docker/containers *, /usr/bin/truncate -s 0 /var/lib/docker/containers/*
EOF

# visudo -c validates the file before we move it into sudoers.d
if ! sudo visudo -c -f "$SUDOERS_TMP" >/dev/null; then
    echo "ERROR: generated sudoers fragment failed visudo validation" >&2
    rm -f "$SUDOERS_TMP"
    exit 1
fi
sudo install -m 0440 -o root -g root "$SUDOERS_TMP" "$SUDOERS_FILE"
rm -f "$SUDOERS_TMP"
echo "[ok] sudoers fragment installed"

# ---------- 4. install/refresh crontab for ${TARGET_USER} ---------------------
echo "[..] updating crontab for ${TARGET_USER}"
TMPCRON="$(mktemp)"
crontab -l 2>/dev/null | grep -vF "$CRON_MARKER" > "$TMPCRON" || true

cat >> "$TMPCRON" <<EOF
0 2 * * * ${NIGHTLY}                >> ${BACKUP_LOG} 2>&1   ${CRON_MARKER}
0 8 * * * ${VERIFY}                 >> ${BACKUP_LOG} 2>&1   ${CRON_MARKER}
0 3 * * 0 ${WEEKLY}                 >> ${CLEANUP_LOG} 2>&1  ${CRON_MARKER}
EOF

crontab "$TMPCRON"
rm -f "$TMPCRON"
echo "[ok] crontab installed"

# ---------- 5. summary --------------------------------------------------------
cat <<EOF

=========================================================================
QuailSync backup / cleanup cron jobs are installed.

Cron entries (crontab -l):
  0 2 * * *   ${NIGHTLY}
  0 8 * * *   ${VERIFY}
  0 3 * * 0   ${WEEKLY}

Logs:
  ${BACKUP_LOG}    (nightly-backup + morning-verify)
  ${CLEANUP_LOG}   (weekly-cleanup)

Sudoers fragment:
  ${SUDOERS_FILE}

Next steps:
  1. Confirm the QuailSync server is reachable from the Pi:
       curl -fsS http://localhost:3000/health
     Failure alerts are POSTed to /api/alerts and surface in the
     Android app's Dashboard bell icon.
  2. Verify:  bash ${NIGHTLY} --dry-run
  3. Tail logs:  tail -f ${BACKUP_LOG}
  4. Confirm cron sees the jobs:  crontab -l | grep QSYNC-MANAGED

To remove: re-run this with the cron block edited out, or
  crontab -l | grep -vF '${CRON_MARKER}' | crontab -
  sudo rm -f ${SUDOERS_FILE}
=========================================================================
EOF
