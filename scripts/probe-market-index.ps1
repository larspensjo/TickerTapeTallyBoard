<#
.SYNOPSIS
    Probes market index symbols against the Yahoo chart request the app sends for prices.

.DESCRIPTION
    This is a live endpoint-shape probe. For each candidate index symbol it sends
    the chart request twice, with the query the app uses (period1, period2,
    interval=1d, events=div,splits):

      - "literal": the symbol as the app's URL builder writes a path segment
        today, which leaves "^" unencoded, over a long range;
      - "encoded": the symbol fully percent-encoded ("^" as "%5E"), over the
        app's latest-refresh window.

    It records the HTTP status, whether closes and a currency are present (the
    app rejects a response without a currency), the UTC offset and how the bars
    map to dates the way the app maps them, the history depth and the largest
    gap. Raw responses and a summary are written to a folder per run under
    .local/logs/market-index-probe/, which is not versioned.

    The probe is deliberately small: it stops at -MaxRequests, waits
    -DelaySeconds between requests, retries a transport failure or server error
    at most once, and ends the run at the first answer that can mean throttling
    or blocking (HTTP 401, 403, 429, 503 or 999). It opens no ledger and needs
    no key.

.EXAMPLE
    pwsh -File scripts/probe-market-index.ps1
    pwsh -File scripts/probe-market-index.ps1 -Symbols '^GSPC,^OMXSPI'
#>
#Requires -Version 7
[CmdletBinding()]
param(
    [string[]]$Symbols = @('^GSPC', '^OMXSPI', '^OMX', '^STOXX', '^AEX'),
    [string]$YahooBaseUrl = "https://query1.finance.yahoo.com/v8/finance/chart",
    [string]$UserAgent,
    [ValidateRange(1, 15)]
    [int]$MaxRequests = 15,
    [ValidateRange(1, 60)]
    [int]$DelaySeconds = 4,
    [int]$LongDays = 400,
    [int]$LatestDays = 37,
    [int]$TimeoutSeconds = 20
)

$ErrorActionPreference = "Stop"

