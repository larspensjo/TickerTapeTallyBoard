# TickerTapeTallyBoard
A portfolio management application

## Prerequisites

- Rust toolchain with Cargo
- Node.js and npm
- PowerShell on Windows

## Running the application

Install frontend dependencies once:

```powershell
cd frontend
npm install
```

Run the production composition:

```powershell
.\scripts\start.ps1
```

With no flags, the script builds the release backend and `frontend/dist`, then
runs the release backend as the sole application process at
`http://127.0.0.1:8480/`. The backend serves the built static frontend. It
resolves its production ledger at
`%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite` from backend
configuration. On a fresh clone, create that ledger once with
`.\scripts\start.ps1 -InitLedger`. Existing installs that still use the old
repository-local ledger must first run `.\scripts\migrate-ledger.ps1`. The first
release build can take several minutes.

The script blocks until Ctrl+C. Run a second PowerShell window when a procedure
needs a second instance.

For the debug backend plus Vite development loop (including HMR):

```powershell
.\scripts\start.ps1 -Dev
```

Vite proxies `/api` to the backend, so the frontend can call `/api/health`
without a separate development API URL. Development can scan to a free backend
port and a free Vite port.

Development uses the separate ledger
`%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio-dev.sqlite`. Create a new dev
ledger once with `.\scripts\start.ps1 -Dev -InitLedger`. The old
`.local\db\tttb-ledger-test.sqlite` file is retired, not reused as dev data;
the launcher refuses non-demo starts while it or its WAL/SHM sidecars exist. Use
`.\scripts\migrate-ledger.ps1` for the one-time production move. Realistic dev
data comes from `-Demo` or from deliberately copying a backup snapshot onto the
dev path.

- `GET /api/data-version` reports the data revision the backend is serving, the
  date it considers today, and whether a price refresh is running. The frontend
  includes that revision and date in the cache keys for data queries made through
  the shared query layer, so a token change refetches those data panels together.
  For valuations and data requests, "today" is the backend machine's local date.
  The footer's backup-age label is a display-only exception that uses the browser's
  local date.

For the seeded, in-memory demo:

```powershell
.\scripts\start.ps1 -Demo
```

`-Demo` alone uses the release/static composition for a presentation-ready
demo. It composes with `-Dev` when the Vite loop is wanted.

For a faster rerun after dependencies and builds are already current:

```powershell
.\scripts\start.ps1 -SkipInstall -SkipBuild
```

`-SkipBuild` in the release/static composition can serve a stale
`frontend/dist` and release binary.

Use `-DatabaseUrl` to explicitly override the non-demo ledger, and `-Port` to
explicitly override the backend port:

```powershell
.\scripts\start.ps1 -DatabaseUrl "sqlite://C:/path/to/ledger.sqlite" -Port 8481
```

`-InitLedger` sets `TTTB_CREATE_LEDGER_IF_MISSING=1`. `-NoBackup` sets
`TTTB_BACKUP_ENABLED=0` for the ordinary launch snapshot only. `-NoRefresh`
disables the launch-time market-data refresh.

`-ProductionDb`, `-LocalDatabaseUrl`, and `-ProductionDatabaseUrl` are retired
and fail immediately with the replacement command. The backend silently ignores
the retired `TTTB_DEMO_MODE`; use `-Demo` or `TTTB_MODE=demo`.

## Backend Commands

Run from `backend/`:

