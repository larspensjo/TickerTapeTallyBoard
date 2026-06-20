[CmdletBinding()]
param(
    [switch]$SkipInstall,
    [switch]$SkipBuild,
    [switch]$BuildOnly,
    [int]$FrontendPort = 5173,
    [switch]$ProductionDb,
    [string]$LocalDatabaseUrl,
    [string]$ProductionDatabaseUrl
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BackendDir = Join-Path $RepoRoot "backend"
$FrontendDir = Join-Path $RepoRoot "frontend"
$DefaultLocalDatabasePath = Join-Path $RepoRoot ".local/db/tttb-ledger-test.sqlite"
$DefaultProductionDatabasePath = Join-Path ([Environment]::GetFolderPath("MyDocuments")) "TickerTapeTallyBoard/portfolio.sqlite"

function Assert-Command {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command '$Name' was not found on PATH."
    }
}

function Invoke-Step {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,
        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Command
}

function Receive-AppJobOutput {
    param(
        [Parameter(Mandatory = $true)]
        [string]$StandardOutputPath,
        [Parameter(Mandatory = $true)]
        [string]$StandardErrorPath,
        [Parameter(Mandatory = $true)]
        [string]$Prefix
    )

    foreach ($path in @($StandardOutputPath, $StandardErrorPath)) {
        if (Test-Path $path) {
            Get-Content $path | ForEach-Object {
                Write-Host "[$Prefix] $_"
            }
        }
    }
}

function Stop-ProcessTree {
    param(
        [Parameter(Mandatory = $true)]
        [System.Diagnostics.Process]$Process
    )

    if (-not $Process.HasExited) {
        & taskkill.exe /PID $Process.Id /T /F | Out-Null
    }
}

function Wait-Url {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Url,
        [int]$TimeoutSeconds = 30
    )

    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        try {
            Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 2 | Out-Null
            return
        }
        catch {
            Start-Sleep -Milliseconds 500
        }
    }

    throw "Timed out waiting for $Url."
}

function Test-PortAvailable {
    param(
        [Parameter(Mandatory = $true)]
        [int]$Port
    )

    # Local-dev preflight only; the backend bind remains the source of truth.
    $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    try {
        $listener.Start()
        return $true
    }
    catch {
        return $false
    }
    finally {
        $listener.Stop()
    }
}

function Resolve-BackendPort {
    param(
        [Parameter(Mandatory = $true)]
        [int]$PreferredPort,
        [Parameter(Mandatory = $true)]
        [int]$FrontendPort
    )

    if (-not [string]::IsNullOrWhiteSpace($env:TTTB_PORT)) {
        $explicitPort = [int]$env:TTTB_PORT
        if ($explicitPort -eq $FrontendPort) {
            throw "TTTB_PORT ($explicitPort) must not match the frontend port ($FrontendPort)."
        }

        return $explicitPort
    }

    for ($port = $PreferredPort; $port -le 65535; $port++) {
        if ($port -eq $FrontendPort) {
            continue
        }

        if (Test-PortAvailable -Port $port) {
            return $port
        }
    }

    throw "No free backend port was found starting at $PreferredPort."
}

function Resolve-FrontendPort {
    param(
        [Parameter(Mandatory = $true)]
        [int]$PreferredPort
    )

    for ($port = $PreferredPort; $port -le 65535; $port++) {
        if (Test-PortAvailable -Port $port) {
            return $port
        }
    }

    throw "No free frontend port was found starting at $PreferredPort."
}

function ConvertTo-SqliteUrl {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Value
    )

    if ($Value.StartsWith("sqlite:", [System.StringComparison]::OrdinalIgnoreCase)) {
        return $Value
    }

    $fullPath = [System.IO.Path]::GetFullPath($Value)
    $parent = Split-Path -Parent $fullPath
    if ($parent) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }

    $normalized = $fullPath.Replace("\", "/")
    return "sqlite://$normalized"
}

function Resolve-DatabaseUrl {
    if ($ProductionDb) {
        $candidate = if (-not [string]::IsNullOrWhiteSpace($ProductionDatabaseUrl)) {
            $ProductionDatabaseUrl
        }
        elseif (-not [string]::IsNullOrWhiteSpace($env:TTTB_PRODUCTION_DATABASE_URL)) {
            $env:TTTB_PRODUCTION_DATABASE_URL
        }
        else {
            $DefaultProductionDatabasePath
        }

        return @{
            Mode = "production"
            Url  = ConvertTo-SqliteUrl $candidate
        }
    }

    $candidate = if (-not [string]::IsNullOrWhiteSpace($LocalDatabaseUrl)) {
        $LocalDatabaseUrl
    }
    elseif (-not [string]::IsNullOrWhiteSpace($env:TTTB_LOCAL_DATABASE_URL)) {
        $env:TTTB_LOCAL_DATABASE_URL
    }
    else {
        $DefaultLocalDatabasePath
    }

    return @{
        Mode = "local test"
        Url  = ConvertTo-SqliteUrl $candidate
    }
}

