# Plan — Hand-entered prices for instruments no provider carries

## Summary

Let the user enter dated closing prices by hand for an instrument, and make those
prices flow through the existing valuation machinery so the holding is valued like
any other: market value, unrealized gain, period gains, the asset price chart,
portfolio totals, and conviction targets.

Motivating case: instrument 32 "AVA SAMSUNG TRACKER" (ISIN `JE00BJ7HNC92`, 220
shares, SEK, exchange `AVANZA`). Yahoo genuinely does not carry it, so it has no
`instrument_provider_symbols` row, the holdings response omits `valuation`
entirely, and every surface reads "Valuation missing". A second instrument
benefits: id 38 AstraZeneca, whose YAHOO mapping the refresh auto-disabled on a
currency mismatch (`AZN.L` quotes GBp against a SEK instrument).

**Out of scope:** any new automated provider (Nasdaq Nordic, Avanza, TwelveData),
bulk paste/CSV of prices, export/import of hand-entered prices, and the
command/undo subsystem.

**No `prices` migration is required** — `prices` already has
`CHECK (provider IN ('YAHOO','TWELVE_DATA','MANUAL'))` and
`UNIQUE (instrument_id, provider, date)`, and `db/prices.rs::upsert` already
targets that conflict. Hand-entered prices are ordinary rows with
`provider = 'MANUAL'`. One small additive migration *is* needed later in the plan,
for the refresh-run hand-priced counter.

## Settled decisions (implement these; do not re-litigate)

1. **Dated history, not a single current price.** The user enters (date, close)
   pairs building a series, because the existing valuation/gains/chart code already
   reads that shape.
2. **Precedence: fetched wins, hand-entered fills gaps, resolved per date.** For
   each date a feed row wins; where there is no feed row for that date the MANUAL
   row is used. "Latest price on or before D" is therefore the newest row from
   either source, with the feed breaking a same-date tie.
3. **Provenance must be surfaced, and names the concrete source.** Because of (2),
   the source behind a displayed number can change without user action. Price
   snapshots and price-history points carry the source, and the API says `YAHOO`
   vs `MANUAL` — not a generic "automatic vs manual" pair. Two reasons: the
   "Data & mapping" panel already shows the Yahoo symbol today and a generic label
   would regress it; and an anticipated second feed (Nasdaq Nordic) must be
   distinguishable from Yahoo without a breaking field change later.
4. **Currency comes from the instrument, never typed by the user.**
5. **Entry is one row at a time** — add / edit / delete a single dated price.
6. **Staleness is NOT special-cased.** Hand-entered prices age under exactly the
   same `DataFreshness::from_age` rule. Accepted consequence: hand-priced holdings
   sit permanently in warning-stale state, and the rebalance page's
   "Selected rung includes warning-stale trades…" notice will fire for them.
7. **Deleting an instrument that has hand-entered prices stays possible, but the
   confirmation is enforced by the backend**, not by frontend text alone. The
   delete endpoint refuses to destroy authored rows unless the caller acknowledges
   the exact number of hand-entered prices that will be lost, checked inside the
   deleting transaction. Deletion must remain possible after acknowledgement — it
   is not blocked outright.
8. **A disabled (`enabled = false`) feed mapping counts as no feed**, so MANUAL
   prices take over for that instrument. Hand-entry is the repair path for a
   broken feed, not only for uncovered instruments. Stored feed rows behind a
   disabled mapping must NOT be promoted back into valuation.
