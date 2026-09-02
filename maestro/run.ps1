<#
.SYNOPSIS
    Runs every Maestro flow in maestro/flows/, collects screenshots, writes maestro/results.json.

.DESCRIPTION
    A feature exists if and only if it has a passing Maestro flow. This script produces
    the evidence: it runs each flow against the connected device, files the screenshots
    each flow took under maestro/screenshots/<flow-name>/, and records pass/fail into
    maestro/results.json. scripts/gen_features.py turns that into docs/FEATURES.md.

    Screenshot handling: `takeScreenshot: <name>` writes <name>.png into Maestro's
    *working directory*, not a configurable location. So each flow is run in its own
    throwaway working directory and the PNGs it drops there are moved into
    maestro/screenshots/<flow-name>/. Flows therefore stay free of path juggling.

    This file is deliberately pure ASCII: Windows PowerShell 5.1 reads .ps1 files as the
    system ANSI codepage, and literal non-ASCII characters break the parse.

.PARAMETER Flow
    Run only flows whose file name (without .yaml) matches this. Default: all.

.PARAMETER Device
    Device/emulator id passed through to `maestro --device`. Default: Maestro picks.

.EXAMPLE
    powershell -File maestro/run.ps1
    powershell -File maestro/run.ps1 -Flow dashboard-overview
#>
[CmdletBinding()]
param(
    [string]$Flow = "*",
    [string]$Device
)

$ErrorActionPreference = "Stop"

$RepoRoot       = Split-Path -Parent $PSScriptRoot
$FlowsDir       = Join-Path $PSScriptRoot "flows"
$ScreenshotsDir = Join-Path $PSScriptRoot "screenshots"
$ResultsPath    = Join-Path $PSScriptRoot "results.json"

# --- Locate the Maestro CLI -------------------------------------------------
# Preference order:
#   1. MAESTRO_CMD env var (full path to a maestro executable)
#   2. `maestro` on PATH (a normal CLI install)
#   3. The CLI bundled inside Maestro Studio for Windows. Studio ships the whole
#      maestro-cli in resources/app.asar.unpacked/dist-server/studio-server.jar
#      plus its own JRE 17, so it can be driven headlessly without WSL. This is
#      the path that works on this machine today.
$maestroExe  = $null
$maestroArgs = @()

if ($env:MAESTRO_CMD -and (Test-Path $env:MAESTRO_CMD)) {
    $maestroExe = $env:MAESTRO_CMD
} else {
    $onPath = Get-Command maestro -ErrorAction SilentlyContinue
    if ($onPath) {
        $maestroExe = $onPath.Source
    } else {
        $studio = Join-Path $env:LOCALAPPDATA "Programs\Maestro Studio\resources\app.asar.unpacked"
        $studioJava = Join-Path $studio "bundled-jvm\windows-x64\bin\java.exe"
        $studioJar  = Join-Path $studio "dist-server\studio-server.jar"
        if ((Test-Path $studioJava) -and (Test-Path $studioJar)) {
            $maestroExe  = $studioJava
            $maestroArgs = @("-cp", $studioJar, "maestro.cli.AppKt")
        }
    }
}

if (-not $maestroExe) {
    throw "No Maestro CLI found. Install the Maestro CLI, or set MAESTRO_CMD, or install Maestro Studio for Windows (its bundled CLI is used automatically)."
}

# Quiet the CLI's analytics banner and the "Analyze with AI" nag so stdout stays parseable.
$env:MAESTRO_CLI_NO_ANALYTICS = "1"
$env:MAESTRO_CLI_ANALYSIS_NOTIFICATION_DISABLED = "true"

function Invoke-Maestro {
    param([string[]]$Arguments, [string]$WorkingDirectory)
    $prev = Get-Location
    Set-Location $WorkingDirectory
    try {
        $output = & $maestroExe @($maestroArgs + $Arguments) 2>&1 | Out-String
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = $output }
    } finally {
        Set-Location $prev
    }
}

$maestroVersion = "unknown"
$versionRun = Invoke-Maestro -Arguments @("--version") -WorkingDirectory $RepoRoot
if ($versionRun.ExitCode -eq 0) {
    $lastLine = ($versionRun.Output -split "`r?`n" | Where-Object { $_.Trim() -ne "" } | Select-Object -Last 1)
    if ($lastLine) { $maestroVersion = $lastLine.Trim() }
}

