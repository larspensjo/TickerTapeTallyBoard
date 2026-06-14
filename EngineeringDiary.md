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

## 2026-06-13 - Frontend app shell
Replaced the placeholder frontend with a compact dark portfolio board shell and applied review fixes that make the shell controls and typography more complete.
What changed:
- Added TanStack Query for `/api/health` server state and kept board-view selection in a reducer.
- Added a dark theme foundation using the approved CSS tokens, an app bar, totals band, holdings and transactions tables, and an API health panel.
- Added Lucide icons for app-bar actions, wired the Refresh action to refetch `/api/health`, and show a spinning refresh icon while a health request is in flight.
- Added accessible pressed state for the board view toggle and stable ids for mock transaction rows.
- Bundled Inter and JetBrains Mono through local font packages and added base body typography plus consistent focus rings for app navigation.
- Bumped the frontend package version to `0.2.1`.
Observed:
- `npm run check`, `npm run fmt`, `npm run build`, `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed.
- A headless Chrome smoke test rendered the app through Vite with `API ok`, frontend version `0.2.1`, Inter and JetBrains Mono active, the Holdings toggle marked `aria-pressed="true"`, and no browser exceptions or console errors.
Open question:
- None.
Refs: `frontend/index.html`, `frontend/src/App.tsx`, `frontend/src/main.tsx`, `frontend/src/styles.css`, `frontend/package.json`, `frontend/package-lock.json`.

## 2026-06-13 - Sharesight import spike
Added a throwaway Rust example command that parses the private Sharesight All Trades CSV and records sanitized aggregate findings.
What changed:
- Added `backend/examples/sharesight_import_spike.rs` for header discovery, metadata parsing, decimal/date normalization, aggregate validation, FX/value model comparison, and optional split-position invariant checking.
- Added a sanitized spike note with importer findings and kept row-level/private values out of versioned docs.
- Recorded durable Sharesight CSV import conventions in the decision log.
Observed:
- The spike parsed all 189 rows with counts matching the planning observation: 105 buys, 83 sells, and 1 split.
- All brokerage currencies were SEK, all `Cost base per share (SEK)` values were zero, `Market + Code` had no identity conflicts, and there were no duplicate full rows.
- The closest FX/value model was native gross divided by the exported exchange rate plus brokerage, implying the export rate is instrument currency per SEK and should be inverted for canonical SEK-per-instrument storage.
Open question:
- The split row derives a clean 5/1 delta-semantics ratio, but the current Sharesight position is still needed to confirm the invariant.
Refs: `backend/examples/sharesight_import_spike.rs`, `docs/spikes/SharesightImportSpike.md`, `docs/DecisionLog.md`; implements: Sharesight CSV Import Conventions.

## 2026-06-13 - Sharesight spike usage
Documented how to run the Sharesight import spike from the README.
What changed:
- Added the default spike command and the optional split-position invariant command.
Observed:
- No code changed.
Open question:
- None.
Refs: `README.md`.

## 2026-06-13 - Sharesight spike dependency and interpretation cleanup
Moved spike-only parser crates out of backend runtime dependencies and made the brokerage-inclusion finding provisional.
What changed:
- Moved `chrono`, `csv`, and `rust_decimal` to backend dev-dependencies because only the spike example uses them.
- Disabled default features for `rust_decimal`.
- Updated the Sharesight import convention and spike note so `Value` brokerage inclusion requires manual buy/sell reconciliation before Phase 1 import math depends on it.
Observed:
- The FX direction conclusion remains unchanged: the export rate is instrument currency per SEK and should be inverted for canonical storage.
Open question:
- Does `Value` include brokerage for both buys and sells? Reconcile one buy and one sell manually against the raw export or Sharesight UI.
Refs: `backend/Cargo.toml`, `docs/DecisionLog.md`, `docs/spikes/SharesightImportSpike.md`.

## 2026-06-13 - Sharesight split invariant confirmed
Recorded the result of rerunning the Sharesight import spike with the current Sharesight position for the split holding.
What changed:
- Updated the sanitized spike note to state that the provided current position matched the summed export quantity.
- Recorded that Sharesight split quantities use delta semantics for this export.
Observed:
- The split row derives a clean 5/1 ratio under delta semantics.
Open question:
- None for split semantics.
Refs: `docs/spikes/SharesightImportSpike.md`, `docs/DecisionLog.md`; implements: Sharesight Split Quantity Delta Confirmed.

## 2026-06-13 - Price provider spike
Compared free-friendly equity and FX provider candidates for the representative USD and EUR instruments.
What changed:
- Added a sanitized spike note with tested request URLs, observed response shapes, provider-symbol mappings, missing-price handling guidance, and fallback strategy.
- Recorded the v1 market-data source decision.
Observed:
- Yahoo returned daily candles for `MSFT` and `ASML.AS` without a key, and Frankfurter v2 returned current and historical USD/SEK and EUR/SEK rates without a key.
- Twelve Data confirmed `ASML` on Euronext/XAMS through symbol search but requires a real key for time series; Stooq was blocked by browser verification; Alpha Vantage's free tier requires a key and has a tight documented daily call limit.
Open question:
- Before using Twelve Data as a live fallback, verify representative time-series calls with a real free key.
Refs: `docs/spikes/PriceProviderSpike.md`, `docs/DecisionLog.md`; implements: Primary Market Data Sources.

## 2026-06-13 - Currency and FX rules
Documented the Phase 0 base-currency and FX interpretation rules for the SEK-based ISK portfolio.
What changed:
- Added a dedicated currency/FX design note covering canonical FX storage, Sharesight import interpretation, brokerage handling, display rounding, and cost-basis scope.
- Recorded durable decisions for base-currency/FX handling and ISK tax scope.
- Marked the Phase 0 currency/FX planning increment complete.
Observed:
- Buy-side Sharesight `Value` is the SEK account debit and includes brokerage, but remains an audit/source field rather than a primary ledger input.
- Frankfurter FX requests should be pinned to ECB rates when supported.
Open question:
- None for currency/FX rules.
Refs: `docs/CurrencyAndFxRules.md`, `docs/DecisionLog.md`, `docs/plans/Phase0.SpikesAndSkeleton.Plan.md`; implements: Base Currency And FX Rules, ISK Tax Scope.

## 2026-06-13 - Static serving and local workflow
Made the local development and production-style serving paths concrete.
What changed:
- Added backend disk static serving for built Vite assets, with SPA index fallback and `/api` routes kept separate.
- Added `TTTB_STATIC_DIR` for overriding the built frontend directory.
- Documented Windows setup, development, check, and static-serving commands in the README.
Observed:
- Backend router tests cover root static serving, frontend route fallback, and API route precedence.
Open question:
- None.
Refs: `backend/src/api/mod.rs`, `backend/src/app.rs`, `backend/src/config.rs`, `backend/Cargo.toml`, `README.md`, `docs/DecisionLog.md`; implements: Static Frontend Serving.

## 2026-06-13 - Static root file serving
Fixed static serving so root-level files from the built frontend directory are served before the SPA fallback.
What changed:
- Served the full built frontend directory through `ServeDir` instead of only `/assets`.
- Added regression tests proving `/manifest.json` and `/assets/app.css` return static files rather than `index.html`.
- Added a test fixture cleanup guard so temp directories are removed on assertion failures.
Observed:
- `ServeDir::fallback` preserves the SPA fallback status, while `not_found_service` forces a 404.
Open question:
- None.
Refs: `backend/src/api/mod.rs`.

## 2026-06-14 - Frontend CSS import declarations
Added Vite client ambient types so TypeScript accepts side-effect CSS imports used by the app entry point.
What changed:
- Added `frontend/src/vite-env.d.ts` with the Vite client type reference.
Observed:
- `npm run check` passed from `frontend/`.
Open question:
- None.
Refs: `frontend/src/vite-env.d.ts`.

## 2026-06-14 - Backend DB foundation and schema
Added the SQLite persistence foundation for the ledger core and wired app startup through shared Axum state.
What changed:
- Added `sqlx` runtime dependencies, the first ledger migration, and the SQLite pool/migration helpers.
- Added `AppState` and threaded it through `app.rs`, `api/mod.rs`, and the existing health/import route tests.
- Extended config with `TTTB_DATABASE_URL` and a default local SQLite file, and ignored the dev database file in `.gitignore`.
Observed:
- `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
- The migration test verified schema creation plus unique, check, and foreign-key constraints against a real in-memory database.
Open question:
- None.
Refs: `backend/Cargo.toml`, `backend/migrations/0001_create_ledger_core.sql`, `backend/src/db/`, `backend/src/state.rs`, `backend/src/config.rs`, `backend/src/app.rs`, `backend/src/api/mod.rs`, `backend/src/api/health.rs`, `backend/src/api/sharesight.rs`, `.gitignore`.

