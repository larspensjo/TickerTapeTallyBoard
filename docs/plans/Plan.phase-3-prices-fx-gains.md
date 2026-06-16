# Phase 3 Implementation Plan - Prices, FX, Valuation, and Gains View

**Goal:** Add end-of-day market data, FX caching, valuation math, and a new board view that explains unrealized gains in SEK with native-currency detail.

**Outcome:** The app can refresh latest EOD prices/FX, value the current portfolio in SEK, show market value/unrealized gain/day change on holdings, and expose a third board view named `Gains` beside `Holdings` and `Transactions`.

**Current context:** The ledger and holdings derivation already exist. The board currently switches between `holdings` and `transactions` in `frontend/src/App.tsx`. There are no `prices`, `fx_rates`, provider-symbol, or valuation tables yet. `backend/src/providers/mod.rs` is still empty.

**Non-goals for this phase:** Realtime quotes, tax reporting, dividend support, realized-gain reports for sold shares, value-history charts, allocation charts, benchmark comparison, and currency-gain attribution. Phase 3 may backfill the price cache needed by charts, but the chart UI belongs to Phase 4.

---

## Open Questions For Lars

These should be answered before or during Phase 3 review. The plan states conservative defaults so implementation can proceed once they are accepted.

> **Status:** Lars accepted the defaults except questions 3 and 4, which are resolved below and recorded in `docs/DecisionLog.md` (2026-06-16 Market Data Staleness Display Rules; 2026-06-16 Market Data Refresh Triggering).

1. **What should the Gains view include first?**
   - Default: unrealized gains for open positions only.
   - Defer: realized gains from sells, dividend income, and combined performance.

2. **Should gains be total SEK only, or also split into native price gain and FX effect?**
   - Default: show total unrealized SEK gain plus native market value/cost detail.
   - Defer: explicit currency-gain attribution, because the decision log says the schema preserves it for later.

3. **How stale may a price or FX rate be before the UI marks it as stale?** *(Resolved)*
   - Decision: Valuation uses the last close/rate on or before the valuation date. The app first tries to fetch the current price. If the fetch fails and the cached data is older than 2 trading days, show a clear visual warning (prominent stale chip). If the fetch fails but the data is within 2 trading days, show only a minor icon. Staleness is counted in trading days, and a stale value is always shown, never hidden or zeroed.

4. **What is the preferred daily refresh time on the home PC?** *(Resolved)*
   - Decision: No scheduled/time-based refresh. The app fetches fresh data as a background job on launch (with an animated loading indicator) and offers a manual refresh action for long-lived sessions. Only one refresh runs at a time.

5. **How should Yahoo symbols be supplied for all imported instruments?**
   - Default: add a provider-symbol table, seed obvious NASDAQ/NYSE mappings from existing symbols, and generate proposed European mappings from an `exchange -> Yahoo suffix` helper where known; keep proposed/unconfirmed mappings visibly flagged until confirmed.
   - Human input needed: verify the actual Yahoo symbols for each private holding, especially European listings.

6. **How far back should the first backfill go?**
   - Default: earliest ledger trade date through today for every mapped open or historical instrument, with caching so repeated runs are cheap.
   - Human choice needed: full history is best for Phase 4 charts, but a smaller backfill is faster while developing.

7. **How should totals handle rows with missing market value or unavailable SEK cost basis?**
   - Default: totals include only rows whose SEK market value and SEK cost basis are both available, and the summary reports how many rows were excluded.
   - Human choice needed: whether partial totals are acceptable or should be hidden whenever any row is incomplete.

8. **Should day change include FX movement?**
   - Default: yes. Day change compares close-to-close SEK value using previous available price and previous available FX.
   - Human choice needed: some users expect native price movement and FX movement to be separate columns.

9. **Do we accept Yahoo Finance for unattended local use after a terms re-check?**
   - Default: yes for this self-hosted app, per the existing decision, with fixture-backed tests and Twelve Data/manual CSV fallback left available.

10. **How should split-adjusted historical prices be handled before Phase 4 charts?**
    - Default: Phase 3 uses current prices for current gains and stores historical provider closes as returned. Historical value charting must explicitly revisit split adjustment before display.

---

## Working Agreement

