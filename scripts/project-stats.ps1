#Requires -Version 5.1

# ============================================================================
# Project Statistics Report Generator
# ============================================================================
#
# Usage: .\scripts\project-stats.ps1
#
# Generates a compact statistics report for the TickerTapeTallyBoard project.
# The report includes:
# - Rust source lines and test counts in the backend
# - Frontend TypeScript, CSS, and HTML lines
# - Documentation lines split between planning and other docs
# - PowerShell script lines
# - Config/data file counts
# - Backend and frontend dependency counts
# ============================================================================

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

function Format-Number {
    param(
        [double]$Value,
        [int]$Width = 0
    )

    $formatted = $Value.ToString("N0", [System.Globalization.CultureInfo]::InvariantCulture)
    if ($Width -gt 0) {
        $formatted = $formatted.PadLeft($Width)
    }

    return $formatted
}

function Measure-LineCount {
    param(
        [System.IO.FileInfo[]]$Files = @()
    )

    if (-not $Files -or $Files.Count -eq 0) {
        return 0
    }

    $totalLines = 0
    foreach ($file in $Files) {
        try {
            $totalLines += (Get-Content $file.FullName -ErrorAction SilentlyContinue).Count
        } catch {
            Write-Warning "Could not read file: $($file.FullName)"
        }
    }

    return $totalLines
}

function Measure-RustTestCount {
    param(
        [System.IO.FileInfo[]]$Files = @()
    )

    if (-not $Files -or $Files.Count -eq 0) {
        return 0
    }

    $totalTests = 0
    foreach ($file in $Files) {
        try {
            $content = Get-Content $file.FullName -Raw -ErrorAction SilentlyContinue
            if ($content) {
                $testMatches = [regex]::Matches($content, '#\[(?:[\w:]+::)?test[^\]]*\]')
                $totalTests += $testMatches.Count
            }
        } catch {
            Write-Warning "Could not read file: $($file.FullName)"
        }
    }

    return $totalTests
}

function Test-IgnoredPath {
    param(
        [string]$Path
    )

    return $Path -match '\\node_modules\\' -or
        $Path -match '\\dist\\' -or
        $Path -match '\\target\\' -or
        $Path -match '\\\.git\\' -or
        $Path -match '\\\.vscode\\' -or
        $Path -match '\\\.local\\'
}

function Get-FileSet {
    param(
        [string]$Root,
        [string[]]$Include,
        [string[]]$ExcludeNames = @()
    )

    if (-not (Test-Path $Root)) {
        return @()
    }

    $files = Get-ChildItem -Path $Root -Recurse -File -ErrorAction SilentlyContinue | Where-Object {
        -not (Test-IgnoredPath -Path $_.FullName) -and
        ($Include -contains $_.Extension.TrimStart('.').ToLowerInvariant() -or
            ($Include -contains $_.Name.ToLowerInvariant()))
    }

    if ($ExcludeNames.Count -gt 0) {
        $files = $files | Where-Object { $ExcludeNames -notcontains $_.Name }
    }

    return @($files)
}

function Get-BackendStats {
    $backendRoot = Join-Path $projectRoot "backend"
    $srcFiles = Get-FileSet -Root (Join-Path $backendRoot "src") -Include @("rs")
    $exampleFiles = Get-FileSet -Root (Join-Path $backendRoot "examples") -Include @("rs")
    $migrationFiles = Get-FileSet -Root (Join-Path $backendRoot "migrations") -Include @("sql")
    $allRustFiles = @($srcFiles + $exampleFiles)

    return @{
        Rust = @{
            Lines = Measure-LineCount -Files $allRustFiles
            Files = $allRustFiles.Count
            Tests = Measure-RustTestCount -Files $allRustFiles
        }
        Migrations = @{
            Lines = Measure-LineCount -Files $migrationFiles
            Files = $migrationFiles.Count
        }
    }
}

function Get-FrontendStats {
    $frontendRoot = Join-Path $projectRoot "frontend"

    $tsFiles = Get-FileSet -Root $frontendRoot -Include @("ts", "tsx") -ExcludeNames @("vite-env.d.ts")
    $cssFiles = Get-FileSet -Root $frontendRoot -Include @("css")
    $htmlFiles = Get-FileSet -Root $frontendRoot -Include @("html")

    return @{
        TypeScript = @{
            Lines = Measure-LineCount -Files $tsFiles
            Files = $tsFiles.Count
        }
        Css = @{
            Lines = Measure-LineCount -Files $cssFiles
            Files = $cssFiles.Count
        }
        Html = @{
            Lines = Measure-LineCount -Files $htmlFiles
            Files = $htmlFiles.Count
        }
    }
}