## 2026-06-14 - Project statistics script
Added a repository-specific PowerShell stats report adapted from the other project and scoped to this repo's backend, frontend, docs, and scripts layout.
What changed:
- Added `scripts/project-stats.ps1` to report backend Rust and migration lines, frontend source lines, documentation, script, config/data, and dependency counts.
- Filtered out generated paths such as `backend/target`, `frontend/dist`, `frontend/node_modules`, and repo-local editor/runtime clutter.
- Removed duplicated SQL accounting after the first run so totals are internally consistent.
Observed:
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\project-stats.ps1` completed successfully.
- The current report shows 1,944 backend Rust lines, 39 migration lines, 1,011 frontend lines, 5,859 documentation lines, and 658 PowerShell script lines.
Open question:
- None.
Refs: `scripts/project-stats.ps1`.

## 2026-06-14 - DB foundation review polish
Applied low-risk review findings from the ledger database foundation pass.
What changed:
- Restored schema comments for transaction date and signed quantity semantics.
- Logged the configured database URL after startup connection, removed an unnecessary router state clone, and collapsed a redundant runtime database ignore rule.
- Added a regression test for the production SQLite `connect` path covering file creation, migration application, and foreign-key pragma setup.
Observed:
- `cargo test`, `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
Open question:
- None.
Refs: `backend/migrations/0001_create_ledger_core.sql`, `backend/src/app.rs`, `backend/src/db/pool.rs`, `.gitignore`.

