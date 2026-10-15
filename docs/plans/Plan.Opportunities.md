# Plan — Opportunities page (recent fallers to buy more of, recent risers to take gains on)

## Summary

A new read-only **Opportunities** page in the portfolio navigation, next to
Rebalance, lists every currently held asset ordered by how much its share price
moved over a recent window (1 week, 2 weeks or 30 days; default 1 week).
"Biggest fallers" first is the buy-more view; "Biggest risers" first is the
take-gains view. Beside the change, each row shows the latest price, how today's
price compares with the user's average purchase price (with a "Below your
average" marker), the existing SEK gain including fees and currency (with a
stale signal when either its price or its exchange rate is stale), and — as
context only — the gap to the asset's conviction target.

The backend owns the window: a dedicated `GET /api/opportunities?window=7d|14d|30d`
endpoint resolves dates from the application clock, applies price-pair validity
guardrails, and returns the resolved period, which the page uses to name the
column. The launch/manual "latest" refresh window widens so it always covers the
longest window plus the start-close tolerance.

Done = the page works in both orders and all three windows, persists its
settings, never names a window it is not showing, has backend contract tests and
frontend reducer/view-model/component tests, and the documents, decision log and
versions are updated.

## Settled decisions (from the brief — do not re-open)

- **Held assets only** (`quantity > 0`). Watchlist rows and closed positions are
  excluded.
- **Ordering = recent price change over the selected window only.** Biggest
  fallers first by default; reversed = biggest risers first. The
  price-vs-your-average comparison is a context column, with a "Below your
  average" marker on rows priced below it. Two-tier ordering and a weighted
  combined score are rejected.
- **A separate page**, not an extension of Rebalance.
- **Page-local window switch with exactly three choices:** 1 week (7 days),
  2 weeks (14 days), 30 days; default 1 week. This is a deliberate exception to
  the 2026-06-29 "Shared Dashboard And Gains Performance Interval" entry and is
  recorded in the decision log. No 14d/30d presets are added to the shared
  Gains/Dashboard interval. The endpoint takes its own window parameter and the
  backend resolves the dates ("One Clock Owns Today"). The column header naming
  the window comes from the **resolved period in the response**, never from the
  switch state ("The Chart Panel Never Names A Period It Is Not Plotting").
- **The split guard is kept and unconditional** (settled after review): a share
  split in the ledger between the start and end closes makes the change
  unavailable (`split_in_period`), and Entry A records it without conditions.
- **Accepted after review:** the target-gap context column (below), hand-priced
  holdings showing "No price source" until hand-entered prices land, the
  refresh-cost finding, and "next minor version" bump wording.

## Brief items settled by Claude, as this plan implements them

- Name "Opportunities", nav item directly after Rebalance, route `/opportunities`.
- No "significant" threshold: every held asset is listed.
- Price change and price-vs-average are in the instrument's native currency
  (close-to-close; `average_cost_native`). A separate column shows the existing
  SEK unrealized gain % (fees and FX included, the same number Holdings shows).
  The two are named in plain language so they cannot be confused.
- Guardrails: bounded start-close age, same price source for both closes, stale
  end prices shown with the existing stale chip (and stale FX shown beside the
  SEK gain, which uses it), unavailable changes sorted last
  in both orders and never shown as zero, widened latest-refresh window.
- Ex-dividend drops: an accepted, documented limitation with a short note on
  the page; no detection.
- Conviction-target context column, reusing the single source of truth for
  targets; no influence on ordering, no actions.
- Window and order persist client-side.
- Read-only: no trade actions.

**One refinement (accepted).** The brief describes the context column as
"portfolio weight vs conviction target". This plan shows the **target gap** — the
same signed gap percent and diverging bar the Holdings Target column already
renders from the same backend derivation — rather than a new current-weight vs
target-weight pair. Reasons: Holdings' `Portfolio %` uses the whole portfolio as
its denominator while targets are relative to the conviction pool, so showing
the two side by side would compare numbers with different denominators; the gap
already answers "is this buy candidate already overweight?"; and reusing the
Holdings presentation keeps one look for one figure (DecisionLog 2026-07-07
"Target gap colouring follows plain sign"). Blank when the asset has no target.

## Corrections to the brief's picture of the code

- **Hand-entered (MANUAL) prices are not valued today.** `PRICE_PROVIDER_PRECEDENCE`
  is `[Yahoo, NasdaqNordic]`; MANUAL appears only in the provider enum and a
  frontend label, and `docs/plans/Plan.HandEnteredPrices.md` is still an
  unimplemented plan. A hand-priced holding therefore currently has no price
  coverage and appears on this page with "No price source", sorted last. Because
  the endpoint reads closes only through `market_data::effective_prices`, it will
  pick MANUAL closes up — with their stale chip and the same-source rule — when
  that work adds MANUAL to the effective resolution. No extra work is planned
  here for it.
- The `latest` refresh issues **one provider request per enabled mapping and one
  FX request per currency regardless of window length** (Yahoo chart
  `period1/period2`, Nasdaq `fromDate/toDate`, Frankfurter time series). Widening
  the window grows each response (roughly 25 instead of 10 trading-day rows) and
  the upserts per run, not the request count. The phase that widens it still
  measures run duration before and after.

