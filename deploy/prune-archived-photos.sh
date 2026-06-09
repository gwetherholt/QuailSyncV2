#!/usr/bin/env bash
#
# QuailSync — prune (delete) Pi-side bird photos AFTER a verified archive
# =============================================================================
# SAFETY CONTRACT
# ---------------
# This is the ONLY script that deletes photos from the Pi. It is deliberately
# separate from the copy/verify logic so that "delete cannot run unless verify
# passed" is obvious and enforced — not interleaved with the copy.
#
# Before deleting ANYTHING, this script INDEPENDENTLY re-verifies the archive:
#   1. all required arguments are present,
#   2. the destination archive exists on the share and is non-zero,
#   3. its SHA-256 matches the expected source checksum,
#   4. the manifest of files-to-delete exists.
# If ANY check fails it deletes NOTHING and exits non-zero. So even if this
# script is run by hand, it will not remove a photo whose backup it cannot
# re-confirm right now.
#
# Usage:
#   prune-archived-photos.sh <PHOTO_SRC_DIR> <DEST_ZIP> <EXPECTED_SHA256> <MANIFEST>
#
# Exit codes (the caller maps these to specific alerts):
#   0  deleted successfully
#   2  bad/missing arguments
#   3  destination archive missing or zero-byte
#   4  checksum mismatch (or could not compute) — copy not trustworthy
#   5  manifest missing
# =============================================================================

set -uo pipefail

LOGFILE="${LOGFILE:-/home/gwetherholt/quailsync/logs/photo-backup.log}"

log() {
    local line
    line="$(date '+%Y-%m-%d %H:%M:%S') [photo-prune] $*"
    echo "$line"
    echo "$line" >>"$LOGFILE" 2>/dev/null || true
}

# --- 1. Arguments ------------------------------------------------------------

if [[ $# -ne 4 ]]; then
    log "REFUSING TO DELETE: expected 4 args (src_dir, dest_zip, expected_sha, manifest), got $#."
    exit 2
fi

PHOTO_SRC_DIR="$1"
DEST_ZIP="$2"
EXPECTED_SHA="$3"
MANIFEST="$4"

if [[ -z "$PHOTO_SRC_DIR" || -z "$DEST_ZIP" || -z "$EXPECTED_SHA" || -z "$MANIFEST" ]]; then
    log "REFUSING TO DELETE: one or more arguments are empty."
    exit 2
fi
if [[ ! -d "$PHOTO_SRC_DIR" ]]; then
    log "REFUSING TO DELETE: source dir '$PHOTO_SRC_DIR' does not exist."
    exit 2
fi

# --- 2. Destination archive present and non-zero -----------------------------

if [[ ! -e "$DEST_ZIP" ]]; then
    log "REFUSING TO DELETE: destination archive '$DEST_ZIP' is missing."
    exit 3
fi
if [[ ! -s "$DEST_ZIP" ]]; then
    log "REFUSING TO DELETE: destination archive '$DEST_ZIP' is zero-byte."
    exit 3
fi

# --- 3. Independent checksum re-verification ---------------------------------

ACTUAL_SHA="$(sha256sum "$DEST_ZIP" 2>>"$LOGFILE" | awk '{print $1}')"
if [[ -z "$ACTUAL_SHA" ]]; then
    log "REFUSING TO DELETE: could not compute SHA-256 of '$DEST_ZIP'."
    exit 4
fi
if [[ "$ACTUAL_SHA" != "$EXPECTED_SHA" ]]; then
    log "REFUSING TO DELETE: checksum mismatch on re-verify (expected $EXPECTED_SHA, got $ACTUAL_SHA)."
    exit 4
fi

# --- 4. Manifest present -----------------------------------------------------

if [[ ! -f "$MANIFEST" ]]; then
    log "REFUSING TO DELETE: manifest '$MANIFEST' is missing."
    exit 5
fi

# --- Verified. Delete EXACTLY the archived files. ----------------------------
# We only remove the paths captured in the manifest, so any photo that arrived
# after the snapshot is left untouched.

log "Re-verification passed (SHA-256 $ACTUAL_SHA). Deleting archived Pi copies."

deleted=0
missing=0
while IFS= read -r rel; do
    [[ -z "$rel" ]] && continue
    target="$PHOTO_SRC_DIR/$rel"
    if [[ -f "$target" ]]; then
        if rm -f -- "$target" 2>>"$LOGFILE"; then
            deleted=$((deleted + 1))
        else
            log "WARNING: failed to delete '$target' (left in place)."
        fi
    else
        missing=$((missing + 1))
    fi
done <"$MANIFEST"

# Tidy now-empty subdirectories, but never remove the source root itself.
find "$PHOTO_SRC_DIR" -mindepth 1 -type d -empty -delete 2>/dev/null || true

log "Prune complete: deleted $deleted file(s); $missing already gone. Source root retained."
exit 0