- Keep the transaction ledger as the source of truth. Prices and FX are caches used at valuation time.
- Keep provider details behind traits. Domain valuation code must not know Yahoo or Frankfurter response shapes.
- Represent missing price/FX explicitly with reason codes. Never store or display missing data as zero.
- Store decimals as TEXT and convert through `rust_decimal`, matching the existing persistence stack.
- Add tests at the same layer as the risk: pure valuation tests for math, DB tests for migrations/repositories, API tests for contracts, and frontend checks for TypeScript/lint.
- Do not call live providers in normal tests. Use recorded/synthetic fixtures and fake providers.
- Add Engineering Diary entries only when implementation changes the application. This plan document itself does not need a diary entry.
- Bump frontend/backend versions when the implemented phase changes visible behavior.

---

## Resolved Implementation Decisions From Review

These settle the cross-cutting gaps called out in `docs/reviews/Review.phase-3-prices-fx-gains.md` before implementation starts.

1. **Provider injection lives in application state.** Extend `AppState` with a cloneable market-data service handle. The service owns injected price/FX provider trait objects, the refresh coordinator, and a single-flight guard. API handlers and launch refresh use the same service. Tests use a constructor/builder that injects fake providers.

2. **Provider traits use boxed dyn dispatch through `async-trait`.** Add `async-trait` and store providers as `Arc<dyn PriceProvider + Send + Sync>` and `Arc<dyn FxRateProvider + Send + Sync>`. This keeps API tests and launch jobs simple while avoiding provider construction inside handlers.

3. **Live HTTP uses `reqwest` with rustls.** Add `reqwest` with `default-features = false` and features `json` and `rustls-tls`. Normal tests must use fixture parsers or fake providers, not live HTTP.

4. **Single-flight refresh is service-owned.** Use a service-level async guard such as `tokio::sync::Mutex`/`try_lock` or an equivalent `AtomicBool` guard so a launch refresh and manual refresh cannot run concurrently. A second refresh request returns the current running status instead of starting another provider run.

5. **Trading-day staleness uses a Phase 3 weekday approximation.** Count Monday-Friday business days between the cached data date and the valuation date. This intentionally ignores exchange-specific holidays until a market-calendar source is added, so holiday weeks can overcount by one or more days.

6. **FX direction is pinned to the existing transaction convention.** `fx_rates.rate` stores SEK per one unit of `base`, for example `base=USD`, `quote=SEK`, `rate=10.25` means 10.25 SEK per 1 USD. This is the same direction as `transactions.fx_rate_to_base`, so valuation multiplies native market value by the latest FX rate.

7. **Reason codes reuse existing cost-basis reasons.** When SEK cost basis is unavailable, propagate the existing position reasons such as `missing_fx` with transaction ids. Do not hide those behind a generic `missing_base_cost` code. Add new reason codes only for new market-data states such as `symbol_unmapped`, `missing_price`, `stale_price`, and `missing_previous_close`.

8. **Previous close/rate means prior stored market-data point.** Day change uses the most recent stored price strictly before the latest stored price date, and the most recent stored FX rate strictly before the latest FX date. It is not calendar T-1. SEK instruments use identity FX for both legs.

---

## Proposed User Experience

### Board navigation

Extend the existing segmented control:

```text
Holdings | Gains | Transactions
```

`Gains` stays inside the existing `board` app view, not under `Import`. The top app navigation remains:

```text
Board | Import
```

### Totals band

Once valuation exists, replace the placeholder holdings count as the primary value:

```text
Portfolio
SEK 1,234,567.89

Holdings 14 | Unrealized +123,456.78 | Day +1,234.56
```

If valuation is incomplete, keep the available total visible with a warning chip such as `2 missing` or hide the total if Lars chooses strict totals in the open questions.

### Holdings view additions

Keep the current holdings columns and add valuation columns additively:

- Latest price, native currency.
- Market value, SEK.
- Unrealized gain, SEK.
- Unrealized gain percent.
- Day change, SEK/percent.
- Stale/missing price chip when needed.

### New Gains view

The Gains view is a denser explanation table focused on gain ranking and missing data. Suggested columns:

- Instrument.
- Quantity.
- Latest close and price date.
- Native market value.
- SEK market value.
- SEK cost basis.
- Unrealized gain SEK.
- Unrealized gain percent.
- Day change SEK.
- Data status.

Default sort: unrealized gain SEK descending. Add a compact filter input if the table grows enough to need it, reusing the transactions table pattern.