9. **Day change is computed from an adjacent previous close paired with that
   close's own FX rate.** Two halves of one defect:
   - *Close adjacency, for every source:* day change is reported only when the
     previous close is the immediately preceding trading day; otherwise it is
     explicitly unavailable. This fixes a latent bug that already affects fetched
     prices with gaps, and is user-visible: day change will go blank in existing
     cases where a feed has gaps.
   - *FX pairing:* the previous FX rate resolves as the last available rate on or
     before the **previous close's** date, so both factors of
     `previous_price.close × previous_fx.rate × quantity` come from the same day.
     Today the previous rate is fetched relative to `latest_fx.date`, a date
     unrelated to either price.

   The **latest** endpoint is unchanged and must stay unchanged: latest price and
   latest FX both keep resolving on-or-before the valuation date, independently,
   per `DecisionLog 2026-06-16 Market Data Staleness Display Rules` ("valuation
   always uses the last available close/rate on or before the valuation date").
   Only the previous endpoint changes, and no committed decision governs it — its
   current anchor is an implementation choice, not a decision.
10. **Never fabricate price history.** Do not seed a series from transaction
    prices. Where a period figure cannot be computed for want of entered history,
    the app explains why rather than silently dropping the holding into
    `totals.excluded_rows`. (The chart's existing native transaction-price anchors
    from the 2026-06-22 Chart Axis decision are display anchors, not stored
    history — leave them in place.)
11. **Hand-entered price writes are plain, non-undoable resource mutations**,
    carved out in `docs/DecisionLog.md` the way standalone `POST /api/instruments`
    already is. They may join the command/undo system later additively. Because
    they are non-undoable, every destructive action on them is explicitly
    confirmed (see decision 7 and the per-price delete confirmation).
12. **Refresh reporting distinguishes hand-priced from unmapped**, so a run
    finishing PARTIAL keeps meaning "action needed".
13. **MANUAL does not get an `instrument_provider_symbols` row.** The presence of
    MANUAL price rows is itself the signal that an instrument is hand-priced. The
    `prices.provider_symbol` column is `NOT NULL`, so MANUAL rows store a single
    named sentinel constant (`MANUAL_PROVIDER_SYMBOL = "HAND_ENTERED"`) rather
    than an empty string: the database is read directly by the user and `''` reads
    as an unset field, while a sentinel reads as deliberate and can never be
    mistaken for a real provider symbol.
14. **`prices` is no longer a pure cache.** It holds user-authored data that cannot
    be reconstructed from any external source.
15. **Future-dated hand-entered prices are rejected.** A future close does not
    exist; the realistic way to create one is a mistyped year; and such a row would
    immediately become the newest price and drive valuation until the calendar
    caught up.
16. **The price editor is its own full-width panel on the asset page, directly
    below the price-history chart and above the two existing columns.** The price
    list is the data behind the chart, so the two read together; and a full-width
    panel absorbs the list's growth without unbalancing the columns beneath it —
    the right-hand column (Data & mapping + Conviction) is already the shorter one,
    and a list growing to a dozen rows there would turn a compact three-row status
    readout into the longest thing on the page and push Conviction far down.
    "Data & mapping" still carries its one-line price-source row: the provenance
    requirement in decision 3 is unchanged; that panel simply does not host the
    editor.
17. **The price chart does not visually distinguish hand-entered from fetched
    points.** A deliberate deferral, not an oversight: both instruments driving
    this work will have entirely hand-entered series, so there is nothing to
    distinguish; a mixed series only arises for an instrument whose *enabled* feed
    has gaps, which does not exist in the user's data today; and the chart already
    carries a break-even price line plus buy/sell markers, so a third visual
    encoding plus a legend is real clutter for a case that has not arisen. Every
    price-history point still ships its `source`, so adding the distinction later
    is a rendering change only, with no API change.

## Architecture

### One source of truth for the price source (DRY)

`"YAHOO"` is currently hardcoded in three independent places — `api/valuation.rs`
(`PRICE_PROVIDER`), `market_data/refresh.rs` (`YAHOO_PROVIDER`) and `demo/mod.rs`
(`PRICE_PROVIDER`) — and `"FRANKFURTER"` in the same three. `Agents.md` requires
one source of truth, and this work multiplies the number of readers, so the
constants move to the provider boundary:

```
providers/mod.rs:
    pub const FEED_PRICE_PROVIDER: MarketDataProvider = MarketDataProvider::Yahoo;
    pub const MANUAL_PRICE_PROVIDER: MarketDataProvider = MarketDataProvider::Manual;
    pub const BASE_FX_PROVIDER: FxProvider = FxProvider::Frankfurter;
```

Every site uses `FEED_PRICE_PROVIDER.as_str()` etc. `MarketDataProvider::Manual`
stops being dead code. The three `BASE_CURRENCY`/`SEK` copies are *not* folded in
here — base currency is a domain concept, `domain/valuation.rs` also hardcodes
`"SEK"`, and untangling it is a separate refactor with no bearing on this feature.

### Provenance without provider types in the domain

`backend/src/domain/` must stay free of provider-specific types, but it still has
to carry which source produced a number. The domain therefore carries a provider
*identity* it does not interpret:

```rust
// domain/price_resolution.rs
pub struct ProviderCode(String);           // opaque to the domain; as_str() only
pub enum PriceSource {
    Fetched { provider: ProviderCode },
    Manual,
}
```

The domain never enumerates providers and never imports `providers::*`; the IO
layer constructs `ProviderCode` from the stored `provider` column. Precedence and
"is this hand-entered?" are **structural** checks on the enum, never string
comparisons against `"YAHOO"`.

The API serializes a flat code — `"YAHOO"` for fetched rows, `"MANUAL"` for
hand-entered — matching `MarketDataProvider`'s existing SCREAMING_SNAKE serde and
the DB column. A future `NASDAQ_NORDIC` is a new value, not a breaking change. The
frontend maps codes to display names in one pure, tested helper
(`priceSourceLabel`): `MANUAL` → "Hand-entered", `YAHOO` → "Yahoo", anything
else → the prettified code.

`prices.provider` also permits `TWELVE_DATA`, which nothing writes. The IO layer
queries `FEED_PRICE_PROVIDER` and `MANUAL_PRICE_PROVIDER` explicitly, so any other
stored provider is simply not loaded — the same behavior as today.

### Price resolution: pure rule + IO wrapper

Two new modules, named after stable behavior:

- **`backend/src/domain/price_resolution.rs`** (pure; no axum/sqlx/provider types)
  - `ProviderCode`, `PriceSource` as above.
  - `pub fn resolve_price_series(feed: &[PriceCandidate], manual: &[PriceCandidate]) -> Vec<PriceCandidate>`
    — merge two date-ascending series into one; for a shared date the feed row
    wins. Precondition (documented + `debug_assert`ed): both inputs ascending.
  - `pub fn pick_latest_on_or_before(...)` / `pub fn pick_previous_before(...)`
    — resolve a winner from an optional feed candidate and an optional manual
    candidate (feed wins a date tie, newer date otherwise). Used by the indexed
    single-row lookups so the valuation hot path never loads full history.
  - `PriceCandidate`, `PriceSnapshot` and the price-history `PricePoint` (all in
    `domain/valuation.rs`) gain `source: PriceSource`. `build_price_history` must
    carry the candidate's source through to the point it emits, otherwise
    provenance is lost before serialization. The compiler will find every
    construction site.
  - `build_value_history` deliberately does **not** gain provenance: a portfolio
    value point aggregates many instruments and a mixed source set would not mean
    anything useful. Noted so a later reader does not think it was forgotten.

- **`backend/src/market_data/effective_prices.rs`** (IO; the only place that knows
  a disabled mapping means "no feed")
  - `pub async fn feed_mapping_enabled(pool, instrument_id) -> Result<bool, RepoError>`
  - `pub async fn has_manual_prices(pool, instrument_id) -> Result<bool, RepoError>`
  - `pub async fn effective_series(pool, instrument_id, from, to) -> Result<Vec<PriceCandidate>, RepoError>`
  - `pub async fn effective_latest_on_or_before(pool, instrument_id, date) -> Result<Option<PriceCandidate>, RepoError>`
  - `pub async fn effective_previous_before(pool, instrument_id, before_date) -> Result<Option<PriceCandidate>, RepoError>`

  Feed rows are read only when the mapping exists and is enabled; MANUAL rows are
  always read. Row decode failures are internal invariant violations: log through
  `engine_error!` with instrument id, row id and field, then return
  `RepoError::Decode`. This deliberately replaces the silent `.and_then(...)` drop
  currently in `api/valuation.rs` and matches what `instrument_prices.rs` and
  `portfolio.rs` already do.

All four current gating sites route through this module:
`api/valuation.rs` (`load_valuation_inputs`, `load_period_inputs`),
`api/instrument_prices.rs`, `api/portfolio.rs`.

### Coverage flag

`ValuationInputs.price_mapping_enabled` is renamed to
`has_price_coverage: bool = feed_mapping_enabled || has_manual_prices`. This is
the flag `api/valued_holdings.rs` uses to decide whether the holdings response
carries a `valuation` object at all — the direct cause of "Valuation missing" on
instrument 32. `api/portfolio.rs::value_history` is required work for the same
reason: its date spine is built from stored price dates behind the mapping gate.

Consequence to accept: a hand-priced watchlist/zero-quantity instrument now has
price availability, so it can enter the conviction-target pool and the rebalance
ladder (2026-07-08 derived-watchlist decision). That is intended.

### Storage of hand-entered rows

MANUAL rows are ordinary `prices` rows:
- `provider = 'MANUAL'`, `currency` = the instrument's currency (never from the
  request body), `close` = exact decimal string.
