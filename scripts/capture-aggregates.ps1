<#
.SYNOPSIS
    Captures valuation API responses or compares two aggregate captures.

.DESCRIPTION
    Capture mode (the default) requests the gains, value-history, and holdings
    endpoints and writes their raw JSON responses to a timestamped directory
    under .local/aggregates/.

    The gains request is pinned to include_closed=true, method=xirr, and an
    explicit end_date (today by default). Pass an explicit -EndDate, and the
    same -StartDate and -Method when applicable, when creating captures that
    will be compared. Diff mode prints both captures' gains metadata and refuses
    to continue unless their report periods, percentage methods, closed-position
    settings, and base currencies match. A differing as-of date is informational.
    Holdings is captured with include_watchlist=false. It is validated and
    retained for the record, but is intentionally not diffed by this tool.

    Diff mode is selected by supplying -BeforeDirectory and -AfterDirectory.
    It reports added, removed, changed, and (optionally) unchanged gains rows
    and value-history points. Gains rows compare market_value_base,
    cost_basis_base, unrealized_gain_base, and total_return_base. Availability
    transitions are printed explicitly; an unavailable value is never rendered
    as a blank. All three files must be present and valid in both directories
    before diffing starts.

    The backend URL defaults to http://127.0.0.1:<port>, where the port comes
    from TTTB_PORT or 8480. Override it with -BaseUrl or -Port when capturing
    from a second backend instance backed by a restored database copy.

.EXAMPLE
    pwsh -File scripts/capture-aggregates.ps1

.EXAMPLE
    pwsh -File scripts/capture-aggregates.ps1 -Port 8081 -EndDate 2026-08-28

.EXAMPLE
    pwsh -File scripts/capture-aggregates.ps1 -BeforeDirectory .local/aggregates/capture-before -AfterDirectory .local/aggregates/capture-after

.EXAMPLE
    pwsh -File scripts/capture-aggregates.ps1 -BeforeDirectory .local/aggregates/capture-before -AfterDirectory .local/aggregates/capture-after -ShowUnchanged
#>
[CmdletBinding(DefaultParameterSetName = "Capture")]
param(
    [Parameter(ParameterSetName = "Capture")]
    [string]$BaseUrl,

    [Parameter(ParameterSetName = "Capture")]
    [ValidateRange(1, 65535)]
    [int]$Port = $(if ($env:TTTB_PORT) { [int]$env:TTTB_PORT } else { 8480 }),

    [Parameter(ParameterSetName = "Capture")]
    [ValidatePattern('^\d{4}-\d{2}-\d{2}$')]
    [string]$StartDate,

    [Parameter(ParameterSetName = "Capture")]
    [ValidatePattern('^\d{4}-\d{2}-\d{2}$')]
    [string]$EndDate = (Get-Date).ToString("yyyy-MM-dd"),

    [Parameter(ParameterSetName = "Capture")]
    [ValidateSet("xirr", "simple", "modified_dietz")]
    [string]$Method = "xirr",

    [Parameter(ParameterSetName = "Capture")]
    [string]$UserAgent,

    [Parameter(ParameterSetName = "Capture")]
    [ValidateRange(1, 300)]
    [int]$TimeoutSeconds = 30,

    [Parameter(ParameterSetName = "Diff", Mandatory = $true, Position = 0)]
    [ValidateNotNullOrEmpty()]
    [string]$BeforeDirectory,

    [Parameter(ParameterSetName = "Diff", Mandatory = $true, Position = 1)]
    [ValidateNotNullOrEmpty()]
    [string]$AfterDirectory,

    [Parameter(ParameterSetName = "Diff")]
    [switch]$ShowUnchanged
)

$ErrorActionPreference = "Stop"