function Get-DocumentationStats {
    $docsRoot = Join-Path $projectRoot "docs"
    $topLevelDocs = @(Get-ChildItem -Path $projectRoot -Filter "*.md" -File -ErrorAction SilentlyContinue)
    $allDocs = @()

    if (Test-Path $docsRoot) {
        $allDocs = @(Get-ChildItem -Path $docsRoot -Filter "*.md" -Recurse -File -ErrorAction SilentlyContinue | Where-Object {
            -not (Test-IgnoredPath -Path $_.FullName)
        })
    }

    $planningDocs = @($allDocs | Where-Object { $_.FullName -match '\\docs\\plans\\' })
    $otherDocs = @($allDocs | Where-Object { $_.FullName -notmatch '\\docs\\plans\\' })
    $otherDocs += $topLevelDocs

    return @{
        Planning = @{
            Lines = Measure-LineCount -Files $planningDocs
            Files = $planningDocs.Count
        }
        Other = @{
            Lines = Measure-LineCount -Files $otherDocs
            Files = $otherDocs.Count
        }
    }
}

function Get-ScriptStats {
    $scriptsRoot = Join-Path $projectRoot "scripts"
    $psFiles = @()

    if (Test-Path $scriptsRoot) {
        $psFiles += Get-ChildItem -Path $scriptsRoot -Filter "*.ps1" -File -ErrorAction SilentlyContinue
    }

    $rootPsFiles = Get-ChildItem -Path $projectRoot -Filter "*.ps1" -File -ErrorAction SilentlyContinue
    if ($rootPsFiles) {
        $psFiles += $rootPsFiles
    }

    return @{
        Lines = Measure-LineCount -Files @($psFiles | Where-Object { -not (Test-IgnoredPath -Path $_.FullName) })
        Files = (@($psFiles | Where-Object { -not (Test-IgnoredPath -Path $_.FullName) })).Count
    }
}

function Get-ConfigDataStats {
    $backendRoot = Join-Path $projectRoot "backend"
    $frontendRoot = Join-Path $projectRoot "frontend"

    $jsonFiles = @()
    if (Test-Path $backendRoot) {
        $jsonFiles += Get-ChildItem -Path $backendRoot -Filter "*.json" -Recurse -File -ErrorAction SilentlyContinue
    }
    if (Test-Path $frontendRoot) {
        $jsonFiles += Get-ChildItem -Path $frontendRoot -Filter "*.json" -Recurse -File -ErrorAction SilentlyContinue
    }

    $jsonFiles = @($jsonFiles | Where-Object { -not (Test-IgnoredPath -Path $_.FullName) })
    $lockFiles = @($jsonFiles | Where-Object { $_.Name -match 'lock' })
    $countedJsonFiles = @($jsonFiles | Where-Object { $_.Name -notmatch 'lock' })

    $tomlFiles = Get-FileSet -Root $projectRoot -Include @("toml")

    return @{
        Json = @{
            Lines = Measure-LineCount -Files $countedJsonFiles
            Files = $countedJsonFiles.Count
            Lockfiles = $lockFiles.Count
        }
        Toml = @{
            Lines = Measure-LineCount -Files $tomlFiles
            Files = $tomlFiles.Count
        }
    }
}

function Get-DependencyCount {
    $backendCargoToml = Join-Path $projectRoot "backend\Cargo.toml"
    $frontendPackageJson = Join-Path $projectRoot "frontend\package.json"

    $backendDeps = 0
    if (Test-Path $backendCargoToml) {
        $content = Get-Content $backendCargoToml -Raw
        if ($content -match '(?s)\[dependencies\](.*?)(\r?\n\[|$)') {
            $depsSection = $matches[1]
            $backendDeps = @($depsSection -split "`n" | Where-Object {
                $_ -match '^\s*[a-zA-Z0-9_-]+\s*='
            }).Count
        }
    }

    $frontendDeps = 0
    $frontendDevDeps = 0
    if (Test-Path $frontendPackageJson) {
        $packageJson = Get-Content $frontendPackageJson -Raw | ConvertFrom-Json
        if ($packageJson.dependencies) {
            $frontendDeps = ($packageJson.dependencies.PSObject.Properties | Measure-Object).Count
        }
        if ($packageJson.devDependencies) {
            $frontendDevDeps = ($packageJson.devDependencies.PSObject.Properties | Measure-Object).Count
        }
    }

    return @{
        Backend = $backendDeps
        Frontend = $frontendDeps
        FrontendDev = $frontendDevDeps
        Total = $backendDeps + $frontendDeps + $frontendDevDeps
    }
}