### Market data refresh

Market-data refresh is launch-triggered and manual, not scheduled (see DecisionLog 2026-06-16 Market Data Refresh Triggering):

- On application launch, a background job fetches fresh prices/FX. While it runs, show an animated loading indicator near the totals band or refresh action.
- Provide a visible manual refresh action for sessions that stay open long enough for data to age. The existing `Refresh` button can continue to mean "refetch UI data"; the market-data action should be visually distinct, for example `Refresh prices`.
- Either trigger invalidates `holdings`, `gains`, and any market-data status query. Only one refresh runs at a time.

Staleness is shown per the accepted rule: fresh after a successful fetch, a minor icon when a failed fetch leaves data within 2 trading days, and a prominent warning chip when data is older than 2 trading days.

---

## Backend Design

### New database tables

Add a forward-only migration, for example `0002_create_market_data.sql`.

#### `instrument_provider_symbols`

Stores provider-specific symbols outside instrument identity.

Provider CHECK constraints are intentionally narrow for v1. Adding a future provider will require a migration.

```text
id                  INTEGER PRIMARY KEY AUTOINCREMENT
instrument_id       INTEGER NOT NULL REFERENCES instruments(id)
provider            TEXT NOT NULL CHECK (provider IN ('YAHOO', 'TWELVE_DATA', 'MANUAL'))
provider_symbol     TEXT NOT NULL
currency            TEXT
enabled             INTEGER NOT NULL DEFAULT 1
created_at          TEXT NOT NULL
updated_at          TEXT NOT NULL
UNIQUE(instrument_id, provider)
```

Indexes:

- `instrument_id`.
- `(provider, provider_symbol)`.

#### `prices`

Stores one EOD close per instrument/provider/date.

`provider_symbol` is intentionally denormalized into each stored price row as an audit snapshot. The uniqueness key ignores it so a later provider-symbol correction does not collide with already cached history.

```text
id                  INTEGER PRIMARY KEY AUTOINCREMENT
instrument_id       INTEGER NOT NULL REFERENCES instruments(id)
provider            TEXT NOT NULL CHECK (provider IN ('YAHOO', 'TWELVE_DATA', 'MANUAL'))
provider_symbol     TEXT NOT NULL
date                TEXT NOT NULL
close               TEXT NOT NULL
currency            TEXT NOT NULL
fetched_at          TEXT NOT NULL
UNIQUE(instrument_id, provider, date)
```

Indexes:

- `(instrument_id, date)` for latest/prior-close lookup.
- `(date)` for refresh/status queries.

#### `fx_rates`

Stores canonical quote-to-SEK rates such as USD -> SEK.

Invariant: `rate` is SEK per one unit of `base`, matching `transactions.fx_rate_to_base`. For example, `base=USD`, `quote=SEK`, `rate=10.25` means 1 USD = 10.25 SEK.

```text
id                  INTEGER PRIMARY KEY AUTOINCREMENT
base                TEXT NOT NULL
quote               TEXT NOT NULL
date                TEXT NOT NULL
rate                TEXT NOT NULL
provider            TEXT NOT NULL CHECK (provider IN ('FRANKFURTER', 'YAHOO', 'MANUAL'))
fetched_at          TEXT NOT NULL
UNIQUE(base, quote, provider, date)
```

For v1, `quote` should be `SEK`. SEK-to-SEK can be handled in code as rate `1` and does not need a stored row.

Indexes:

- `(base, quote, date)` for latest/prior-rate lookup.

#### `market_data_refresh_runs`

Tracks manual, launch, and backfill refresh attempts without turning the app into a job system.

```text
id                  INTEGER PRIMARY KEY AUTOINCREMENT
trigger             TEXT NOT NULL CHECK (trigger IN ('MANUAL', 'LAUNCH', 'BACKFILL'))
started_at          TEXT NOT NULL
finished_at         TEXT
status              TEXT NOT NULL CHECK (status IN ('RUNNING', 'SUCCEEDED', 'PARTIAL', 'FAILED'))
message             TEXT
```

#### `market_data_refresh_items`

Optional but useful for UI diagnostics. Start with `market_data_refresh_runs` plus in-memory item details in the refresh response if that is enough; promote item persistence when diagnostics need to survive process restart.

