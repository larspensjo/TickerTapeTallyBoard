<#
.SYNOPSIS
    Builds and runs TickerTapeTallyBoard.

.DESCRIPTION
    With no flags, builds the release desktop shell and frontend static assets,
    then opens the native desktop window. Close the window to stop the app.

.PARAMETER Dev
    Runs the browser development loop: the debug backend plus the Vite
    development server. Combine with -Desktop for the debug desktop window.

.PARAMETER Desktop
    With -Dev, opens the debug desktop window instead of the browser loop.
    Without -Dev it is accepted for compatibility; the desktop is the default.

.PARAMETER ProbeWebView
    Runs the native WebView2 probe in the desktop window.

.PARAMETER Demo
    Runs the seeded, in-memory demo. Alone it opens the desktop window; with
    -Dev it uses the browser development loop.

.PARAMETER InitLedger
    Allows the active non-demo ledger to be created when it is missing.

.PARAMETER NoBackup
    Disables the launch snapshot only.

.PARAMETER NoRefresh
    Disables the launch-time market-data refresh.

.PARAMETER DatabaseUrl
    Explicit SQLite database URL or path override for non-demo runs.

.PARAMETER Port
    Explicit backend port override for the browser development loop.

.PARAMETER FrontendPort
    Preferred Vite port for the browser development loop.

.PARAMETER NoBrowser
    Starts the browser development loop without opening a browser.
#>
[CmdletBinding()]
param(
    [switch]$Dev,
    [switch]$Desktop,
    [switch]$ProbeWebView,
    [switch]$Demo,
    [switch]$InitLedger,
    [switch]$NoBackup,
    [switch]$NoRefresh,
    [string]$DatabaseUrl,
    [ValidateRange(1, 65535)]
    [int]$Port,
    [ValidateRange(1, 65535)]
    [int]$FrontendPort = 5173,
    [switch]$SkipInstall,
    [switch]$SkipBuild,
    [switch]$BuildOnly,
    [switch]$NoBrowser,

    # Retired selectors remain as throwing stubs so callers receive a useful error.
    [switch]$ProductionDb,
    [string]$LocalDatabaseUrl,
    [string]$ProductionDatabaseUrl
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BackendDir = Join-Path $RepoRoot "backend"
$FrontendDir = Join-Path $RepoRoot "frontend"
$LegacyDatabasePath = Join-Path $RepoRoot ".local/db/tttb-ledger-test.sqlite"
$DefaultBackendPort = 8480

function Assert-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found on PATH."
    }
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][scriptblock]$Command
    )
    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Command
}

function Invoke-NativeCommand {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @()
    )
    & $FilePath @ArgumentList
    $exitCode = $LASTEXITCODE
    if ($exitCode -ne 0) {
        $commandLine = (@($FilePath) + $ArgumentList) -join " "
        throw "Command '$commandLine' failed with exit code $exitCode."
    }
}

function Receive-AppJobOutput {
    param(
        [Parameter(Mandatory = $true)][string]$StandardOutputPath,
        [Parameter(Mandatory = $true)][string]$StandardErrorPath,
        [Parameter(Mandatory = $true)][string]$Prefix
    )
    foreach ($path in @($StandardOutputPath, $StandardErrorPath)) {
        if (Test-Path $path) {
            Get-Content $path | ForEach-Object { Write-Host "[$Prefix] $_" }
        }
    }
}

