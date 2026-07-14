# Portfolio Tracker — Project Document

**Status:** Draft for planning · **Owner:** Lars · **Date:** 2026-06-12

## 1. Summary

A self-hosted portfolio tracking application for stocks, ETFs, and funds, running on a home PC. Single Rust binary serving a TypeScript web frontend on the local network. Data is owned locally (SQLite), with initial portfolio history imported from Sharesight.

### Goals
- Track holdings, transactions, dividends, and performance for stocks, ETFs, and funds.
- Multi-currency from day one (e.g. SEK base currency with USD/EUR-denominated holdings).
- Import full transaction history from Sharesight (All Trades Report export); manual entry as fallback and for ongoing trades.
- Daily (end-of-day) price updates — no realtime/streaming requirement.
- Accessible from any device on the LAN via browser.

### Non-goals (v1)
- Options, crypto, bonds, real estate.
- Multi-user accounts / authentication beyond LAN trust.
- Tax reporting (deklaration/K4) — schema should not preclude it, but no v1 feature.
- Broker integration / automatic trade sync.

## 2. Architecture

### Stack

| Layer | Choice | Rationale |
|---|---|---|
| Backend | Rust, **axum**, **tokio** | Single async binary; team's core competence |
| Database | **SQLite** via **sqlx** | Zero-ops, single-file backup, compile-time checked SQL |
| Money/quantities | **rust_decimal** | Exact decimal arithmetic; never f64 for money |
| Frontend | **Vite + React + TypeScript** | Mature ecosystem for data-heavy UI |
| Server state | **TanStack Query** | Caching, refetch, optimistic updates |
| Tables | **TanStack Table** | Sorting/filtering/virtualization for holdings & transactions |
| Charts | **Lightweight Charts** (TradingView OSS) | Purpose-built financial time series, ~45 KB |
| Static serving | tower-http `ServeDir` | Backend binary serves the built frontend |
| Scheduling | tokio task (cron-like) | Daily EOD price + FX fetch |
| Testing | **`cargo test`** + **Vitest** (+ React Testing Library / jsdom) | Pure logic on both sides unit-tested; one runner per stack |

### Deployment model
- **Dev:** `cargo run` (API on :8080) + `npm run dev` (Vite on :5173, proxying `/api`).
- **Prod (home PC):** `npm run build` → static files embedded or served from disk by the Rust binary. One executable + one `.db` file. Backup = copy the file. Runs as a Windows scheduled task or service; reachable at `http://<host>:8080` on the LAN.

### Testing strategy

The architecture is deliberately shaped so the parts that are easy to get wrong are pure and unit-testable on both sides of the stack. Tests target behavior and public contracts — reducer transitions, emitted effects, derived view-models — not internal wiring.

- **Backend (`cargo test`):** the valuation/ledger domain (`backend/src/domain/`) is pure (no axum/sqlx/IO) and carries the bulk of the tests — position derivation, split adjustment, FX carry-forward, value/price-history builders. HTTP handlers get focused integration tests via the in-process router against an in-memory SQLite pool.
- **Frontend (Vitest):** the unidirectional `input → action → reducer → state → render` flow keeps logic out of components. Reducers (e.g. board UI state), view-models (`assetViewModel`), and selectors (dashboard top-movers, allocation, chart series) are pure functions over typed API shapes and are unit-tested with Vitest in a fast node environment. React Testing Library + jsdom are available (opt-in per file) for component and interaction tests where rendering or events are the behavior under test.
- **One gate per stack.** Backend completion runs `cargo clippy --all-targets -- -D warnings` then `cargo fmt`; frontend `npm run check` runs `tsc --noEmit`, Biome lint, **and** `vitest run` together, so types, lint, and tests pass or fail as a unit.
- **Convention going forward:** new pure logic ships with a test in the same change; existing untested pure units (`assetViewModel`, the board reducer, formatting helpers) are backfilled opportunistically when next touched, rather than as a separate retrofit pass. Bug fixes add a regression test when practical.