function Stop-OrphanVite {
    param([int]$Port)

    $pids = netstat -ano |
        Where-Object { $_ -match "TCP\s+127\.0\.0\.1:$Port\s+.*LISTENING\s+(\d+)" } |
        ForEach-Object { if ($_ -match "\s(\d+)\s*$") { [int]$Matches[1] } }

    foreach ($orphanPid in $pids) {
        $proc = Get-Process -Id $orphanPid -ErrorAction SilentlyContinue
        if ($proc -and $proc.ProcessName -eq "node") {
            Write-Host "Stopping orphan Vite process (PID $orphanPid) on port $Port." -ForegroundColor Yellow
            & taskkill.exe /PID $orphanPid /T /F | Out-Null
        }
    }
}

Assert-Command "cargo"
Assert-Command "npm"

# Kill any leftover Vite dev servers (e.g. orphaned by AI tooling) so the
# preferred port is always available to this script.
Stop-OrphanVite -Port $FrontendPort

if (-not (Test-Path $BackendDir)) {
    throw "Backend directory not found: $BackendDir"
}

if (-not (Test-Path $FrontendDir)) {
    throw "Frontend directory not found: $FrontendDir"
}

if (-not $SkipInstall) {
    Invoke-Step "Install frontend dependencies" {
        Push-Location $FrontendDir
        try {
            npm install
        }
        finally {
            Pop-Location
        }
    }
}

if (-not $SkipBuild) {
    Invoke-Step "Build backend" {
        Push-Location $BackendDir
        try {
            cargo build
        }
        finally {
            Pop-Location
        }
    }

    Invoke-Step "Build frontend" {
        Push-Location $FrontendDir
        try {
            npm run build
        }
        finally {
            Pop-Location
        }
    }
}

if ($BuildOnly) {
    Write-Host ""
    Write-Host "Build-only run completed." -ForegroundColor Green
    exit 0
}

$Database = Resolve-DatabaseUrl
$ResolvedFrontendPort = Resolve-FrontendPort -PreferredPort $FrontendPort
$BackendPort = Resolve-BackendPort -PreferredPort 8080 -FrontendPort $ResolvedFrontendPort

Write-Host ""
Write-Host "==> Start application" -ForegroundColor Cyan
Write-Host "Backend:  http://127.0.0.1:$BackendPort/"
Write-Host "Frontend: http://127.0.0.1:$ResolvedFrontendPort/"
if ($ResolvedFrontendPort -ne $FrontendPort) {
    Write-Host "Preferred frontend port $FrontendPort was busy; using $ResolvedFrontendPort instead." -ForegroundColor Yellow
}
Write-Host "Database: $($Database.Mode) ($($Database.Url))"
Write-Host "Press Ctrl+C to stop both processes."
Write-Host ""

$BackendExe = Join-Path $BackendDir "target/debug/ticker-tape-tally-board-backend.exe"
if (-not (Test-Path $BackendExe)) {
    throw "Backend executable not found: $BackendExe. Run without -SkipBuild first."
}

$NpmCommand = Get-Command "npm.cmd" -ErrorAction SilentlyContinue
if (-not $NpmCommand) {
    $NpmCommand = Get-Command "npm" -ErrorAction Stop
}

$LogDir = Join-Path $RepoRoot ".local/logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

$BackendStdout = Join-Path $LogDir "backend.out.log"
$BackendStderr = Join-Path $LogDir "backend.err.log"
$FrontendStdout = Join-Path $LogDir "frontend.out.log"
$FrontendStderr = Join-Path $LogDir "frontend.err.log"
Remove-Item $BackendStdout, $BackendStderr, $FrontendStdout, $FrontendStderr -ErrorAction SilentlyContinue

$PreviousDatabaseUrl = $env:TTTB_DATABASE_URL
$PreviousBackendPort = $env:TTTB_PORT
$env:TTTB_DATABASE_URL = $Database.Url
$env:TTTB_PORT = $BackendPort

$backendProcess = $null
$frontendProcess = $null

try {
    $backendProcess = Start-Process `
        -FilePath $BackendExe `
        -WorkingDirectory $BackendDir `
        -RedirectStandardOutput $BackendStdout `
        -RedirectStandardError $BackendStderr `
        -PassThru `
        -WindowStyle Hidden

    $frontendProcess = Start-Process `
        -FilePath $NpmCommand.Source `
        -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1", "--port", $ResolvedFrontendPort, "--strictPort") `
        -WorkingDirectory $FrontendDir `
        -RedirectStandardOutput $FrontendStdout `
        -RedirectStandardError $FrontendStderr `
        -PassThru `
        -WindowStyle Hidden

    Wait-Url "http://127.0.0.1:$BackendPort/"
    Wait-Url "http://127.0.0.1:$ResolvedFrontendPort/"
    Write-Host "Application is running." -ForegroundColor Green
    Write-Host ""

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

    if ($frontendProcess) {
        Stop-ProcessTree $frontendProcess
    }
    if ($backendProcess) {
        Stop-ProcessTree $backendProcess
    }

    if ($null -eq $PreviousDatabaseUrl) {
        Remove-Item Env:\TTTB_DATABASE_URL -ErrorAction SilentlyContinue
    }
    else {
        $env:TTTB_DATABASE_URL = $PreviousDatabaseUrl
    }

    if ($null -eq $PreviousBackendPort) {
        Remove-Item Env:\TTTB_PORT -ErrorAction SilentlyContinue
    }
    else {
        $env:TTTB_PORT = $PreviousBackendPort
    }
}