- `provider_symbol = MANUAL_PROVIDER_SYMBOL` (`"HAND_ENTERED"`), one named
  constant in the repository, per decision 13.
- `fetched_at` stores the ISO-8601 timestamp of the user's create/last-edit. The
  column name is now slightly off for MANUAL rows; renaming it would need a table
  rebuild and is not worth it. Document the dual meaning in the repository module.

### Deleting authored data safely

`api/instruments.rs::remove` today runs a provider-blind
`DELETE FROM prices WHERE instrument_id = ?` with no guard. A frontend-only
warning cannot protect authored rows from a stale tab, a second client, or a
direct API call, so the contract is server-side:

- `DELETE /api/instruments/{id}?expected_manual_prices=N`.
- Inside the transaction the handler already opens, it counts MANUAL rows for the
  instrument. When the parameter is absent the count must be `0`; when present it
  must equal the count exactly.
- Absent parameter with authored rows present → `409 manual_prices_present` with
  `details: { instrument_id, manual_prices }`. The UI builds its confirmation text
  from that authoritative count.
- Present but mismatched → `409 manual_prices_changed` with
  `details: { expected, actual }`; the UI refetches and re-prompts.
- After acknowledgement the delete proceeds exactly as today. Deletion stays
  possible; it is never blocked outright.

The count and the delete share one transaction so the check cannot be raced within
this single-writer application. (`api/instruments.rs::remove` still refuses any
instrument that has transactions, so in practice this covers never-traded and
watchlist instruments — the guard is still required, because those are exactly the
instruments a user may have hand-priced.)

### Frontend

Unidirectional flow is preserved. The editor's form state is a `useReducer` with
an exported pure reducer; all derivation (row list, superseded-by-feed marking,
draft validation, confirmation wording) lives in a pure view-model module with
Vitest coverage; the component renders and dispatches. Server writes go through
`api/queries.ts` mutations that invalidate the affected query keys, reusing the
existing `invalidateInstrumentData` helper rather than a new bespoke list.

## Phases

Plans are ephemeral. Durable documents and code must name the behavior, never
these phase numbers.

Verification commands, everywhere: from `backend/` run `cargo build`, then
`cargo test`, then `cargo clippy --all-targets -- -D warnings`, then `cargo fmt`.
From `frontend/` run `npm run check` (which covers `tsc --noEmit`, Biome and
`vitest run`), then `npm run fmt`.

---

### Phase 1 — One resolved price source (refactor, no user-visible change)

Because no MANUAL rows exist in the database yet, this phase must be behavior-
preserving apart from one additive response field.

1. Add `FEED_PRICE_PROVIDER`, `MANUAL_PRICE_PROVIDER`, `BASE_FX_PROVIDER` to
   `providers/mod.rs`. Delete `api/valuation.rs::PRICE_PROVIDER`/`FX_PROVIDER`,
   `refresh.rs::YAHOO_PROVIDER`/`FRANKFURTER_PROVIDER`, `demo/mod.rs::PRICE_PROVIDER`/
   `FX_PROVIDER` and repoint every reader, including the test helpers in
   `api/test_support.rs`, `api/gains/tests.rs`, `api/holdings.rs`,
   `api/rebalance.rs`, `api/instrument_prices.rs`, `api/portfolio.rs`.
