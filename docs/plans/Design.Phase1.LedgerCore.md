# Phase 1 — Ledger Core (Design & Plan)

**Status:** Draft for review · **Owner:** Lars · **Date:** 2026-06-14

This document is the design and incremental plan for Phase 1. It is ephemeral:
archive it once implemented. Durable commitments belong in `docs/DecisionLog.md`.

## 1. Purpose & scope

Deliver a usable **manual** portfolio tracker with no live prices: a SQLite-backed
transaction ledger, a transaction/instrument CRUD API, a manual-entry UI, and a
derived holdings view (positions, weighted-average cost, per-currency cost basis).

### In scope
- SQLite schema + migrations (sqlx).
- Domain ledger logic: position derivation, weighted-average cost, split handling.
- Transaction & instrument CRUD API.
- Derived holdings endpoint.
- Frontend: manual-entry form + transactions/holdings tables (TanStack Table).

### Out of scope (deferred)
- Price fetching, FX fetching, market value, day change, unrealized P&L — Phase 3.
- Realized P&L on sells — Phase 3.
- Sharesight import wiring — Phase 2 (schema must not preclude it).
- Cash balance ledger, deposits/withdrawals, tax — v1 non-goals.
- Dividend manual entry UI — later phase. The `DIVIDEND` type is admitted by the
  `transactions.type` CHECK constraint (forward-compatible) but has no manual-entry
  UI or API support yet; a Dividend create is rejected (see §2.3).

## 2. Decisions locked in this planning round

These resolve the questions Phase 1 was explicitly left to answer.