```powershell
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Configuration:

- `TTTB_MODE`: `production`, `development`, or `demo`; default `production`.
- `TTTB_HOST`: backend bind IP address, default `127.0.0.1`; only `127.0.0.0/8` and `::1` are accepted.
- `TTTB_PORT`: backend port, default `8480`
- `PORT`: hosting-platform fallback port when `TTTB_PORT` is not set
- `TTTB_STATIC_DIR`: built frontend directory, default `../frontend/dist`
- `TTTB_DATABASE_URL`: optional explicit backend SQLite database URL. When omitted, production uses `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite` and development uses `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio-dev.sqlite`; demo always uses memory and ignores this setting.
- `TTTB_CREATE_LEDGER_IF_MISSING`: default `false`; set to `1` only to create and migrate a missing ledger (the script's `-InitLedger` switch does this). Otherwise a missing ledger is refused and no empty file is created.
- `TTTB_BACKUP_ENABLED`: enables the ordinary launch snapshot; default `true` outside demo mode. It never disables a mandatory pre-migration snapshot.
- `TTTB_BACKUP_DIR`: backup directory. Defaults to `%OneDrive%/TickerTapeTallyBoard/Backups` in production and `%LOCALAPPDATA%/TickerTapeTallyBoard/backups-dev` in development. If the production default cannot resolve, startup remains available unless a migration is pending.
- `TTTB_LOG_FILE`: backend log file. Defaults to `%LOCALAPPDATA%/TickerTapeTallyBoard/logs/engine.log` in production, `engine-development.log` in development, and `engine-demo.log` in demo.
- `TTTB_MARKET_DATA_REFRESH_ENABLED`: enables launch-time market-data refresh, default `true`
- `TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED`: enables startup market-data refresh, default `true`

## Logs

The backend log lives outside the repository at
`%LOCALAPPDATA%\TickerTapeTallyBoard\logs\`. Its mode-specific name keeps
production, development, and demo timelines separate. The active log is capped
at 5 MiB and keeps three rotations, for an approximate 20 MiB maximum.
Rotation never splits a newline-delimited log line; embedded newlines in one
message create separate rotation boundaries.
If the log file cannot be opened, the app still starts and writes logs to the
terminal instead.

Each `scripts/start.ps1` launch writes its redirected process output under
`.local/logs/run-<UTC timestamp>/` as `backend.out.log`, `backend.err.log`,
`frontend.out.log`, and `frontend.err.log`. The launcher keeps the five newest
run directories; connectivity and provider probe logs directly under
`.local/logs/` are retained separately.

## Backups and restore

Each ordinary production or development launch writes an integrity-checked SQLite
snapshot to the configured backup folder. The default production folder is
`%OneDrive%\\TickerTapeTallyBoard\\Backups`; snapshots are plain, unencrypted
SQLite files in OneDrive. Retention keeps the ten newest launch snapshots, one
per ISO week for eight weeks, one per month for twelve months, ten
pre-migration snapshots, and three failed verification files. Abandoned partial
writes older than one hour are reclaimed.

An ordinary launch snapshot failure only raises a visible warning in health, the
footer, and the engine log, so the application remains available. A snapshot
before a pending migration is mandatory: it cannot be disabled by mode, flag, or
environment variable, and its failure blocks startup before the migration runs.
“Backed up” means the snapshot was written locally to the synced folder; OneDrive
upload is not verified.

### Restore drill

Run this procedure with two PowerShell windows because `scripts/start.ps1` blocks
until Ctrl+C. It captures the live instance before any writes, restores the
launch snapshot into a clean directory, and compares the two read-only views.
Before starting, build the current checkout once with
`.\scripts\start.ps1 -BuildOnly` so `-SkipBuild` cannot run stale artifacts.

1. In window 1, start the live production instance with launch refresh disabled.
   `-SkipInstall` and `-SkipBuild` use the already-installed dependencies and
   built artifacts:

   ```powershell
   .\scripts\start.ps1 -SkipInstall -SkipBuild -NoBrowser -NoRefresh
   ```

2. In window 2, define the comparison settings and immediately capture from the
   live instance. Do not write to the application while this procedure is in
   progress. `-NoRefresh` matters because the launch snapshot and the captured
   state must be identical.

   ```powershell
   $endDate = (Get-Date).ToString("yyyy-MM-dd")
   $method = "xirr"
   $previousCapture = Get-ChildItem .local\aggregates\capture-* -Directory -ErrorAction SilentlyContinue |
       Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
   pwsh -NoProfile -File .\scripts\capture-aggregates.ps1 -EndDate $endDate -Method $method
   if ($LASTEXITCODE -ne 0) { throw "Capturing the live instance failed with exit code $LASTEXITCODE." }
   $captureA = Get-ChildItem .local\aggregates\capture-* -Directory |
       Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
   if ($null -eq $captureA -or ($null -ne $previousCapture -and $captureA.FullName -eq $previousCapture.FullName)) {
       throw "Capturing the live instance did not create a new capture directory."
   }
   ```

3. Stop the live instance in window 1 with Ctrl+C.

4. In window 2, select the newest ordinary launch snapshot, excluding
   pre-migration snapshots and incomplete or failed files. Restore it into a
   freshly removed-and-recreated directory. Removing the directory first is
   required because stale SQLite `-wal`/`-shm` sidecars would otherwise be
   replayed over the restored file.

   If `TTTB_BACKUP_DIR` overrides the default, select the snapshot from that
   directory instead.

   ```powershell
   $backupDirectory = if ($env:TTTB_BACKUP_DIR) {
       $env:TTTB_BACKUP_DIR
   } else {
       Join-Path $env:OneDrive "TickerTapeTallyBoard\Backups"
   }
   $snapshot = Get-ChildItem $backupDirectory -Filter "portfolio-*.sqlite" -File |
       Where-Object { $_.Name -notlike "*-premigration.sqlite" } |
       Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
   if ($null -eq $snapshot) { throw "No ordinary launch snapshot was found." }

   $restoreDirectory = Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard\restore-drill"
   Remove-Item -LiteralPath $restoreDirectory -Recurse -Force -ErrorAction SilentlyContinue
   New-Item -ItemType Directory -Force -Path $restoreDirectory | Out-Null
   Copy-Item -LiteralPath $snapshot.FullName -Destination (Join-Path $restoreDirectory "portfolio.sqlite")
   ```

5. In window 1, start a second backend against the restored copy. Both
   `-NoBackup` and `-NoRefresh` are required: the first prevents the drill from
   polluting the retention ladder, and the second prevents fresh prices from
   changing the copy being measured. `-NoBackup` does not disable the mandatory
   pre-migration snapshot. Therefore always drill with a current-schema launch
   snapshot; otherwise a pending migration may create that mandatory snapshot
   and the result is not a clean measurement.

   ```powershell
   $restoreDirectory = Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard\restore-drill"
   .\scripts\start.ps1 -SkipInstall -SkipBuild -NoBrowser -NoBackup -NoRefresh `
       -Port 8481 -DatabaseUrl (Join-Path $restoreDirectory "portfolio.sqlite")
   ```