### Core design principle: transaction ledger as source of truth
Positions, cost basis, realized/unrealized P&L, and historical performance are all **derived** from an append-only transaction ledger (BUY, SELL, DIVIDEND, FEE, SPLIT, DEPOSIT, WITHDRAWAL). No stored "holdings" table as primary data. This enables point-in-time portfolio reconstruction, performance charts, and future tax-lot accounting without schema rewrites.

### Data model (essentials)

```
instruments      id, symbol, exchange, name, type (STOCK|ETF|FUND),
                 currency, external_ids (e.g. Yahoo symbol)
transactions     id, instrument_id (nullable for cash tx), type, trade_date,
                 quantity, price, fees, currency, fx_rate_to_base (nullable),
                 note, import_batch_id (nullable)
prices           instrument_id, date, close, currency        -- EOD cache
fx_rates         base, quote, date, rate                     -- EOD cache
import_batches   id, source (SHARESIGHT|CSV|MANUAL), imported_at, raw_file_hash
settings         key, value (base_currency, data provider keys, …)
```

Multi-currency rules: every transaction stores its native currency; the FX rate to base currency is captured at trade date (from import where available — Sharesight exports include the exchange rate — otherwise from the fx_rates cache). Valuations convert at the rate of the valuation date; cost basis converts at the trade-date rate.

### Market data
- **Requirement:** EOD closes for Stockholm-listed (ST) and US-listed instruments, plus daily FX (USD/SEK, EUR/SEK).
- **Approach:** provider behind a Rust trait (`PriceProvider`) so the source is swappable. Candidates: Yahoo Finance unofficial API (free, good Nordic coverage, unofficial), Twelve Data / Alpha Vantage (official free tiers, rate-limited — fine for EOD), EODHD (paid, reliable). FX from the same provider or ECB reference rates (free, official).
- All fetched data is cached in SQLite; the app never re-fetches a (instrument, date) it already has. Scheduled fetch once daily after market close; manual "refresh now" button.
- **Decision needed (Tech Lead):** pick primary provider in Phase 0 spike; the trait keeps the cost of being wrong low.

### Sharesight import
Sharesight's **All Trades Report** exports to XLS/Google Sheets with trade date, type (buy/sell/adjustment), quantity, price, fees, exchange rate, and trade value. Importer parses the export (convert XLS→CSV or parse XLS directly via `calamine` crate), maps rows to ledger transactions, matches/creates instruments, and tags everything with an `import_batch_id` so a bad import can be rolled back atomically. Dividends may need a second report (Taxable Income / dividend report) or manual entry — verify during Phase 0 spike.

TODO: Avanza import rows already parse and count dividends, but dividend rows are not written into the ledger yet. When dividend support lands, route them into the income/gains model instead of leaving them as import-only metadata.

### API surface (v1 sketch)
REST/JSON: `GET/POST /api/transactions`, `GET /api/holdings` (derived), `GET /api/instruments`, `GET /api/portfolio/value-history`, `GET /api/instruments/:id/prices`, `POST /api/import/sharesight`, `POST /api/prices/refresh`.

## 3. Implementation phases

Each phase ends with something usable. Estimates assume one experienced developer, part-time; treat as relative sizing.

### Phase 0 — Spikes & skeleton ✅ Done (2026-06-13)
- **Skeleton:** `backend/` (axum/tokio) + `frontend/` (Vite/React/TS), dev `/api` proxy, local start script, `engine_logging`, `/api/health` exposing both frontend and backend versions, and disk static serving of the built frontend.
- **Spike A (Sharesight):** parsed the real All Trades CSV (189 rows: 105 buys, 83 sells, 1 split) and fixed the CSV/FX conventions. All holdings are EUR/USD across Euronext, Frankfurt, Xetra, NASDAQ, and NYSE — no Stockholm-listed or fund-type rows, and no dividends in this report (so v1 takes dividends by manual entry). Split quantities use delta semantics (5/1 confirmed against the live position).
- **Spike B (prices/FX):** chose Yahoo Finance chart endpoint for equity EOD/history and Frankfurter v2 (pinned to ECB) for FX→SEK. Twelve Data is the keyed fallback; manual price CSV import is the last resort. Missing prices are represented explicitly with a reason, never as zero.
- **Currency/FX:** SEK base; native price and trade-date FX stored separately (never a pre-converted SEK value); the portfolio is an ISK account, so v1 does no tax calculation. Rules captured in `docs/CurrencyAndFxRules.md`.
- Exit criteria met. Detail in `docs/DecisionLog.md` and `docs/spikes/`.