## Planner decisions (routine, reversible — recorded so review can see them)

- **Dedicated endpoint, not the gains machinery.** `GET /api/gains` carries
  Modified Dietz/XIRR period reconstruction, `method`, `include_closed`, and the
  shared period presets the user chose not to extend; its rows lack conviction
  targets; and its start price comes from an unbounded on-or-before lookup.
  A small GET-only endpoint mirrors `/api/rebalance`: it works in read-only demo
  mode, carries `data_revision` so it fits `useVersionedQuery`, and keeps the
  window presets a backend contract.
- **Window values `7d`, `14d`, `30d`** (the first matches the existing Gains `7d`
  spelling). Absent `window` means `7d`; any other value is
  `400 invalid_window`, following the `rank_by` precedent (absent defaults,
  unknown rejected). UI labels: "1 week", "2 weeks", "30 days".
- **Period anchoring:** `start_date = today − N calendar days`, `end_date = today`,
  both from the state clock — the same rule as the Gains `7d` preset, so the
  response can name one period for every row. Per-row closes are then:
  - **End close** = the holding's latest effective close on or before today —
    exactly the price snapshot Holdings and Rebalance value with (same source,
    same freshness).
  - **Start close** = the latest effective close on or before `start_date`,
    accepted only within the tolerance below.
- **Start-close tolerance: at most 3 trading days** before `start_date`, counted
  with the existing weekday calendar approximation (`trading_days_stale`; DecisionLog
  2026-06-16 "Market Data Staleness Calendar Approximation"). Justification: a
  `start_date` on a weekend already resolves to Friday at 0 days; 3 weekdays covers
  the longest routine closures in the user's markets — Swedish Christmas
  (24–26 December falling on weekdays) and Easter (Good Friday plus Easter Monday)
  — because the weekday approximation counts those holidays as trading days. It
  sits one day above the 2-trading-day staleness warning threshold, so a start
  close that would itself be "warning stale" relative to its own date is still
  rejected after one more missing day. In calendar terms the accepted start close
  is never more than 5 days before `start_date`. A start close older than that
  makes the change **unavailable**, never measured against an arbitrarily old
  price.
- **Validity rules for the change** (each failure is an explicit reason, the row
  stays listed and sorts last):

  | Condition | Reason code | Plain-language label |
  |---|---|---|
  | No enabled price source | `no_price_source` | "No price source" |
  | Price source but no close at all | `missing_latest_close` | "No price yet" |
  | Latest close is on or before `start_date` | `no_close_since_period_start` | "No price since {start}" |
  | No close on or before `start_date` | `missing_start_close` | "No price before {start}" |
  | Start close older than the tolerance | `start_close_too_old` | "No price near {start}" |
  | Start and end closes from different sources | `start_close_source_mismatch` | "Prices from different sources" |
  | Start and end closes in different currencies | `start_close_currency_mismatch` | "Prices in different currencies" |
  | Start close is zero | `zero_start_close` | "Start price is zero" |
  | A ledger split dated after the start close and on or before the end close | `split_in_period` | "Share split in this period" |

  `no_close_since_period_start` also guarantees the start and end closes are
  different rows, so a single old close can never produce a "0.00%" change.
- **The split guard is unconditional.** Yahoo closes are split-adjusted and the
  widened refresh re-fetches them, but a split whose closes were not re-fetched
  (failed refresh) or a Nasdaq Nordic series with unverified split adjustment
  would otherwise show a false −50% move at the top of the buy list. The cost is
  a blank change for at most 30 days after a rare split. The guard reads split
  dates from the ledger (`LedgerTransaction.kind == Split`, `trade_date`).
- **Stale end prices are shown, never hidden.** The latest-price cell carries
  the existing freshness signal (minor icon / warning chip) whether or not the
  change is available.
- **Price vs your average** = `(latest close − average_cost_native) /
  average_cost_native × 100`, exact decimal on the backend. The average is the
  native weighted-average purchase price **excluding fees** (fees are SEK-only),
  i.e. the same level as the asset chart's break-even line (DecisionLog
  2026-07-09). Unavailable when there is no latest close, when the close's
  currency differs from the instrument currency (`price_currency_mismatch`), or
  when the average is zero (`zero_average_cost`). The backend also returns
  `below_average: true|false|null` from an exact decimal comparison
  (`latest close < average_cost_native`), so the frontend never infers the
  marker from a rounded string. A latest price a hair below the average shows
  the marker even though its percentage displays as `0.00`; only exact equality
  (or above) shows no marker.
- **SEK gain column** = the holding's existing `unrealized_gain_percent`
  availability, computed by the same `value_position` path as Holdings. Because
  `value_position` keeps the gain available when the exchange rate is stale, the
  response also carries `unrealized_gain_freshness` — the staler of the latest
  price's and the latest FX rate's freshness (`staler_freshness`, exactly as
  Rebalance combines them; FX is always fresh for SEK instruments) — and the SEK
  gain cell shows the existing minor icon / warning chip from it (DecisionLog
  2026-06-16 "Market Data Staleness Display Rules"). A fresh share price with an
  old exchange rate therefore never reads as a current SEK gain. The native
  columns (change, latest price, vs average) do not use FX and keep the
  price-only freshness.