2. Add `domain/price_resolution.rs` (`ProviderCode`, `PriceSource`,
   `resolve_price_series`, `pick_latest_on_or_before`, `pick_previous_before`);
   export from `domain/mod.rs` (which stays a thin wrapper).
3. Add `source: PriceSource` to `PriceCandidate`, `PriceSnapshot` **and**
   `PricePoint`, and carry it through `build_price_history`.
4. Add `market_data/effective_prices.rs` and rewire `api/valuation.rs`,
   `api/instrument_prices.rs`, `api/portfolio.rs` through it. Rename
   `price_mapping_enabled` → `has_price_coverage` and update
   `api/valued_holdings.rs`.
5. Serialize provenance additively as the flat provider code: `source` on
   `PriceSnapshotResponse` and on `PricePointResponse`.

Tests:
- `price_resolution` unit tests: feed wins a shared date; manual fills a gap;
  manual-only series; feed-only series; empty inputs; interleaved dates keep
  ascending order; `pick_*` tie-break and newer-date selection; `PriceSource`
  distinguishes fetched-vs-manual structurally.
- `build_price_history` unit test: a point's source equals its candidate's source,
  including a mixed feed/manual series.
- `effective_prices` integration tests against the in-memory pool: disabled
  mapping yields no feed rows; missing mapping yields no feed rows; a decode-
  broken row surfaces as an error rather than a silently missing point.
- Existing gains/holdings/price-history/value-history tests must pass unchanged
  (they seed feed rows); add assertions that `source` is `"YAHOO"` on those paths.

Verify:
- Backend command sequence above; every pre-existing test green.
- No frontend change; no UI change. External human testing: not needed.

---

### Phase 2 — Hand-entered price storage, CRUD API, and safe deletion (backend end-to-end)

1. `db/prices.rs`: `MANUAL_PROVIDER_SYMBOL`, plus manual-scoped helpers —
   `list_manual_for_instrument`, `find_manual_by_date`, `upsert_manual`,
   `delete_manual_by_date`, `count_manual_for_instrument`,
   `count_manual_for_instrument_in_tx`. `upsert_manual` reuses the existing
   conflict target; no schema change.
2. New `api/manual_prices.rs` mounted in `api/mod.rs`:
   - `GET /api/instruments/{id}/manual-prices` →
     `{ instrument_id, currency, prices: [{ date, close, recorded_at, superseded_by_feed }] }`,
     newest first. `superseded_by_feed` is true when an enabled-mapping feed row
     exists for the same date, making the precedence rule visible in the editor.
   - `POST /api/instruments/{id}/manual-prices` `{ date, close }` → `201`;
     `409 manual_price_exists` when that date already has a manual row.
   - `PUT /api/instruments/{id}/manual-prices/{date}` `{ close }` → `200`,
     create-or-update at that date (idempotent retry).
   - `DELETE /api/instruments/{id}/manual-prices/{date}` → `204`;
     `404` when absent.
   - All three mutations call `crate::api::reject_demo_mutation`.
   - Validation: unknown instrument → `404`; unparseable date →
     `400 invalid_date`; date after today (UTC) → `400 invalid_date` with a
     future-date message (decision 15); unparseable, zero or negative close →
     `400 invalid_price` (missing data is explicit, never zero). Currency is read
     from the instrument and never accepted from the body.
   - Log each write through `engine_info!` with instrument id, symbol, date and
     operation; log rejections through `engine_warn!` with the same context.
3. Server-enforced instrument-deletion guard in `api/instruments.rs::remove`:
   the `expected_manual_prices` query parameter, the in-transaction count, and the
   `manual_prices_present` / `manual_prices_changed` conflicts described in
   Architecture. Log the refusal with instrument id, symbol and both counts.
4. Add the three manual-price routes to the demo read-only assertion list in
   `api/mod.rs` tests.

Tests (backend integration, in-process router + in-memory pool):
- **Regression test for the reported defect:** an open position on an instrument
  with no `instrument_provider_symbols` row and one MANUAL price returns a
  `valuation` object from `/api/holdings` with an available `market_value_base`,
  and appears in the portfolio summary rather than in "Valuation missing".
- **Disabled-mapping repair path (instrument 38 shape):** a disabled mapping with
  stored feed rows plus one MANUAL row values from the MANUAL row; the stored feed
  rows are never used.
- **Per-date precedence:** feed and manual rows on the same date → feed wins;
  a date only the manual row covers → manual is used; latest-on-or-before across
  mixed sources picks the newest date; the serialized `source` matches which row
  won.
- `/api/instruments/{id}/prices` and `/api/portfolio/value-history` include
  manual-only dates, and price-history points report the winning source.
- CRUD contract tests: create/edit/delete round-trip; duplicate create conflicts;
  delete of an absent date is `404`; future date and non-positive close rejected;
  the stored row's currency equals the instrument currency even if the client
  sends something else; the stored `provider_symbol` is the sentinel.
- **Deletion guard regression tests:** delete without the parameter on an
  instrument with manual rows → `409 manual_prices_present` and *nothing is
  deleted*; delete with a stale/mismatched count → `409 manual_prices_changed` and
  nothing is deleted; delete with the correct count succeeds and removes both the
  instrument and its manual rows; delete of an instrument with zero manual rows
  still works with no parameter.
