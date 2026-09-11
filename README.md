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
resolves its production ledger location from configuration. The first release
build can take several minutes.

The script blocks until Ctrl+C. Run a second PowerShell window when a procedure
needs a second instance.

For the debug backend plus Vite development loop (including HMR):

```powershell
.\scripts\start.ps1 -Dev
```

Vite proxies `/api` to the backend, so the frontend can call `/api/health`
without a separate development API URL. Development can scan to a free backend
port and a free Vite port.

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
and fail immediately with the replacement command.

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
- `TTTB_DATABASE_URL`: backend SQLite database URL. When omitted, production uses `sqlite://%LOCALAPPDATA%/TickerTapeTallyBoard/portfolio.sqlite` and development uses `sqlite://%LOCALAPPDATA%/TickerTapeTallyBoard/portfolio-dev.sqlite`; demo always uses memory and ignores this setting.
- `TTTB_CREATE_LEDGER_IF_MISSING`: default `false`; set to `1` only to create and migrate a missing ledger (the script's `-InitLedger` switch does this). Otherwise a missing ledger is refused and no empty file is created.
- `TTTB_BACKUP_ENABLED`: enables the ordinary launch snapshot; default `true` outside demo mode. It never disables a mandatory pre-migration snapshot.
- `TTTB_BACKUP_DIR`: backup directory. Defaults to `%OneDrive%/TickerTapeTallyBoard/Backups` in production and `%LOCALAPPDATA%/TickerTapeTallyBoard/backups-dev` in development. If the production default cannot resolve, startup remains available unless a migration is pending.
- `TTTB_LOG_FILE`: backend log file. Defaults to `%LOCALAPPDATA%/TickerTapeTallyBoard/logs/engine.log` in production, `engine-development.log` in development, and `engine-demo.log` in demo.
- `TTTB_MARKET_DATA_REFRESH_ENABLED`: enables launch-time market-data refresh, default `true`
- `TTTB_MARKET_DATA_LAUNCH_REFRESH_ENABLED`: enables startup market-data refresh, default `true`
- `TTTB_DEMO_MODE`: retired; use `TTTB_MODE=demo`.

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

## Backups

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
