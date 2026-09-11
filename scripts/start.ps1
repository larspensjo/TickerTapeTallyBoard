<#
.SYNOPSIS
    Builds and runs TickerTapeTallyBoard.

.DESCRIPTION
    With no flags, builds the release backend and frontend static assets, then
    runs the backend as the single application process on port 8480.

.PARAMETER Dev
    Uses the debug backend and Vite development server.

.PARAMETER Demo
    Runs the seeded, in-memory demo. Alone it uses release static serving; with
    -Dev it uses the Vite development loop.

.PARAMETER InitLedger
    Allows the active non-demo ledger to be created when it is missing.

.PARAMETER NoBackup
    Disables the launch snapshot only.

.PARAMETER NoRefresh
    Disables the launch-time market-data refresh.

.PARAMETER DatabaseUrl
    Explicit SQLite database URL or path override for non-demo runs.

.PARAMETER Port
    Explicit backend port override.

.PARAMETER FrontendPort
    Preferred Vite port for -Dev runs.
#>
[CmdletBinding()]
param(
    [switch]$Dev,
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
$DefaultLegacyDatabasePath = Join-Path $RepoRoot ".local/db/tttb-ledger-test.sqlite"
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

function Get-PortOccupant {
    param([Parameter(Mandatory = $true)][int]$Port)
    $processId = $null
    try {
        $connection = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction Stop |
            Select-Object -First 1
        if ($connection) { $processId = [int]$connection.OwningProcess }
    }
    catch {
        # Fall through to netstat for older Windows versions and restricted
        # Get-NetTCPConnection environments.
    }
    if ($null -eq $processId) {
        $netstatLine = netstat -ano |
            Where-Object { $_ -match "^\s*TCP\s+\S+:$Port\s+\S+\s+LISTENING\s+(\d+)\s*$" } |
            Select-Object -First 1
        if ($netstatLine -match "\s(\d+)\s*$") { $processId = [int]$Matches[1] }
    }
    if ($null -eq $processId) { return $null }
    $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
    return [pscustomobject]@{
        Id = $processId
        Name = if ($process) { $process.ProcessName } else { "unknown" }
    }
}

function Assert-PinnedPortAvailable {
    param([Parameter(Mandatory = $true)][int]$Port)
    $occupant = Get-PortOccupant -Port $Port
    if ($occupant) {
        throw "Port $Port is already in use by process $($occupant.Name) (PID $($occupant.Id)). Use -Port <int> to choose another port."
    }
    if (-not (Test-PortAvailable -Port $Port)) {
        throw "Port $Port is already in use, but its occupying process could not be identified. Use -Port <int> to choose another port."
    }
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

Assert-Command "cargo"
Assert-Command "npm.cmd"
if (-not (Test-Path $BackendDir)) { throw "Backend directory not found: $BackendDir" }
if (-not (Test-Path $FrontendDir)) { throw "Frontend directory not found: $FrontendDir" }

$RunMode = if ($Demo) { "demo" } elseif ($Dev) { "development" } else { "production" }
$UsesVite = $Dev.IsPresent
$UsesPinnedPort = (-not $Dev) -and (-not $Demo)
$BackendPort = if ($UsesPinnedPort) {
    if ($PSBoundParameters.ContainsKey("Port")) { $Port } else { $DefaultBackendPort }
} else {
    $null
}
$ResolvedDatabaseUrl = if ($Demo) { $null } elseif ($PSBoundParameters.ContainsKey("DatabaseUrl")) {
    ConvertTo-SqliteUrl -Value $DatabaseUrl
} else {
    ConvertTo-SqliteUrl -Value $DefaultLegacyDatabasePath
}
$StaticAssetsDir = [System.IO.Path]::GetFullPath((Join-Path $FrontendDir "dist"))

# Check the production socket before starting installers or build tools. A busy
# pinned port is an operator error, not a reason to perform a partial launch.
if ($UsesPinnedPort -and -not $BuildOnly) {
    Assert-PinnedPortAvailable -Port $BackendPort
}

if (-not $SkipInstall) {
    Invoke-Step "Install frontend dependencies" {
        Push-Location $FrontendDir
        try { Invoke-NativeCommand "npm.cmd" @("install") }
        finally { Pop-Location }
    }
}
if (-not $SkipBuild) {
    Invoke-Step "Build backend" {
        Push-Location $BackendDir
        try {
            $arguments = if ($UsesVite) { @("build") } else { @("build", "--release") }
            Invoke-NativeCommand "cargo" $arguments
        }
        finally { Pop-Location }
    }
    if (-not $UsesVite) {
        Invoke-Step "Build frontend" {
            Push-Location $FrontendDir
            try { Invoke-NativeCommand "npm.cmd" @("run", "build") }
            finally { Pop-Location }
        }
    }
}
elseif (-not $UsesVite) {
    Write-Warning "-SkipBuild can serve a stale frontend/dist and stale release binary."
}

if ($BuildOnly) {
    Write-Host ""
    Write-Host "Build-only run completed." -ForegroundColor Green
    exit 0
}

$ResolvedFrontendPort = $null
if ($UsesVite) {
    Stop-OrphanVite -Port $FrontendPort
    $ResolvedFrontendPort = Resolve-FrontendPort -PreferredPort $FrontendPort
}
# A pinned port was resolved and pre-checked above and is never scanned away.
# Only the scanning modes choose their port here.
if (-not $UsesPinnedPort) {
    if ($PSBoundParameters.ContainsKey("Port")) {
        if ($UsesVite -and $Port -eq $ResolvedFrontendPort) {
            throw "-Port ($Port) must not match the frontend port ($ResolvedFrontendPort)."
        }
        $BackendPort = $Port
    }
    elseif ($UsesVite) {
        $BackendPort = Resolve-BackendPort -PreferredPort $DefaultBackendPort -FrontendPort $ResolvedFrontendPort
    }
    else {
        $BackendPort = Resolve-BackendPort -PreferredPort $DefaultBackendPort
    }
}

Write-Host ""
Write-Host "==> Start application" -ForegroundColor Cyan
Write-Host "Backend: http://127.0.0.1:$BackendPort/"
if ($UsesVite) {
    Write-Host "Frontend: http://127.0.0.1:$ResolvedFrontendPort/"
    if ($ResolvedFrontendPort -ne $FrontendPort) {
        Write-Host "Preferred frontend port $FrontendPort was busy; using $ResolvedFrontendPort instead." -ForegroundColor Yellow
    }
}
if ($Demo) { Write-Host "Database: demo (in-memory, seeded)" }
else { Write-Host "Database: $ResolvedDatabaseUrl" }
Write-Host "Press Ctrl+C to stop the application."
Write-Host ""

$BuildProfile = if ($UsesVite) { "debug" } else { "release" }
$BackendExe = Join-Path $BackendDir "target/$BuildProfile/ticker-tape-tally-board-backend.exe"
if (-not (Test-Path $BackendExe)) {
    throw "Backend executable not found: $BackendExe. Run without -SkipBuild first."
}
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
    if ($Demo) { Remove-Item Env:\TTTB_DATABASE_URL -ErrorAction SilentlyContinue }
    else { $env:TTTB_DATABASE_URL = $ResolvedDatabaseUrl }
    $env:TTTB_MODE = $RunMode
    $env:TTTB_CREATE_LEDGER_IF_MISSING = if ($InitLedger) { "1" } else { "0" }
    $env:TTTB_BACKUP_ENABLED = if ($NoBackup) { "0" } else { "1" }
    $env:TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED = if ($NoRefresh) { "0" } else { "1" }
    $env:TTTB_PORT = $BackendPort
    if ($UsesVite) { Remove-Item Env:\TTTB_STATIC_DIR -ErrorAction SilentlyContinue }
    else { $env:TTTB_STATIC_DIR = $StaticAssetsDir }

    $backendProcess = Start-Process `
        -FilePath $BackendExe `
        -WorkingDirectory $BackendDir `
        -RedirectStandardOutput $BackendStdout `
        -RedirectStandardError $BackendStderr `
        -PassThru `
        -WindowStyle Hidden
    if ($UsesVite) {
        $frontendProcess = Start-Process `
            -FilePath "npm.cmd" `
            -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1", "--port", $ResolvedFrontendPort, "--strictPort") `
            -WorkingDirectory $FrontendDir `
            -RedirectStandardOutput $FrontendStdout `
            -RedirectStandardError $FrontendStderr `
            -PassThru `
            -WindowStyle Hidden
    }
    Wait-Url `
        -Url "http://127.0.0.1:$BackendPort/" `
        -Process $backendProcess `
        -StandardOutputPath $BackendStdout `
        -StandardErrorPath $BackendStderr `
        -Prefix "backend"
    if ($UsesVite) {
        Wait-Url `
            -Url "http://127.0.0.1:$ResolvedFrontendPort/" `
            -Process $frontendProcess `
            -StandardOutputPath $FrontendStdout `
            -StandardErrorPath $FrontendStderr `
            -Prefix "frontend"
    }
    Write-Host "Application is running." -ForegroundColor Green
    Write-Host ""
    if (-not $NoBrowser) {
        $BrowserUrl = if ($UsesVite) { "http://127.0.0.1:$ResolvedFrontendPort/" } else { "http://127.0.0.1:$BackendPort/" }
        Write-Host "Opening $BrowserUrl in the default browser..." -ForegroundColor Cyan
        Start-Process $BrowserUrl
    }
    while ($true) {
        if ($backendProcess.HasExited) {
            Receive-AppJobOutput -StandardOutputPath $BackendStdout -StandardErrorPath $BackendStderr -Prefix "backend"
            throw "Backend process exited with code $($backendProcess.ExitCode)."
        }
        if ($UsesVite -and $frontendProcess.HasExited) {
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