- Demo mode rejects all three manual-price mutations with `demo_read_only`.

Verify:
- Backend command sequence; all tests green.
- **External human testing recommended (API level, before any UI exists), against
  a *copy* of the real database, never the live one** — this step writes
  persistent authored rows. Copy the SQLite file and start the backend with
  `TTTB_DATABASE_URL` pointing at the copy. Then `POST` two or three closes for
  instrument 32 and load `/api/holdings`, `/api/gains` and
  `/api/portfolio/value-history`: the holding is valued, totals move, no other
  holding changed. Also exercise the deletion guard against a throwaway
  hand-priced instrument in the copy.

---

### Phase 3 — Day change: adjacent previous close, paired with its own FX rate

Independent latent-bug fix, kept separate so its user-visible effect can be
inspected on its own. Both halves ship together because they are the same defect
on the two sides of one calculation.

**3a — close adjacency.**

1. `domain/valuation.rs`: add
   `pub fn is_previous_trading_day(previous: NaiveDate, latest: NaiveDate) -> bool`
   built on the existing weekday-counting helper (Mon–Fri approximation per the
   2026-06-16 staleness-calendar decision; holidays still unmodelled).
2. Add `ValuationReason::PreviousCloseNotAdjacent`, serialized
   `previous_close_not_adjacent`.
3. In `value_position`, when a previous close exists but is not the immediately
   preceding trading day, the previous-price availability becomes
   `Unavailable { PreviousCloseNotAdjacent }`, so `day_change_base` and
   `day_change_percent` are explicitly unavailable rather than computed across a
   gap. Applies to every source, feed and manual alike.

**3b — pair the previous FX rate to the previous close's date.**

4. In `load_valuation_inputs`, the previous FX rate stops being fetched relative to
   `latest_fx.date` (`find_previous_before`) and instead resolves as the last
   available rate **on or before `previous_price.date`**
   (`find_latest_on_or_before`), anchored on the previous close already resolved
   earlier in the same function. When no previous close exists there is no anchor,
   so no previous FX is resolved; day change is unavailable for
   `MissingPreviousClose` regardless, and that reason must remain first in the
   merged reason list.
5. Latest price and latest FX resolution is **untouched** — both still resolve
   independently on-or-before the valuation date, per the 2026-06-16 staleness
   decision. Do not "align" them.
6. Reason codes stay honest: a previous FX that cannot be resolved at all still
   surfaces the existing `MissingPreviousFx`, never a silent fallback to some other
   day's rate.
7. Period inputs (`load_period_inputs`) are explicitly **not** in scope here —
   decision 9 governs day change only. Leave start/end FX resolution alone.

**3c — frontend.**

8. Add `previous_close_not_adjacent` → "No adjacent previous close" to
   `reasonLabel` in `valuationDisplay.tsx`, with a unit test.

Why pairing rather than extending adjacency to FX: extending adjacency would blank
day change for *every* holding in a currency the moment that currency's feed had
one gap, where a price gap affects a single instrument — and it would blank cases
the pairing rule answers correctly. Pairing fixes the defect without blanking day
change any more often than 3a already does.

Self-correcting property worth stating: with a sparse FX series, the previous rate
now frequently resolves to the *same* rate as the latest, so the currency term
cancels and day change reports the pure price move. A sparse FX series therefore
contributes **no** spurious movement instead of a spurious multi-day one.

Tests:
- Domain unit tests: Friday→Monday is adjacent; Thursday→Monday is not;
  same-day is not; a one-weekday gap is not; a manual/feed mix behaves the same.
- Regression test (close adjacency): a holding whose two most recent stored closes
  are a week apart reports day change unavailable with the new reason, while
  market value stays available.
- **Worked-example regression test (FX pairing):** FX rows exist for Friday and
  Monday only; price rows for Tuesday and Wednesday; valuation on Wednesday.
  Before, this yielded `price(Wed)×rate(Mon) − price(Tue)×rate(Fri)` — one day of
  price movement against three days of currency movement. After, `previous_fx`
  resolves to the last rate on or before Tuesday (Monday's, the same rate as
  `latest_fx`), the currency term cancels, and day change equals the pure price
  move. Pin the expected amount, not just availability.
- Reason-set test: no previous close → the merged reasons lead with
  `MissingPreviousClose`; a non-SEK instrument with no resolvable previous rate
  still reports `MissingPreviousFx`.
- SEK instruments take the identity path and are unaffected — pin that too.

Verify:
- Backend and frontend command sequences.
- **External human testing REQUIRED, and it must include a foreign-currency
  holding** (USD, EUR, DKK and NOK positions all exist in the real portfolio), not
  only a SEK one:
  - Day change now goes blank (with the explanatory chip/tooltip) for instruments
    whose feed history has gaps. Expected and correct, but a visible change to
    existing rows.
  - On a non-SEK holding, the reported day change reflects the day's price move
    and no longer carries multi-day currency drift; spot-check one against
    `price(latest) − price(previous)` times quantity times the rate.
  - Portfolio-level day change on the Dashboard moves consistently with the rows.

---

### Phase 4 — Asset-page price editor and provenance

1. `api/types.ts`: `source: string` on `PriceSnapshot` and `PriceHistoryPoint`;
   new `ManualPrice` and `ManualPricesResponse`.