```text
id                  INTEGER PRIMARY KEY AUTOINCREMENT
run_id              INTEGER NOT NULL REFERENCES market_data_refresh_runs(id)
kind                TEXT NOT NULL CHECK (kind IN ('PRICE', 'FX'))
instrument_id       INTEGER REFERENCES instruments(id)
symbol_or_pair      TEXT NOT NULL
status              TEXT NOT NULL CHECK (status IN ('FETCHED', 'UNCHANGED', 'MISSING', 'FAILED', 'UNMAPPED'))
reason              TEXT
rows_written        INTEGER NOT NULL DEFAULT 0
```

If this table feels too heavy during implementation, keep the run table and return item-level details from memory in the refresh response first.

### New backend modules

```text
backend/src/db/provider_symbols.rs
backend/src/db/prices.rs
backend/src/db/fx_rates.rs
backend/src/db/market_data_runs.rs
backend/src/domain/valuation.rs
backend/src/market_data/mod.rs
backend/src/market_data/refresh.rs
backend/src/providers/mod.rs
backend/src/providers/yahoo.rs
backend/src/providers/frankfurter.rs
backend/src/api/prices.rs
backend/src/api/gains.rs
```

Keep entry files thin:

- `api/prices.rs` parses HTTP requests and calls refresh/status services.
- `api/gains.rs` serializes domain valuation output.
- `market_data/refresh.rs` coordinates repositories and providers.
- `domain/valuation.rs` owns all valuation math and missing-data propagation.

### Provider traits

Use `async-trait` and boxed dyn dispatch so the market-data service can inject fake providers in tests and live providers in production. Keep the trait small.

```rust
pub struct DailyClose {
    pub provider: MarketDataProvider,
    pub provider_symbol: String,
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
}

pub struct FxRate {
    pub provider: FxProvider,
    pub base: String,
    pub quote: String,
    pub date: NaiveDate,
    pub rate: Decimal,
}

pub enum ProviderMissingReason {
    SymbolUnmapped,
    NotListed,
    MarketClosed,
    RateLimited,
    ProviderError,
    NoDataInRange,
}

#[async_trait::async_trait]
pub trait PriceProvider {
    async fn daily_history(
        &self,
        symbol: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<DailyClose>, ProviderError>;
}

#[async_trait::async_trait]
pub trait FxRateProvider {
    async fn fx_history(
        &self,
        base: &str,
        quote: &str,
        start: NaiveDate,
        end: NaiveDate,
    ) -> Result<Vec<FxRate>, ProviderError>;
}
```

Implementation notes:

- Add `async-trait` for object-safe async provider traits.
- Add `reqwest` with `default-features = false` and features `json` and `rustls-tls`.
- Parse Yahoo and Frankfurter JSON into provider DTOs, then normalize into domain structs.
- Log provider failures through `engine_logging` with provider, symbol/pair, date range, and reason.
- Keep fixture JSON under backend test fixtures or inline strings. Do not store private portfolio exports.

### Market-data service and `AppState`

Extend `AppState` from a pool-only state to a cloneable state with a service handle:

```text
AppState
   pool: SqlitePool
   market_data: Arc<MarketDataService>

MarketDataService
   price_provider: Arc<dyn PriceProvider + Send + Sync>
   fx_provider: Arc<dyn FxRateProvider + Send + Sync>
   refresh_guard: single-flight guard
```

Construction rules:

- Production startup builds Yahoo and Frankfurter providers from config and injects them into `MarketDataService`.
- `AppState::for_tests()` keeps using an in-memory migrated database and injects fake providers.
- Add a `with_market_data_for_tests(...)` or equivalent builder if tests need per-case provider behavior.
- API handlers never instantiate live providers directly.
- Launch refresh and manual refresh both call the same service entry point.

### Valuation model

Add a pure valuation module that accepts:

- An instrument.
- A derived current position.
- Latest and previous price candidates.
- Latest and previous FX candidates.
- A valuation date and staleness policy.

Return structured values rather than throwing away partial information.

```text
ValuedHolding
  instrument
  quantity
  cost_basis_native
  base_cost_basis: available | unavailable
  latest_price: available | missing
  previous_price: available | missing
  latest_fx: available | missing | identity
  previous_fx: available | missing | identity
  market_value_native: available | unavailable
  market_value_base: available | unavailable
  unrealized_gain_base: available | unavailable
  unrealized_gain_percent: available | unavailable
  day_change_base: available | unavailable
  day_change_percent: available | unavailable
  reasons[]
```