function Stop-ProcessTree {
    param([Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process)
    if (-not $Process.HasExited) {
        & taskkill.exe /PID $Process.Id /T /F | Out-Null
    }
}

function Wait-Url {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [int]$TimeoutSeconds = 30,
        [System.Diagnostics.Process]$Process,
        [string]$StandardOutputPath,
        [string]$StandardErrorPath,
        [string]$Prefix = "process"
    )
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ($true) {
        if ($Process -and $Process.HasExited) {
            Receive-AppJobOutput `
                -StandardOutputPath $StandardOutputPath `
                -StandardErrorPath $StandardErrorPath `
                -Prefix $Prefix
            throw "$Prefix process exited with code $($Process.ExitCode) while waiting for $Url. Logs: $StandardOutputPath, $StandardErrorPath"
        }
        if ((Get-Date) -ge $deadline) { break }
        try {
            Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2 | Out-Null
            return
        }
        catch { Start-Sleep -Milliseconds 500 }
    }
    throw "Timed out waiting for $Url. Logs: $StandardOutputPath, $StandardErrorPath"
}

function Test-PortAvailable {
    param([Parameter(Mandatory = $true)][int]$Port)
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
        return $true
    }
    catch { return $false }
    finally { $listener.Stop() }
}

function Resolve-BackendPort {
    param(
        [Parameter(Mandatory = $true)][int]$PreferredPort,
        [int]$FrontendPort = 0
    )
    for ($candidate = $PreferredPort; $candidate -le 65535; $candidate++) {
        if ($candidate -ne $FrontendPort -and (Test-PortAvailable -Port $candidate)) {
            return $candidate
        }
    }
    throw "No free backend port was found starting at $PreferredPort."
}

function Resolve-FrontendPort {
    param([Parameter(Mandatory = $true)][int]$PreferredPort)
    for ($candidate = $PreferredPort; $candidate -le 65535; $candidate++) {
        if (Test-PortAvailable -Port $candidate) { return $candidate }
    }
    throw "No free frontend port was found starting at $PreferredPort."
}

function ConvertTo-SqliteUrl {
    param([Parameter(Mandatory = $true)][string]$Value)
    if ($Value.StartsWith("sqlite:", [System.StringComparison]::OrdinalIgnoreCase)) {
        return $Value
    }
    $fullPath = [System.IO.Path]::GetFullPath($Value)
    $parent = Split-Path -Parent $fullPath
    if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
    return "sqlite://$($fullPath.Replace('\', '/'))"
}

function Test-RepoViteCommandLine {
    param(
        [string]$CommandLine,
        [Parameter(Mandatory = $true)][string]$RepoRootPath
    )
    if ([string]::IsNullOrWhiteSpace($CommandLine)) { return $false }
    return (($CommandLine -match "vite") -and
        ($CommandLine.IndexOf($RepoRootPath, [System.StringComparison]::OrdinalIgnoreCase) -ge 0))
}

function Stop-OrphanVite {
    param([int]$Port)
    $pids = netstat -ano |
        Where-Object { $_ -match "TCP\s+127\.0\.0\.1:$Port\s+.*LISTENING\s+(\d+)" } |
        ForEach-Object { if ($_ -match "\s(\d+)\s*$") { [int]$Matches[1] } }
    foreach ($orphanPid in $pids) {
        $process = Get-Process -Id $orphanPid -ErrorAction SilentlyContinue
        if (-not $process -or $process.ProcessName -ne "node") { continue }
        $commandLine = (Get-CimInstance Win32_Process -Filter "ProcessId = $orphanPid" -ErrorAction SilentlyContinue).CommandLine
        if (Test-RepoViteCommandLine -CommandLine $commandLine -RepoRootPath $RepoRoot.Path) {
            Write-Host "Stopping orphan Vite process (PID $orphanPid) on port $Port." -ForegroundColor Yellow
            & taskkill.exe /PID $orphanPid /T /F | Out-Null
        }
        else {
            Write-Host "Port $Port is in use by an unrelated process (PID $orphanPid); leaving it running." -ForegroundColor Yellow
        }
    }
}

if ($ProductionDb) {
    throw "-ProductionDb is retired; use scripts/start.ps1 with no flags, or -DatabaseUrl <string> for an explicit ledger."
}
if ($PSBoundParameters.ContainsKey("LocalDatabaseUrl")) {
    throw "-LocalDatabaseUrl is retired; use -DatabaseUrl <string>."
}
if ($PSBoundParameters.ContainsKey("ProductionDatabaseUrl")) {
    throw "-ProductionDatabaseUrl is retired; use -DatabaseUrl <string>."
}
if ($Demo -and $PSBoundParameters.ContainsKey("DatabaseUrl")) {
    throw "-Demo cannot be combined with -DatabaseUrl. Demo mode uses an in-memory ledger."
}
if ($Demo -and $InitLedger) {
    throw "-Demo cannot be combined with -InitLedger. Demo mode uses an in-memory ledger."
}
# The desktop window is the application; only -Dev without -Desktop runs the
# browser development loop.
$UsesDesktop = $Desktop.IsPresent -or (-not $Dev)
$UsesVite = -not $UsesDesktop
if ($ProbeWebView -and -not $UsesDesktop) {
    throw "-ProbeWebView runs in the desktop window; use it without -Dev, or with -Dev -Desktop."
}
if ($UsesDesktop -and $PSBoundParameters.ContainsKey("Port")) {
    throw "-Port applies only to the browser development loop (-Dev); the desktop shell has no listener."
}
if ($UsesDesktop -and $PSBoundParameters.ContainsKey("FrontendPort")) {
    throw "-FrontendPort applies only to the browser development loop (-Dev)."
}
if ($UsesDesktop -and $NoBrowser) {
    throw "-NoBrowser applies only to the browser development loop (-Dev); the desktop opens no browser."
}
if ($Desktop -and -not $Dev) {
    Write-Host "-Desktop is no longer needed; the desktop window is the default." -ForegroundColor Yellow
}
if ($ProbeWebView -and $PSBoundParameters.ContainsKey("DatabaseUrl")) {
    throw "-Desktop -ProbeWebView cannot be combined with -DatabaseUrl; the probe always uses demo mode."
}
if (-not $Demo -and -not $ProbeWebView -and -not $BuildOnly) {
    $legacyArtifacts = @(
        $LegacyDatabasePath,
        "$LegacyDatabasePath-wal",
        "$LegacyDatabasePath-shm"
    ) | Where-Object { Test-Path -LiteralPath $_ }
    if ($legacyArtifacts.Count -gt 0) {
        $legacyArtifactList = ($legacyArtifacts | ForEach-Object { "'$_'" }) -join ", "
        throw "Legacy ledger artifact(s) still exist: $legacyArtifactList. Refusing to start because the ledger may be split between its database and SQLite sidecars. If the target portfolio.sqlite does not exist, run scripts/migrate-ledger.ps1. If it does exist, do not start or re-run migration: recover one complete legacy set from %LOCALAPPDATA%\TickerTapeTallyBoard\pre-move-backup, preserve and remove the incomplete target set, then retry the migration."
    }
}

Assert-Command "cargo"
Assert-Command "npm.cmd"
if (-not (Test-Path $BackendDir)) { throw "Backend directory not found: $BackendDir" }
if (-not (Test-Path $FrontendDir)) { throw "Frontend directory not found: $FrontendDir" }

$RunMode = if ($Demo) { "demo" } elseif ($Dev) { "development" } else { "production" }
$ExplicitDatabaseUrl = if ($PSBoundParameters.ContainsKey("DatabaseUrl")) {
    ConvertTo-SqliteUrl -Value $DatabaseUrl
}
$StaticAssetsDir = [System.IO.Path]::GetFullPath((Join-Path $FrontendDir "dist"))

if (-not $SkipInstall) {
    Invoke-Step "Install frontend dependencies" {
        Push-Location $FrontendDir
        try { Invoke-NativeCommand "npm.cmd" @("install") }
        finally { Pop-Location }
    }
}
if (-not $SkipBuild) {
    Invoke-Step $(if ($UsesDesktop) { "Build desktop" } else { "Build backend" }) {
        Push-Location $RepoRoot
        try {
            $package = if ($UsesDesktop) { "ticker-tape-tally-board-desktop" } else { "ticker-tape-tally-board-backend" }
            $arguments = if ($Dev) {
                @("build", "-p", $package)
            }
            else {
                @("build", "-p", $package, "--release")
            }
            Invoke-NativeCommand "cargo" $arguments
        }
        finally { Pop-Location }
    }
    if ($UsesDesktop) {
        Invoke-Step "Build frontend" {
            Push-Location $FrontendDir
            try { Invoke-NativeCommand "npm.cmd" @("run", "build") }
            finally { Pop-Location }
        }
    }
}
elseif ($UsesDesktop) {
    Write-Warning "-SkipBuild can serve a stale frontend/dist and stale desktop binary."
}

if ($BuildOnly) {
    Write-Host ""
    Write-Host "Build-only run completed." -ForegroundColor Green
    exit 0
}

$BuildProfile = if ($Dev) { "debug" } else { "release" }
$DesktopExe = Join-Path $RepoRoot "target/$BuildProfile/ticker-tape-tally-board-desktop.exe"
if ($UsesDesktop -and -not (Test-Path $DesktopExe)) {
    throw "Desktop executable not found: $DesktopExe. Run without -SkipBuild first."
}

$ResolvedFrontendPort = $null
if ($UsesVite) {
    Stop-OrphanVite -Port $FrontendPort
    $ResolvedFrontendPort = Resolve-FrontendPort -PreferredPort $FrontendPort
}
$BackendPort = $null
if ($UsesVite) {
    if ($PSBoundParameters.ContainsKey("Port")) {
        if ($Port -eq $ResolvedFrontendPort) {
            throw "-Port ($Port) must not match the frontend port ($ResolvedFrontendPort)."
        }
        $BackendPort = $Port
    }
    else {
        $BackendPort = Resolve-BackendPort -PreferredPort $DefaultBackendPort -FrontendPort $ResolvedFrontendPort
    }
}

Write-Host ""
Write-Host "==> Start application" -ForegroundColor Cyan
if ($UsesDesktop) { Write-Host "Desktop: $DesktopExe" }
else { Write-Host "Backend: http://127.0.0.1:$BackendPort/" }
if ($UsesVite) {
    Write-Host "Frontend: http://127.0.0.1:$ResolvedFrontendPort/"
    if ($ResolvedFrontendPort -ne $FrontendPort) {
        Write-Host "Preferred frontend port $FrontendPort was busy; using $ResolvedFrontendPort instead." -ForegroundColor Yellow
    }
}
if ($ProbeWebView) { Write-Host "Database: demo (in-memory, seeded; probe mode)" }
elseif ($Demo) { Write-Host "Database: demo (in-memory, seeded)" }
elseif ($PSBoundParameters.ContainsKey("DatabaseUrl")) { Write-Host "Database: $ExplicitDatabaseUrl" }
else { Write-Host "Database: application resolves the $RunMode default ledger; the runtime log records the resolved path" }
if ($UsesDesktop) {
    if ($env:LOCALAPPDATA) {
        Write-Host "Desktop logs: $(Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard\logs") (engine-desktop*.log)"
    }
    else { Write-Host "Desktop log: LOCALAPPDATA is unavailable (terminal logging only)" }
}
if ($UsesDesktop) { Write-Host "Close the window to stop the app." }
else { Write-Host "Press Ctrl+C to stop the application." }
Write-Host ""

$BackendExe = Join-Path $RepoRoot "target/$BuildProfile/ticker-tape-tally-board-backend.exe"
if ($UsesVite -and -not (Test-Path $BackendExe)) {
    throw "Backend executable not found: $BackendExe. Run without -SkipBuild first."
}
$BackendStdout = $null
$BackendStderr = $null
$FrontendStdout = $null
$FrontendStderr = $null
if ($UsesVite) {
    $LogRoot = Join-Path $RepoRoot ".local/logs"
    New-Item -ItemType Directory -Force -Path $LogRoot | Out-Null
    $RunStamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
    $RunLogDir = Join-Path $LogRoot "run-$RunStamp"
    New-Item -ItemType Directory -Path $RunLogDir -ErrorAction Stop | Out-Null
    $BackendStdout = Join-Path $RunLogDir "backend.out.log"
    $BackendStderr = Join-Path $RunLogDir "backend.err.log"
    $FrontendStdout = Join-Path $RunLogDir "frontend.out.log"
    $FrontendStderr = Join-Path $RunLogDir "frontend.err.log"

    # Keep probe logs and other files directly under .local/logs; only completed
    # run directories participate in launcher-log retention.
    Get-ChildItem -LiteralPath $LogRoot -Directory |
        Where-Object { $_.Name -like "run-*" } |
        Sort-Object -Property Name -Descending |
        Select-Object -Skip 5 |
        ForEach-Object {
            try {
                Remove-Item -LiteralPath $_.FullName -Recurse -Force
            }
            catch {
                Write-Host "Warning: could not prune old run log directory '$($_.FullName)': $($_.Exception.Message)" -ForegroundColor Yellow
            }
        }
}

$PreviousDatabaseUrl = $env:TTTB_DATABASE_URL
$PreviousBackendPort = $env:TTTB_PORT
$PreviousMode = $env:TTTB_MODE
$PreviousCreateLedgerIfMissing = $env:TTTB_CREATE_LEDGER_IF_MISSING
$PreviousBackupEnabled = $env:TTTB_BACKUP_ENABLED
$PreviousLaunchRefreshEnabled = $env:TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED
$PreviousStaticAssetsDir = $env:TTTB_STATIC_DIR
$backendProcess = $null
$frontendProcess = $null

try {
    if ($Demo -or $ProbeWebView) { Remove-Item Env:\TTTB_DATABASE_URL -ErrorAction SilentlyContinue }
    elseif ($PSBoundParameters.ContainsKey("DatabaseUrl")) { $env:TTTB_DATABASE_URL = $ExplicitDatabaseUrl }
    else { Remove-Item Env:\TTTB_DATABASE_URL -ErrorAction SilentlyContinue }
    $env:TTTB_MODE = if ($ProbeWebView) { "demo" } else { $RunMode }
    $env:TTTB_CREATE_LEDGER_IF_MISSING = if ($InitLedger) { "1" } else { "0" }
    $env:TTTB_BACKUP_ENABLED = if ($NoBackup) { "0" } else { "1" }
    $env:TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED = if ($NoRefresh) { "0" } else { "1" }
    if ($UsesDesktop) { Remove-Item Env:\TTTB_PORT -ErrorAction SilentlyContinue }
    else { $env:TTTB_PORT = $BackendPort }
    if ($UsesVite) { Remove-Item Env:\TTTB_STATIC_DIR -ErrorAction SilentlyContinue }
    else { $env:TTTB_STATIC_DIR = $StaticAssetsDir }

    if ($UsesDesktop) {
        $desktopArguments = if ($ProbeWebView) { @("--probe-webview") } else { @() }
        $desktopStart = @{
            FilePath = $DesktopExe
            WorkingDirectory = $RepoRoot
            NoNewWindow = $true
            Wait = $true
            PassThru = $true
        }
        if ($desktopArguments.Count -gt 0) {
            $desktopStart.ArgumentList = $desktopArguments
        }
        $desktopProcess = Start-Process @desktopStart
        $desktopExitCode = $desktopProcess.ExitCode
        if ($desktopExitCode -ne 0) {
            Write-Host "Desktop process exited with code $desktopExitCode." -ForegroundColor Red
        }
        exit $desktopExitCode
    }

    $backendProcess = Start-Process `
        -FilePath $BackendExe `
        -WorkingDirectory $BackendDir `
        -RedirectStandardOutput $BackendStdout `
        -RedirectStandardError $BackendStderr `
        -PassThru `
        -WindowStyle Hidden
    $frontendProcess = Start-Process `
        -FilePath "npm.cmd" `
        -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1", "--port", $ResolvedFrontendPort, "--strictPort") `
        -WorkingDirectory $FrontendDir `
        -RedirectStandardOutput $FrontendStdout `
        -RedirectStandardError $FrontendStderr `
        -PassThru `
        -WindowStyle Hidden
    Wait-Url `
        -Url "http://127.0.0.1:$BackendPort/" `
        -Process $backendProcess `
        -StandardOutputPath $BackendStdout `
        -StandardErrorPath $BackendStderr `
        -Prefix "backend"
    Wait-Url `
        -Url "http://127.0.0.1:$ResolvedFrontendPort/" `
        -Process $frontendProcess `
        -StandardOutputPath $FrontendStdout `
        -StandardErrorPath $FrontendStderr `
        -Prefix "frontend"
    Write-Host "Application is running." -ForegroundColor Green
    Write-Host ""
    if (-not $NoBrowser) {
        $BrowserUrl = "http://127.0.0.1:$ResolvedFrontendPort/"
        Write-Host "Opening $BrowserUrl in the default browser..." -ForegroundColor Cyan
        Start-Process $BrowserUrl
    }
    while ($true) {
        if ($backendProcess.HasExited) {
            Receive-AppJobOutput -StandardOutputPath $BackendStdout -StandardErrorPath $BackendStderr -Prefix "backend"
            throw "Backend process exited with code $($backendProcess.ExitCode)."
        }
        if ($frontendProcess.HasExited) {
            Receive-AppJobOutput -StandardOutputPath $FrontendStdout -StandardErrorPath $FrontendStderr -Prefix "frontend"
            throw "Frontend process exited with code $($frontendProcess.ExitCode)."
        }
        Start-Sleep -Milliseconds 500
    }
}
finally {
    Write-Host ""
    Write-Host "Stopping application processes..." -ForegroundColor Yellow
    if ($frontendProcess) { Stop-ProcessTree $frontendProcess }
    if ($backendProcess) { Stop-ProcessTree $backendProcess }
    $environmentValues = @{
        "TTTB_DATABASE_URL" = $PreviousDatabaseUrl
        "TTTB_PORT" = $PreviousBackendPort
        "TTTB_MODE" = $PreviousMode
        "TTTB_CREATE_LEDGER_IF_MISSING" = $PreviousCreateLedgerIfMissing
        "TTTB_BACKUP_ENABLED" = $PreviousBackupEnabled
        "TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED" = $PreviousLaunchRefreshEnabled
        "TTTB_STATIC_DIR" = $PreviousStaticAssetsDir
    }
    foreach ($entry in $environmentValues.GetEnumerator()) {
        if ($null -eq $entry.Value) { Remove-Item "Env:\$($entry.Key)" -ErrorAction SilentlyContinue }
        else { Set-Item "Env:\$($entry.Key)" $entry.Value }
    }
}