# "pwsh -File" passes a comma-separated list as one string, so split it here.
$Symbols = @($Symbols | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if ([string]::IsNullOrWhiteSpace($UserAgent)) {
    # The app's user agent is "<backend package name>/<workspace version>".
    $PackageName = (Get-Content (Join-Path $RepoRoot "backend/Cargo.toml") |
        Select-String '^name\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $PackageVersion = (Get-Content (Join-Path $RepoRoot "Cargo.toml") |
        Select-String '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $UserAgent = "$PackageName/$PackageVersion"
}
$LogDir = Join-Path $RepoRoot ".local/logs"
# One folder per run, so a later run never overwrites an earlier run's responses.
$OutDir = Join-Path $LogDir ("market-index-probe/{0}" -f (Get-Date).ToString("yyyyMMdd-HHmmss"))
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$LogPath = Join-Path $LogDir "market-index-probe.log"

$script:requests = 0
$script:throttled = $false

function Write-Line {
    param([string]$Text)
    $stamp = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ss.fff")
    $line = "$stamp  $Text"
    Write-Host $line
    Add-Content -Path $LogPath -Value $line
}

function Get-SafeName {
    param([string]$Symbol)
    return ($Symbol -replace '[^A-Za-z0-9]', '_').Trim('_').ToLowerInvariant()
}

# The characters the app's URL builder percent-encodes in a path segment
# (controls, space, " < > ` # ? { } / % \ and non-ASCII). "^" is not among them.
function ConvertTo-AppPathSegment {
    param([string]$Symbol)
    $builder = [System.Text.StringBuilder]::new()
    foreach ($byte in [System.Text.Encoding]::UTF8.GetBytes($Symbol)) {
        $char = [char]$byte
        if ($byte -le 0x20 -or $byte -ge 0x7F -or '"<>`#?{}/%\'.Contains($char)) {
            [void]$builder.AppendFormat("%{0:X2}", $byte)
        }
        else {
            [void]$builder.Append($char)
        }
    }
    return $builder.ToString()
}

function Get-ChartUrl {
    param([string]$Segment, [datetime]$Start, [datetime]$End)
    $epoch = [datetime]::new(1970, 1, 1, 0, 0, 0, [System.DateTimeKind]::Utc)
    $period1 = [long]($Start.Date - $epoch.Date).TotalSeconds
    $period2 = [long]($End.Date - $epoch.Date).TotalSeconds + 86400
    return "{0}/{1}?period1={2}&period2={3}&interval=1d&events=div%2Csplits" -f `
        $YahooBaseUrl.TrimEnd('/'), $Segment, $period1, $period2
}

function Invoke-Chart {
    param([string]$Url, [string]$BodyPath)
    if ($script:requests -ge $MaxRequests) {
        return @{ Skipped = $true; Reason = "request cap of $MaxRequests reached" }
    }
    if ($script:requests -gt 0) { Start-Sleep -Seconds $DelaySeconds }
    $script:requests++
    if (Test-Path $BodyPath) { Remove-Item -LiteralPath $BodyPath -Force }
    $connect = [Math]::Min(10, $TimeoutSeconds)
    $written = & curl.exe --silent --show-error --globoff --path-as-is `
        --max-time $TimeoutSeconds --connect-timeout $connect `
        --user-agent $UserAgent --output $BodyPath `
        --write-out "%{http_code}|%{http_version}|%{url_effective}" $Url 2>&1
    $text = ($written | Out-String).Trim()
    $parts = $text.Split('|')
    $status = 0
    if ($parts.Count -ge 3) { [void][int]::TryParse($parts[0], [ref]$status) }
    $body = if (Test-Path $BodyPath) { Get-Content -LiteralPath $BodyPath -Raw } else { "" }
    return @{
        Skipped      = $false
        StatusCode   = $status
        HttpVersion  = if ($parts.Count -ge 3) { $parts[1] } else { "" }
        EffectiveUrl = if ($parts.Count -ge 3) { $parts[2] } else { "" }
        Transport    = if ($status -eq 0) { $text } else { "" }
        Body         = $body
    }
}

# Any answer that can mean "slow down" or "blocked" ends the run: no retry, no further requests.
function Test-Throttled {
    param($Response)
    if ($Response.StatusCode -in 401, 403, 429, 503, 999) { return $true }
    return $Response.Body -match '(?i)too many requests|rate limit'
}

function Get-WeekdaysBetween {
    param([datetime]$From, [datetime]$To)
    $count = 0
    $day = $From.AddDays(1)
    while ($day -lt $To) {
        if ($day.DayOfWeek -notin 'Saturday', 'Sunday') { $count++ }
        $day = $day.AddDays(1)
    }
    return $count
}

function Get-ChartFacts {
    param([string]$Body)
    $facts = [ordered]@{ parsed = $false }
    try { $json = $Body | ConvertFrom-Json -Depth 20 } catch { $facts.error = "not JSON"; return $facts }
    if ($null -eq $json.chart) { $facts.error = "no chart object"; return $facts }
    if ($null -ne $json.chart.error) {
        $facts.error = "{0}: {1}" -f $json.chart.error.code, $json.chart.error.description
        return $facts
    }
    $result = @($json.chart.result)[0]
    if ($null -eq $result) { $facts.error = "no result"; return $facts }
    $facts.parsed = $true
    $meta = $result.meta
    $facts.meta_symbol = $meta.symbol
    $facts.currency = $meta.currency
    $facts.gmtoffset = $meta.gmtoffset
    $facts.exchange = $meta.exchangeName
    $facts.exchange_full = $meta.fullExchangeName
    $facts.timezone = $meta.exchangeTimezoneName
    $facts.instrument_type = $meta.instrumentType
    $facts.name = if ($meta.longName) { $meta.longName } else { $meta.shortName }
    $facts.event_kinds = if ($null -ne $result.events) { @($result.events.PSObject.Properties.Name) -join "," } else { "" }

    $timestamps = @($result.timestamp)
    $closes = @(@($result.indicators.quote)[0].close)
    $offset = if ($null -ne $meta.gmtoffset) { [long]$meta.gmtoffset } else { 0 }
    $epoch = [datetime]::new(1970, 1, 1, 0, 0, 0, [System.DateTimeKind]::Utc)
    $dates = [System.Collections.Generic.List[datetime]]::new()
    $nulls = 0
    for ($i = 0; $i -lt $timestamps.Count; $i++) {
        if ($i -ge $closes.Count -or $null -eq $closes[$i]) { $nulls++; continue }
        # The app's date for a bar: the UTC date of timestamp + gmtoffset.
        $dates.Add($epoch.AddSeconds([long]$timestamps[$i] + $offset).Date)
    }
    $facts.bars = $timestamps.Count
    $facts.closes = $dates.Count
    $facts.null_closes = $nulls
    $facts.app_accepts = (-not [string]::IsNullOrEmpty($meta.currency)) -and $dates.Count -gt 0
    if ($dates.Count -gt 0) {
        $sorted = @($dates | Sort-Object)
        $facts.first_date = $sorted[0].ToString("yyyy-MM-dd")
        $facts.last_date = $sorted[-1].ToString("yyyy-MM-dd")
        $facts.duplicate_dates = $sorted.Count - @($sorted | Select-Object -Unique).Count
        $facts.weekend_dates = @($sorted | Where-Object { $_.DayOfWeek -in 'Saturday', 'Sunday' }).Count
        $maxGap = 0
        $gapStart = $null
        for ($i = 1; $i -lt $sorted.Count; $i++) {
            $gap = Get-WeekdaysBetween $sorted[$i - 1] $sorted[$i]
            if ($gap -gt $maxGap) { $maxGap = $gap; $gapStart = $sorted[$i - 1] }
        }
        $facts.max_missing_weekdays = $maxGap
        $facts.max_gap_after = if ($gapStart) { $gapStart.ToString("yyyy-MM-dd") } else { "" }
        # Local time of day of the bars, to tell session-open stamps from a live last bar.
        $times = @($timestamps | ForEach-Object { $epoch.AddSeconds([long]$_ + $offset).ToString("HH:mm") })
        $facts.usual_bar_time = ($times | Group-Object | Sort-Object Count -Descending | Select-Object -First 1).Name
        $facts.last_bar_time = $times[-1]
        if ($null -ne $meta.regularMarketTime) {
            $facts.market_time_local = $epoch.AddSeconds([long]$meta.regularMarketTime + $offset).ToString("yyyy-MM-dd HH:mm")
        }
    }
    return $facts
}

$today = (Get-Date).Date
$summary = [ordered]@{
    run_at      = (Get-Date).ToString("yyyy-MM-ddTHH:mm:ssK")
    user_agent  = $UserAgent
    base_url    = $YahooBaseUrl
    long_days   = $LongDays
    latest_days = $LatestDays
    symbols     = [ordered]@{}
}

Write-Line "probe started yahoo=$YahooBaseUrl user_agent=$UserAgent max_requests=$MaxRequests delay=${DelaySeconds}s"

foreach ($symbol in $Symbols) {
    if ($script:throttled) { break }
    $safe = Get-SafeName $symbol
    $forms = @(
        @{ Name = "literal"; Segment = (ConvertTo-AppPathSegment $symbol); Days = $LongDays },
        @{ Name = "encoded"; Segment = [uri]::EscapeDataString($symbol); Days = $LatestDays }
    )
    $entry = [ordered]@{}
    foreach ($form in $forms) {
        if ($script:throttled) { break }
        $url = Get-ChartUrl -Segment $form.Segment -Start $today.AddDays(-$form.Days) -End $today
        $bodyPath = Join-Path $OutDir ("{0}_{1}.json" -f $safe, $form.Name)
        $response = Invoke-Chart -Url $url -BodyPath $bodyPath
        if ($response.Skipped) {
            Write-Line "SKIP $symbol $($form.Name): $($response.Reason)"
            $entry[$form.Name] = [ordered]@{ skipped = $response.Reason }
            continue
        }
        if (Test-Throttled $response) {
            $script:throttled = $true
            Write-Line "STOP $symbol $($form.Name): throttled (HTTP $($response.StatusCode)); no further requests"
            $entry[$form.Name] = [ordered]@{ http_status = $response.StatusCode; throttled = $true }
            break
        }
        $attempts = 1
        if ($response.StatusCode -eq 0 -or $response.StatusCode -ge 500) {
            Write-Line "RETRY $symbol $($form.Name): HTTP $($response.StatusCode) $($response.Transport)"
            $retry = Invoke-Chart -Url $url -BodyPath $bodyPath
            if (-not $retry.Skipped) {
                $response = $retry
                $attempts = 2
                if (Test-Throttled $response) {
                    $script:throttled = $true
                    Write-Line "STOP $symbol $($form.Name): throttled on retry; no further requests"
                    break
                }
            }
        }
        $facts = Get-ChartFacts $response.Body
        $record = [ordered]@{
            path_segment  = $form.Segment
            range_days    = $form.Days
            attempts      = $attempts
            http_status   = $response.StatusCode
            http_version  = $response.HttpVersion
            effective_url = $response.EffectiveUrl
            transport     = $response.Transport
        }
        foreach ($key in $facts.Keys) { $record[$key] = $facts[$key] }
        $entry[$form.Name] = $record
        if ($facts.parsed) {
            Write-Line ("OK   {0} {1}: HTTP {2}; closes {3} (null {4}); currency {5}; gmtoffset {6}; {7}..{8}; max missing weekdays {9}; app accepts {10}" -f `
                    $symbol, $form.Name, $response.StatusCode, $facts.closes, $facts.null_closes, $facts.currency,
                $facts.gmtoffset, $facts.first_date, $facts.last_date, $facts.max_missing_weekdays, $facts.app_accepts)
        }
        else {
            Write-Line ("FAIL {0} {1}: HTTP {2}; {3} {4}" -f $symbol, $form.Name, $response.StatusCode, $facts.error, $response.Transport)
        }
    }
    $summary.symbols[$symbol] = $entry
}

$summary.requests = $script:requests
$summary.throttled = $script:throttled
$summaryPath = Join-Path $OutDir "summary.json"
$summary | ConvertTo-Json -Depth 8 | Set-Content -Path $summaryPath -Encoding utf8
Write-Line "probe finished requests=$($script:requests) throttled=$($script:throttled) summary=$summaryPath"
if ($script:throttled) { exit 2 }