# --- Run the flows ----------------------------------------------------------
$flowFiles = @(Get-ChildItem -Path $FlowsDir -Filter "*.yaml" | Where-Object { $_.BaseName -like $Flow } | Sort-Object Name)
if ($flowFiles.Count -eq 0) { throw "No flows matched '$Flow' in $FlowsDir" }

Write-Host "Maestro $maestroVersion  |  $($flowFiles.Count) flow(s)" -ForegroundColor Cyan

$results = @()

foreach ($file in $flowFiles) {
    $name = $file.BaseName
    Write-Host ""
    Write-Host "--> $name" -ForegroundColor Cyan

    # Isolated working directory: whatever PNGs appear here belong to this flow.
    $workDir = Join-Path ([System.IO.Path]::GetTempPath()) ("maestro-run-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $workDir | Out-Null

    $flowArgs = @("test")
    if ($Device) { $flowArgs += @("--device", $Device) }
    $flowArgs += $file.FullName

    $startedAt = (Get-Date).ToUniversalTime()
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $run = Invoke-Maestro -Arguments $flowArgs -WorkingDirectory $workDir
    $sw.Stop()

    $passed = ($run.ExitCode -eq 0)

    # Collect this flow's screenshots.
    $shotDir = Join-Path $ScreenshotsDir $name
    if (Test-Path $shotDir) { Remove-Item -Path (Join-Path $shotDir "*.png") -Force -ErrorAction SilentlyContinue }
    else { New-Item -ItemType Directory -Path $shotDir | Out-Null }

    $shots = @()
    foreach ($png in @(Get-ChildItem -Path $workDir -Filter "*.png" -Recurse -ErrorAction SilentlyContinue | Sort-Object Name)) {
        Move-Item -Path $png.FullName -Destination (Join-Path $shotDir $png.Name) -Force
        # Repo-relative, forward slashes, so FEATURES.md links work on any platform.
        $shots += "maestro/screenshots/$name/$($png.Name)"
    }

    Remove-Item -Path $workDir -Recurse -Force -ErrorAction SilentlyContinue

    if ($passed) {
        Write-Host "    PASS  ($([math]::Round($sw.Elapsed.TotalSeconds,1))s, $($shots.Count) screenshot(s))" -ForegroundColor Green
    } else {
        Write-Host "    FAIL  ($([math]::Round($sw.Elapsed.TotalSeconds,1))s)" -ForegroundColor Red
    }

    # On failure keep the tail of the CLI output; it names the step that broke.
    # This text is embedded verbatim in docs/FEATURES.md, so strip what is noise
    # there: ANSI colour codes, the logger's debug-path lines, and the "Run your
    # flows on Maestro Cloud" banner the CLI prints after every failure.
    $errorText = $null
    if (-not $passed) {
        $lines = @($run.Output -split "`r?`n" |
            ForEach-Object { $_ -replace "\x1b\[[0-9;]*[A-Za-z]", "" } |
            Where-Object { $_.Trim() -ne "" } |
            Where-Object { $_ -notmatch "maestro\.cli\.report\.TestDebugReporter" } |
            Where-Object { $_ -notmatch "Maestro Cloud" } |
            Where-Object { $_ -notmatch "maestro cloud app_file" } |
            Where-Object { $_ -notmatch "Debug tests faster" } |
            # Banner borders: drop any line with no letters or digits, which
            # covers box-drawing glyphs and the '?' they degrade to alike.
            Where-Object { $_ -match "[A-Za-z0-9]" })
        $errorText = ($lines | Select-Object -Last 25) -join "`n"
    }

    $results += [pscustomobject]@{
        flow             = $name
        file             = "maestro/flows/$($file.Name)"
        status           = if ($passed) { "pass" } else { "fail" }
        exit_code        = $run.ExitCode
        started_at       = $startedAt.ToString("yyyy-MM-ddTHH:mm:ssZ")
        duration_seconds = [math]::Round($sw.Elapsed.TotalSeconds, 1)
        screenshots      = @($shots)
        error            = $errorText
    }
}

$payload = [pscustomobject]@{
    generated_at    = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    maestro_version = $maestroVersion
    device          = if ($Device) { $Device } else { "default" }
    flows           = @($results)
}

$payload | ConvertTo-Json -Depth 6 | Out-File -FilePath $ResultsPath -Encoding utf8

$passCount = @($results | Where-Object { $_.status -eq "pass" }).Count
$failCount = $results.Count - $passCount

Write-Host ""
Write-Host "$passCount passed, $failCount failed  ->  maestro/results.json" -ForegroundColor Cyan
Write-Host "Next: python scripts/gen_features.py"

if ($failCount -gt 0) { exit 1 }