function Show-StatisticsReport {
    param(
        [hashtable]$BackendStats,
        [hashtable]$FrontendStats,
        [hashtable]$DocumentationStats,
        [hashtable]$ScriptStats,
        [hashtable]$ConfigDataStats,
        [hashtable]$DependencyStats
    )

    $totalBackendLines = $BackendStats.Rust.Lines + $BackendStats.Migrations.Lines
    $totalFrontendLines = ($FrontendStats.Values | ForEach-Object { $_.Lines } | Measure-Object -Sum).Sum
    $totalDocLines = ($DocumentationStats.Values | ForEach-Object { $_.Lines } | Measure-Object -Sum).Sum
    $totalProjectLines = $totalBackendLines + $totalFrontendLines + $ScriptStats.Lines + $totalDocLines + $ConfigDataStats.Json.Lines + $ConfigDataStats.Toml.Lines

    Write-Host ""
    Write-Host "================================================================" -ForegroundColor Cyan
    Write-Host "          PROJECT STATISTICS REPORT" -ForegroundColor Cyan
    Write-Host "          TickerTapeTallyBoard" -ForegroundColor Cyan
    Write-Host "================================================================" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  BACKEND" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  Rust source .................." -NoNewline
    Write-Host (Format-Number $BackendStats.Rust.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($BackendStats.Rust.Files) files, $($BackendStats.Rust.Tests) tests)"

    Write-Host "  SQL migrations ..............." -NoNewline
    Write-Host (Format-Number $BackendStats.Migrations.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($BackendStats.Migrations.Files) files)"
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  FRONTEND" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  TypeScript ..................." -NoNewline
    Write-Host (Format-Number $FrontendStats.TypeScript.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($FrontendStats.TypeScript.Files) files)"

    Write-Host "  CSS .........................." -NoNewline
    Write-Host (Format-Number $FrontendStats.Css.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($FrontendStats.Css.Files) files)"

    Write-Host "  HTML ........................." -NoNewline
    Write-Host (Format-Number $FrontendStats.Html.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($FrontendStats.Html.Files) files)"
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  DOCUMENTATION" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  Planning docs ................" -NoNewline
    Write-Host (Format-Number $DocumentationStats.Planning.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($DocumentationStats.Planning.Files) files)"

    Write-Host "  Other docs ..................." -NoNewline
    Write-Host (Format-Number $DocumentationStats.Other.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($DocumentationStats.Other.Files) files)"
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  SCRIPTS" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  PowerShell scripts ..........." -NoNewline
    Write-Host (Format-Number $ScriptStats.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($ScriptStats.Files) files)"
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  CONFIG AND DATA" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  JSON files ..................." -NoNewline
    Write-Host (Format-Number $ConfigDataStats.Json.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($ConfigDataStats.Json.Files) files)"

    if ($ConfigDataStats.Json.Lockfiles -gt 0) {
        Write-Host "  JSON lockfiles ..............." -NoNewline
        Write-Host (Format-Number $ConfigDataStats.Json.Lockfiles -Width 10) -ForegroundColor DarkGreen -NoNewline
        Write-Host " files excluded from line totals"
    }

    Write-Host "  TOML files ..................." -NoNewline
    Write-Host (Format-Number $ConfigDataStats.Toml.Lines -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " lines ($($ConfigDataStats.Toml.Files) files)"

    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  DEPENDENCIES" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  Backend crates ................" -NoNewline
    Write-Host (Format-Number $DependencyStats.Backend -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " dependencies"

    Write-Host "  Frontend packages ............." -NoNewline
    Write-Host (Format-Number $DependencyStats.Frontend -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " runtime dependencies"

    Write-Host "  Frontend dev packages ........." -NoNewline
    Write-Host (Format-Number $DependencyStats.FrontendDev -Width 10) -ForegroundColor Green -NoNewline
    Write-Host " dev dependencies"
    Write-Host ""

    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host "  SUMMARY" -ForegroundColor Cyan
    Write-Host "---------------------------------------------------------------" -ForegroundColor Cyan
    Write-Host ""

    Write-Host "  Total project lines ..........." -NoNewline
    Write-Host (Format-Number $totalProjectLines -Width 10) -ForegroundColor Magenta -NoNewline
    Write-Host " lines" -ForegroundColor Magenta

    if ($totalProjectLines -gt 0) {
        $backendPercent = [math]::Round(($totalBackendLines / $totalProjectLines) * 100)
        $frontendPercent = [math]::Round(($totalFrontendLines / $totalProjectLines) * 100)
    } else {
        $backendPercent = 0
        $frontendPercent = 0
    }

    Write-Host $("  Backend share ................. {0}% of counted lines" -f $backendPercent) -ForegroundColor Magenta
    Write-Host $("  Frontend share ................ {0}% of counted lines" -f $frontendPercent) -ForegroundColor Magenta
    Write-Host ""

    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    Write-Host "Generated: $timestamp" -ForegroundColor DarkGray
    Write-Host ""
}

function Invoke-ProjectStatsReport {
    try {
        if (-not (Test-Path $projectRoot)) {
            throw "Project root not found: $projectRoot"
        }

        Write-Host "Collecting project statistics..." -ForegroundColor Yellow

        $backendStats = Get-BackendStats
        $frontendStats = Get-FrontendStats
        $documentationStats = Get-DocumentationStats
        $scriptStats = Get-ScriptStats
        $configDataStats = Get-ConfigDataStats
        $dependencyStats = Get-DependencyCount

        Show-StatisticsReport `
            -BackendStats $backendStats `
            -FrontendStats $frontendStats `
            -DocumentationStats $documentationStats `
            -ScriptStats $scriptStats `
            -ConfigDataStats $configDataStats `
            -DependencyStats $dependencyStats
    } catch {
        Write-Host $("ERROR: {0}" -f $_) -ForegroundColor Red
        Write-Host $_.ScriptStackTrace -ForegroundColor Red
        exit 1
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    Invoke-ProjectStatsReport
}
