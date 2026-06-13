# TickerTapeTallyBoard
A portfolio management application

## Prerequisites

- Rust toolchain with Cargo
- Node.js and npm
- PowerShell on Windows

## Local Development

Install frontend dependencies once:

```powershell
cd frontend
npm install
```

Run the backend and frontend together:

```powershell
.\scripts\start.ps1
```

The script builds both projects, starts the backend on `http://127.0.0.1:8080/`,
starts Vite on `http://127.0.0.1:5173/`, and stops both processes when it exits.

For a faster rerun after dependencies and builds are already current:

```powershell
.\scripts\start.ps1 -SkipInstall -SkipBuild
```

The Vite dev server proxies `/api` to the backend, so the frontend can call
`/api/health` without a separate development API URL.

## Backend Commands

Run from `backend/`:

```powershell
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Configuration:

- `TTTB_HOST`: backend bind IP address, default `127.0.0.1`
- `TTTB_PORT`: backend port, default `8080`
- `PORT`: hosting-platform fallback port when `TTTB_PORT` is not set
- `TTTB_STATIC_DIR`: built frontend directory, default `../frontend/dist`

## Frontend Commands

Run from `frontend/`:

```powershell
npm run dev
npm run check
npm run fmt
npm run build
```

## Production-Style Static Serving

The initial production serving model is disk-based static assets. Build the
frontend first:

```powershell
cd frontend
npm run build
```

Then run the backend from `backend/`. With the default working directory, it
serves `../frontend/dist` for frontend routes and keeps API routes under `/api`.

```powershell
cd ..\backend
cargo run
```

Smoke-test both surfaces:

```powershell
Invoke-WebRequest http://127.0.0.1:8080/ -UseBasicParsing
Invoke-WebRequest http://127.0.0.1:8080/api/health -UseBasicParsing
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