Rules:

- `market_value_native = quantity * latest_close` when price is available.
- `market_value_base = market_value_native * latest_fx`, or identity FX for SEK.
- `unrealized_gain_base = market_value_base - cost_basis_base` only when both sides are available.
- `unrealized_gain_percent = unrealized_gain_base / cost_basis_base` when cost basis is positive.
- `day_change_base = quantity * ((latest_close * latest_fx) - (previous_close * previous_fx))`.
- Previous price/FX means the latest stored market-data point strictly before the latest point, not calendar T-1.
- Staleness uses Monday-Friday business-day counting in Phase 3. Public holidays are a known limitation until a market-calendar source exists.
- Closed positions are excluded from current holdings/gains.
- Missing values carry reason codes such as `symbol_unmapped`, `missing_price`, `missing_fx`, `stale_price`, and `missing_previous_close`.
- SEK cost-basis unavailability propagates the existing position reasons, including `missing_fx` with the transaction ids that contaminated the weighted-average base cost.

Required valuation tests:

- USD and EUR market value multiply native value by `fx_rates.rate` in the same direction as `transactions.fx_rate_to_base`.
- SEK instruments use identity FX for latest and previous values.
- Day change uses prior stored market-data points, not calendar T-1.
- Weekday staleness excludes weekends and documents the holiday limitation.
- Cost-basis unavailability propagates existing position reasons rather than emitting a generic missing-base-cost code.

### API contracts

All decimal and currency values should be serialized as strings, following current API style.

#### `POST /api/prices/refresh`

Triggers a refresh for mapped instruments and needed FX pairs.

Suggested request:

```json
{
  "mode": "latest",
  "start_date": null,
  "end_date": null
}
```

Modes:

- `latest`: fetch a short recent window for open positions, enough to update latest and previous close.
- `backfill`: fetch from earliest trade date or supplied `start_date` through `end_date`.

Suggested response:

```json
{
  "run_id": 12,
  "status": "partial",
  "prices_written": 28,
  "fx_rates_written": 12,
  "unmapped_instruments": 2,
  "failed_items": 1,
  "items": [
    {
      "kind": "price",
      "instrument_id": 7,
      "symbol_or_pair": "ASML.AS",
      "status": "fetched",
      "reason": null,
      "rows_written": 5
    }
  ]
}
```

#### `GET /api/prices/status`

Returns last refresh time/status and per-instrument market-data readiness. This lets the UI show whether gains are incomplete before the user inspects the table.

#### `GET /api/gains`

Returns portfolio-level and row-level valuation.

Suggested response:

```json
{
  "as_of_date": "2026-06-16",
  "base_currency": "SEK",
  "summary": {
    "market_value_base": { "status": "available", "value": "1234567.89" },
    "cost_basis_base": { "status": "available", "value": "1111111.11" },
    "unrealized_gain_base": { "status": "available", "value": "123456.78" },
    "unrealized_gain_percent": { "status": "available", "value": "11.11" },
    "day_change_base": { "status": "available", "value": "1234.56" },
    "excluded_rows": 0
  },
  "rows": []
}
```

Rows include the `ValuedHolding` data described above.

#### `GET /api/holdings`

Augment the existing response additively with a nullable/structured `valuation` field. Keep current cost-basis fields stable so existing UI work does not break.

#### Provider-symbol endpoints

Add minimal management endpoints only if needed for human mapping:

```text
GET /api/instruments/{id}/provider-symbols
PUT /api/instruments/{id}/provider-symbols/{provider}
```

If implementation starts with seed data only, keep these endpoints for a later Phase 3 task and surface unmapped rows in the refresh response.

---

## Frontend Design

### TypeScript contracts

Add API types in `frontend/src/api/types.ts`:

- `MoneyValue` / `PercentValue` as available/unavailable tagged unions.
- `PriceSnapshot` with `status`, `date`, `close`, `currency`, `provider`, `stale`.
- `FxSnapshot` with `status`, `date`, `rate`, `base`, `quote`, `provider`, `stale`.
- `GainsSummary`.
- `GainsRow`.
- `GainsResponse`.
- `RefreshPricesResult`.

### Query hooks