2. `api/queries.ts`: `useManualPrices(id)` plus `useCreateManualPrice`,
   `useUpdateManualPrice`, `useDeleteManualPrice`. On success they invalidate the
   shared instrument/portfolio key set (extend `invalidateInstrumentData` with the
   `manual-prices` key rather than duplicating the list) — holdings, gains,
   price-status, instrument-prices, portfolio-value-history, rebalance.
   `useDeleteInstrument` takes `{ instrumentId, expectedManualPrices }` and passes
   it as the `expected_manual_prices` query parameter.
3. New pure `components/manualPriceViewModel.ts`:
   - `manualPriceFormReducer(state, action)` — exported `(state, action) => state`
     for the add/edit draft (date, close, editing target, field errors).
   - `validateManualPriceDraft(draft, today)` — mirrors backend validation
     (including the future-date rule) so the user gets immediate feedback; the
     backend stays authoritative.
   - `manualPriceRows(response)` — sorted rows with the superseded flag and a
     display-formatted close via the existing adaptive unit-price formatter.
   - `priceSourceLabel(source)` — `MANUAL` → "Hand-entered", `YAHOO` → "Yahoo",
     otherwise the prettified code.
   - `deleteManualPriceConfirmation(row, currency)` and
     `deleteInstrumentConfirmation(count)` — the confirmation wording, derived
     purely so it is testable.
   Unit tests for the reducer transitions and each derivation.
4. New `components/ManualPriceEditor.tsx` rendered as **its own full-width panel
   directly below the price-history chart and above the two existing columns**
   (decision 16): a compact date + close form, a list of entered prices with edit
   and delete, and inline error text. Follows `docs/VisualDesign.DarkTheme.md` —
   `--surface-2` inputs with `--radius-md` and `--hairline` border, accent-pill
   primary action, neutral status chips, table density for the list, semantics as
   text colour. The list is the panel's growth area, so it must degrade gracefully
   at a dozen-plus rows (the reason it does not live in the right-hand column).
   Hidden (or disabled with the existing demo affordance) in demo mode via
   `useAppMode`.
5. **Deleting one hand-entered price is explicitly confirmed**, naming the date and
   value and stating that the removal cannot be undone (decision 11 makes these
   writes non-undoable). Soft deletion was considered and rejected: it needs a
   schema column and a purge policy for a single row the user can retype in
   seconds, and it would put non-current rows into a table valuation reads.
6. Provenance stays in "Data & mapping" (decision 3, not optional; the panel keeps
   this even though the editor moved out): a one-line "Price source" row naming the
   concrete source of the displayed latest price, derived from
   `gain.latest_price.source` through `priceSourceLabel`, plus the count of
   entered prices. `providerContent` must stop rendering a bare
   "Unmapped"/"Mapping disabled" warning when hand-entered prices are supplying the
   value — it should say the instrument is hand-priced, and keep showing the Yahoo
   symbol when a mapping exists.
7. Instrument deletion (decision 7): the confirm text states how many hand-entered
   prices will be permanently lost, using the count from `useManualPrices`, and the
   request carries that count. On `409 manual_prices_changed` the UI refetches and
   re-prompts with the fresh number; on `409 manual_prices_present` (a UI bug or a
   stale tab) it refetches and prompts rather than failing silently.

Tests:
- Vitest on the reducer and the derivations, including both confirmation strings
  and the source-label mapping (no snapshots, no DOM-structure assertions).
- `api/client.ts`-level test for the new mutation paths if the existing client
  test file covers comparable endpoints.
- One role/text component test for the delete-confirmation flow, since the
  count-carrying request and the re-prompt on conflict live nowhere else.

Verify:
- Frontend command sequence.
- **External human testing REQUIRED** (this is where the feature actually lands):
  - Instrument 32: add three or four closes; the "Valuation missing" chip
    disappears; market value, unrealized gain, portfolio weight, portfolio totals
    and the conviction target all populate; the asset price chart draws the
    entered series.
  - Layout: the editor panel sits full width directly under the chart, reads as
    the data behind it, and with a dozen rows entered it does not push the
    Data & mapping / Conviction column off-screen.
  - Edit a price → figures move. Delete it (confirming the prompt) → the app
    returns to the previous state without fabricating anything.
  - Instrument 38 (disabled AZN.L mapping): entering a manual price takes over
    valuation; the panel names "Hand-entered" as the source; the stale GBp feed
    rows are not resurrected.
  - Enter a price for a date the feed already covers on a *mapped* instrument:
    the feed value still wins, the panel names "Yahoo", and the row is marked
    superseded.
  - Freshness: confirm the hand-priced holding shows the ordinary warning-stale
    chip (decision 6) and that the Rebalance page's "Selected rung includes
    warning-stale trades…" notice fires for it — accepted, not a bug.
  - Try a future date and a zero/negative price; both are refused with a clear
    message.
  - Delete a hand-priced (never-traded) instrument: the confirmation names the
    exact count and the deletion succeeds after confirming.
  - Demo mode (`scripts/start.ps1 -Demo`): the editor is unavailable.

---

### Phase 5 — Honest reporting: hand-priced refresh runs and missing-history reasons

**5a — refresh run counter (persisted end to end).**

1. Migration `0007_add_refresh_hand_priced_count.sql` — additive
   `ALTER TABLE market_data_refresh_runs ADD COLUMN hand_priced_instruments INTEGER NOT NULL DEFAULT 0;`
   (forward-only additive, per the 2026-06-14 persistence decision).
