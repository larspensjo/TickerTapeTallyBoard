<#
.SYNOPSIS
    Captures raw Yahoo fundamentals, Yahoo search, Yahoo split events and Nasdaq US
    earnings-date responses for a list of symbols.

.DESCRIPTION
    Read-only probe for the company-figures sources. For every symbol in the list it
    calls the Yahoo fundamentals-timeseries endpoint (quarterly, annual and trailing
    types in one request), the Yahoo search endpoint (by ISIN when given, else by
    symbol), the Yahoo chart endpoint for daily closes with split events (the closes
    give the listing price on each market-value evidence date), and, for symbols
    without an exchange suffix, the Nasdaq per-symbol earnings-date endpoint. It can
    also scan the Nasdaq US earnings calendar day by day. Yahoo requests use the same
    plain shape and user agent as the backend's Yahoo chart client; Nasdaq requests add
    `Accept: application/json`.

    Raw bodies are written under -OutDir (default docs/spikes/private/fundamentals/,
    which is gitignored), together with latency.csv recording one line per call.

.PARAMETER SymbolFile
    CSV with a `symbol` column and an optional `isin` column. Private symbol lists
    belong in docs/spikes/private/.

.PARAMETER Symbols
    Comma-separated symbols to probe instead of, or in addition to, -SymbolFile.

.PARAMETER Types
    Comma-separated timeseries types. Defaults to every type the source spike
    evaluated (quarterly, annual and trailing variants of 17 series).

.PARAMETER ChartRange
    Chart range, for example 1mo or 2y. Defaults to 2y.

.PARAMETER ChartInterval
    Chart interval, for example 1d or 1mo. Defaults to 1d.

.PARAMETER Kinds
    Comma-separated per-symbol call kinds to run: timeseries, search, chart,
    nasdaq-earnings-date. Defaults to all of them.

.PARAMETER CalendarFrom
    First day of the Nasdaq calendar scan (yyyy-MM-dd). Omit to skip the scan.

.PARAMETER CalendarTo
    Last day of the Nasdaq calendar scan (yyyy-MM-dd). Weekends are skipped.

.EXAMPLE
    pwsh -File scripts/probe-fundamentals.ps1 -Symbols MSFT,ASML.AS,EVO.ST
    pwsh -File scripts/probe-fundamentals.ps1 -SymbolFile docs/spikes/private/symbols.csv -CalendarFrom 2026-11-02 -CalendarTo 2026-11-27
    pwsh -File scripts/probe-fundamentals.ps1 -Symbols ORLY -Kinds chart -ChartRange 2y -ChartInterval 1mo