$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$AggregateRoot = Join-Path $RepoRoot ".local/aggregates"
$GainsDiffFields = @("market_value_base", "cost_basis_base", "unrealized_gain_base", "total_return_base")
$ValueHistoryDiffFields = @("value_base", "invested_base", "incomplete", "included_count", "excluded_count")
$GainsMetadataDefinitions = @(
    [pscustomobject]@{ Name = "as_of_date"; Path = @("as_of_date"); RefuseOnMismatch = $false }
    [pscustomobject]@{ Name = "report_period.start_date"; Path = @("report_period", "start_date"); RefuseOnMismatch = $true }
    [pscustomobject]@{ Name = "report_period.end_date"; Path = @("report_period", "end_date"); RefuseOnMismatch = $true }
    [pscustomobject]@{ Name = "percentage_method"; Path = @("percentage_method"); RefuseOnMismatch = $true }
    [pscustomobject]@{ Name = "include_closed_positions"; Path = @("include_closed_positions"); RefuseOnMismatch = $true }
    [pscustomobject]@{ Name = "base_currency"; Path = @("base_currency"); RefuseOnMismatch = $true }
)

function Get-UserAgent {
    param([string]$ExplicitUserAgent)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitUserAgent)) {
        return $ExplicitUserAgent
    }

    $cargoManifest = Get-Content (Join-Path $RepoRoot "backend/Cargo.toml")
    $packageName = ($cargoManifest | Select-String '^name\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    $packageVersion = ($cargoManifest | Select-String '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches.Groups[1].Value
    if ([string]::IsNullOrWhiteSpace($packageName) -or [string]::IsNullOrWhiteSpace($packageVersion)) {
        throw "Could not derive the backend User-Agent from backend/Cargo.toml."
    }
    return "$packageName/$packageVersion"
}

function Get-EndpointDefinitions {
    # This table is the single source of truth for both capture filenames and
    # diff coverage. Do not add an endpoint to only one of those code paths.
    return @(
        [pscustomobject]@{
            Name     = "gains"
            FileName = "gains.json"
            Path     = "/api/gains"
            ArrayKey = "rows"
        }
        [pscustomobject]@{
            Name     = "value-history"
            FileName = "value-history.json"
            Path     = "/api/portfolio/value-history"
            ArrayKey = "points"
        }
        [pscustomobject]@{
            Name     = "holdings"
            FileName = "holdings.json"
            Path     = "/api/holdings"
            ArrayKey = "holdings"
        }
    )
}

function Get-CaptureEndpointDefinitions {
    param(
        [string]$GainsStartDate,
        [string]$GainsEndDate,
        [string]$GainsMethod
    )

    $gainsQuery = [ordered]@{
        include_closed = "true"
        end_date       = $GainsEndDate
        method         = $GainsMethod
    }
    if (-not [string]::IsNullOrWhiteSpace($GainsStartDate)) {
        $gainsQuery.start_date = $GainsStartDate
    }

    foreach ($endpoint in Get-EndpointDefinitions) {
        $query = switch ($endpoint.Name) {
            "gains" { $gainsQuery }
            "holdings" { [ordered]@{ include_watchlist = "false" } }
            default { [ordered]@{} }
        }
        [pscustomobject]@{
            Name     = $endpoint.Name
            FileName = $endpoint.FileName
            Path     = $endpoint.Path
            Query    = $query
            ArrayKey = $endpoint.ArrayKey
        }
    }
}

function ConvertTo-QueryString {
    param([System.Collections.IDictionary]$Query)

    if ($Query.Count -eq 0) {
        return ""
    }

    $pairs = foreach ($entry in $Query.GetEnumerator()) {
        "{0}={1}" -f [uri]::EscapeDataString($entry.Key), [uri]::EscapeDataString([string]$entry.Value)
    }
    return "?" + ($pairs -join "&")
}

function Get-EndpointUrl {
    param(
        [string]$EndpointBaseUrl,
        $Endpoint
    )

    return "{0}{1}{2}" -f $EndpointBaseUrl.TrimEnd('/'), $Endpoint.Path, (ConvertTo-QueryString $Endpoint.Query)
}

function New-HttpClient {
    param(
        [string]$Agent,
        [int]$Timeout
    )

    Add-Type -AssemblyName System.Net.Http
    $client = [System.Net.Http.HttpClient]::new()
    $client.Timeout = [TimeSpan]::FromSeconds($Timeout)
    $client.DefaultRequestHeaders.UserAgent.ParseAdd($Agent)
    return $client
}