2. `db/market_data_runs.rs` projects every column explicitly, so the new column
   must be threaded through **all** of: `LIST_SQL`, `FIND_SQL`, `LATEST_SQL`,
   `START_SQL` (its `RETURNING` list), `FINISH_SQL` (both the `SET` list and its
   `RETURNING` list, keeping the bind order aligned), `RefreshRunRow`,
   `RefreshRunCounts`, and the `finish_run` bindings. Extend the existing
   `refresh_runs_track_lifecycle_and_latest_run` repository test to assert the new
   count round-trips.
3. `market_data/refresh.rs`: an instrument with no enabled feed mapping but at
   least one MANUAL price is reported as a new `RefreshItemStatus::HandPriced`
   (serialized `hand_priced`) and counted in a new `hand_priced_instruments`
   field on `RefreshOutcome`, `RefreshRunSummary` (including
   `RefreshRunSummary::running`), `RefreshPricesResponse`, the `finish_run` call
   site, and `latest_run_summary`'s reconstruction from `RefreshRunRow`.
   It is **not** counted in `unmapped_instruments`, so the run-status rule
   (`Succeeded` when `failed_items == 0 && unmapped_instruments == 0`) makes a
   fully hand-priced portfolio finish SUCCEEDED instead of permanent PARTIAL.
   Staleness rules are untouched (decision 6). Include the split counts in the
   run-finished `engine_info!` line and the run `message`.
4. `/api/prices/status`: `latest_price_snapshot` currently returns `unmapped` for a
   disabled mapping — it must instead report the hand-entered latest price with its
   source so the dev status strip and the asset panel agree with valuation.
5. Frontend: `hand_priced_instruments` in `RefreshRunSummary`; the refresh status
   surface distinguishes "N hand-priced" from "N unmapped"; `reasonLabel` gains
   `hand_priced`.

**5b — per-row period explanations (decision 10).**

The current API cannot support a per-row explanation: `GainRow.reasons` carries
*current-valuation* reasons, while the period failure reasons live only in
`PerformanceAccumulator`'s aggregate set — and that set is discarded entirely when
partial totals succeed. Both gaps are fixed before any UI work:

6. `api/gains/performance.rs`: `PerformanceAccumulator::add` returns the per-row
   outcome (included, or the deduplicated reasons that excluded it) so
   `api/gains.rs` can attach it to the row it just built. The accumulated
   `unavailable_reasons` are always serialized on `totals` alongside
   `excluded_rows`, not only when the totals themselves are unavailable.
7. `GainRow` gains an additive `period_contribution` field, tagged like the other
   availability shapes:
   `{ "status": "included" }` | `{ "status": "excluded", "reasons": [...] }` |
   `{ "status": "not_exposed" }` (no period exposure at all — honestly distinct
   from "excluded"). `open_gain_row` and `closed_gain_row` both carry it.
8. Frontend: thread `period_contribution` and `totals.excluded_reasons` into
   `api/types.ts`; `reasonLabel` gains explicit wording for
   `missing_start_price`, `missing_end_price`, `missing_start_fx`,
   `missing_end_fx` — e.g. "No entered price at the period start"; the Gains
   incomplete indicator lists the excluded rows it can see with their reasons and
   falls back to the aggregate `excluded_reasons` for rows the current filter
   hides (a closed row excluded from totals is not rendered when
   "Include closed positions" is off).

Tests:
- Refresh integration tests: hand-priced instrument is reported as `hand_priced`,
  is excluded from `unmapped_instruments`, and a run with only hand-priced
  instruments finishes SUCCEEDED; a genuinely unmapped instrument still counts as
  unmapped and still yields PARTIAL; the count survives a `finish_run` →
  `latest_run` round-trip.
- `/api/prices/status` test: a disabled mapping plus manual rows reports the
  hand-entered price, not `unmapped`.
- Gains tests: a row missing a period start price is `excluded` with
  `missing_start_price`; a row with no period exposure is `not_exposed`; a fully
  valued row is `included`; `totals.excluded_reasons` is populated even when the
  totals themselves are available.
- Vitest for the new reason labels, the refresh-summary derivation, and the
  incomplete-indicator derivation.

Verify:
- Backend and frontend command sequences.
- **External human testing recommended:** run a manual refresh with instruments
  32 and 38 hand-priced and confirm the run no longer reports them as unmapped and
  no longer finishes PARTIAL for that reason; confirm a Gains row excluded for
  missing period history explains why.

---

### Phase 6 — Documents, decision log, version bumps, final sweep

1. `docs/Design.HighLevel.md`
   - Correct the data-model line `prices  instrument_id, date, close, currency  -- EOD cache`
     to state that `prices` holds fetched end-of-day closes **and** hand-entered
     closes that cannot be reconstructed from any external source.
   - Add a sentence to the Market data section: prices may come from a provider or
     from the user, resolved per date with the provider winning; a disabled mapping
     counts as no provider.
   - The v1 acceptance criterion "Database survives and restores from a single-file
     backup" now covers user-authored data; note that in the same section and point
     at the backup follow-up recorded below.
2. `docs/CurrencyAndFxRules.md` — one line under the canonical currency model:
   hand-entered market prices are recorded in the instrument's currency and the
   currency is never entered by the user.
