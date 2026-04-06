# install_scheduler.ps1 — Register a daily Windows Task Scheduler job
# that runs sync_snapshots.ps1 at 9:00 AM.
#
# Usage (run as Administrator):
#   .\tools\install_scheduler.ps1
#
# To remove the task later:
#   Unregister-ScheduledTask -TaskName "QuailSync-SnapshotPrep" -Confirm:$false

$ErrorActionPreference = "Stop"

$TaskName   = "QuailSync-SnapshotPrep"
$ScriptDir  = Split-Path -Parent $MyInvocation.MyCommand.Definition
$ScriptPath = Join-Path $ScriptDir "sync_snapshots.ps1"

if (-not (Test-Path $ScriptPath)) {
    Write-Host "ERROR: sync_snapshots.ps1 not found at $ScriptPath" -ForegroundColor Red
    exit 1
}

# Find PowerShell executable
$PwshPath = (Get-Command pwsh -ErrorAction SilentlyContinue).Source
if (-not $PwshPath) {
    $PwshPath = (Get-Command powershell -ErrorAction SilentlyContinue).Source
}
if (-not $PwshPath) {
    Write-Host "ERROR: Could not find PowerShell executable" -ForegroundColor Red
    exit 1
}

# Remove existing task if it exists
$existing = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
if ($existing) {
    Write-Host "Removing existing task '$TaskName'..."
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
}

# Create the scheduled task
$action  = New-ScheduledTaskAction `
    -Execute $PwshPath `
    -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$ScriptPath`"" `
    -WorkingDirectory $ScriptDir

$trigger = New-ScheduledTaskTrigger -Daily -At "9:00AM"

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -RunOnlyIfNetworkAvailable

$principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive

Register-ScheduledTask `
    -TaskName $TaskName `
    -Action $action `
    -Trigger $trigger `
    -Settings $settings `
    -Principal $principal `
    -Description "QuailSync daily snapshot preprocessing for Roboflow upload" | Out-Null

Write-Host ""
Write-Host "Task registered successfully:" -ForegroundColor Green
Write-Host "  Name:     $TaskName"
Write-Host "  Schedule: Daily at 9:00 AM"
Write-Host "  Script:   $ScriptPath"
Write-Host "  Shell:    $PwshPath"
Write-Host ""
Write-Host "To run it now:  Start-ScheduledTask -TaskName '$TaskName'"
Write-Host "To remove it:   Unregister-ScheduledTask -TaskName '$TaskName' -Confirm:`$false"
