# sync_snapshots.ps1 — QuailSync daily snapshot prep pipeline
#
# Runs roboflow_prep.py on new images only, tracking what's already been
# processed in .processed_log.txt. Safe to run multiple times (idempotent).
#
# Usage:
#   .\tools\sync_snapshots.ps1
#
# Requires: Python 3 with opencv-python-headless installed.

$ErrorActionPreference = "Stop"

# --- Configuration ---
$SnapshotDir   = "C:\QuailSyncSnapshots"
$DateStamp     = Get-Date -Format "yyyy-MM-dd"
$StagingDir    = Join-Path $SnapshotDir "${DateStamp}_roboflow-ready"
$BlurThreshold = 50
$ScriptDir     = Split-Path -Parent $MyInvocation.MyCommand.Definition
$RepoRoot      = Split-Path -Parent $ScriptDir
$PrepScript    = Join-Path $ScriptDir "roboflow_prep.py"
$ProcessedLog  = Join-Path $ScriptDir ".processed_log.txt"

# --- Validate prerequisites ---
if (-not (Test-Path $SnapshotDir)) {
    Write-Host "[sync] Snapshot directory not found: $SnapshotDir" -ForegroundColor Yellow
    Write-Host "[sync] Nothing to process."
    exit 0
}

if (-not (Test-Path $PrepScript)) {
    Write-Host "[sync] ERROR: roboflow_prep.py not found at $PrepScript" -ForegroundColor Red
    exit 1
}

# --- Load processed log ---
$processed = @{}
if (Test-Path $ProcessedLog) {
    Get-Content $ProcessedLog | ForEach-Object {
        if ($_.Trim()) { $processed[$_.Trim()] = $true }
    }
}
$previousCount = $processed.Count
Write-Host "[sync] Loaded $previousCount previously processed filenames"

# --- Find new images ---
$extensions = @("*.jpg", "*.jpeg", "*.png")
$allImages = @()
foreach ($ext in $extensions) {
    $allImages += Get-ChildItem -Path $SnapshotDir -Filter $ext -File
}

$newImages = $allImages | Where-Object { -not $processed.ContainsKey($_.Name) }
$newCount = ($newImages | Measure-Object).Count

if ($newCount -eq 0) {
    Write-Host "[sync] No new images to process."
    exit 0
}

Write-Host "[sync] Found $newCount new images ($(($allImages | Measure-Object).Count) total, $previousCount already processed)"

# --- Create a temp directory with only new images ---
$tempDir = Join-Path $env:TEMP "quailsync-prep-$(Get-Date -Format 'yyyyMMdd-HHmmss')"
New-Item -ItemType Directory -Path $tempDir -Force | Out-Null

foreach ($img in $newImages) {
    Copy-Item -Path $img.FullName -Destination $tempDir
}

Write-Host "[sync] Copied $newCount new images to temp dir for processing"

# --- Resolve Python executable ---
$PythonExe = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $PythonExe) {
    $PythonExe = (Get-Command python3 -ErrorAction SilentlyContinue).Source
}
if (-not $PythonExe) {
    $PythonExe = (Get-Command py -ErrorAction SilentlyContinue).Source
}
if (-not $PythonExe) {
    Write-Host "[sync] ERROR: Could not find Python executable" -ForegroundColor Red
    Remove-Item -Path $tempDir -Recurse -Force
    exit 1
}
Write-Host "[sync] Using Python: $PythonExe"

# --- Run roboflow_prep.py ---
Write-Host "[sync] Running roboflow_prep.py with blur threshold $BlurThreshold..."
Write-Host ""

& $PythonExe $PrepScript $tempDir $StagingDir --blur-threshold $BlurThreshold 2>&1 | ForEach-Object { Write-Host $_ }

$exitCode = $LASTEXITCODE
Write-Host ""

if ($exitCode -ne 0) {
    Write-Host "[sync] WARNING: roboflow_prep.py exited with code $exitCode" -ForegroundColor Yellow
}

# --- Update processed log with ALL new images (even ones that were skipped as blurry/duplicate) ---
foreach ($img in $newImages) {
    if (-not $processed.ContainsKey($img.Name)) {
        Add-Content -Path $ProcessedLog -Value $img.Name
    }
}

# --- Count how many actually made it to staging ---
$stagedCount = 0
if (Test-Path $StagingDir) {
    foreach ($img in $newImages) {
        $stagedPath = Join-Path $StagingDir $img.Name
        if (Test-Path $stagedPath) {
            $stagedCount++
        }
    }
}

# --- Cleanup temp directory ---
Remove-Item -Path $tempDir -Recurse -Force

# --- Summary ---
Write-Host "=========================================="
Write-Host "  New images processed: $newCount"
Write-Host "  Added to staging:     $stagedCount"
Write-Host "  Staging directory:    $StagingDir"
Write-Host "=========================================="