#>
[CmdletBinding()]
param(
    [string]$SymbolFile,
    [string]$Symbols,
    [string]$Types,
    [string]$ChartRange = "2y",
    [string]$ChartInterval = "1d",
    [string]$Kinds = "timeseries,search,chart,nasdaq-earnings-date",
    [string]$CalendarFrom,
    [string]$CalendarTo,
    [string]$OutDir,
    [string]$YahooTimeseriesBaseUrl = "https://query1.finance.yahoo.com/ws/fundamentals-timeseries/v1/finance/timeseries",
    [string]$YahooSearchBaseUrl = "https://query1.finance.yahoo.com/v1/finance/search",
    [string]$YahooChartBaseUrl = "https://query1.finance.yahoo.com/v8/finance/chart",
    [string]$NasdaqBaseUrl = "https://api.nasdaq.com/api",
    [string]$UserAgent,
    [int]$TimeoutSeconds = 30,
    [int]$DelayMilliseconds = 300
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if ([string]::IsNullOrWhiteSpace($UserAgent)) {
    $WorkspaceManifest = Get-Content (Join-Path $RepoRoot "Cargo.toml")
    $Version = ($WorkspaceManifest | Select-String '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $UserAgent = "ticker-tape-tally-board-backend/$Version"
}
if ([string]::IsNullOrWhiteSpace($OutDir)) {
    $OutDir = Join-Path $RepoRoot "docs/spikes/private/fundamentals"
}
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$LatencyPath = Join-Path $OutDir "latency.csv"
if (-not (Test-Path $LatencyPath)) {
    Set-Content -Path $LatencyPath -Value "timestamp,kind,key,status,milliseconds,bytes"
}

# Every series type the spike evaluates. Ratio types are captured only to compare
# against the app's own computation; they are never used as figures.
$SeriesNames = @(
    "TotalRevenue", "OperatingRevenue",
    "DilutedEPS", "BasicEPS",
    "NetIncome", "NetIncomeCommonStockholders", "NetIncomeFromContinuingOperationNetMinorityInterest",
    "DilutedNIAvailtoComStockholders",
    "DilutedAverageShares", "BasicAverageShares", "OrdinarySharesNumber", "ShareIssued",
    "FreeCashFlow",
    "MarketCap", "EnterpriseValue", "PeRatio", "PsRatio"
)
$TypeNames = if ([string]::IsNullOrWhiteSpace($Types)) {
    foreach ($prefix in @("quarterly", "annual", "trailing")) {
        foreach ($name in $SeriesNames) { "$prefix$name" }
    }
}
else {
    @($Types -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}

Add-Type -AssemblyName System.Net.Http
$client = [System.Net.Http.HttpClient]::new()
$client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)
$client.DefaultRequestHeaders.UserAgent.ParseAdd($UserAgent)

function Save-Response {
    param([string]$Kind, [string]$Key, [string]$Url, [switch]$AcceptJson)
    $safeKey = $Key -replace '[^A-Za-z0-9._-]', '_'
    $path = Join-Path $OutDir "$Kind.$safeKey.json"
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $status = 0
    $body = ""
    try {
        $request = [System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, $Url)
        if ($AcceptJson) { $request.Headers.Accept.ParseAdd("application/json") }
        $response = $client.SendAsync($request).GetAwaiter().GetResult()
        $status = [int]$response.StatusCode
        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
    }
    catch {
        $body = "PROBE_ERROR: $($_.Exception.Message)"
    }
    $watch.Stop()
    Set-Content -Path $path -Value $body -NoNewline -Encoding utf8
    $stamp = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss.fff")
    Add-Content -Path $LatencyPath -Value "$stamp,$Kind,$Key,$status,$($watch.ElapsedMilliseconds),$($body.Length)"
    Write-Host ("{0,-20} {1,-14} HTTP {2} {3,6} ms {4,8} bytes" -f $Kind, $Key, $status, $watch.ElapsedMilliseconds, $body.Length)
    Start-Sleep -Milliseconds $DelayMilliseconds
}

$entries = @()
if (-not [string]::IsNullOrWhiteSpace($SymbolFile)) {
    $entries += Import-Csv $SymbolFile | ForEach-Object { [pscustomobject]@{ Symbol = $_.symbol; Isin = $_.isin } }
}
if (-not [string]::IsNullOrWhiteSpace($Symbols)) {
    $entries += $Symbols -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ } |
        ForEach-Object { [pscustomobject]@{ Symbol = $_; Isin = $null } }
}

$period2 = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
$period1 = [DateTimeOffset]::new(2015, 1, 1, 0, 0, 0, [TimeSpan]::Zero).ToUnixTimeSeconds()
$typeList = [uri]::EscapeDataString($TypeNames -join ",")
$enabledKinds = @($Kinds -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })

Write-Host "probe started symbols=$($entries.Count) out=$OutDir user_agent=$UserAgent"

foreach ($entry in $entries) {
    $symbol = $entry.Symbol
    if ([string]::IsNullOrWhiteSpace($symbol)) { continue }
    $escaped = [uri]::EscapeDataString($symbol)

    if ($enabledKinds -contains "timeseries") {
        Save-Response "timeseries" $symbol ("{0}/{1}?symbol={1}&type={2}&period1={3}&period2={4}" -f $YahooTimeseriesBaseUrl.TrimEnd('/'), $escaped, $typeList, $period1, $period2)
    }

    $query = if ([string]::IsNullOrWhiteSpace($entry.Isin)) { $symbol } else { $entry.Isin }
    if ($enabledKinds -contains "search") {
        Save-Response "search" $query ("{0}?q={1}&quotesCount=10&newsCount=0" -f $YahooSearchBaseUrl.TrimEnd('/'), [uri]::EscapeDataString($query))
    }

    if ($enabledKinds -contains "chart") {
        Save-Response "chart" $symbol ("{0}/{1}?range={2}&interval={3}&events=splits" -f $YahooChartBaseUrl.TrimEnd('/'), $escaped, $ChartRange, $ChartInterval)
    }

    if ($enabledKinds -contains "nasdaq-earnings-date" -and $symbol -notmatch '\.') {
        Save-Response "nasdaq-earnings-date" $symbol ("{0}/analyst/{1}/earnings-date" -f $NasdaqBaseUrl.TrimEnd('/'), $escaped) -AcceptJson
    }
}

if (-not [string]::IsNullOrWhiteSpace($CalendarFrom)) {
    $day = [datetime]::ParseExact($CalendarFrom, "yyyy-MM-dd", $null)
    $last = if ([string]::IsNullOrWhiteSpace($CalendarTo)) { $day } else { [datetime]::ParseExact($CalendarTo, "yyyy-MM-dd", $null) }
    while ($day -le $last) {
        if ($day.DayOfWeek -ne [DayOfWeek]::Saturday -and $day.DayOfWeek -ne [DayOfWeek]::Sunday) {
            $date = $day.ToString("yyyy-MM-dd")
            Save-Response "nasdaq-calendar" $date ("{0}/calendar/earnings?date={1}" -f $NasdaqBaseUrl.TrimEnd('/'), $date) -AcceptJson
        }
        $day = $day.AddDays(1)
    }
}

Write-Host "probe finished"
