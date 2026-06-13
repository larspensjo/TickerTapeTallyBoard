[CmdletBinding()]
param(
    [switch]$SkipInstall,
    [switch]$SkipBuild,
    [switch]$BuildOnly,
    [int]$FrontendPort = 5173
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$BackendDir = Join-Path $RepoRoot "backend"
$FrontendDir = Join-Path $RepoRoot "frontend"

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

Assert-Command "cargo"
Assert-Command "npm"

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

Write-Host ""
Write-Host "==> Start application" -ForegroundColor Cyan
Write-Host "Backend:  http://127.0.0.1:8080/"
Write-Host "Frontend: http://127.0.0.1:$FrontendPort/"
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

$backendProcess = Start-Process `
    -FilePath $BackendExe `
    -WorkingDirectory $BackendDir `
    -RedirectStandardOutput $BackendStdout `
    -RedirectStandardError $BackendStderr `
    -PassThru `
    -WindowStyle Hidden

$frontendProcess = Start-Process `
    -FilePath $NpmCommand.Source `
    -ArgumentList @("run", "dev", "--", "--host", "127.0.0.1", "--port", $FrontendPort) `
    -WorkingDirectory $FrontendDir `
    -RedirectStandardOutput $FrontendStdout `
    -RedirectStandardError $FrontendStderr `
    -PassThru `
    -WindowStyle Hidden

try {
    Wait-Url "http://127.0.0.1:8080/"
    Wait-Url "http://127.0.0.1:$FrontendPort/"
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

    Stop-ProcessTree $frontendProcess
    Stop-ProcessTree $backendProcess
}