Add in `frontend/src/api/queries.ts`:

- `useGains()` for `GET /api/gains`.
- `usePriceStatus()` for `GET /api/prices/status` if implemented.
- `useRefreshPrices()` for `POST /api/prices/refresh`.

On refresh success, invalidate:

- `['holdings']`.
- `['gains']`.
- `['price-status']`.

### App reducer

Change:

```ts
type BoardView = "holdings" | "transactions";
```

to:

```ts
type BoardView = "holdings" | "gains" | "transactions";
```

Add the `Gains` segment between holdings and transactions. Keep the reducer pattern and `BoardSection` loading/error/empty wrapper.

### New component

Create:

```text
frontend/src/components/GainsTable.tsx
```

Recommended behavior:

- Use TanStack Table, matching `HoldingsTable` and `TransactionsTable`.
- Default sort by `unrealized_gain_base` descending.
- Right-align all numeric columns and use existing `.number`, `.up`, `.down`, `.flat`, and `.status-chip.warning` classes.
- Use a status chip for missing/stale data instead of putting explanatory paragraphs in the app surface.
- Keep the table compact and scannable according to `docs/VisualDesign.DarkTheme.md`.

### Styling

Reuse existing table styles where possible. Add only small additions for:

- Positive/negative percent display.
- Stale price chips.
- Summary metric warning state.
- Optional market-data refresh status strip.

Do not introduce a new palette or card-heavy layout.

---

# Implementation Phases

Each phase should leave the tree in a coherent state and include its own verification. Human testing is called out where it matters.

## Phase 3.1 - Market Data Schema And Repositories

**Outcome:** The app can store provider symbols, EOD prices, FX rates, and refresh-run metadata. No live provider calls yet.

Tasks:

- [ ] Add migration `backend/migrations/0002_create_market_data.sql`.
- [ ] Add repository modules for provider symbols, prices, FX rates, and refresh runs.
- [ ] Add upsert functions that keep `(instrument, provider, date)` and `(base, quote, provider, date)` idempotent.
- [ ] Add lookup functions for latest close/rate on or before a date and previous close/rate before a date.
- [ ] Add DB integration tests using migrated in-memory SQLite.
- [ ] Keep all SQL as static strings, matching the sqlx repo convention.

Verification:

- Run `cargo test` from `backend/`.
- Run `cargo build` from `backend/`.
- Inspect the migration once before implementation proceeds, because this is the hardest Phase 3 change to unwind.

Human testing recommended:

- Review table names and provider-symbol shape before any private Yahoo mappings are entered.

## Phase 3.2 - Provider Boundary And Fixture Parsers

**Outcome:** Yahoo and Frankfurter response parsing is implemented behind traits and covered by deterministic tests.

Tasks:

- [ ] Add provider traits and normalized structs in `backend/src/providers/mod.rs`.
- [ ] Add `async-trait` and `reqwest` (`default-features = false`, features `json`, `rustls-tls`) to `backend/Cargo.toml`.
- [ ] Store providers behind `Arc<dyn PriceProvider + Send + Sync>` and `Arc<dyn FxRateProvider + Send + Sync>`.
- [ ] Add a Yahoo chart client/parser for daily equity closes.
- [ ] Add a Frankfurter client/parser for historical FX rates.
- [ ] Add fake provider implementations for service tests.
- [ ] Add recorded or synthetic JSON fixtures for `MSFT`, `ASML.AS`, `USD/SEK`, and `EUR/SEK`.
- [ ] Map provider errors to stable missing/error reason codes.
- [ ] Add contextual `engine_logging` on provider failures.

Verification:

- Run `cargo test` from `backend/`.
- Run `cargo clippy --all-targets -- -D warnings` from `backend/` once the provider code compiles.
- Confirm tests do not require network access.

Human testing recommended:

- Optionally run one manual live-provider smoke command outside the test suite and compare returned latest dates with provider websites.

## Phase 3.3 - Refresh Service, Backfill, And Mapping Readiness

**Outcome:** Manual refresh can populate caches for mapped instruments and return clear diagnostics for unmapped or failed items.

Tasks:

