# Engineering Diary

Chronological "what happened" log: every submitted code change, plus research findings,
planning notes, surprises, and open questions encountered during work. This is Stage 1 of
the project workflow; entries may later be promoted to concept notes, research questions,
experiments, or decisions.

## Instructions how to use
- Add one entry per logical change. A logical change can span several related commits.
- Every code change submitted must be reflected by some diary entry. Non-code activities
  (research, planning, observations, things tried that did not pan out) also belong here.
- Decisions and commitments belong in `DecisionLog.md`, not here.
- Keep entries short and reference concrete artifacts.
- New entries go to the end of the file.
- If a change implements a prior decision, note it in the Refs line.
- Don't reference planning documents. Entries shall stand on their own, even after plans are archived.
- There is no need for entries when meta documents are created. E.g. plans or ideas. Only changes to the application.
- Do not modify older entries if they were commited.

## Entry Template
```
## YYYY-MM-DD - <topic>
<one or two sentence summary>
What changed:
- <bullet>
Observed:
- <bullet>
Open question:
- <bullet>
Refs: <files, commits>; implements: <decision title> (if applicable)
```

## 2026-06-13 - Repo skeleton
Added the initial backend and frontend project skeletons using the chosen top-level `backend/` and `frontend/` layout.
What changed:
- Added a minimal Axum/Tokio backend crate with placeholder domain, API, provider, import, database, and migration paths.
- Added a minimal Vite React TypeScript frontend with TypeScript, Biome, build, check, and format scripts.
- Ignored generated frontend dependency and build-output paths.
Observed:
- `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
- `npm run check`, `npm run fmt`, `npm run build`, and `npm audit` passed from `frontend/`.
- The Vite dev server returned the expected root HTML from `http://127.0.0.1:5173/`.
Open question:
- None.
Refs: `backend/`, `frontend/`, `.gitignore`; implements: Repository Layout.

## 2026-06-13 - VS Code workspace configuration
Adapted copied VS Code launch, task, and editor settings to the top-level backend/frontend layout.
What changed:
- Updated debugger launch settings to build and run the backend binary from `backend/`.
- Updated task commands to run Cargo commands from `backend/` and npm commands from `frontend/`.
- Pointed rust-analyzer at `backend/Cargo.toml` and excluded generated frontend output from editor/search views.
Observed:
- `.vscode` JSON files parse successfully.
- No stale `qsf_app` or `crates/qsf_browser_server/ui` references remain in `.vscode`.
- `cargo build` from `backend/` and `npm run check` from `frontend/` passed.
Open question:
- None.
Refs: `.vscode/launch.json`, `.vscode/settings.json`, `.vscode/tasks.json`.

## 2026-06-13 - Local start script
Added a PowerShell startup script for building and running the local backend and frontend together.
What changed:
- Added `scripts/start.ps1` to install frontend dependencies, build backend/frontend artifacts, and start both dev processes.
- Added options for `-SkipInstall`, `-SkipBuild`, `-BuildOnly`, and `-FrontendPort`.
- Runs the built backend executable directly and stops backend/frontend process trees during cleanup.
Observed:
- `scripts/start.ps1 -SkipInstall -BuildOnly` built the backend and frontend successfully.
- A live smoke test kept the script running and returned responses from `http://127.0.0.1:8080/` and `http://127.0.0.1:5173/`.
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt`, and `npm run check` passed.
Open question:
- None.
Refs: `scripts/start.ps1`.

## 2026-06-13 - Backend engine logging
Adapted the engine logging pattern from the related project into the backend so runtime runs write to `engine.log`.
What changed:
- Added a backend-local `engine_logging` module with `engine_*` logging macros and a production initializer using `log` and `simplelog`.
- Initialized logging during backend startup and logged the listening address.
- Ignored generated `engine.log` files.
Observed:
- `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
- A backend smoke run returned `HTTP 200` from `http://127.0.0.1:8080/` and wrote the startup line to `backend/engine.log`.
Open question:
- None.
Refs: `backend/src/engine_logging.rs`, `backend/src/main.rs`, `backend/Cargo.toml`, `.gitignore`.

## 2026-06-13 - Backend API skeleton
Expanded the backend into a thin runnable API host with local-development defaults and initial JSON endpoints.
What changed:
- Added `app` and `config` modules so `main.rs` only initializes logging, loads configuration, and starts the server.
- Added `GET /api/health` with version/build metadata and a temporary Sharesight schema-preview endpoint for importer spike visibility.
- Split API endpoint handlers, CORS setup, and tests into submodules so `api/mod.rs` remains a thin route assembler.
- Added local-dev CORS support and graceful Ctrl+C shutdown handling.
- Added config parsing tests for host/port validation and env-var precedence.
Observed:
- `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
- `cargo test` passed from `backend/`.
- A smoke request to `http://127.0.0.1:8080/api/health` returned `status: ok`, version `0.1.0`, and debug build metadata.
Open question:
- None.
Refs: `backend/src/app.rs`, `backend/src/config.rs`, `backend/src/api/`, `backend/src/main.rs`, `backend/Cargo.toml`.

## 2026-06-13 - Frontend version footer
Added a footer that exposes the frontend package version and backend API version.
What changed:
- Read the frontend package version from `package.json`.
- Added a Vite `/api` development proxy and a footer that reads the backend Cargo version from `/api/health`.
Observed:
- `npm run check`, `npm run fmt`, `scripts/start.ps1 -SkipInstall -BuildOnly`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed.
- A Vite dev-server request to `/api/health` returned backend version `0.1.0`; the served app imports frontend version `0.1.0` from `package.json`.
Open question:
- None.
Refs: `frontend/vite.config.ts`, `frontend/src/App.tsx`, `frontend/src/styles.css`; implements: Displayed Application Versions.
