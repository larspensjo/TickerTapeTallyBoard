<#
.SYNOPSIS
    Probes the Nasdaq Nordic search and price-history contracts and checks split adjustment against Yahoo.

.DESCRIPTION
    This is a live endpoint-shape probe for the Nasdaq Nordic provider. It checks
    the pinned AVA Samsung and Ericsson searches, AstraZeneca and AVA price
    history, the typed not-found response, and the Investor B split-adjustment
    cross-check. Results are written to .local/logs/.

.EXAMPLE
    pwsh -File scripts/probe-nasdaq-nordic.ps1
    pwsh -File scripts/probe-nasdaq-nordic.ps1 -TimeoutSeconds 30
#>
[CmdletBinding()]
param(
    [string]$NasdaqBaseUrl = $(if ($env:TTTB_NASDAQ_BASE_URL) { $env:TTTB_NASDAQ_BASE_URL } else { "https://api.nasdaq.com/api/nordic" }),
    [string]$YahooBaseUrl = "https://query1.finance.yahoo.com/v8/finance/chart",
    [string]$UserAgent,
    [int]$TimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$CargoManifest = Get-Content (Join-Path $RepoRoot "backend/Cargo.toml")
if ([string]::IsNullOrWhiteSpace($UserAgent)) {
    $PackageName = ($CargoManifest | Select-String '^name\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $PackageVersion = ($CargoManifest | Select-String '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $UserAgent = "$PackageName/$PackageVersion"
}
$LogDir = Join-Path $RepoRoot ".local/logs"
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$LogPath = Join-Path $LogDir "nasdaq-nordic-probe.log"

Add-Type -AssemblyName System.Net.Http
$client = [System.Net.Http.HttpClient]::new()
$client.Timeout = [TimeSpan]::FromSeconds($TimeoutSeconds)
$client.DefaultRequestHeaders.UserAgent.ParseAdd($UserAgent)

$checks = 0
$failures = 0

function Write-Line {
    param([string]$Text)
    $stamp = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss.fff")
    $line = "$stamp  $Text"
    Write-Host $line
    Add-Content -Path $LogPath -Value $line
}

function Get-JsonResponse {
    param([string]$Url)
    try {
        $response = $client.GetAsync($Url).GetAwaiter().GetResult()
        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        $json = $null
        if (-not [string]::IsNullOrWhiteSpace($body)) {
            try { $json = $body | ConvertFrom-Json } catch { $json = $null }
        }
        return @{ StatusCode = [int]$response.StatusCode; Body = $body; Json = $json }
    }
    catch {
        return @{ StatusCode = 0; Body = ""; Json = $null; Error = $_.Exception.Message }
    }
}

function Assert-Check {
    param([bool]$Condition, [string]$Message)
    $script:checks++
    if ($Condition) {
        Write-Line "PASS $Message"
    }
    else {
        $script:failures++
        Write-Line "FAIL $Message"
    }
}

function Property-Exists {
    param($Value, [string]$Name)
    return $null -ne $Value -and $null -ne $Value.PSObject.Properties[$Name]
}

function Nasdaq-Url {
    param([string]$Path, [hashtable]$Query)
    $pairs = foreach ($entry in $Query.GetEnumerator()) {
        "{0}={1}" -f [uri]::EscapeDataString($entry.Key), [uri]::EscapeDataString([string]$entry.Value)
    }
    return "{0}/{1}?{2}" -f $NasdaqBaseUrl.TrimEnd('/'), $Path.TrimStart('/'), ($pairs -join "&")
}

function Parse-NasdaqNumber {
    param([string]$Value)
    $normalized = $Value -replace ",| |\u00A0", ""
    return [decimal]::Parse($normalized, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Check-SearchShape {
    param($Response, [string]$Label, [int]$ExpectedCount = 1)
    Assert-Check ($Response.StatusCode -eq 200) "$Label returns HTTP 200"
    Assert-Check ($null -ne $Response.Json -and $null -ne $Response.Json.data) "$Label has data"
    $groups = @($Response.Json.data)
    foreach ($group in $groups) {
        Assert-Check (Property-Exists $group "group") "$Label result has group"
    }
    $instruments = @($groups | ForEach-Object { $_.instruments })
    Assert-Check ($instruments.Count -eq $ExpectedCount) "$Label has $ExpectedCount instrument match(es)"
    foreach ($instrument in $instruments) {
        foreach ($field in @("orderbookId", "isin", "fullName", "assetClass", "currency")) {
            Assert-Check (Property-Exists $instrument $field) "$Label instrument has $field"
        }
    }
}

function Check-NoMatchSearchShape {
    param($Response, [string]$Label)
    Assert-Check ($Response.StatusCode -eq 200) "$Label returns HTTP 200"
    Assert-Check ($null -ne $Response.Json) "$Label returns JSON"
    Assert-Check (Property-Exists $Response.Json "data") "$Label has data field"
    Assert-Check ($Response.Json.status.rCode -eq 200) "$Label status rCode is 200"
    Assert-Check ($null -eq $Response.Json.status.bCodeMessage) "$Label status bCodeMessage is null"
    Assert-Check ($null -eq $Response.Json.data) "$Label data is null"
    Assert-Check ($Response.Json.messages.code -eq "NO_INST_FOUND") "$Label message code is NO_INST_FOUND"
}

function Check-PriceShape {
    param($Response, [string]$Label)
    Assert-Check ($Response.StatusCode -eq 200) "$Label returns HTTP 200"
    $rows = @($Response.Json.data.priceHistory.rows)
    Assert-Check ($rows.Count -gt 0) "$Label has price-history rows"
    if ($rows.Count -gt 0) {
        foreach ($field in @("date", "close")) {
            Assert-Check (Property-Exists $rows[0] $field) "$Label row has $field"
        }
    }
}

Write-Line "probe started nasdaq=$NasdaqBaseUrl yahoo=$YahooBaseUrl user_agent=$UserAgent timeout=${TimeoutSeconds}s"

try {
    $avaSearch = Get-JsonResponse (Nasdaq-Url "search" @{ searchText = "JE00BJ7HNC92" })
    Check-SearchShape $avaSearch "AVA Samsung search"

    $ericssonSearch = Get-JsonResponse (Nasdaq-Url "search" @{ searchText = "SE0000108656" })
    Check-SearchShape $ericssonSearch "Ericsson search" 2
    if (@($ericssonSearch.Json.data | ForEach-Object { $_.instruments }).Count -eq 2) {
        $ericssonInstruments = @($ericssonSearch.Json.data | ForEach-Object { $_.instruments })
        Assert-Check ($ericssonInstruments[0].currency -ne $ericssonInstruments[1].currency) "Ericsson matches have distinct currencies"
        Assert-Check ($ericssonInstruments[0].orderbookId -ne $ericssonInstruments[1].orderbookId) "Ericsson matches have distinct orderbook ids"
    }

    $noMatchSearch = Get-JsonResponse (Nasdaq-Url "search" @{ searchText = "ZZ0000000000" })
    Check-NoMatchSearchShape $noMatchSearch "nonsense ISIN search"

    $astraPrice = Get-JsonResponse (Nasdaq-Url "instruments/TX271/price-history" @{ assetClass = "SHARES"; fromDate = "2025-06-02"; toDate = "2026-08-27" })
    Check-PriceShape $astraPrice "AstraZeneca price history"
    $avaPrice = Get-JsonResponse (Nasdaq-Url "instruments/TX2997672/price-history" @{ assetClass = "TRACKER_CERTIFICATES"; fromDate = "2025-01-02"; toDate = "2026-08-27" })
    Check-PriceShape $avaPrice "AVA Samsung price history"

    $notFound = Get-JsonResponse (Nasdaq-Url "instruments/TX-NOT-FOUND/price-history" @{ assetClass = "SHARES"; fromDate = "2021-03-01"; toDate = "2021-08-31" })
    Assert-Check ($notFound.StatusCode -ge 400) "not-found history returns an error HTTP status"
    Assert-Check ($null -ne $notFound.Json.status -and (Property-Exists $notFound.Json.status "rCode")) "not-found response has status.rCode"
    Assert-Check ($null -ne $notFound.Json.status.bCodeMessage -and @($notFound.Json.status.bCodeMessage).Count -gt 0) "not-found response has status.bCodeMessage"
    if ($null -ne $notFound.Json.status.bCodeMessage -and @($notFound.Json.status.bCodeMessage).Count -gt 0) {
        Assert-Check (Property-Exists $notFound.Json.status.bCodeMessage[0] "errorMessage") "not-found message has errorMessage"
    }

    $nasdaqSplit = Get-JsonResponse (Nasdaq-Url "instruments/TX76/price-history" @{ assetClass = "SHARES"; fromDate = "2021-03-01"; toDate = "2021-08-31" })
    Check-PriceShape $nasdaqSplit "Investor B Nasdaq price history"
    $yahooSplitUrl = "$($YahooBaseUrl.TrimEnd('/'))/INVE-B.ST?period1=1614556800&period2=1630454400&interval=1d&events=div%2Csplits"
    $yahooSplit = Get-JsonResponse $yahooSplitUrl
    Assert-Check ($yahooSplit.StatusCode -eq 200) "Investor B Yahoo history returns HTTP 200"

    $yahooByDate = @{}
    if ($null -ne $yahooSplit.Json.chart.result -and @($yahooSplit.Json.chart.result).Count -gt 0) {
        $result = $yahooSplit.Json.chart.result[0]
        $offset = [int64]$result.meta.gmtoffset
        $timestamps = @($result.timestamp)
        $closes = @($result.indicators.quote[0].close)
        for ($index = 0; $index -lt $timestamps.Count; $index++) {
            if ($null -ne $closes[$index]) {
                $date = [DateTimeOffset]::FromUnixTimeSeconds([int64]$timestamps[$index]).ToOffset([TimeSpan]::FromSeconds($offset)).Date.ToString("yyyy-MM-dd")
                $yahooByDate[$date] = [decimal]$closes[$index]
            }
        }
    }

    $overlap = 0
    $mismatches = 0
    foreach ($row in @($nasdaqSplit.Json.data.priceHistory.rows)) {
        if ($null -eq $yahooByDate[$row.date] -or [string]::IsNullOrEmpty($row.close)) { continue }
        $overlap++
        $nasdaqClose = Parse-NasdaqNumber $row.close
        $yahooClose = $yahooByDate[$row.date]
        if ([math]::Abs([double]($nasdaqClose - $yahooClose)) -gt [math]::Abs([double]$yahooClose) * 0.001) {
            $mismatches++
        }
    }
    Assert-Check ($overlap -ge 100) "Investor B split comparison has substantial overlap ($overlap days)"
    Assert-Check ($mismatches -eq 0) "Investor B Nasdaq and Yahoo closes agree within 0.1% ($mismatches mismatches)"
}
catch {
    $failures++
    Write-Line "FAIL unexpected probe error: $($_.Exception.Message)"
}
finally {
    $client.Dispose()
}

Write-Line "summary checks=$checks failures=$failures"
if ($failures -gt 0) { exit 1 }
exit 0
