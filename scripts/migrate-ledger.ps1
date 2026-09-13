<#
.SYNOPSIS
    Moves the legacy live ledger into the application-data directory.

.DESCRIPTION
    Refuses unsafe states, makes a non-overwriting safety copy including any
    SQLite sidecars, moves the files together, and verifies the moved database.
    By default this operates on the real repository ledger and LOCALAPPDATA
    location. Use -SourcePath and -TargetRoot only for isolated testing.

.PARAMETER SourcePath
    Legacy SQLite database path. Defaults to .local/db/tttb-ledger-test.sqlite
    in the repository.

.PARAMETER TargetRoot
    Application-data directory. Defaults to
    %LOCALAPPDATA%\TickerTapeTallyBoard.
#>
[CmdletBinding()]
param(
    [string]$SourcePath,
    [string]$TargetRoot
)

$ErrorActionPreference = "Stop"
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
if ([string]::IsNullOrWhiteSpace($SourcePath)) {
    $SourcePath = Join-Path $RepoRoot ".local/db/tttb-ledger-test.sqlite"
}
if ([string]::IsNullOrWhiteSpace($TargetRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        throw "LOCALAPPDATA is not set; provide -TargetRoot explicitly."
    }
    $TargetRoot = Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard"
}

$SourcePath = [System.IO.Path]::GetFullPath($SourcePath)
$TargetRoot = [System.IO.Path]::GetFullPath($TargetRoot)
$SourceDirectory = Split-Path -Parent $SourcePath
$SourceName = [System.IO.Path]::GetFileNameWithoutExtension($SourcePath)
$SourceExtension = [System.IO.Path]::GetExtension($SourcePath)
$SourceFiles = @(
    [pscustomobject]@{ Source = $SourcePath; DestinationName = "portfolio.sqlite" }
)
foreach ($suffix in @("-wal", "-shm")) {
    $sidecar = "$SourceDirectory\$SourceName$SourceExtension$suffix"
    if (Test-Path -LiteralPath $sidecar) {
        $SourceFiles += [pscustomobject]@{
            Source = $sidecar
            DestinationName = "portfolio.sqlite$suffix"
        }
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

function Get-ListeningProcessForPort {
    param([Parameter(Mandatory = $true)][int]$Port)

    try {
        $connection = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction Stop |
            Select-Object -First 1
        if ($connection) {
            $process = Get-Process -Id $connection.OwningProcess -ErrorAction SilentlyContinue
            if ($process) { return "$($process.ProcessName) (PID $($process.Id))" }
            return "PID $($connection.OwningProcess)"
        }
    }
    catch {
        # Fall through to netstat for older Windows versions and restricted
        # Get-NetTCPConnection environments.
    }

    $netstatLine = netstat -ano |
        Where-Object { $_ -match "^\s*TCP\s+\S+:$Port\s+\S+\s+LISTENING\s+(\d+)\s*$" } |
        Select-Object -First 1
    if ($netstatLine -and $netstatLine -match "\s(\d+)\s*$") {
        $processId = [int]$Matches[1]
        $process = Get-Process -Id $processId -ErrorAction SilentlyContinue
        if ($process) { return "$($process.ProcessName) (PID $processId)" }
        return "PID $processId"
    }
    return $null
}

function Assert-MigrationSafe {
    $portOccupant = Get-ListeningProcessForPort -Port 8480
    if ($portOccupant) {
        throw "Refusing migration: production backend port 8480 is in use by $portOccupant. Stop the application first."
    }

    $backendProcesses = Get-Process -Name "ticker-tape-tally-board-backend" -ErrorAction SilentlyContinue
    if ($backendProcesses) {
        $details = ($backendProcesses | ForEach-Object { "PID $($_.Id)" }) -join ", "
        throw "Refusing migration: ticker-tape-tally-board-backend is running ($details). Stop the application first."
    }
}

function Assert-RegularFile {
    param([Parameter(Mandatory = $true)][string]$Path)
    $item = Get-Item -LiteralPath $Path -ErrorAction Stop
    if ($item.PSIsContainer) { throw "Expected a file but found a directory: $Path" }
    return $item
}

function Assert-FileAvailableForMigration {
    param([Parameter(Mandatory = $true)][string]$Path)

    try {
        $stream = [System.IO.File]::Open(
            $Path,
            [System.IO.FileMode]::Open,
            [System.IO.FileAccess]::ReadWrite,
            [System.IO.FileShare]::None
        )
        $stream.Close()
    }
    catch {
        if ($stream) { $stream.Dispose() }
        throw "Refusing migration: source file cannot be opened exclusively: $Path. Close the application or process using it, then retry. $($_.Exception.Message)"
    }
}

function Get-FileFingerprint {
    param([Parameter(Mandatory = $true)][string]$Path)

    $item = Assert-RegularFile -Path $Path
    return [pscustomobject]@{
        Size = $item.Length
        Hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    }
}

Assert-MigrationSafe

$targetDatabasePath = Join-Path $TargetRoot "portfolio.sqlite"
if (Test-Path -LiteralPath $targetDatabasePath) {
    throw "Refusing migration: target ledger already exists: $targetDatabasePath"
}
foreach ($suffix in @("-wal", "-shm", "-journal")) {
    $targetSidecarPath = "$targetDatabasePath$suffix"
    if (Test-Path -LiteralPath $targetSidecarPath) {
        throw "Refusing migration: target SQLite sidecar already exists: $targetSidecarPath"
    }
}

if (-not (Test-Path -LiteralPath $SourcePath)) {
    throw "Refusing migration: source ledger does not exist: $SourcePath"
}
$null = Assert-RegularFile -Path $SourcePath
foreach ($sourceFile in $SourceFiles | Select-Object -Skip 1) {
    $null = Assert-RegularFile -Path $sourceFile.Source
}

foreach ($sourceFile in $SourceFiles) {
    Assert-FileAvailableForMigration -Path $sourceFile.Source
    $fingerprint = Get-FileFingerprint -Path $sourceFile.Source
    $sourceFile | Add-Member -NotePropertyName Size -NotePropertyValue $fingerprint.Size
    $sourceFile | Add-Member -NotePropertyName Hash -NotePropertyValue $fingerprint.Hash
}

$sourceSize = $SourceFiles[0].Size
$sourceHash = $SourceFiles[0].Hash
Write-Host "Source ledger: $SourcePath"
Write-Host "Source size: $sourceSize bytes"
Write-Host "Source SHA-256: $sourceHash"

$backupRoot = Join-Path $TargetRoot "pre-move-backup"
$reuseSafetyCopy = $false
if (Test-Path -LiteralPath $backupRoot) {
    $backupRootItem = Get-Item -LiteralPath $backupRoot -Force
    if (-not $backupRootItem.PSIsContainer) {
        throw "Refusing migration: safety copy path is not a directory: $backupRoot"
    }

    $backupContents = @(Get-ChildItem -LiteralPath $backupRoot -Force)
    if ($backupContents.Count -gt 0) {
        $expectedBackupNames = @($SourceFiles | ForEach-Object { [System.IO.Path]::GetFileName($_.Source) })
        $reuseSafetyCopy = ($backupContents.Count -eq $SourceFiles.Count)
        foreach ($sourceFile in $SourceFiles) {
            $backupPath = Join-Path $backupRoot ([System.IO.Path]::GetFileName($sourceFile.Source))
            if (-not (Test-Path -LiteralPath $backupPath -PathType Leaf)) {
                $reuseSafetyCopy = $false
                continue
            }
            $currentSourceFingerprint = Get-FileFingerprint -Path $sourceFile.Source
            $backupFingerprint = Get-FileFingerprint -Path $backupPath
            if (
                $currentSourceFingerprint.Size -ne $sourceFile.Size -or
                $currentSourceFingerprint.Hash -ne $sourceFile.Hash -or
                $backupFingerprint.Size -ne $currentSourceFingerprint.Size -or
                $backupFingerprint.Hash -ne $currentSourceFingerprint.Hash
            ) {
                $reuseSafetyCopy = $false
            }
        }
        foreach ($backupItem in $backupContents) {
            if ($backupItem.Name -notin $expectedBackupNames -or $backupItem.PSIsContainer) {
                $reuseSafetyCopy = $false
            }
        }

        if (-not $reuseSafetyCopy) {
            throw "Refusing migration: existing safety copy does not exactly match the current source files: $backupRoot. Every current source file is still in place and the target ledger does not exist, so it is safe to delete this safety copy directory before retrying after confirming those facts yourself. Do not delete it if a source file is missing or it may be your only recovery copy."
        }
    }
}

if ($reuseSafetyCopy) {
    Invoke-Step "Verify existing safety copy" {
        foreach ($sourceFile in $SourceFiles) {
            $backupPath = Join-Path $backupRoot ([System.IO.Path]::GetFileName($sourceFile.Source))
            Write-Host "Verified safety copy: $backupPath ($($sourceFile.Size) bytes, SHA-256 $($sourceFile.Hash))"
        }
    }
}
else {
    try {
        Invoke-Step "Create and verify non-overwriting safety copy" {
            New-Item -ItemType Directory -Path $backupRoot -Force | Out-Null
            foreach ($sourceFile in $SourceFiles) {
                $backupPath = Join-Path $backupRoot ([System.IO.Path]::GetFileName($sourceFile.Source))
                if (Test-Path -LiteralPath $backupPath) {
                    throw "Refusing migration: safety copy would overwrite an existing file: $backupPath"
                }
                Copy-Item -LiteralPath $sourceFile.Source -Destination $backupPath
            }
            foreach ($sourceFile in $SourceFiles) {
                $backupPath = Join-Path $backupRoot ([System.IO.Path]::GetFileName($sourceFile.Source))
                $currentSourceFingerprint = Get-FileFingerprint -Path $sourceFile.Source
                $backupFingerprint = Get-FileFingerprint -Path $backupPath
                if (
                    $currentSourceFingerprint.Size -ne $sourceFile.Size -or
                    $currentSourceFingerprint.Hash -ne $sourceFile.Hash
                ) {
                    throw "SOURCE CHANGED DURING SAFETY COPY: $($sourceFile.Source) no longer matches its pre-copy size/hash."
                }
                if (
                    $backupFingerprint.Size -ne $currentSourceFingerprint.Size -or
                    $backupFingerprint.Hash -ne $currentSourceFingerprint.Hash
                ) {
                    throw "SAFETY COPY VERIFICATION FAILED: $backupPath differs from $($sourceFile.Source)."
                }
                Write-Host "Verified safety copy: $backupPath ($($backupFingerprint.Size) bytes, SHA-256 $($backupFingerprint.Hash))"
            }
        }
    }
    catch {
        throw "Safety copy could not be created and verified; no move was attempted. $($_.Exception.Message) The safety copy directory may be incomplete: $backupRoot. If every source file remains in place and the target ledger still does not exist, it is safe to delete that directory before retrying. Do not delete it if it may be your only recovery copy."
    }
}

Invoke-Step "Move ledger and SQLite sidecars" {
    New-Item -ItemType Directory -Path $TargetRoot -Force | Out-Null
    $moveFiles = @($SourceFiles | Select-Object -Skip 1) + @($SourceFiles[0])
    $movedFiles = @()
    try {
        foreach ($sourceFile in $moveFiles) {
            Move-Item -LiteralPath $sourceFile.Source -Destination (Join-Path $TargetRoot $sourceFile.DestinationName)
            $movedFiles += $sourceFile
        }
    }
    catch {
        $moveError = $_.Exception.Message
        $rollbackErrors = @()
        for ($index = $movedFiles.Count - 1; $index -ge 0; $index--) {
            $movedFile = $movedFiles[$index]
            $movedPath = Join-Path $TargetRoot $movedFile.DestinationName
            try {
                if ((Test-Path -LiteralPath $movedPath) -and -not (Test-Path -LiteralPath $movedFile.Source)) {
                    Move-Item -LiteralPath $movedPath -Destination $movedFile.Source
                }
                elseif ((Test-Path -LiteralPath $movedPath) -or -not (Test-Path -LiteralPath $movedFile.Source)) {
                    $rollbackErrors += "Could not establish one restored source copy for $($movedFile.Source)."
                }
            }
            catch {
                $rollbackErrors += "Could not restore $movedPath to $($movedFile.Source): $($_.Exception.Message)"
            }
        }

        foreach ($sourceFile in $SourceFiles) {
            $movedPath = Join-Path $TargetRoot $sourceFile.DestinationName
            if (-not (Test-Path -LiteralPath $sourceFile.Source) -or (Test-Path -LiteralPath $movedPath)) {
                $rollbackErrors += "Split-state artifact: source '$($sourceFile.Source)', target '$movedPath'."
            }
        }

        if ($rollbackErrors.Count -eq 0) {
            throw "Migration move failed: $moveError All files already moved were restored to their source names. The source set is intact; the verified safety copy remains at $backupRoot."
        }

        $rollbackDetails = $rollbackErrors -join " "
        throw "SPLIT LEDGER STATE: migration move failed and rollback could not restore one complete source set. Do not start the application. Recover the complete ledger and sidecars from the verified pre-move-backup at $backupRoot. Move error: $moveError Rollback details: $rollbackDetails"
    }
}

$movedItem = Get-Item -LiteralPath $targetDatabasePath -ErrorAction Stop
$movedSize = $movedItem.Length
$movedHash = (Get-FileHash -LiteralPath $targetDatabasePath -Algorithm SHA256).Hash
if ($movedSize -ne $sourceSize -or $movedHash -ne $sourceHash) {
    throw "LEDGER VERIFICATION FAILED: moved ledger size/hash differs from the source. Source was $sourceSize bytes/$sourceHash; moved ledger is $movedSize bytes/$movedHash."
}

Write-Host "Moved ledger: $targetDatabasePath"
Write-Host "Moved size: $movedSize bytes"
Write-Host "Moved SHA-256: $movedHash"
foreach ($sourceFile in $SourceFiles | Select-Object -Skip 1) {
    Write-Host "Moved sidecar: $(Join-Path $TargetRoot $sourceFile.DestinationName)"
}
Write-Host "Safety copy: $backupRoot"
Write-Host "Next: .\scripts\start.ps1"