3. `docs/DecisionLog.md` — four new entries, following the file's own template and
   appended at the end, naming behaviors and never this plan's phases.

   > **Note — the decision log is deliberately not written during planning.**
   > `Agents.md` observes that decisions are *sometimes* recorded during planning
   > rather than during implementation. This plan takes the other option on
   > purpose: the four entries below are **proposals**, not commitments already
   > made. Do not append them to `docs/DecisionLog.md` until this plan is actually
   > being implemented, and re-check each one against what was built before writing
   > it — a plan can still change, and the log records settled commitments only.
   > Their absence from the log today is intentional, not an omission.

   - **Hand-entered prices as a second price source** — per-date precedence
     (provider wins, hand-entered fills gaps), a disabled mapping counts as no
     provider, provenance names the concrete source, currency comes from the
     instrument, future-dated entries are rejected, staleness is not
     special-cased, and refresh reporting separates hand-priced from unmapped.
   - **`prices` is data of record, not a cache** — status change, backup
     implications, the `HAND_ENTERED` sentinel for `provider_symbol`, and the
     server-enforced acknowledgement required before an instrument delete destroys
     authored rows.
   - **Hand-entered price writes are non-undoable resource mutations** — the
     carve-out alongside standalone `POST /api/instruments`, the resulting rule
     that every destructive action on them is explicitly confirmed, and the door
     left open to additive command metadata later (2026-06-16).
   - **Day change uses an adjacent previous close paired with that close's FX
     rate** — close adjacency applies to every price source; the previous FX rate
     resolves on or before the previous close's date so both factors share a day;
     the latest close/rate resolution is explicitly unchanged. Record that
     extending adjacency to FX was considered and rejected (one currency-feed gap
     would blank day change for every holding in that currency at once), and that
     a sparse FX series now contributes no spurious movement rather than a
     spurious multi-day one. Refines the 2026-06-16 staleness rules.
4. Version bumps (one release for the whole feature; intermediate phases are not
   released): `backend/Cargo.toml` `0.14.2 → 0.15.0`,
   `frontend/package.json` `0.22.11 → 0.23.0` (and the matching
   `frontend/package-lock.json` version fields).
5. Full verification sweep on both stacks.

Verify:
- Backend: `cargo build`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt`. Frontend: `npm run check`, `npm run fmt`.
- **External human testing REQUIRED:** launch the app, confirm the two version
  values in the UI footer match the bumped manifests, and re-walk the Phase 4
  human checklist once against the real database.

## Documents to update

| Document | Change | Phase |
|---|---|---|
| `docs/Design.HighLevel.md` | `prices` is no longer an "EOD cache"; market-data section notes the second source and per-date precedence; backup criterion note | 6 |
| `docs/CurrencyAndFxRules.md` | hand-entered prices use the instrument currency | 6 |
| `docs/DecisionLog.md` | four new entries (see Phase 6) — **proposals only; deliberately not written until implementation** | 6 |
| `backend/Cargo.toml`, `frontend/package.json` (+ lock) | version bumps | 6 |

No `docs/VisualDesign.DarkTheme.md` change is expected — the editor reuses
existing form, chip and table tokens, and the chart gains no new encoding
(decision 17).

## Deliberately out of scope (recorded, not overlooked)

- **A tested database backup/restore procedure.** Automated SQLite backup is still
  only backlog work in `docs/Design.HighLevel.md`'s hardening phase, and treating
  `prices` as data of record does raise the stakes. It is deliberately *not* a
  prerequisite or a gate on this work: the exposure is whole-database and already
  exists for every table — the ledger, imports, conviction metadata — so tying it
  to this feature would neither reduce the risk nor size the job correctly. It
  belongs in its own plan covering backup, retention and a restore drill.
- **Bulk paste / CSV entry of prices**, export/import of hand-entered prices, and
  any new automated provider.
- **Visually distinguishing hand-entered points in the price chart** (decision 17)
  — deferred with reasons, and additive whenever it is wanted, because the API
  already carries `source` on every point.
- **Pairing FX to price dates outside day change.** Period start/end FX resolution
  in `load_period_inputs` keeps its current behavior; decision 9 governs day change
  only.

## Risks and accepted consequences

- **Day change goes blank on gappy feed history** (decision 9). Correct, but
  visible on existing rows; called out as a required human check in Phase 3.
  The FX-pairing half of that decision does *not* widen the blanking — it changes
  which rate is used, and in the sparse-FX case makes the currency term cancel.
- **Hand-priced holdings are permanently warning-stale** (decision 6) and will
  therefore keep tripping the rebalance warning-stale notice. Deliberate.
- **Extra per-instrument query.** `has_manual_prices` adds one indexed lookup per
  instrument on holdings/gains requests, which already issue several per
  instrument. Acceptable at single-portfolio scale; revisit only if the <1 s LAN
  load criterion is threatened.
- **Instrument delete still destroys authored prices** — by design (decision 7),
  now behind a server-enforced acknowledgement of the exact count.
- **Import rollback is unaffected.** Verified: rollback deletes the batch's
  transactions and the `import_batches` row, and touches neither instruments nor
  prices, so hand-entered prices cannot be lost through it. No work needed.
- **Backfill targeting is unaffected.** Verified: `RefreshMode::Backfill` derives
  `has_history` from grouped *transactions*, not stored prices, so MANUAL rows
  cannot cause a transacted instrument to be skipped by a later feed backfill. Do
  not "fix" this.

## Open Questions

None. Every question raised during planning and review has been settled and is
recorded in Settled Decisions above — editor placement (16), chart provenance
(17), and the previous-FX pairing rule (9). If implementation surfaces a genuinely
new choice, surface it before resolving it silently, per `Agents.md`.