- [ ] Extend `AppState` with an injected `MarketDataService` handle.
- [ ] Add a test constructor/builder that injects fake price/FX providers.
- [ ] Add a service-owned single-flight guard shared by launch refresh and manual refresh.
- [ ] Add `market_data::refresh` service that gathers instruments, provider symbols, needed currencies, and missing date ranges.
- [ ] Implement `latest` refresh mode using a short recent window.
- [ ] Implement `backfill` refresh mode using earliest transaction date or request dates.
- [ ] Add `POST /api/prices/refresh`.
- [ ] Add `GET /api/prices/status` if the UI needs readiness separate from refresh response.
- [ ] Seed obvious provider symbols for NASDAQ/NYSE instruments where stored symbol equals Yahoo symbol.
- [ ] Add an `exchange -> Yahoo suffix` helper for proposed European mappings (for example `.AS`/`.DE` where known), keeping those mappings visible for human confirmation before relying on them.
- [ ] Return `symbol_unmapped` diagnostics for instruments without an enabled mapping.
- [ ] Add endpoint or seed path for manual provider-symbol corrections if the private portfolio needs them before valuation can be useful.

Verification:

- Run backend API tests with fake providers.
- Run `cargo test` from `backend/`.
- Run `cargo build` from `backend/`.

Human testing recommended:

- Enter or confirm Yahoo symbols for the real imported holdings.
- Run a manual refresh against live providers and inspect the refresh response for missing/unmapped rows.

## Phase 3.4 - Pure Valuation Engine

**Outcome:** Current holdings can be valued from cached price/FX data without provider calls.

Tasks:

- [ ] Add `backend/src/domain/valuation.rs`.
- [ ] Add value types that preserve available/unavailable states and reason codes.
- [ ] Reuse existing `derive_position` for current quantity and cost basis.
- [ ] Implement market value, unrealized gain, gain percent, and day change calculations.
- [ ] Implement trading-day staleness detection (fresh / minor-stale within 2 trading days / warning-stale beyond 2 trading days) without hiding the stale value.
- [ ] Use the Phase 3 Monday-Friday business-day approximation for staleness and document the holiday limitation in tests.
- [ ] Add summary aggregation that tracks excluded rows.
- [ ] Add a unit test pinning FX direction: `market_value_base = native_value * fx_rates.rate`, matching `transactions.fx_rate_to_base`.
- [ ] Add unit tests for USD, EUR, SEK identity FX, missing price, missing FX, stale price, propagated cost-basis unavailability reasons, and previous-close day change.
- [ ] Add a current-position split test so current quantity and current price combine correctly.

Verification:

- Run `cargo test` from `backend/`.
- Review one hand-calculated USD example and one EUR example against the expected SEK arithmetic.

Human testing recommended:

- Pick two private holdings and manually verify the app's market value/gain against calculator arithmetic using provider close and FX rate.

## Phase 3.5 - Gains And Holdings API

**Outcome:** The frontend can request a complete gains table and holdings can display valuation add-ons.

Tasks:

- [ ] Add `GET /api/gains`.
- [ ] Add additive valuation data to `GET /api/holdings`.
- [ ] Keep current holdings response fields backward compatible.
- [ ] Derive positions from transactions sorted in the same `(trade_date, id)` order used by `holdings.rs`.
- [ ] Add API tests for all-available valuation, missing price, missing FX, unavailable SEK cost basis, stale data, and excluded summary rows.
- [ ] Ensure API serialization uses string decimals and stable reason codes.
- [ ] Add route registration in `backend/src/api/mod.rs`.

Verification:

- Run `cargo test` from `backend/`.
- Run `cargo clippy --all-targets -- -D warnings` from `backend/`.
- Run `cargo fmt` from `backend/` after backend implementation is complete.

Human testing recommended:

- Use the real imported database to compare `GET /api/gains` totals with a small manual sample before building the UI around it.

## Phase 3.6 - Frontend Gains View

**Outcome:** The board has three views: Holdings, Gains, and Transactions. Gains renders valuation rows with clear missing/stale states.

Tasks:

- [ ] Add `GainsResponse` and supporting tagged-union types.
- [ ] Add `useGains`, `useRefreshPrices`, and optionally `usePriceStatus`.
- [ ] Extend `BoardView` to include `gains`.
- [ ] Add the `Gains` segmented-control button between Holdings and Transactions.
- [ ] Create `GainsTable.tsx` using TanStack Table.
- [ ] Add market value/gain/day-change columns to `HoldingsTable` if the holdings API has valuation data.
- [ ] Update the totals band to show portfolio market value and gain when available.
- [ ] Add a market-data refresh action that triggers `POST /api/prices/refresh` and invalidates the relevant queries.
- [ ] Keep missing/stale data visible through chips and unavailable cells.