6. In window 2, capture the restored instance with the same `$endDate` and
   `$method`:

   ```powershell
   $previousCapture = Get-ChildItem .local\aggregates\capture-* -Directory -ErrorAction SilentlyContinue |
       Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
   pwsh -NoProfile -File .\scripts\capture-aggregates.ps1 -Port 8481 -EndDate $endDate -Method $method
   if ($LASTEXITCODE -ne 0) { throw "Capturing the restored instance failed with exit code $LASTEXITCODE." }
   $captureB = Get-ChildItem .local\aggregates\capture-* -Directory |
       Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
   if ($null -eq $captureB -or $captureB.FullName -eq $captureA.FullName -or
       ($null -ne $previousCapture -and $captureB.FullName -eq $previousCapture.FullName)) {
       throw "Capturing the restored instance did not create a distinct new capture directory."
   }
   ```

7. Diff the captures with `-FailOnChange`. The normal diff summary prints first;
   the command passes only when `$LASTEXITCODE -eq 0`.

   ```powershell
   $global:LASTEXITCODE = $null
   pwsh -NoProfile -File .\scripts\capture-aggregates.ps1 `
       -BeforeDirectory $captureA.FullName -AfterDirectory $captureB.FullName `
       -FailOnChange
   $diffExitCode = $LASTEXITCODE
   $diffExitCode -eq 0
   if ($diffExitCode -ne 0) { throw "Restore drill diff failed with exit code $diffExitCode." }
   ```

The pass criterion is zero added, removed, or changed gains rows and zero
changed, added, or removed value-history points. Without `-FailOnChange`, diff
mode remains an informational report and exits 0 even when differences exist.

For a real restore, stop the application first. Copy the current production
`portfolio.sqlite` and any `-wal`/`-shm` sidecars into a separate safety
directory. Then remove the production sidecars and copy the chosen snapshot over
`%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`. Restart the application
normally afterward.

For example, with `$snapshot` set to the chosen backup:

```powershell
$ledgerDirectory = Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard"
$safetyDirectory = Join-Path $ledgerDirectory ("pre-restore-" + (Get-Date).ToUniversalTime().ToString("yyyyMMddTHHmmssZ"))
New-Item -ItemType Directory -Path $safetyDirectory | Out-Null
"portfolio.sqlite", "portfolio.sqlite-wal", "portfolio.sqlite-shm" | ForEach-Object {
    $currentFile = Join-Path $ledgerDirectory $_
    if (Test-Path -LiteralPath $currentFile) {
        Copy-Item -LiteralPath $currentFile -Destination $safetyDirectory
    }
}
Remove-Item -LiteralPath (Join-Path $ledgerDirectory "portfolio.sqlite-wal"), (Join-Path $ledgerDirectory "portfolio.sqlite-shm") -Force -ErrorAction SilentlyContinue
Copy-Item -LiteralPath $snapshot.FullName -Destination (Join-Path $ledgerDirectory "portfolio.sqlite") -Force
```

## Frontend Commands

Run from `frontend/`:

```powershell
npm run dev
npm run check
npm run fmt
npm run build
```

## Static-serving smoke test

After starting the default production composition, smoke-test both surfaces:

```powershell
Invoke-WebRequest http://127.0.0.1:8480/ -UseBasicParsing
Invoke-WebRequest http://127.0.0.1:8480/api/health -UseBasicParsing
```

## Sharesight Import Spike

Run the Sharesight import spike against the local private export:

```powershell
cd backend
cargo run --example sharesight_import_spike
```

To verify the split-position invariant when the current Sharesight `NOW` position is known:

```powershell
cd backend
cargo run --example sharesight_import_spike -- --split-current-position <CURRENT_NOW_POSITION>
```