## 2026-06-14 - Pure ledger domain
Added the pure domain ledger layer for transaction validation and weighted-average position derivation.
What changed:
- Introduced `backend/src/domain/transaction.rs` with transaction-kind mapping, stateless field validation, and ledger-state error types.
- Introduced `backend/src/domain/position.rs` with pure position folding, split handling, and missing-FX contamination tracking for base cost basis.
- Re-exported the domain surface from `backend/src/domain/mod.rs` and added regression tests for sign handling, validation, weighted-average math, split-as-delta, and recovery after closing a contaminated position.
- Applied review polish by strengthening the repeating-decimal average-base test, removing redundant item-level dead-code allows, making validation panic-free, and adding a debug-only sorted-input guard for position derivation.
Observed:
- `cargo test domain`, `cargo build`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
Open question:
- None.
Refs: `backend/src/domain/mod.rs`, `backend/src/domain/transaction.rs`, `backend/src/domain/position.rs`; implements: Pure Domain Ledger.

## 2026-06-14 - Transaction and instrument CRUD API
Added the manual ledger write/read API over SQLite repositories, with stateful ledger validation before each transaction write.
What changed:
- Added instrument and transaction repositories, PascalCase API DTOs, and a shared JSON error response shape.
- Added `GET/POST /api/instruments`, `GET/POST /api/transactions`, and `PUT/DELETE /api/transactions/{id}`.
- Enforced signed stored quantities, upsert-like instrument creation, Buy/Sell currency matching, Dividend rejection, and write-time ledger derivability.
- Applied review fixes so create-time ledger errors do not expose a fake transaction id, Buy/Sell currency casing is normalized to the instrument, and successful deletes verify a row was removed.
Observed:
- API integration tests cover duplicate instrument creation, transaction round trips, oversell rejection, split/dividend validation, full replacement, cross-instrument move validation, guarded and successful delete, unknown instruments, instrument validation, currency normalization, and currency mismatch.
Open question:
- None.
Refs: `backend/src/db/`, `backend/src/api/`, `backend/Cargo.toml`; implements: Ledger-Write Validity Invariant, Instrument Identity.

## 2026-06-14 - Startup database modes
Made the local startup script choose an explicit disposable local database by default and added a production database flag for portfolio data.
What changed:
- Added `-ProductionDb`, `-LocalDatabaseUrl`, and `-ProductionDatabaseUrl` to `scripts/start.ps1`.
- The script now sets `TTTB_DATABASE_URL` for its backend child process, prints the active database mode, and restores the caller's environment after shutdown.
- Documented the default local and production database paths plus override environment variables in the README.
Observed:
- `scripts/start.ps1 -SkipInstall -SkipBuild -BuildOnly` still completes without needing a database.
Open question:
- None.
Refs: `scripts/start.ps1`, `README.md`.

## 2026-06-14 - Derived holdings API
Added the read-only holdings endpoint that derives open positions from the stored transaction ledger.
What changed:
- Added `GET /api/holdings` with instrument details, quantity, native cost basis, weighted-average cost, and base-cost availability or missing-FX reasons.
- Registered the holdings route and covered weighted-average cost, split rescaling, missing-FX reporting, closed-position omission, partial-sell formatting, and multi-holding order with API integration tests.
- Formats derived money values to two decimal places at the API boundary so Decimal scale and partial-sell precision artifacts do not leak into response values.
- Fetches all transactions for holdings derivation in one ordered query and groups them in memory instead of querying once per instrument.
Observed:
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt` passed from `backend/`.
Open question:
- Should the weighted-average sell fold eliminate tiny internal Decimal residuals instead of relying on API-boundary money formatting?
Refs: `backend/src/api/holdings.rs`, `backend/src/api/mod.rs`, `backend/src/db/transactions.rs`; implements: Cost-Basis Accounting Method, Missing-FX Contamination Rule, Ledger Ordering.