Verification:

- Run `npm run check` from `frontend/`.
- Run `npm run fmt` from `frontend/`.
- Run `npm run build` from `frontend/`.

Human testing recommended:

- Start the app, switch between Holdings/Gains/Transactions, run a market-data refresh, and confirm no table layout shifts or unreadable numeric columns on desktop and a narrow viewport.

## Phase 3.7 - Launch Refresh And Operational Polish

**Outcome:** The app refreshes EOD data on launch and on demand while remaining transparent when data is incomplete.

Tasks:

- [ ] Add market-data refresh configuration to `AppConfig` (enable flag, launch-refresh toggle), with no scheduled time.
- [ ] Start a launch-time background refresh job from app startup when enabled.
- [ ] Surface an animated loading indicator in the UI while the launch refresh runs.
- [ ] Keep a manual refresh action available at all times for long-lived sessions.
- [ ] Ensure only one refresh runs at a time (single-flight guard).
- [ ] Log refresh start/end/status with enough context to diagnose failures.
- [ ] Surface last refresh status in the UI.
- [ ] Add version bumps for backend and frontend.
- [ ] Add Engineering Diary entries for the implemented application changes.
- [ ] Update `docs/DecisionLog.md` only if implementation resolves one of the open questions as a durable project rule.

Verification:

- Run `cargo build` from `backend/`.
- Run `cargo clippy --all-targets -- -D warnings` from `backend/`.
- Run `cargo fmt` from `backend/`.
- Run `npm run check` from `frontend/`.
- Run `npm run fmt` from `frontend/`.
- Run `npm run build` from `frontend/`.

Human testing recommended:

- Launch the app and confirm the background refresh runs with a visible loading indicator.
- Use the manual refresh action in a long-lived session and confirm refresh status, log output, and gains values update.

---

## Acceptance Criteria

Phase 3 is done when:

1. Every open holding with a confirmed provider symbol has latest cached EOD price data.
2. Every needed non-SEK currency has cached SEK FX data for latest and previous valuation dates.
3. `POST /api/prices/refresh` is idempotent and reports unmapped/missing/failed items explicitly.
4. `GET /api/gains` returns per-instrument market value, unrealized gain, gain percent, and day change with explicit unavailable states.
5. Holdings display market value and unrealized P&L without breaking existing cost-basis behavior.
6. The board offers `Holdings`, `Gains`, and `Transactions` as sibling views.
7. Missing prices, missing FX, stale data, and unavailable base cost are visible in the UI and never rendered as zero.
8. Backend tests, clippy, and format pass.
9. Frontend check, format, and build pass.
10. At least one USD holding and one EUR holding are manually spot-checked against provider close, FX rate, and weighted-average cost basis.

---

## Risks And Mitigations

| Risk | Mitigation |
|---|---|
| Yahoo symbol mapping is wrong for European instruments | Store provider symbols separately, show unmapped/missing states, and require human confirmation for private holdings. |
| Provider responses change | Keep response parsing isolated, test against fixtures, and surface provider errors instead of hiding them. |
| Free provider rate limits interrupt backfill | Cache aggressively, fetch missing ranges only, and keep Twelve Data/manual CSV fallback available. |
| FX or price gaps make totals misleading | Use tagged unavailable values and summary excluded-row counts. |
| Split-adjusted historical prices confuse future charts | Phase 3 current gains use current quantity/current price; Phase 4 value history must revisit split adjustment before chart display. |
| Day-change semantics surprise the user | Resolved: day change is close-to-close SEK (price + FX). Label the column as SEK and keep the rule explicit. |

---

## Suggested First Review Checklist

- [x] Lars confirms the Gains view starts with unrealized open-position gains only.
- [x] Lars confirms stale-data threshold (2 trading days; minor icon within, warning beyond).
- [x] Lars confirms refresh model (launch background fetch + manual refresh; no scheduler).
- [ ] Lars confirms partial-total behavior.
- [ ] Lars provides or approves Yahoo symbols for current imported holdings.
- [x] Lars confirms day change includes FX movement (close-to-close SEK).
