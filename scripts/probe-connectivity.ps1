<#
.SYNOPSIS
    Continuously probes backend connectivity through both the Vite dev proxy and
    a direct connection, logging any failure to .local/logs/connectivity-probe.log.

.DESCRIPTION
    The app intermittently shows "Could not load asset data" because the browser
    -> Vite proxy -> backend hop occasionally fails at TCP connect ("connect
    ETIMEDOUT 127.0.0.1"), in short self-healing clusters, while the backend
    itself stays healthy.

    Each tick this hits /api/health two ways at (near) the same instant:
      * proxy  : GET http://127.0.0.1:<FrontendPort>/api/health   (node -> backend)
      * direct : GET http://127.0.0.1:<BackendPort>/api/health     (this process -> backend)

    The discriminator we need next time it fails:
      * proxy fails while direct succeeds  -> node/Vite proxy-specific
      * both fail together                 -> OS / loopback stall (Defender, CPU starvation)

    Leave it running in a spare terminal alongside the app. Only failures and a
    periodic heartbeat are written, so the log stays small.

.EXAMPLE
    pwsh -File scripts/probe-connectivity.ps1
    pwsh -File scripts/probe-connectivity.ps1 -FrontendPort 5174 -IntervalSeconds 1
#>
[CmdletBinding()]
param(
    [int]$FrontendPort = 5173,
    [int]$BackendPort = $(if ($env:TTTB_PORT) { [int]$env:TTTB_PORT } else { 8080 }),
    [double]$IntervalSeconds = 1,
    [int]$TimeoutSeconds = 5,
    [int]$HeartbeatMinutes = 10
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$LogDir = Join-Path $RepoRoot ".local/logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$LogPath = Join-Path $LogDir "connectivity-probe.log"

$ProxyUrl = "http://127.0.0.1:$FrontendPort/api/health"
$DirectUrl = "http://127.0.0.1:$BackendPort/api/health"

Add-Type -AssemblyName System.Net.Http
$client = [System.Net.Http.HttpClient]::new()
$client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)

function Write-Line {
    param([string]$Text)
    $stamp = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss.fff")
    $line = "$stamp  $Text"
    Write-Host $line
    Add-Content -Path $LogPath -Value $line
}

# Returns a hashtable: Ok (bool), Ms (int), Detail (string)
function Test-Endpoint {
    param([string]$Url)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $response = $client.GetAsync($Url).GetAwaiter().GetResult()
        $sw.Stop()
        $code = [int]$response.StatusCode
        return @{ Ok = $response.IsSuccessStatusCode; Ms = $sw.ElapsedMilliseconds; Detail = "http $code" }
    }
    catch {
        $sw.Stop()
        $inner = $_.Exception
        while ($inner.InnerException) { $inner = $inner.InnerException }
        return @{ Ok = $false; Ms = $sw.ElapsedMilliseconds; Detail = "$($inner.GetType().Name): $($inner.Message)" }
    }
}

Write-Line "probe started proxy=$ProxyUrl direct=$DirectUrl interval=${IntervalSeconds}s timeout=${TimeoutSeconds}s"
$lastHeartbeat = Get-Date
$ticks = 0
$fails = 0

try {
    while ($true) {
        $ticks++
        $proxy = Test-Endpoint $ProxyUrl
        $direct = Test-Endpoint $DirectUrl

        if (-not $proxy.Ok -or -not $direct.Ok) {
            $fails++
            $verdict = if (-not $proxy.Ok -and $direct.Ok) {
                "PROXY-ONLY (node/Vite proxy hop)"
            }
            elseif ($proxy.Ok -and -not $direct.Ok) {
                "DIRECT-ONLY (backend not accepting)"
            }
            else {
                "BOTH (OS/loopback stall)"
            }
            Write-Line ("FAIL {0} | proxy: ok={1} {2}ms [{3}] | direct: ok={4} {5}ms [{6}]" -f `
                    $verdict, $proxy.Ok, $proxy.Ms, $proxy.Detail, $direct.Ok, $direct.Ms, $direct.Detail)
        }

        if (((Get-Date) - $lastHeartbeat).TotalMinutes -ge $HeartbeatMinutes) {
            Write-Line "heartbeat ticks=$ticks fails=$fails proxy_last=$($proxy.Ms)ms direct_last=$($direct.Ms)ms"
            $lastHeartbeat = Get-Date
        }

        Start-Sleep -Seconds $IntervalSeconds
    }
}
finally {
    Write-Line "probe stopped ticks=$ticks fails=$fails"
    $client.Dispose()
}