1. **Cost-basis accounting = weighted-average cost** per instrument. FIFO tax-lots
   stay in the post-v1 backlog. (`docs/CurrencyAndFxRules.md` deferred "the chosen
   portfolio accounting method" to Phase 1; this is that choice.) → **should get a
   DecisionLog entry** (see §11).
2. **Realized P&L deferred to Phase 3.** Phase 1 derived data is positions +
   average cost + per-currency cost basis only.
3. **Manual entry supports Buy, Sell, Split only; the type CHECK admits `DIVIDEND`.**
   (Owner decision, 2026-06-14.) The Phase 1 `transactions.type` CHECK constraint
   allows `BUY|SELL|SPLIT|DIVIDEND` so the schema is forward-compatible and no
   table-rebuild migration is needed when dividend support is designed. Manual entry
   and the API still support **only** Buy, Sell, and Split: a `Dividend` create is
   rejected with a `dividend_not_supported` validation error until dividend fields
   and validation exist, and derivation treats any stored `Dividend` row as a
   position no-op. (Earlier in this round the constraint was proposed to be tightened
   to `BUY|SELL|SPLIT` only; the owner reversed that to keep `DIVIDEND` admitted —
   see §11.)

### Assumptions carried into the design
- **Instrument flow:** the transaction form can create an instrument inline
  (symbol, exchange, name, type, currency) or pick an existing one. No separate
  instruments-management screen in Phase 1. Instrument identity is `(exchange,
  symbol)` (currency is an attribute of the instrument, not part of its identity);
  inline create is upsert-like — it returns the existing instrument instead of
  creating a duplicate that would fragment holdings. See §5/§6.
- **Manual FX:** for non-SEK trades the form accepts an optional trade-date
  `fx_rate_to_base`. If omitted, SEK cost basis is an explicit missing-data state,
  never zero or a guess. No FX *fetching* in Phase 1.
- **Edits/deletes** on transactions are allowed for manual correction; no
  soft-delete / audit-trail requirement yet. The ledger remains the source of truth.

## 3. Architecture & module layout

Preserve the established boundaries (`docs/DecisionLog.md` 2026-06-12 Repository
Layout) and unidirectional data flow.

- `backend/src/domain/` — pure ledger types + derivation. No axum, sqlx, HTTP, or
  provider types. Fully unit-testable.
- `backend/src/db/` — `SqlitePool`, migration runner, repositories. The only place
  SQL lives; other modules call repositories, not sqlx directly.
- `backend/src/api/transactions.rs`, `api/holdings.rs`, `api/instruments.rs` — thin
  handlers. Router gains an `AppState { pool }` injected via axum `State`.
- `main.rs` / `mod.rs` stay thin wrappers.

## 4. Conventions / implementation guardrails (folded in here per planning decision)

These are working rules for Phase 1 and beyond; kept here rather than in Agents.md
by request.

- **Money:** use `rust_decimal` for all money, prices, quantities, and FX — never
  `f64`. Serialize decimals as **strings** in JSON (the frontend already treats
  values as strings).
- **Native + FX separate:** store native value and trade-date FX separately;
  convert to SEK at read/valuation time. Never persist a pre-converted SEK value.
  See `docs/CurrencyAndFxRules.md`.
- **Missing data:** represent missing price/FX/market data explicitly with a
  reason — never as zero.
- **Domain purity:** domain logic must not depend on axum, sqlx, HTTP, or provider
  types; keep it pure and unit-testable.
- **SQL location:** keep SQL inside `db/` repositories; other modules call
  repositories.
- **Migrations:** forward-only and additive. Never edit a committed migration; add
  a new one. Test DB code against a temporary or in-memory SQLite with migrations
  applied.

## 5. Data model (Phase 1)

First migration creates the ledger tables. Tables not yet needed are deferred to
the phase that uses them (additive migrations are cheap).

- **`instruments`** — `id`, `symbol`, `exchange`, `name`, `type`
  (`STOCK|ETF|FUND`), `currency`, with a **`UNIQUE (exchange, symbol)`** constraint
  so inline creation cannot fragment holdings into duplicate instruments.
  (Provider-specific symbols are stored separately in Phase 3; not added now.)
- **`transactions`** — `id`, `instrument_id`, `type` (CHECK constraint allows
  `BUY|SELL|SPLIT|DIVIDEND`; manual entry/API use only `BUY|SELL|SPLIT` in Phase 1 —
  see decision §2.3), `trade_date`, `quantity`
  (integer), native `price` (**nullable** — required for BUY/SELL, unused for
  SPLIT), native `currency` (**nullable** — same rule), trade-date `fx_rate_to_base`
  (nullable), brokerage amount + brokerage currency, source `value` +
  **`source_currency`** (nullable audit pair; Sharesight `Value` is SEK today, but
  the currency is persisted explicitly so Phase 2 reconciliation/audits are
  unambiguous), `note` (nullable), `import_batch_id` (nullable). Rows are
  ledger-ordered by `(trade_date, id)` — see §7.
- **`import_batches`** — `id`, `source` (`SHARESIGHT|CSV|MANUAL`), `imported_at`,
  `raw_file_hash`. Created now with its durable shape (per
  `docs/Design.HighLevel.md`) so Phase 2 atomic-rollback batch tagging needs no
  immediate migration or backfill. Manual entries keep `import_batch_id = null`;
  otherwise unused by Phase 1 logic.

Deferred: `prices`, `fx_rates` (Phase 3 caches); `settings` (base currency is the
SEK constant until a setting is actually needed).

## 6. API surface (Phase 1)

REST/JSON under `/api`. Decimals as strings.

- `GET /api/instruments`, `POST /api/instruments`
- `GET /api/transactions`, `POST /api/transactions`, `PUT /api/transactions/:id`,
  `DELETE /api/transactions/:id`
- `GET /api/holdings` (derived; positions, average cost, per-currency cost basis)

Validation (**type-specific**):
- **Buy / Sell:** native `price` and `currency` required; `quantity` a positive
  integer; `fx_rate_to_base` optional (omitted ⇒ explicit missing-SEK-basis state,
  never zero); brokerage in SEK. A Sell may not drive the position below zero.
- **Split:** non-zero integer `quantity` **delta** required; `price`/`currency`
  omitted (not a cost input); rejected if no position exists yet, or if it would
  drive the position to zero or negative.
- All types: `type` ∈ {Buy, Sell, Split}.

Instrument create (`POST /api/instruments`) is **upsert-like on `(exchange,
symbol)`**: a duplicate returns the existing instrument rather than creating a
second one.

**API contract note (settle before Step 3):**
- JSON enum casing is canonical **PascalCase** (`Buy`/`Sell`/`Split`) on the wire;
  the DB stores uppercase (`BUY`); repositories map between the two.
- Decimals serialize as **strings**; `quantity` is a JSON integer.
- Errors use one shape: `{ "error": { "code", "message", "details"? } }`.
- `PUT /api/transactions/:id` is a **full replacement** of the editable fields, not
  a partial patch.

## 7. Domain logic specifics

- **Ledger ordering (deterministic):** derivation processes a single instrument's
  transactions in `(trade_date, id)` order. Weighted-average cost is order-sensitive
  when buys and sells interleave on the same `trade_date`, and same-day
  same-instrument rows must stay distinct (`docs/DecisionLog.md` 2026-06-13
  Sharesight conventions). `id` is the monotonic insert/import-row tiebreaker; a
  later explicit import sequence can replace the tiebreaker without changing this
  contract. Unit tests cover same-day buy/sell permutations so the behavior cannot
  drift.

- **Separate cost-basis components (never mix units):** native value, FX, and SEK
  brokerage stay separate per `docs/CurrencyAndFxRules.md`. A position tracks:
  - `quantity`
  - `cost_basis_native` — instrument currency; Σ native gross of the open shares
  - `cost_basis_base` — SEK; native gross × trade-date FX + SEK brokerage,
    available **only when FX is known for every contributing buy** (see missing-FX
    rule below); otherwise an explicit *unavailable* state with reasons
  - `fee_component_base` — SEK brokerage folded into `cost_basis_base`, reduced
    proportionally on sells
  The holdings API names the averages explicitly — **`average_cost_native`** and
  **`average_cost_base`** — never a single ambiguous "average cost".

- **Weighted-average cost:** a BUY adds to `quantity` and `cost_basis_native` (and
  to `cost_basis_base` when FX is known); a SELL reduces `quantity` and reduces each
  cost-basis component proportionally at the current average, leaving average cost
  per share unchanged.

- **Split (delta semantics):** a SPLIT row carries a quantity **delta** (per
  `docs/DecisionLog.md` 2026-06-13). The domain adds the delta to `quantity` and
  leaves the cost-basis totals unchanged, so both `average_cost_native` and
  `average_cost_base` per share are rescaled. Price on a split row is not a cost
  input, and a split does not change FX-availability state.

- **Missing-FX propagation (weighted-average-consistent):** native basis is always
  derivable. `cost_basis_base` is available for a position only while **every BUY
  contributing to the currently open quantity had a known `fx_rate_to_base`**. As
  soon as a BUY with missing FX is folded into the blended average, the position's
  `cost_basis_base` / `average_cost_base` becomes *unavailable* (with reasons and
  the offending transaction IDs) — under one blended average the missing-FX shares
  can no longer be isolated. It stays unavailable until the position is fully closed
  (`quantity → 0`) and rebuilt from buys that all have FX. (Per-lot recovery is out
  of scope — FIFO tax-lots are post-v1.)

- **Per-currency reporting:** report `cost_basis_native` grouped by instrument
  currency; report `cost_basis_base` (SEK) where available, otherwise the explicit
  missing-data state above.

## 8. Frontend plan

- Add **TanStack Table**; replace the mock arrays in `frontend/src/App.tsx` with
  TanStack Query against the real endpoints.
- **Add-transaction form:** instrument pick-or-create inline; type Buy/Sell/Split;
  quantity, native price + currency, optional trade-date FX, SEK brokerage, note.
- **Tables:** transactions and holdings get sort/filter; loading/empty/error
  states per `docs/VisualDesign.DarkTheme.md`.
- Follow input → action → reducer → state → render; keep the reducer pure.

## 9. Incremental steps

Each step ends with something runnable and a verification gate. Backend steps end
with `cargo clippy --all-targets -- -D warnings` then `cargo fmt`; frontend with
`npm run check` then `npm run fmt`. Document each in `EngineeringDiary.md`.

### Step 1 — DB foundation & schema
Add `sqlx` (sqlite, runtime-tokio, macros, migrate, rust_decimal, chrono) and
`rust_decimal` as runtime deps. Wire `SqlitePool` + migration runner at startup.
First migration: `instruments` (with `UNIQUE (exchange, symbol)`), `transactions`
(type CHECK = `BUY|SELL|SPLIT|DIVIDEND`; nullable `price`/`currency`; `source_currency`),
`import_batches` (`source`, `imported_at`, `raw_file_hash`).
**sqlx workflow:** use the compile-time-checked macros with **offline metadata** —
commit a `.sqlx/` directory generated by `cargo sqlx prepare`, and build/lint with
`SQLX_OFFLINE=true` so neither `cargo clippy --all-targets -- -D warnings` nor CI
needs a live `DATABASE_URL`. (Fallback if this proves friction: drop to runtime
`sqlx::query`/`query_as` functions and forgo compile-time checking.)
**Verify:** a migration test creates a temp DB, runs migrations, asserts
tables/columns plus the `UNIQUE` and `type` CHECK constraints. Clippy/fmt clean.

### Step 2 — Pure domain ledger
Domain types + position derivation in `(trade_date, id)` order: quantity, separate
`cost_basis_native` / `cost_basis_base` with explicit averages, missing-FX
propagation, split-as-delta. Validation is type-specific (BUY/SELL need
price/currency; SPLIT needs a non-zero delta and a pre-existing position, and may
not drive quantity ≤ 0).
**Verify:** `cargo test` with fixtures incl. the synthetic FX vector from
`docs/CurrencyAndFxRules.md`; same-day buy/sell **permutation** tests; a missing-FX
contamination test (base basis becomes unavailable, native stays derivable); and
split cases (valid positive split, split before any position, split driving quantity
≤ 0). Deterministic, no I/O.

### Step 3 — Transaction & instrument CRUD API
Repositories + handlers for `/api/transactions` and `/api/instruments`; `AppState`
injection; type-specific validation and the §6 API contract (PascalCase enums,
decimal strings, error shape, PUT = full replacement). `POST /api/instruments` is
upsert-like on `(exchange, symbol)`.
**Verify:** integration tests via `tower::oneshot` against a temp-DB router:
create→list round-trip; duplicate-instrument returns the existing one; validation
rejections (split without a position, sell below zero, buy missing price).

### Step 4 — Derived holdings endpoint
`GET /api/holdings` runs Step 2 derivation over Step 3 data; the response exposes
`average_cost_native` and `average_cost_base` (with the missing-data state where FX
is absent).
**Verify:** integration test seeds transactions and asserts derived holdings,
including a split scenario (post-split quantity and average cost) and a missing-FX
holding (native present, base unavailable with reasons).

### Step 5 — Frontend: manual entry + real data
TanStack Table; real-data wiring; add-transaction form; sortable/filterable tables;
dark-theme states. Bump `frontend/package.json` and `backend/Cargo.toml` versions.
**Verify:** `npm run check` + `npm run fmt`. **Human smoke test recommended:** add a
few trades and confirm holdings + cost basis match a hand calculation.

## 10. Acceptance for Phase 1
- A user can manually enter Buy/Sell/Split trades in the UI in well under 30s each.
- Holdings show correct positions, weighted-average cost, and per-currency cost
  basis derived purely from the ledger.
- A split entered as a delta yields the correct post-split position and average
  cost.
- No market value / P&L claims appear (deferred).
- All backend/domain logic is covered by tests; the app builds and lints clean.

## 11. Open items to confirm before implementation
- **DecisionLog entries** worth recording once confirmed: weighted-average-cost
  accounting (resolves the question `CurrencyAndFxRules.md` deferred to Phase 1);
  ledger ordering = `(trade_date, id)`; instrument identity = `UNIQUE (exchange,
  symbol)`; the missing-FX "contamination" rule for blended `cost_basis_base`.
- **Resolved (2026-06-14):** the Phase 1 `transactions.type` CHECK constraint keeps
  `DIVIDEND` in the allowed set (`BUY|SELL|SPLIT|DIVIDEND`). An interim proposal to
  tighten it to `BUY|SELL|SPLIT` only was reversed by the owner so the schema stays
  forward-compatible; manual entry and the API still reject Dividend creates until
  dividend fields exist. See decision §2.3.
- **Spec/plan location:** this file lives in `docs/plans/` per repo convention; the
  brainstorming default was `docs/superpowers/specs/`. Move if preferred.

## 12. References
- `docs/Design.HighLevel.md` §3 Phase 1
- `docs/DecisionLog.md` (2026-06-12 Repository Layout; 2026-06-13 native/FX
  separation, Sharesight conventions, base currency & FX rules, ISK tax scope)
- `docs/CurrencyAndFxRules.md`
- `docs/VisualDesign.DarkTheme.md`