### Phase 1 — Ledger core ✅ Done (2026-06-14)
- SQLite schema + migrations (sqlx).
- Transaction CRUD API + manual-entry UI (form + TanStack Table list with filter/sort).
- Derived holdings endpoint: current positions, average cost, per-currency cost basis.
- **Deliverable:** usable manual portfolio tracker (no live prices yet).

### Phase 2 — Sharesight import ✅ Done (2026-06-15)
- File upload endpoint + parser (`calamine`/CSV), instrument matching/creation, batch tagging, dry-run preview UI ("N trades, M new instruments, K warnings") before commit, rollback per batch.
- **Deliverable:** full historical portfolio loaded from Sharesight; spot-check positions against Sharesight's own numbers as acceptance test.

### Phase 3 — Prices, FX & valuation ✅ Done (2026-06-18)
- `PriceProvider` trait + chosen implementation; FX rate fetching; SQLite caching; daily scheduled job + manual refresh.
- Holdings view gains market value, unrealized P&L, day change — all in base currency with native-currency detail.
- Backfill historical prices for charting.
- **Deliverable:** live (EOD) portfolio valuation in SEK.

### Phase 4 — Charts & dashboard ✅ Done (2026-07-14)
- Value-history and per-instrument price-history APIs ✅ Done (2026-06-22).
- Per-instrument price chart ✅ Done (2026-06-22).
- Portfolio value-over-time chart (Lightweight Charts) ✅ Done (2026-06-22).
- Dashboard landing page with total value, day/unrealized change, top movers, and portfolio gains waterfall ✅ Done (2026-07-14).
- **Deliverable:** v1 feature-complete.

### Phase 5 — Hardening & deployment (~0.5–1 week)
- Embed frontend in binary (or ship alongside), Windows scheduled-task/service setup, automated SQLite backup (daily copy + retention), structured logging, basic error surfacing in UI.
- **Deliverable:** running unattended on the home PC.

### Backlog (post-v1, prioritized later)
Dividend tracking & reinvestment view · performance metrics (TWR/MWR, vs. index benchmark) · stock splits & corporate actions handling · watchlist · CSV export · tax-lot (FIFO) realized-gain report · command-based undo/redo with a visible activity/stack UI (see `docs/Design.CommandUndo.md`) · auth if ever exposed beyond LAN.

## 4. Risks & open questions

| # | Risk / question | Mitigation |
|---|---|---|
| 1 | Sharesight export may not include dividends or fund transactions cleanly | Phase 0 Spike A with real data before committing import design |
| 2 | Free price providers: rate limits, Nordic fund coverage gaps, unofficial APIs breaking | Provider trait; cache aggressively; budget option (EODHD) as fallback |
| 3 | FX correctness (trade-date vs. valuation-date rates) is the most common bug source in multi-currency P&L | Encode rules in one tested valuation module; unit tests with known fixtures |
| 4 | Corporate actions (splits) silently corrupt derived positions if ignored | SPLIT transaction type exists in schema from day one; handling can ship post-v1 |
| 5 | Scope creep toward realtime data | Explicit non-goal; EOD only in v1 |

## 5. Acceptance criteria (v1)

1. Full Sharesight history imported; positions match Sharesight within rounding tolerance.
2. New trades can be entered manually in < 30 seconds.
3. Portfolio value in SEK updates automatically each day without intervention.
4. Holdings table and value-history chart load in < 1 s on LAN.
5. Database survives and restores from a single-file backup.