function Get-RawJson {
    param(
        [System.Net.Http.HttpClient]$Client,
        [string]$Url,
        [string]$EndpointName
    )

    $response = $null
    try {
        $response = $Client.GetAsync($Url).GetAwaiter().GetResult()
        $body = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        if (-not $response.IsSuccessStatusCode) {
            throw "$EndpointName returned HTTP $([int]$response.StatusCode) $($response.ReasonPhrase). Body: $body"
        }
        if ([string]::IsNullOrWhiteSpace($body)) {
            throw "$EndpointName returned an empty response body."
        }
        try {
            $null = ConvertFrom-Json -InputObject $body
        }
        catch {
            throw "$EndpointName returned invalid JSON: $($_.Exception.Message)"
        }
        return $body
    }
    catch {
        throw "Request for $EndpointName failed at $Url. $($_.Exception.Message)"
    }
    finally {
        if ($null -ne $response) {
            $response.Dispose()
        }
    }
}

function Write-Utf8File {
    param(
        [string]$Path,
        [string]$Content
    )

    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

function Invoke-Capture {
    param(
        [string]$EndpointBaseUrl,
        [string]$Agent,
        [int]$Timeout,
        $Endpoints
    )

    $captureStamp = (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssfffZ")
    $captureDirectory = Join-Path $AggregateRoot "capture-$captureStamp"
    $captureDirectoryCreated = $false
    $suffix = 1
    while (Test-Path -LiteralPath $captureDirectory) {
        $captureDirectory = Join-Path $AggregateRoot "capture-$captureStamp-$suffix"
        $suffix++
    }

    $client = New-HttpClient -Agent $Agent -Timeout $Timeout
    try {
        Write-Host "Capturing aggregates from $EndpointBaseUrl into $captureDirectory"
        foreach ($endpoint in $Endpoints) {
            $url = Get-EndpointUrl -EndpointBaseUrl $EndpointBaseUrl -Endpoint $endpoint
            $body = Get-RawJson -Client $client -Url $url -EndpointName $endpoint.Name
            if (-not $captureDirectoryCreated) {
                New-Item -ItemType Directory -Force -Path $captureDirectory | Out-Null
                $captureDirectoryCreated = $true
            }
            $path = Join-Path $captureDirectory $endpoint.FileName
            Write-Utf8File -Path $path -Content $body
            Write-Host "  wrote $($endpoint.FileName)"
        }
        Write-Host "Capture complete: $captureDirectory"
        return $captureDirectory
    }
    catch {
        $failureMessage = $_.Exception.Message
        if ($captureDirectoryCreated -and (Test-Path -LiteralPath $captureDirectory)) {
            Remove-Item -LiteralPath $captureDirectory -Recurse -Force
        }
        Write-Error "CAPTURE FAILED. Removed the incomplete capture directory (if created): $captureDirectory. $failureMessage"
        throw
    }
    finally {
        $client.Dispose()
    }
}

function Get-RequiredProperty {
    param(
        $Object,
        [string]$Name,
        [string]$Context
    )

    if ($null -eq $Object) {
        throw "$Context is null; expected property '$Name'."
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        throw "$Context is missing required property '$Name'."
    }
    return $property.Value
}

function Read-CaptureSet {
    param(
        [string]$Directory,
        $Endpoints
    )

    $resolvedDirectory = Resolve-Path -LiteralPath $Directory -ErrorAction Stop
    if (-not (Test-Path -LiteralPath $resolvedDirectory -PathType Container)) {
        throw "Capture path is not a directory: $Directory"
    }

    $responses = @{}
    $missingFiles = @()
    foreach ($endpoint in $Endpoints) {
        $path = Join-Path $resolvedDirectory $endpoint.FileName
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $missingFiles += $endpoint.FileName
            continue
        }

        $raw = Get-Content -LiteralPath $path -Raw
        if ([string]::IsNullOrWhiteSpace($raw)) {
            throw "Capture '$resolvedDirectory' has an empty file: $($endpoint.FileName)"
        }
        try {
            $responses[$endpoint.Name] = ConvertFrom-Json -InputObject $raw
        }
        catch {
            throw "Capture '$resolvedDirectory' has invalid JSON in $($endpoint.FileName): $($_.Exception.Message)"
        }
    }
    if ($missingFiles.Count -gt 0) {
        throw "Capture '$resolvedDirectory' is incomplete; missing required file(s): $($missingFiles -join ', ')"
    }

    foreach ($endpoint in $Endpoints) {
        $response = $responses[$endpoint.Name]
        $arrayProperty = if ($null -eq $response) { $null } else { $response.PSObject.Properties[$endpoint.ArrayKey] }
        if ($null -eq $arrayProperty) {
            throw "$($endpoint.Name) response in '$resolvedDirectory' is missing '$($endpoint.ArrayKey)'; refusing to diff it."
        }
        if ($null -eq $arrayProperty.Value) {
            throw "$($endpoint.Name) response in '$resolvedDirectory' has null '$($endpoint.ArrayKey)'; refusing to diff it."
        }
    }

    $gainsMetadata = @{}
    foreach ($definition in $GainsMetadataDefinitions) {
        $gainsMetadata[$definition.Name] = Get-MetadataSnapshot -Object $responses["gains"] -Definition $definition
    }

    return [pscustomobject]@{
        Directory = [string]$resolvedDirectory
        Gains = $responses["gains"]
        GainsMetadata = $gainsMetadata
        ValueHistory = $responses["value-history"]
        Holdings = $responses["holdings"]
    }
}

function Get-MetadataSnapshot {
    param(
        $Object,
        $Definition
    )

    $current = $Object
    foreach ($segment in $Definition.Path) {
        if ($null -eq $current) {
            return [pscustomobject]@{
                Present = $false
                Display = "<missing>"
                Comparable = "missing"
            }
        }
        $property = $current.PSObject.Properties[$segment]
        if ($null -eq $property) {
            return [pscustomobject]@{
                Present = $false
                Display = "<missing>"
                Comparable = "missing"
            }
        }
        $current = $property.Value
    }

    if ($null -eq $current) {
        return [pscustomobject]@{
            Present = $true
            Display = "<null>"
            Comparable = "null"
        }
    }

    $serialized = ConvertTo-Json -InputObject $current -Compress -Depth 10
    return [pscustomobject]@{
        Present = $true
        Display = $serialized
        Comparable = $serialized
    }
}

function Get-AvailabilitySnapshot {
    param($Availability)

    if ($null -eq $Availability) {
        return [pscustomobject]@{
            State = "missing"
            Display = "<missing availability>"
            Comparable = "missing"
        }
    }

    $statusProperty = $Availability.PSObject.Properties["status"]
    if ($null -eq $statusProperty -or [string]::IsNullOrWhiteSpace([string]$statusProperty.Value)) {
        return [pscustomobject]@{
            State = "missing"
            Display = "<missing availability status>"
            Comparable = "missing-status"
        }
    }

    $status = ([string]$statusProperty.Value).ToLowerInvariant()
    if ($status -eq "available") {
        $valueProperty = $Availability.PSObject.Properties["value"]
        if ($null -eq $valueProperty -or $null -eq $valueProperty.Value) {
            return [pscustomobject]@{
                State = "available"
                Display = "<available: missing value>"
                Comparable = "available|missing-value"
            }
        }
        $value = [string]$valueProperty.Value
        return [pscustomobject]@{
            State = "available"
            Display = "<available: $value>"
            Comparable = "available|$value"
        }
    }

    if ($status -eq "unavailable") {
        $reasonsProperty = $Availability.PSObject.Properties["reasons"]
        $reasons = @()
        if ($null -ne $reasonsProperty -and $null -ne $reasonsProperty.Value) {
            $reasons = @($reasonsProperty.Value | ForEach-Object { [string]$_ })
        }
        $reasonText = if ($reasons.Count -gt 0) { $reasons -join ", " } else { "no reason supplied" }
        return [pscustomobject]@{
            State = "unavailable"
            Display = "<unavailable: $reasonText>"
            Comparable = "unavailable|$($reasons -join ',')"
        }
    }

    return [pscustomobject]@{
        State = $status
        Display = "<unknown availability status: $status>"
        Comparable = "unknown|$status"
    }
}

function Get-FieldText {
    param(
        $Object,
        [string]$Name
    )

    if ($null -eq $Object -or $null -eq $Object.PSObject.Properties[$Name]) {
        return "<missing>"
    }
    if ($null -eq $Object.PSObject.Properties[$Name].Value) {
        return "<null>"
    }
    return [string]$Object.PSObject.Properties[$Name].Value
}

function Get-InstrumentKey {
    param($Row, [string]$Context)

    $instrument = Get-RequiredProperty -Object $Row -Name "instrument" -Context $Context
    $id = Get-RequiredProperty -Object $instrument -Name "id" -Context "$Context.instrument"
    if ([string]::IsNullOrWhiteSpace([string]$id)) {
        throw "$Context.instrument.id is empty."
    }
    return [string]$id
}

function Convert-RowsToMap {
    param(
        $Rows,
        [string]$Directory
    )

    $map = @{}
    foreach ($row in @($Rows)) {
        $key = Get-InstrumentKey -Row $row -Context "Gains row in '$Directory'"
        if ($map.ContainsKey($key)) {
            throw "Capture '$Directory' contains duplicate gains rows for instrument id $key."
        }
        $map[$key] = $row
        foreach ($field in $GainsDiffFields) {
            $null = Get-RequiredProperty -Object $row -Name $field -Context "Gains row $key in '$Directory'"
        }
    }
    return $map
}

function Get-InstrumentLabel {
    param($Row, [string]$Key)

    $instrument = $Row.instrument
    $symbol = Get-FieldText -Object $instrument -Name "symbol"
    $name = Get-FieldText -Object $instrument -Name "name"
    return "#$Key $symbol - $name"
}

function Write-GainFields {
    param(
        $BeforeRow,
        $AfterRow,
        [string]$Mode
    )

    foreach ($field in $GainsDiffFields) {
        $beforeSnapshot = if ($null -eq $BeforeRow) { $null } else { Get-AvailabilitySnapshot -Availability $BeforeRow.$field }
        $afterSnapshot = if ($null -eq $AfterRow) { $null } else { Get-AvailabilitySnapshot -Availability $AfterRow.$field }
        $beforeText = if ($null -eq $beforeSnapshot) { "<absent>" } else { $beforeSnapshot.Display }
        $afterText = if ($null -eq $afterSnapshot) { "<absent>" } else { $afterSnapshot.Display }
        if ($Mode -eq "unchanged") {
            Write-Host "    $field = $beforeText"
            continue
        }

        if ($null -ne $beforeSnapshot -and $null -ne $afterSnapshot -and $beforeSnapshot.State -ne $afterSnapshot.State) {
            Write-Host "    $field availability transition: $($beforeSnapshot.State) -> $($afterSnapshot.State) | $beforeText -> $afterText"
        }
        else {
            Write-Host "    ${field}: $beforeText -> $afterText"
        }
    }
}

function Invoke-GainsDiff {
    param(
        $BeforeCapture,
        $AfterCapture,
        [bool]$IncludeUnchanged
    )

    $beforeRowValues = @(Get-RequiredProperty $BeforeCapture.Gains "rows" "Gains response")
    $afterRowValues = @(Get-RequiredProperty $AfterCapture.Gains "rows" "Gains response")
    $beforeRows = Convert-RowsToMap -Rows $beforeRowValues -Directory $BeforeCapture.Directory
    $afterRows = Convert-RowsToMap -Rows $afterRowValues -Directory $AfterCapture.Directory
    $keys = @($beforeRows.Keys + $afterRows.Keys | Sort-Object -Unique)
    $changed = 0
    $unchanged = 0

    Write-Host ""
    Write-Host "=== Gains row diff ==="
    foreach ($key in $keys) {
        $beforeRow = if ($beforeRows.ContainsKey($key)) { $beforeRows[$key] } else { $null }
        $afterRow = if ($afterRows.ContainsKey($key)) { $afterRows[$key] } else { $null }
        if ($null -eq $beforeRow) {
            $changed++
            Write-Host "[ADDED] $(Get-InstrumentLabel -Row $afterRow -Key $key)"
            Write-GainFields -BeforeRow $null -AfterRow $afterRow -Mode "changed"
            continue
        }
        if ($null -eq $afterRow) {
            $changed++
            Write-Host "[REMOVED] $(Get-InstrumentLabel -Row $beforeRow -Key $key)"
            Write-GainFields -BeforeRow $beforeRow -AfterRow $null -Mode "changed"
            continue
        }

        $fieldChanged = $false
        foreach ($field in $GainsDiffFields) {
            $beforeSnapshot = Get-AvailabilitySnapshot -Availability $beforeRow.$field
            $afterSnapshot = Get-AvailabilitySnapshot -Availability $afterRow.$field
            if ($beforeSnapshot.Comparable -ne $afterSnapshot.Comparable) {
                $fieldChanged = $true
            }
        }
        if ($fieldChanged) {
            $changed++
            Write-Host "[CHANGED] $(Get-InstrumentLabel -Row $afterRow -Key $key)"
            Write-GainFields -BeforeRow $beforeRow -AfterRow $afterRow -Mode "changed"
        }
        else {
            $unchanged++
            if ($IncludeUnchanged) {
                Write-Host "[UNCHANGED] $(Get-InstrumentLabel -Row $afterRow -Key $key)"
                Write-GainFields -BeforeRow $beforeRow -AfterRow $afterRow -Mode "unchanged"
            }
        }
    }
    Write-Host "Gains summary: $changed changed/added/removed, $unchanged unchanged."
    if (-not $IncludeUnchanged -and $unchanged -gt 0) {
        Write-Host "  ($unchanged unchanged rows summarized; use -ShowUnchanged to list them.)"
    }
}

function Convert-PointsToMap {
    param(
        $Points,
        [string]$Directory
    )

    $map = @{}
    foreach ($point in @($Points)) {
        $date = Get-RequiredProperty -Object $point -Name "date" -Context "Value-history point in '$Directory'"
        if ([string]::IsNullOrWhiteSpace([string]$date)) {
            throw "Value-history point in '$Directory' has an empty date."
        }
        if ($map.ContainsKey([string]$date)) {
            throw "Capture '$Directory' contains duplicate value-history points for date $date."
        }
        foreach ($field in $ValueHistoryDiffFields) {
            $null = Get-RequiredProperty -Object $point -Name $field -Context "Value-history point $date in '$Directory'"
        }
        $map[[string]$date] = $point
    }
    return $map
}

function Write-PointFields {
    param(
        $BeforePoint,
        $AfterPoint
    )

    foreach ($field in $ValueHistoryDiffFields) {
        $beforeText = if ($null -eq $BeforePoint) { "<absent>" } else { Get-FieldText -Object $BeforePoint -Name $field }
        $afterText = if ($null -eq $AfterPoint) { "<absent>" } else { Get-FieldText -Object $AfterPoint -Name $field }
        Write-Host "    ${field}: $beforeText -> $afterText"
    }
}

function Invoke-ValueHistoryDiff {
    param(
        $BeforeCapture,
        $AfterCapture,
        [bool]$IncludeUnchanged
    )

    $beforePointValues = @(Get-RequiredProperty $BeforeCapture.ValueHistory "points" "Value-history response")
    $afterPointValues = @(Get-RequiredProperty $AfterCapture.ValueHistory "points" "Value-history response")
    $beforePoints = Convert-PointsToMap -Points $beforePointValues -Directory $BeforeCapture.Directory
    $afterPoints = Convert-PointsToMap -Points $afterPointValues -Directory $AfterCapture.Directory
    $dates = @($beforePoints.Keys + $afterPoints.Keys | Sort-Object -Unique)
    $changed = 0
    $unchanged = 0

    Write-Host ""
    Write-Host "=== Value-history point diff ==="
    foreach ($date in $dates) {
        $beforePoint = if ($beforePoints.ContainsKey($date)) { $beforePoints[$date] } else { $null }
        $afterPoint = if ($afterPoints.ContainsKey($date)) { $afterPoints[$date] } else { $null }
        if ($null -eq $beforePoint) {
            $changed++
            Write-Host "[ADDED] $date"
            Write-PointFields -BeforePoint $null -AfterPoint $afterPoint
            continue
        }
        if ($null -eq $afterPoint) {
            $changed++
            Write-Host "[REMOVED] $date"
            Write-PointFields -BeforePoint $beforePoint -AfterPoint $null
            continue
        }

        $pointChanged = $false
        foreach ($field in $ValueHistoryDiffFields) {
            if ((Get-FieldText -Object $beforePoint -Name $field) -ne (Get-FieldText -Object $afterPoint -Name $field)) {
                $pointChanged = $true
            }
        }
        if ($pointChanged) {
            $changed++
            Write-Host "[CHANGED] $date"
            Write-PointFields -BeforePoint $beforePoint -AfterPoint $afterPoint
        }
        else {
            $unchanged++
            if ($IncludeUnchanged) {
                Write-Host "[UNCHANGED] $date"
                Write-PointFields -BeforePoint $beforePoint -AfterPoint $afterPoint
            }
        }
    }
    Write-Host "Value-history summary: $changed changed/added/removed, $unchanged unchanged."
    if (-not $IncludeUnchanged -and $unchanged -gt 0) {
        Write-Host "  ($unchanged unchanged points summarized; use -ShowUnchanged to list them.)"
    }
}

function Invoke-Diff {
    param(
        [string]$BeforePath,
        [string]$AfterPath,
        [bool]$IncludeUnchanged,
        $Endpoints
    )

    $before = Read-CaptureSet -Directory $BeforePath -Endpoints $Endpoints
    $after = Read-CaptureSet -Directory $AfterPath -Endpoints $Endpoints
    Write-Host "Comparing captures:"
    Write-Host "  before: $($before.Directory)"
    Write-Host "  after : $($after.Directory)"
    Write-Host "  holdings: captured and validated in both; not diffed by this tool."

    Write-Host ""
    Write-Host "=== Gains capture metadata ==="
    $metadataProblems = @()
    foreach ($definition in $GainsMetadataDefinitions) {
        $beforeMetadata = $before.GainsMetadata[$definition.Name]
        $afterMetadata = $after.GainsMetadata[$definition.Name]
        Write-Host "  before $($definition.Name): $($beforeMetadata.Display)"
        Write-Host "  after  $($definition.Name): $($afterMetadata.Display)"

        if (-not $definition.RefuseOnMismatch) {
            $asOfResult = if ($beforeMetadata.Comparable -eq $afterMetadata.Comparable) { "matches" } else { "differs" }
            Write-Host "  $($definition.Name) comparison: $asOfResult (informational only)."
            continue
        }

        if (-not $beforeMetadata.Present -or -not $afterMetadata.Present) {
            $metadataProblems += "$($definition.Name) is absent from at least one capture"
        }
        elseif ($beforeMetadata.Comparable -ne $afterMetadata.Comparable) {
            $metadataProblems += "$($definition.Name) differs"
        }
    }

    if ($metadataProblems.Count -gt 0) {
        Write-Host ""
        Write-Host "REFUSED: capture request metadata is not comparable:"
        foreach ($problem in $metadataProblems) {
            Write-Host "  - $problem"
        }
        throw "No aggregate diff was run. Re-capture with an explicit -EndDate and matching -StartDate, -Method, closed-position setting, and backend base currency."
    }

    Invoke-GainsDiff -BeforeCapture $before -AfterCapture $after -IncludeUnchanged $IncludeUnchanged
    Invoke-ValueHistoryDiff -BeforeCapture $before -AfterCapture $after -IncludeUnchanged $IncludeUnchanged
}

$client = $null
try {
    if ($PSCmdlet.ParameterSetName -eq "Diff") {
        $endpoints = Get-EndpointDefinitions
        Invoke-Diff -BeforePath $BeforeDirectory -AfterPath $AfterDirectory -IncludeUnchanged $ShowUnchanged.IsPresent -Endpoints $endpoints
    }
    else {
        if ([string]::IsNullOrWhiteSpace($BaseUrl)) {
            $BaseUrl = "http://127.0.0.1:$Port"
        }
        try {
            $null = [datetime]::ParseExact($EndDate, "yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)
            if (-not [string]::IsNullOrWhiteSpace($StartDate)) {
                $start = [datetime]::ParseExact($StartDate, "yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)
                $end = [datetime]::ParseExact($EndDate, "yyyy-MM-dd", [System.Globalization.CultureInfo]::InvariantCulture)
                if ($start -gt $end) {
                    throw "StartDate must not be after EndDate."
                }
            }
        }
        catch {
            throw "Invalid capture date range: $($_.Exception.Message)"
        }
        $endpoints = Get-CaptureEndpointDefinitions -GainsStartDate $StartDate -GainsEndDate $EndDate -GainsMethod $Method
        $agent = Get-UserAgent -ExplicitUserAgent $UserAgent
        Invoke-Capture -EndpointBaseUrl $BaseUrl -Agent $agent -Timeout $TimeoutSeconds -Endpoints $endpoints | Out-Null
    }
}
catch {
    Write-Error $_.Exception.Message
    exit 1
}
exit 0