- **Conviction target** = the Holdings `conviction_target` object, derived once
  over the full eligible pool (including watchlist members, per the 2026-07-08
  entry) **before** filtering to held rows, so the numbers are identical to
  Holdings. The response type and builder move out of `api/holdings.rs` into a
  shared `api/conviction_target.rs` used by both endpoints; the frontend reuses
  `targetGapBar` and the existing `target-gap-track` styles.
- **Ordering is view derivation in a pure frontend selector:** sort by the
  2-decimal `change_percent` (parsed for ordering only — display-only float use
  under the 2026-06-18 entry), ascending for fallers, descending for risers; ties
  by symbol then exchange; rows whose change is unavailable after all available
  rows in both orders, by symbol. No column-header sorting: the page answers one
  ordering question, so it uses one two-way control rather than TanStack sorting.
- **Persisted view settings** (2026-07-10 "View settings are persisted
  client-side"): `opportunities.window` (`"7d" | "14d" | "30d"`, default `"7d"`)
  and `opportunities.order` (`"fallers_first" | "risers_first"`, default
  `"fallers_first"`), loaded with `loadSetting` + `isOneOf` so unknown values fall
  back to the defaults.
- **Page state** follows the Rebalance pattern: an exported pure
  `opportunitiesPageReducer` (`windowChanged`, `orderChanged`) initialised from
  storage, saved in an effect; the component consumes a pure view-model.
- **Naming the period:** the column header and the panel subtitle derive only
  from `response.period`. While the displayed response's `period.window` differs
  from the requested window (kept previous data), the header drops the period
  name ("Price change"), the subtitle is replaced by an "Updating…" chip, and the
  rows stay — they simply stop claiming a period.
- **Closes serialize at full stored precision** (as the per-instrument price
  history endpoint does) and render with the adaptive unit-price formatter
  (DecisionLog 2026-07-15). Percentages are 2-decimal strings with negative zero
  normalised to `0.00`.
- **Controls live in the panel header**, not in a page-level action row
  (DecisionLog 2026-10-05): two segmented controls — window ("1 week",
  "2 weeks", "30 days") and order ("Biggest fallers", "Biggest risers") — reusing
  the existing `.segmented-control` with `aria-pressed`.
- **"Below your average" marker** is a neutral status chip in the vs-average
  cell. The soft semantic chips are reserved for movers/alerts, and the cell's
  number already carries the plain-sign colour.
- **Ex-dividend note**, always shown under the table in `--text-muted`: "Price
  change compares closing share prices in each asset's own currency. Dividends are
  not added back, so a share can look like it fell on the day it starts trading
  without its dividend."
- **Column names** (plain language, no jargon): "Instrument", "{1-week|2-week|30-day}
  price change", "Latest price", "Vs your average price", "Your gain incl. fees &
  currency", "Vs target". Per-row tooltip on the change names the actual close
  dates used ("From 7 Oct close 412.50 to 15 Oct close 377.80").
- **Empty state** (no held assets): "No holdings yet." plus the existing
  `LedgerEntryInvitation` `elsewhere` variant (omitted in demo), as on Holdings.
- **Logging:** the handler logs internal inconsistencies and failed start-close
  lookups with `crate::engine_error!`, naming the operation, instrument id,
  exchange/symbol, window and start date (pattern of `log_internal_inconsistency`
  in `api/rebalance.rs`).

## Not in scope

- Trade actions, order suggestions, or anything that writes.
- Dividend-adjusted price series or ex-dividend detection.
- Watchlist rows or closed positions.
- Changing the shared Gains/Dashboard interval or its presets.
- Hand-entered price support (separate plan).
- Column-header sorting or filtering on this page.

## Code map (verified starting points)

| Area | File | Notes |
|---|---|---|
| Valued holdings | `backend/src/api/valued_holdings.rs` | `load_valued_holdings` builds every instrument's ledger, position, valuation (latest price snapshot with source and freshness) and conviction. It drops the ledger after deriving the position; the split guard needs it retained. |
| Effective closes | `backend/src/market_data/effective_prices.rs` | `effective_latest_on_or_before` (per-source lookup, newest wins, precedence on ties). Only enabled mappings in `PRICE_PROVIDER_PRECEDENCE` participate. |
| Staleness calendar | `backend/src/domain/market_snapshot.rs` | `trading_days_stale`, `data_freshness`, `DataFreshness`. |
| Source-mismatch precedent | `backend/src/domain/day_change.rs` | `PreviousCloseSourceMismatch`. |
| Average cost | `backend/src/domain/position.rs` | `average_cost_native()` = native cost basis (no fees) / quantity; split deltas raise quantity so the average is per post-split share. |
| Targets | `backend/src/api/holdings.rs` (`ConvictionTargetResponse`, `build_conviction_target`), `backend/src/domain/conviction.rs` (`derive_targets`) | Builder moves to a shared API module. |
| Endpoint precedent | `backend/src/api/rebalance.rs` | GET-only, `data_revision`, `invalid_rank` parsing pattern, demo test. |
| Router | `backend/src/api/mod.rs` | Add `.route("/opportunities", get(opportunities::handler))`. |
| Serialization helpers | `backend/src/api/valuation.rs` | `AvailabilityResponse`, `serialize_availability`, `serialize_freshness`, `staler_freshness`, `money_string`. |
| Stale-FX behaviour | `backend/src/domain/valuation.rs` (~line 97) | Market value and unrealized gain stay available with a stale FX rate, so availability alone cannot signal it; Rebalance combines price and FX freshness (`api/rebalance.rs`, `serialize_trade`). |
| Test helpers | `backend/src/api/test_support.rs`, `backend/src/state.rs` (`with_clock`), `backend/src/clock.rs` (`Clock::fixed`) | `seed_valued*` use the system date; Opportunities tests need explicit close dates under a pinned clock. |
| Latest refresh window | `backend/src/market_data/refresh.rs` (`LATEST_REFRESH_WINDOW_DAYS = 14`, `refresh_window`) | Fake providers record requests (`FakePriceProvider::calls`). |
| Demo data | `backend/src/demo/mod.rs` | Daily prices up to today, so demo exercises every window. |
| Query layer | `frontend/src/api/queries.ts` (`useVersionedQuery`, `useRebalancePlan`), `frontend/src/api/versionedQueries.test.tsx` | New `useOpportunities(window)`; the test enumerates `use*` hooks. |
| Types | `frontend/src/api/types.ts` | `AvailabilityValue`, `Instrument`, `ConvictionTarget`. |
| Page precedent | `frontend/src/components/RebalancePage.tsx`, `rebalanceViewModel.ts`, `RebalancePage.reducer.test.ts`, `RebalancePage.test.tsx` | Reducer + view-model + component test pattern. |
| Persistence | `frontend/src/components/persistence.ts` | `loadSetting`, `saveSetting`, `isOneOf`. |
| Display helpers | `frontend/src/components/valuationDisplay.tsx` | `formatUnitPrice`, `freshnessLabel`, `freshnessTone`, `signedTone`, `FormattedNumber`. |
| Target bar | `frontend/src/components/holdingsConviction.ts` (`targetGapBar`), `HoldingsTable.tsx` | Reused for the "Vs target" cell. |
| Navigation and routes | `frontend/src/components/appModeViewModel.ts` (+ test), `frontend/src/App.tsx`, `frontend/src/App.test.tsx` | Route under `PortfolioLayout`; App test stubs page modules. |
| Empty-state links | `frontend/src/components/LedgerEntryInvitation.tsx` | `elsewhere` variant. |

## API contract

`GET /api/opportunities?window=7d|14d|30d` (absent → `7d`; anything else →
`400 invalid_window`).

```json
{
  "period": { "window": "14d", "start_date": "2026-10-01", "end_date": "2026-10-15" },
  "rows": [
    {
      "instrument": { "...InstrumentResponse": "..." },
      "quantity": 120,
      "latest_close": {
        "date": "2026-10-15", "close": "377.8", "currency": "SEK",
        "source": "YAHOO", "freshness": "fresh"
      },
      "price_change": {
        "status": "available",
        "start_date": "2026-10-01", "start_close": "412.5",
        "end_date": "2026-10-15", "end_close": "377.8",
        "change_percent": "-8.41"
      },
      "average_cost_native": "398.20",
      "price_vs_average_percent": { "status": "available", "value": "-5.12" },
      "below_average": true,
      "unrealized_gain_percent": { "status": "available", "value": "-6.03" },
      "unrealized_gain_freshness": "fresh",
      "conviction_target": { "...same shape as /api/holdings": "..." }
    }
  ],
  "data_revision": "..."
}
```

- `latest_close` is `null` when the holding has no price source or no close.
- `price_change` is `{ "status": "unavailable", "reasons": ["start_close_too_old"] }`
  when any validity rule fails.
- `rows` are in instrument-list order; the frontend orders them.
- Unavailable SEK gain uses the existing reason codes.
- `unrealized_gain_freshness` is the staler of the latest price and latest FX
  freshness (serialized like every other freshness value); `null` when the
  holding has no valuation (no price source).

## Phases

Verification commands (from `Agents.md`):

- Backend, from the repository root: `cargo build -p ticker-tape-tally-board-backend`,
  `cargo test -p ticker-tape-tally-board-backend`, then
  `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`
  and `cargo fmt`. The phase that bumps the workspace version touches the
  workspace manifest and widens clippy to
  `cargo clippy --workspace --all-targets -- -D warnings`.
- Frontend, from `frontend/`: targeted tests with `npm run test -- <paths>`, then
  `npm run check` (tsc + Biome + full Vitest) and `npm run fmt`. Use `npm.cmd`
  when launching through PowerShell `Start-Process`.
- Any app launch for visual checks or measurement is coordinated with the
  "Project orchestration" session first. Use `.\scripts\start.ps1 -Demo` for a
  realistic seeded dataset; real-data checks use a dev ledger made from a
  deliberately copied backup snapshot (DecisionLog 2026-09-13), never a second
  instance on the production ledger while the user's release app runs.

### Phase 1 — Backend: recent price change domain and the Opportunities endpoint

**Status: done (2026-10-15).** Implemented, reviewed and verified (backend build,
647 tests, clippy `-D warnings`, `cargo fmt`). Notes for later phases: the SEK
gain percent reuses the Holdings formatter, so it keeps Holdings' `-0.00` for
tiny negative gains; `unrealized_gain_freshness` is `null` whenever the gain has
no price+FX pair (not only for unmapped holdings); unmapped holdings report the
gain as unavailable `missing_price`, so Phase 4 labels that cell from the row's
`no_price_source` instead.

Changes:

- New pure module `backend/src/domain/recent_price_change.rs` (no I/O, no axum/sqlx):
  - `RecentWindow { OneWeek, TwoWeeks, ThirtyDays }` with `days()` (7/14/30),
    `as_str()` (`7d`/`14d`/`30d`), `from_query(&str) -> Option<Self>`, and
    `start_date(today)`.
  - `START_CLOSE_MAX_AGE_TRADING_DAYS: i64 = 3`.
  - `RecentChangeReason` enum with `as_str()` for the codes in the table above.
  - `recent_price_change(period_start, end: Option<&PriceSnapshot>,
    start: Option<&PriceCandidate>, split_dates: &[NaiveDate]) ->
    RecentPriceChange` (`Available { start_date, start_close, end_date,
    end_close, change_percent } | Unavailable(Vec<RecentChangeReason>)`),
    applying the rules in table order; `change_percent` exact.
  - `price_versus_average(latest: Option<&PriceSnapshot>, instrument_currency,
    average_cost_native: Option<Decimal>) -> PriceVersusAverage` (`Available {
    percent, below_average }` or `Unavailable(reasons)`).
  - Re-export from `domain/mod.rs`.
- `backend/src/api/valued_holdings.rs`: `ValuedOpenHolding` keeps the instrument's
  ledger (`ledger: Vec<LedgerTransaction>`) it already builds, so callers can read
  split dates without a second transaction query. Holdings and Rebalance are
  otherwise unchanged.
- New `backend/src/api/conviction_target.rs`: `ConvictionTargetResponse`,
  `target_field_response`, `build_conviction_target` moved verbatim from
  `holdings.rs`; `holdings.rs` imports them (no response change — its tests prove
  it).
- New `backend/src/api/opportunities.rs`: `OpportunitiesQuery`, response types,
  `handler`:
  1. parse the window (`invalid_window`), `today = state.clock.today()`,
     `start_date = window.start_date(today)`;
  2. `load_valued_holdings`; `derive_targets` over all of them; keep
     `quantity > 0`;
  3. per held row with price coverage: `effective_prices::effective_latest_on_or_before(pool, id, start_date)`
     for the start candidate (logged with context on failure); the end close is
     the valuation's latest price snapshot; split dates from the retained ledger;
  4. build the row (closes at full precision, percents 2dp with negative zero
     normalised, `serialize_freshness`, `serialize_availability` for the SEK gain,
     `unrealized_gain_freshness` from `staler_freshness(price, fx)` over the
     valuation's latest price and FX snapshots, `build_conviction_target`);
  5. return `period`, `rows`, `data_revision`.
- `backend/src/api/mod.rs`: register the module and the GET route.
- Append DecisionLog entry A (draft below).

Tests:

- Domain unit tests (`recent_price_change.rs`):
  - window start dates for 7/14/30 days, `from_query` accepts exactly
    `7d`/`14d`/`30d` (rejects `7D`, `1w`, empty);
  - tolerance boundaries: start close exactly 3 weekdays before `start_date` is
    accepted, 4 is `start_close_too_old`; a weekend `start_date` resolving to the
    preceding Friday is accepted; a Christmas-shaped gap (start Sunday, last
    close the previous Tuesday) is accepted;
  - latest close on `start_date` → `no_close_since_period_start`; no start
    candidate → `missing_start_close`; source and currency mismatch; zero start
    close; split strictly after the start close and on/before the end close →
    `split_in_period`, a split on/before the start close → available;
  - exact percentage (e.g. 412.5 → 377.8 = −8.4121…), a small negative that
    rounds to zero serialises as `0.00`;
  - price vs average: below / equal / above (`below_average` false only at exact
    equality), a latest close a fraction below a full-precision average whose
    percentage rounds to `0.00` still has `below_average: true`, currency
    mismatch, zero average, no latest close.
- Endpoint tests (`api/opportunities.rs`, `AppState::for_tests().await.with_clock(Clock::fixed(…))`,
  a local helper that seeds a mapped instrument, a buy, and an explicit list of
  dated closes; a second helper adds a Nasdaq Nordic mapping and rows):
  - a held asset that fell over the week: `period` echoes `7d` and the resolved
    dates; `price_change` names the start/end closes used; absent `window` equals
    explicit `7d`;
  - `14d` and `30d` pick their own start closes; `window=bogus` /
    `window=7D` / `window=` → `400 invalid_window`;
  - a long gap before the start date → `start_close_too_old`, while
    `latest_close` is still present;
  - Yahoo-only start, Nasdaq-only end → `start_close_source_mismatch`;
  - a latest close older than the start date → change unavailable, `latest_close`
    present with a `warning_stale_…` freshness (stale shown, not hidden);
  - a watchlist row (bought then fully sold, with conviction) and a closed
    position are absent; an unmapped held asset is present with
    `no_price_source` and `latest_close: null`;
  - a USD holding whose FX rate moved between start and end: `change_percent`
    reflects the native closes only;
  - `price_vs_average_percent` and `below_average` for one row below and one
    above its average;
  - for every row, `unrealized_gain_percent` and `conviction_target` equal the
    same instrument's fields in `GET /api/holdings` (single source of truth);
  - **fresh price, stale FX:** a USD holding with today's close and an FX rate
    10 days old keeps `unrealized_gain_percent` available, reports
    `latest_close.freshness: "fresh"`, and reports
    `unrealized_gain_freshness: "warning_stale_…"` (mirrors
    `freshness_uses_the_staler_of_price_and_fx` in `api/rebalance.rs`); a SEK
    holding with a fresh close reports `"fresh"`; an unmapped holding reports
    `null`;
  - a split in the ledger inside the window → `split_in_period`;
  - `data_revision` present; demo mode answers 200 and the seeded demo dataset
    yields an available change for every held row in all three windows.
- `holdings.rs` existing tests pass unchanged after the builder move.

Verify (repository root): `cargo build -p ticker-tape-tally-board-backend`,
`cargo test -p ticker-tape-tally-board-backend`,
`cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`,
`cargo fmt`. Optional agent smoke: against a `-Demo` run (coordinated),
`GET /api/opportunities?window=30d` returns rows with available changes.

### Phase 2 — Backend: the latest refresh covers the longest window

Changes:

- `domain/recent_price_change.rs`: `RECENT_CHANGE_HISTORY_DAYS` = the longest
  window (30) plus a 7-calendar-day margin, which always contains more than the
  3-weekday start-close tolerance (whose calendar span is at most 5 days).
- `market_data/refresh.rs`: `LATEST_REFRESH_WINDOW_DAYS` is defined from
  `RECENT_CHANGE_HISTORY_DAYS` (37), so the two cannot drift. Backfill is
  unchanged. Consequences, stated in code comments and the decision log: one
  request per mapping/currency as before, larger responses, and a latest refresh
  now re-fetches ~5 weeks of provider closes, which replaces stale pre-split
  closes after a recent split.
- `docs/Design.HighLevel.md` (~line 92): "fills only the 14-calendar-day window
  ending today" → the window that covers the Opportunities page's longest
  period plus its start-close tolerance (37 calendar days).
- Append DecisionLog entry B (draft below).

Tests:

- Domain: for every weekday of `today`, the earliest start close the 30-day
  window can accept lies within `RECENT_CHANGE_HISTORY_DAYS` of today
  (exhaustive over a 7-day cycle, so changing the tolerance or the windows
  without the margin fails).
- Refresh (`market_data/refresh/tests.rs`, pinned clock, fake providers): a
  `latest` run asks the price provider and the FX provider for
  `start = today − 37 days`, `end = today`; a `backfill` run's window is
  unchanged.

Verify (repository root): build, test, per-package clippy, `cargo fmt`.

Measurement (**external human testing recommended**, coordinated with the
"Project orchestration" session): on a dev ledger made from a copied backup
snapshot, run one launch refresh on the commit before this phase and one after,
and record from `GET /api/prices/status` `latest_run` the `started_at` →
`finished_at` duration, `prices_written` and `status`. Expected: same status,
roughly 2–3× `prices_written`, duration within a few seconds of the baseline.
A run that turns `partial`/`failed` or takes markedly longer is a stop condition
to investigate before continuing.

### Phase 3 — Frontend: the page with ordering and the window switch

Changes:

- `api/types.ts`: `OpportunityWindow = "7d" | "14d" | "30d"`,
  `OPPORTUNITY_WINDOWS`, `OpportunityOrder = "fallers_first" | "risers_first"`,
  `OpportunitiesResponse`, `OpportunityRow`, `PriceChange`.
- `api/queries.ts`: `useOpportunities(window)` via
  `useVersionedQuery(["opportunities", window], `/api/opportunities?window=${window}`)`
  (previous data kept by default, which the naming rule relies on).
- New `components/OpportunitiesPage.tsx`: exported `opportunitiesPageReducer`,
  `loadOpportunitiesPageState`, `saveOpportunitiesPageState`, and the component
  (reducer → query → view-model → render).
- New `components/opportunitiesViewModel.ts` (pure):
  `sortOpportunityRows(rows, order)`, `priceChangeReasonLabel(code, startDate)`,
  `buildOpportunitiesViewModel({ response, requestedWindow, order, isFetching,
  isError, errorMessage })` → status (`loading` | `error` | `empty` |
  `available`), `changeColumnLabel` (from `response.period.window`, or "Price
  change" while the shown window differs from the requested one),
  `periodSubtitle` ("Closes from 1 Oct to 15 Oct", null while updating),
  `isUpdating`, and rows with the change label/tone/tooltip, latest-price label
  (adaptive unit price + currency) and freshness kind.
- Columns in this phase: Instrument (`InstrumentCell`), price change, latest
  price with freshness (minor icon / warning chip, as Rebalance renders it).
  Error state with Retry; skeleton loading; empty state with
  `LedgerEntryInvitation`.
- `appModeViewModel.ts`: nav item `{ to: "/opportunities", label: "Opportunities" }`
  directly after Rebalance (all modes — the page is read-only).
- `App.tsx`: lazy `OpportunitiesPage` route under `PortfolioLayout`.
- `styles.css`: only what the table and header controls need beyond existing
  `.panel`, `.table-wrap`, `.segmented-control`, `.status-chip` classes.

Tests:

- `OpportunitiesPage.reducer.test.ts`: window and order changes; same value
  returns the same state; load falls back to defaults for missing, malformed,
  and unknown stored values (`"1w"`, `"desc"`); save writes both keys.
- `opportunitiesViewModel.test.ts`:
  - fallers-first ascending and risers-first descending; unavailable rows last
    in **both** orders; ties by symbol;
  - the column label comes from the response window: response `7d` with
    requested `30d` → "Price change", `isUpdating` true, no subtitle; matching
    windows → "1-week price change" / "2-week price change" / "30-day price
    change" and the subtitle from the response dates;
  - unavailable reasons render as plain language with the start date filled in;
  - a stale latest close yields the warning kind even when the change is
    unavailable;
  - empty rows → `empty`; error → message.
- `OpportunitiesPage.test.tsx` (jsdom; `vi.mock("../api/queries")` and
  `vi.mock("./useAppMode")` as in `HoldingsPage.test.tsx`; by role/text):
  - window buttons "1 week" / "2 weeks" / "30 days" with `aria-pressed`; clicking
    "30 days" requests the `30d` query; order buttons reorder the rows (first
    row's instrument name changes);
  - while the mocked query still returns the `7d` response for a `30d` request,
    the column header reads "Price change" and "Updating…" is visible;
  - an unavailable row shows its plain-language reason and sits last;
  - empty ledger shows the Transactions/Import links, and not in demo.
- `versionedQueries.test.tsx`: `useOpportunities` in the known hooks list, with
  `HOOK_ARGS` `["7d"]`; and a request-parameter block following the existing
  "gains period request parameters" tests (`stubFetch`, real `QueryClient`,
  `renderHook`): rendering `useOpportunities("7d")` sends `window=7d`; rerendering
  the same hook with `"30d"` sends a second request with `window=30d` and no
  other window value — the mocked component test cannot prove what goes over the
  wire.
- `appModeViewModel.test.ts`: nav order includes Opportunities after Rebalance in
  production and demo.
- `App.test.tsx`: stub `./components/OpportunitiesPage`; the "Opportunities" nav
  link exists and `/opportunities` renders the stub inside the portfolio layout.

Verify (from `frontend/`): targeted tests above, `npm run check`, `npm run fmt`.

Measurement (agent, headless Chrome over CDP as in the app-bar plan's measurement
record; coordinated launch with `-Demo`): app-bar height at 1440×900 and
800×600 is unchanged by the extra nav item (56 and 102 at the last
measurement). Growth is a defect to fix in this phase.

### Phase 4 — Frontend: average-price, gain and target context

Changes:

- View-model rows gain: vs-average label/tone and `belowAverage` (from the
  backend `below_average`, never from the rounded string), SEK gain label/tone
  plus its freshness kind from `unrealized_gain_freshness`, and `targetBar` via `targetGapBar(row.conviction_target)` (null → blank cell).
- Columns: "Vs your average price" (signed percent in plain-sign colour, neutral
  chip "Below your average" when `belowAverage`), "Your gain incl. fees &
  currency" (SEK percent, with the minor stale icon or warning stale chip beside
  it when `unrealized_gain_freshness` is stale — e.g. a fresh price with an old
  exchange rate), "Vs target" (the Holdings diverging bar with the same
  tooltip text; blank without a target).
- The ex-dividend / native-currency note under the table.
- `docs/VisualDesign.DarkTheme.md` → **Components**: an Opportunities table note
  (density spec; window and order segmented controls in the panel header; the
  neutral "Below your average" chip; the stale signal on the latest price and,
  separately, beside the SEK gain when its price or exchange rate is stale; the
  Holdings target-gap bar reused; the muted footnote).

Tests:

- `opportunitiesViewModel.test.ts`: `belowAverage` follows the backend flag — a
  row whose percentage displays `0.00` but whose `below_average` is `true` shows
  the marker, and an exactly-at-average row (`below_average: false`) shows none;
  vs-average and gain tones by sign; unavailable vs-average and gain carry reason
  text; the gain freshness kind is `warning_stale` for a row whose latest price is
  fresh but whose `unrealized_gain_freshness` is `warning_stale_…` (price cell
  stays fresh), `minor_stale` and `fresh` likewise, and none when it is `null`;
  `targetBar` null for `no_target` and the excluded statuses.
- `OpportunitiesPage.test.tsx`: the "Below your average" marker appears on the
  below-average row only; a fresh-price/stale-FX row shows the stale chip in the
  gain cell (by its text) while its latest-price cell shows none; column headers "Vs your average price" and "Your gain
  incl. fees & currency" are present; the footnote text is present; the target
  cell is empty for a row without a target.

Verify (from `frontend/`): targeted tests, `npm run check`, `npm run fmt`.

**Human testing recommended here** (coordinated launch): in `-Demo`, and on a dev
ledger copied from a backup snapshot:

1. Switch 1 week → 30 days: the header does not name 30 days until the 30-day
   rows arrive; the choice survives navigating away and reloading; Biggest
   risers reverses the list with unavailable rows still last.
2. Spot-check two holdings against an outside source (broker or Yahoo): the
   1-week change matches the close-to-close move; a holding trading below what
   you paid shows "Below your average".
3. A holding with a stale price shows the stale chip; a foreign holding whose
   exchange rate is stale shows the chip beside its SEK gain; an unmapped holding
   shows "No price source" at the bottom.
4. Desktop window at its default size and the browser at ~800px: the table and
   header controls fit without the app bar growing.

### Phase 5 — Versions, documents, and final verification

- Bump `frontend/package.json` to the next minor above its value when this phase
  lands (0.26.0 → 0.27.0 unless the in-flight app-bar refresh-feedback work has
  already taken it) for the new page.
- Bump root `Cargo.toml` `[workspace.package].version` to the next minor
  (0.19.0 → 0.20.0, same caveat) for the new endpoint and refresh window.
- `docs/Design.HighLevel.md` → API surface: add `GET /api/opportunities`.
- Confirm every document in the table below is updated and that no durable
  document or code comment refers to this plan or its phases.
- Full gates: backend build, test,
  `cargo clippy --workspace --all-targets -- -D warnings` (widened: workspace
  manifest changed), `cargo fmt`; frontend `npm run check`, `npm run fmt`.

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `docs/DecisionLog.md` | Entry A (Opportunities window and measure) | 1 |
| `docs/DecisionLog.md` | Entry B (latest refresh window follows the longest recent-change window) | 2 |
| `docs/Design.HighLevel.md` | Refresh sentence (~line 92): 14-day window → 37-day window and why | 2 |
| `docs/Design.HighLevel.md` | API surface list: `GET /api/opportunities` | 5 |
| `docs/VisualDesign.DarkTheme.md` | Components: Opportunities table note | 4 |
| `frontend/package.json` | Minor version bump | 5 |
| Root `Cargo.toml` | `[workspace.package].version` minor bump | 5 |

`README.md` needs no change (it documents no per-endpoint list beyond the
heartbeat). `docs/CurrencyAndFxRules.md` needs no change: the page converts
nothing; its SEK column reuses existing valuation.

### Decision log drafts

Append at the end of `docs/DecisionLog.md`, dated on the day each is recorded.
Wording names behaviours, not this plan.

**Entry A**

```
## YYYY-MM-DD - The Opportunities Page Measures Recent Share-Price Change Over Its Own Window
Decision: The Opportunities page lists every currently held asset ordered by its share-price change over a page-local window of 1 week, 2 weeks or 30 days (default 1 week), biggest fallers first or biggest risers first. The change is close-to-close in the instrument's own currency, from the latest close on or before the window start to the latest close on or before today, with both dates resolved by the backend from its clock and the resolved period returned with the data; the page names the window only from that response. A change is reported only when the start close lies within 3 trading days (weekday approximation) of the window start, the end close is newer than the window start, both closes come from the same price source and currency, and no share split recorded in the ledger falls after the start close and on or before the end close; otherwise it is explicitly unavailable, never zero, and such rows sort last in both orders. The split rule applies even when the provider's closes are split-adjusted, because an unrefreshed or unadjusted series would otherwise report a split as a crash. Stale latest prices are shown with the stale signal. The comparison of the latest price with the user's native average purchase price (fees excluded) is shown as context with a below-average marker set by an exact comparison, beside the existing SEK gain including fees and currency — which carries the staler of its price and exchange-rate freshness signals — and the existing conviction-target gap; none of these influence the order. The page is read-only and its window and order are persisted view settings.
Context: The user wants held assets that dropped recently, preferably below what they paid, as buy-more candidates, and the reverse order as gain-taking candidates. This measures a recent share-price drop rather than portfolio performance, so it is a deliberate exception to the 2026-06-29 Shared Dashboard And Gains Performance Interval entry, whose presets lack 2 weeks and 30 days and whose extension would change two other pages' contract. An unbounded on-or-before lookup or a cross-source price pair would report false moves.
Consequences: Refines the 2026-06-29 entry: period-performance surfaces still share one interval, while this recent-price-change surface keeps its own. Its window presets are part of the API contract under the 2026-09-12 One Clock Owns Today entry. Closes are not dividend-adjusted, so a share can appear to fall on its ex-dividend day; the page says so and does not detect it. Ordering by change alone and showing the average-price comparison beside it were chosen over a two-tier ordering and over a combined score. A holding's change is blank for up to the window length after a share split.
```

**Entry B**

```
## YYYY-MM-DD - The Latest Price Refresh Covers The Longest Recent-Change Window
Decision: A latest-mode price and exchange-rate refresh fetches a window long enough to cover the Opportunities page's longest period plus its start-close tolerance, and that length is derived from those two values rather than set independently.
Context: The previous 14-day window left the 2-week and 30-day comparisons reading closes that a latest refresh never revisits, including pre-split closes after a recent split. Providers are asked once per mapping or currency regardless of window length, so the cost is larger responses and more upserts, not more requests.
Consequences: Each launch and manual refresh re-fetches about five weeks of closes and rates. Lengthening the page's longest window or its tolerance lengthens every latest refresh. Backfill is unaffected.
```

## Open Questions

None. The split guard is settled (kept, unconditional); the target-gap column,
the "No price source" treatment of hand-priced holdings until hand-entered prices
land, the refresh-cost finding, and the next-minor version wording are accepted.
