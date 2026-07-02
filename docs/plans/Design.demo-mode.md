# Demo Mode

## Goal

Provide a self-contained "demo mode" that boots the app with a rich, believable
portfolio (6 instruments, each with several transactions and dividends) so the UI
can be shown without touching real data, without any network access, and without
being able to modify the database.

## Decisions (confirmed)

- **Switch:** a `-Demo` flag on `scripts/start.ps1`, which sets `TTTB_DEMO_MODE=1`
  for the backend. No separate build.
- **Persistence:** ephemeral, in-memory SQLite. The demo database is created,
  seeded, and thrown away on every launch. The real ledger file is never opened.
- **Read-only:** demo mode must not be able to modify the database. Enforced at two
  levels — SQLite `PRAGMA query_only = ON` after seeding (a hard, DB-level
  guarantee) and an API-layer guard that rejects mutating endpoints with a clean
  error (good UX, and it blocks the network-touching refresh path). See
  [Read-only enforcement](#read-only-enforcement).
- **Dates:** anchored relative to *today* so the date-range presets always have
  data, regardless of when the demo runs. About 18 months of history.
- **Price granularity:** daily, for the smoothest charts.
- **Symbols/names:** fictional-but-plausible tickers/names, so nothing implies a
  real live quote.
- **UI:** a visible `DEMO` badge (driven by a `demo` flag on `/api/health`), and
  all mutating controls hidden or disabled while demo is active.

## Design overview

The frontend talks to the backend over same-origin relative URLs, so it renders
whatever dataset the backend serves. The demo/real switch therefore lives entirely
in the **backend startup path**, plus a small UI badge and control-gating.

In demo mode, `app::serve`:

1. Builds a **single-connection in-memory pool** (the proven `db::memory_pool()`
   pattern: `sqlite::memory:` with `max_connections(1)`, kept alive, so all
   requests share the same in-memory database).
2. Runs migrations and **seeds** the pool from a pure dataset module.
3. **Validates** every seeded instrument ledger derives cleanly (fail fast on a bad
   dataset).
4. Sets **`PRAGMA query_only = ON`** on the connection, making the database
   read-only from that point on.
5. **Skips** `spawn_launch_refresh` and carries a runtime `demo_mode` flag into
   `AppState`.
6. Serves. `/api/health` reports `demo: true`.

### Provider labels the read paths require

The valuation/gains/history read paths are **provider-specific**, not generic:
`api/valuation.rs` hard-codes `PRICE_PROVIDER = "YAHOO"` and
`FX_PROVIDER = "FRANKFURTER"`, and the price lookup first checks for an enabled
`YAHOO` provider-symbol row before reading prices. `portfolio.rs` and
`instrument_prices.rs` reuse those same constants.

Consequently the seed writes **prices and provider-symbol mappings under `YAHOO`**
and **FX under `FRANKFURTER`** — not `MANUAL` — so holdings, gains, value history,
and asset price charts actually read the seeded values. The provider label is only
a string in these local tables; no network is contacted, because the refresh path
is gated (below) and nothing triggers a provider fetch. (Switching the read paths
to a runtime-selected source was considered and rejected as too invasive for this
feature; seeding under the expected providers keeps the change small.)

### Read-only enforcement

Demo mode must not be able to modify the database. Two independent layers:

1. **DB level (hard guarantee).** After migrate + seed, run `PRAGMA query_only = ON`
   on the single pooled connection. SQLite then rejects every write. Ordering is
   important: the seed is the *only* write and must complete before the flag is
   set. Because the pool holds exactly one connection and an in-memory database
   only exists while that connection is alive, the flag stays in effect for the
   whole process; if the connection were ever lost, the database would be gone
   anyway, so there is no data-safety window.
2. **API level (clean UX + no network).** A demo guard rejects mutating requests
   before they reach the database or any provider, returning a clear error (e.g.
   `403 demo_read_only`) instead of a raw SQLite "readonly database" failure. It
   covers: transactions create/replace/delete, instrument create, provider-symbol
   update, all import preview/commit/rollback routes, and `prices/refresh`. Gating
   `prices/refresh` here is what actually guarantees **no Yahoo/Frankfurter network
   call** in demo mode — read-only alone would let the request reach the live
   service before failing on write. Read-only endpoints (`prices/status`, all
   `GET`s) remain available.

## Dataset shape

Six instruments spanning currencies to exercise FX conversion to the SEK base,
using fictional-but-plausible symbols/names:

| # | Kind (example)     | Currency | Notes |
|---|--------------------|----------|-------|
| 1 | large-cap tech     | USD | 2-3 buys, 1 sell, 4 dividends |
| 2 | dividend stock     | USD | 2 buys, 4 dividends |
| 3 | Swedish blue chip  | SEK | 3 buys, 3 dividends |
| 4 | Swedish stock      | SEK | 2 buys, 1 sell, 2 dividends |
| 5 | European stock     | EUR | 2 buys, 3 dividends |
| 6 | ETF                | USD | 2 buys, 2 dividends |

For each instrument the seed generates:

- A **transaction ledger**: buys/sells (integer quantities) and `DIVIDEND` rows
  with `dividend_per_share` set and `price` unset, spread across the window (first
  buy ~18 months ago), with `fx_rate_to_base` on non-SEK trades.
- A **daily `YAHOO` price series** covering the whole window up to today, produced
  by a deterministic seeded walk so results are stable and testable.
- An enabled **`YAHOO` provider-symbol mapping** so the price lookup is satisfied.

Plus **`FRANKFURTER` FX series** for each non-SEK currency (USD/SEK, EUR/SEK)
covering the same window.

The dataset is defined **as data** in a pure module (`dataset(today) -> DemoData`)
so it stays DRY and unit-testable, per the repo architecture rules.

## Phases

Each phase is independently buildable and testable.

### Phase 1 — Config flag

- Add `demo_mode: bool` to `AppConfig`, parsed from `TTTB_DEMO_MODE`
  (reuse the existing `parse_bool`), defaulting to `false`.
- **Verify:** unit tests in `config.rs` for present/absent/invalid values, and
  that `default()` is `false`.

### Phase 2 — Runtime mode in `AppState` + health flag

- Add a runtime `demo_mode: bool` to `AppState`, set from `config.demo_mode` in
  `app::serve` (default `false` everywhere else, including `for_tests`).
- Change the health handler to extract `State<AppState>` and serialize
  `demo: <state.demo_mode>`.
- **Verify:** health endpoint tests for both states — `demo: false` for normal
  state and `demo: true` for a demo-flagged state.

### Phase 3 — Seed dataset (pure)

- New module `backend/src/demo/` (name TBD: `demo` vs `seed`).
- `dataset(today: NaiveDate) -> DemoData` returning instruments, transactions,
  daily price series (provider `YAHOO`), and FX series (provider `FRANKFURTER`) —
  no I/O.
- **Verify:** unit tests assert 6 instruments; each has >=2 transactions and
  >=2 dividends; currency mix includes USD/SEK/EUR; every dividend has
  `dividend_per_share` set and `price` unset; every non-SEK trade carries
  `fx_rate_to_base`; every transaction date falls within the generated price/FX
  window; the walk is deterministic for a fixed `today`.

### Phase 4 — Seed writer

- `async fn seed(pool: &SqlitePool)` writes the dataset in a single SQL
  transaction where practical, via existing helpers: `instruments::upsert`,
  `transactions::insert`, `prices` upsert (`YAHOO`), `fx_rates` upsert
  (`FRANKFURTER`), `provider_symbols::upsert` (`YAHOO`, enabled). `DIVIDEND` rows
  are inserted directly at the repository layer (the manual API rejects dividend
  creation); document this as a seed-only path.
- After writing, **validate** each instrument ledger by deriving its
  position/performance and fail startup if any ledger is inconsistent.
- **Verify:** integration test on `db::memory_pool()` — after `seed`, all ledgers
  derive successfully; `/api/holdings`, `/api/gains?end_date=<today>`,
  `/api/portfolio/value-history`, and `/api/instruments/{id}/prices` return
  available values rather than `missing_price` / `missing_fx`.

### Phase 5 — App wiring

- In `app::serve`, when `config.demo_mode`: build the single-connection in-memory
  pool, run `demo::seed`, validate, set `PRAGMA query_only = ON`, and skip
  `spawn_launch_refresh`. Log a clear `"starting in DEMO mode (in-memory, seeded,
  read-only)"` line.
- **Verify:** a focused test that demo startup produces a pool that reads the
  seeded data but rejects a direct write (proving `query_only`), and does not
  schedule a refresh.

### Phase 6 — API read-only guard

- Add a guard (middleware or shared extractor) that, when `state.demo_mode`,
  rejects the mutating routes — transactions create/replace/delete, instruments
  create, provider-symbol update, import preview/commit/rollback, and
  `prices/refresh` — with `403 demo_read_only` (final code TBD) before touching the
  DB or any provider. Read routes and `prices/status` stay available.
- **Verify:** endpoint tests that each mutating route returns the demo error in
  demo state and behaves normally otherwise; specifically assert `prices/refresh`
  makes **no provider call** in demo mode.

### Phase 7 — Frontend badge + control gating

- Extend the health type with `demo` and render a `DEMO` badge (header/footer),
  styled per `docs/VisualDesign.DarkTheme.md`.
- When `demo` is true, hide or disable mutating controls: add/edit/delete
  transaction, the Import view, refresh/backfill actions, and provider-symbol
  toggles.
- **Verify:** a selector/view-model test that the badge shows and mutating
  controls are hidden/disabled only when `demo` is true.
- **Human testing recommended:** confirm badge placement/styling and that no
  mutating control is reachable in the running demo.

### Phase 8 — `start.ps1 -Demo`

- Add `[switch]$Demo`. When set: reject incompatible database options
  (`-ProductionDb`, `-LocalDatabaseUrl`, `-ProductionDatabaseUrl`) with a clear
  error; set `$env:TTTB_DEMO_MODE = "1"`; do **not** call `Resolve-DatabaseUrl` or
  set `TTTB_DATABASE_URL` (and ensure any pre-existing `TTTB_DATABASE_URL` is not
  passed to the demo backend); print `Database: demo (in-memory, seeded)` exactly
  once in the startup summary.
- Save and restore `$env:TTTB_DEMO_MODE` (and continue restoring
  `TTTB_DATABASE_URL` / `TTTB_PORT`) so the environment does not leak into later
  real-data runs.
- **Human testing recommended:** run `./scripts/start.ps1 -Demo`, confirm the app
  boots with the seeded portfolio, the DEMO badge shows, charts/gains render with
  no network, mutations are blocked, then run a normal `./scripts/start.ps1` and
  confirm the real database path is restored. If script tests are added later,
  cover `-Demo -ProductionDb` rejection and environment restoration.

## Open questions

1. **Concurrency:** the single-connection in-memory pool serializes DB access.
   Acceptable for a single-user demo. A named shared-cache in-memory DB
   (`file:...?mode=memory&cache=shared`) would allow multiple pool connections if
   this ever matters, at the cost of connection-lifecycle care.
2. **Read-only error code:** `403 demo_read_only` for all mutations (consistent),
   versus a more specific `409 demo_mode_refresh_disabled` for refresh. Leaning
   toward a single `403 demo_read_only`.

## Follow-ups

- Add a Decision Log entry once the approach is committed (demo mode is ephemeral,
  in-memory, seeded under the read-path providers, read-only, toggled via
  `start.ps1 -Demo`).
- Bump the frontend/backend versions when this ships (the UI shows both).
