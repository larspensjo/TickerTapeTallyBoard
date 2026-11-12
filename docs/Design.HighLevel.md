# Portfolio Tracker — Project Document

**Status:** Draft for planning · **Owner:** Lars · **Date:** 2026-06-12

## 1. Summary

A self-hosted portfolio tracking application for stocks, ETFs, and funds, running on a home PC. A single Rust binary serves a TypeScript web frontend to the local machine over a loopback-only listener. Data is owned locally (SQLite), with initial portfolio history imported from Sharesight.

### Goals
- Track holdings, transactions, dividends, and performance for stocks, ETFs, and funds.
- Multi-currency from day one (e.g. SEK base currency with USD/EUR-denominated holdings).
- Import full transaction history from Sharesight (All Trades Report export); manual entry as fallback and for ongoing trades.
- Daily (end-of-day) price updates — no realtime/streaming requirement.
- Accessible from the host machine at a loopback URL. LAN access is separate
  future work that must lift the loopback-only enforcement together with the
  authentication required to make it safe.

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
- **Dev:** `scripts/start.ps1 -Dev` runs the browser development loop: the debug backend plus Vite, with Vite proxying `/api` to a loopback backend. The browser is a development surface only.
- **Prod (home PC):** `scripts/start.ps1` builds the release desktop shell and `frontend/dist`, then opens the native window, which serves the frontend and API in-process with no listener. A single installable desktop executable for other users is the intended direction. The production ledger is one SQLite file at `%LOCALAPPDATA%\TickerTapeTallyBoard\portfolio.sqlite`; each launch writes an integrity-checked `VACUUM INTO` snapshot to the configured synced backup folder. Auto-start at logon and LAN access remain future work; LAN access must lift the loopback-only enforcement together with authentication.

### Testing strategy

The architecture is deliberately shaped so the parts that are easy to get wrong are pure and unit-testable on both sides of the stack. Tests target behavior and public contracts — reducer transitions, emitted effects, derived view-models — not internal wiring.

- **Backend (`cargo test`):** the valuation/ledger domain (`backend/src/domain/`) is pure (no axum/sqlx/IO) and carries the bulk of the tests — position derivation, split adjustment, FX carry-forward, value/price-history builders. HTTP handlers get focused integration tests via the in-process router against an in-memory SQLite pool.
- **Frontend (Vitest):** the unidirectional `input → action → reducer → state → render` flow keeps logic out of components. Reducers (e.g. board UI state), view-models (`assetViewModel`), and selectors (dashboard top-movers, allocation, chart series) are pure functions over typed API shapes and are unit-tested with Vitest in a fast node environment. React Testing Library + jsdom are available (opt-in per file) for component and interaction tests where rendering or events are the behavior under test.
- **One gate per stack.** From the repository root, backend completion runs `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings` then `cargo fmt`; frontend `npm run check` runs `tsc --noEmit`, Biome lint, **and** `vitest run` together, so types, lint, and tests pass or fail as a unit.
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
prices           instrument_id, provider, provider_symbol, date, close, currency
                                                               -- EOD cache; keyed by provider, so several providers can cover one instrument
dividend_events  instrument_id, provider, ex_date, amount (TEXT decimal), currency
dividend_coverage
                 mapping_id, start_date, end_date, outcome, reason
fx_rates         base, quote, date, rate                     -- EOD cache
instrument_fundamentals_sources
                 instrument_id, source, provider_symbol, security_type,
                 classification, origin, status, candidates, generation,
                 last_attempt_at/status/reason, last_success_at, consecutive_empty
fundamental_values
                 id, instrument_id, source, provider_symbol, period_type,
                 period_end, metric, value (TEXT decimal), currency,
                 first_seen_at, updated_at, set_aside_at, set_aside_reason
fundamental_period_observations
                 id, instrument_id, period_type, period_end, provider_symbol,
                 first_seen_on, absent_fetch_on, set_aside_at, set_aside_reason
fundamental_split_events
                 id, instrument_id, provider_symbol, split_date, numerator,
                 denominator, updated_at, set_aside_at, set_aside_reason
report_announcements
                 id, instrument_id, source, provider_symbol, scheduled_date,
                 kind, fiscal_quarter_ending, session, first_seen_on, last_seen_on,
                 baseline_period_end, state, resolved_period_end, superseded_by,
                 set_aside_at
report_calendar_scans
                 source, calendar_date, scanned_at, scanned_on, outcome,
                 last_success_at, last_success_on
report_date_lookups
                 instrument_id, source, provider_symbol, last_lookup_day,
                 last_success_on, last_outcome
history_topup_attempts
                 mapping_id, attempted_on, requested_start, requested_end, outcome,
                 earliest_returned_date, remaining_needs
market_data_refresh_runs
                 id, trigger, started_at, finished_at, status, message,
                 prices_written, fx_rates_written, unmapped_instruments,
                 failed_items, claim_owner, heartbeat_at,
                 fundamentals_status, fundamentals_fetched, fundamentals_skipped,
                 fundamentals_superseded, fundamentals_failed
import_batches   id, source (SHARESIGHT|CSV|MANUAL), imported_at, raw_file_hash
settings         key, value (base_currency, data provider keys, …)
```

Multi-currency rules: every transaction stores its native currency; the FX rate to base currency is captured at trade date (from import where available — Sharesight exports include the exchange rate — otherwise from the fx_rates cache). Valuations convert at the rate of the valuation date; cost basis converts at the trade-date rate.

### Market data
- **Requirement:** EOD closes for Stockholm-listed (ST) and US-listed instruments, plus daily FX (USD/SEK, EUR/SEK).
- **Providers and resolution:** Yahoo Finance and Nasdaq Nordic are the automatic EOD price providers, with Yahoo first in precedence and each implementation behind the Rust `PriceProvider` trait. For each date, the highest-precedence enabled source with a stored row wins. Nasdaq is auto-connected only for instruments without an enabled Yahoo mapping; a second source on an instrument with an enabled Yahoo mapping must be hand-mapped. A disabled mapping counts as no source, and rows stored behind it are never promoted into valuation.
- **Nasdaq Nordic:** Nasdaq's `api.nasdaq.com/api/nordic` interface is an undocumented internal API with no SLA, so the client is treated as best-effort. A provider outage or a stored mapping whose provider has no registered client is reported as an explicit `Unavailable` refresh item (serialized as `unavailable`). On backfill, Nasdaq rows that begin more than five calendar days after the requested start produce the `history_clamped` reason on a `Fetched` item. This warning-only heuristic can also be triggered by a recently listed instrument or a long holiday stretch, so it is not proof of a clamp. There is no per-instrument history-start marker; dates without usable rows flow through the existing value-history `incomplete` / `excluded_count` handling.
- **Ambiguous instruments:** The authoritative signal that an instrument needs a hand mapping is a price refresh item with status `Ambiguous` (serialized as `ambiguous`). The asset page shows `No price source` when no provider-symbol rows exist, or `Price sources disabled` when rows exist but all are disabled. Ambiguity is visible only in `engine.log` or the raw refresh response because no UI renders refresh items.

  Search Nasdaq by the instrument ISIN at `https://api.nasdaq.com/api/nordic/search?searchText=<ISIN>`. In `data[].instruments[]`, select the intended listing and copy its `orderbookId`, `assetClass`, and `currency`. Replace `123` below with the `instrument_id` from the `ambiguous` refresh item or the `instrument_id=` field in the corresponding `engine.log` warning, then run this against a backend serving the production ledger. The desktop window has no HTTP listener, so start the browser development loop on that ledger first:

  ```powershell
  .\scripts\start.ps1 -Dev -NoBrowser -NoBackup -Port 8480 -DatabaseUrl (Join-Path $env:LOCALAPPDATA "TickerTapeTallyBoard\portfolio.sqlite")
  ```

  Then, from a second PowerShell window:

  ```powershell
  curl.exe -X PUT "http://localhost:8480/api/instruments/123/provider-symbols/NASDAQ_NORDIC" -H "Content-Type: application/json" -d '{"provider_symbol":"TX2997672","asset_class":"TRACKER_CERTIFICATES","currency":"SEK","enabled":true}'
  ```

  Here `provider_symbol` is the selected `orderbookId`, `asset_class` is its `assetClass`, and `currency` is its `currency`; both `asset_class` and `currency` are required. The mapping does not fetch prices by itself. Backfill the newly mapped instrument from the portfolio's earliest transaction through today with:

  ```powershell
  curl.exe -X POST "http://localhost:8480/api/prices/refresh" -H "Content-Type: application/json" -d '{"mode":"backfill"}'
  ```

  The app-bar **Refresh** button (accessible name "Refresh prices") sends `mode: "latest"` and fills the 37-calendar-day window ending today, which covers the Opportunities page's longest period (30 days) plus its start-close tolerance; the length is that period plus a fixed margin kept wider than the tolerance.
- **Dividend captures:** Yahoo's existing chart request supplies closes and separately validated dividend events without extra calls. Ex-dates use the chart GMT offset, like close dates; positive amounts use the chart currency. Latest refreshes, backfills and history top-ups store valid captures in the closes' fenced transaction: events inside the requested range are replaced (including removal). Per-mapping valid coverage starts at the requested start and ends at the earlier of the requested end and the latest returned close, even for zero events; no valid coverage is added without a returned close on or after its start. Invalid captures preserve events and valid coverage, record the requested range as failed so ex-dividend evidence is unknown, and report a separate `dividend` refresh item with contextual engine logging after the write commits. Overlapping or directly adjacent coverage ranges with the same mapping and outcome merge without bridging uncovered days; merged failures retain the most recently recorded reason. A later valid capture clears failures only in the part it covers. Missing or empty currency and ambiguous sub-unit quotations (GBp, GBX, ZAc, ILA) invalidate the dividend capture defensively; price handling is unchanged: missing chart currency fails the price fetch and empty currency is passed through to price validation. Price/FX outcomes and counts are independent of dividend validation. Failed price fetches leave events and coverage untouched. Nasdaq Nordic and default fake responses supply no dividend information and create no coverage. Repository reads provide single-source events, valid and failed coverage, and the source's dividend-reporting capability to the Opportunities ex-dividend check.
- **Daily history top-up:** After company figures, latest refreshes automatically fetch a look-back year plus the longest recent-change window for held instruments (quantity above zero) whose latest-source series is short, has a gap beyond five weekdays, or contains closes fetched on or before a split's UTC date in the look-back. Only repairable needs trigger a request; a split alone never triggers one for a source whose split adjustment is unverified. History uses the latest close's source and the instrument's currency, following the shared opportunity-evidence rules. Each needing mapping gets one request, at most once per application-clock local day, including failed or interrupted attempts. Successful attempts atomically store prices and a JSON snapshot of the remaining needs, so an unchanged short history or gap is not retried as the window advances; a new gap, split-stale need or changed mapping can trigger another attempt. Invalid snapshots are logged and treated as unusable bookkeeping. A rate limit or provider outage stops further top-ups in the run, leaving unattempted mappings free to try later. A split-only need waits until the UTC fetch date is after the split; a successful attempt for other needs omits such not-yet-repairable splits from its remaining-needs snapshot so they can trigger a later attempt. All writes verify the refresh lease. History outcomes have their own `history` response item kind and never change price/FX status or counts or create a frontend notice. The normal 37-day window remains unchanged, exchange-rate holes are not topped up, and neither backfills nor offline demo run this pass. Added history participates in valuation as a correction and is published through the existing leased finish revision, after the earlier price/FX publication; there is no additional publication rule.
- Fetched data is cached in SQLite and valuation reads the stored rows; refreshes fetch each enabled provider mapping and upsert its returned rows. Refresh runs as a background job at application launch and on demand from the manual refresh action; there is no scheduled fetch.
- **Company figures:** Each instrument has a company-figures source link separate from its price mappings. Discovery verifies an enabled Yahoo listing first, then searches by ISIN or symbol when needed, preferring the same exchange for Nordic listings. Classification distinguishes companies, funds and certificates; uncertain or missing data remains explicit. Yahoo's fundamentals-timeseries endpoint supplies quarterly, annual, trailing and market-value evidence; its chart endpoint supplies classification and split events. Calls happen outside database transactions. Latest-mode refreshes fetch company figures after prices and FX; backfills do not fetch them, and demo mode remains offline. The pure company-figures result also carries EPS consistency independently of P/E and P/S. A shared evaluation compares reported EPS with net income divided by diluted shares over a caller-selected set of quarters, using one aggregate 0.02 rounding allowance, 5%/20% tolerances and summed absolute derived quarter EPS as the scale. Trailing ratios retain their existing results; company-side opportunity checks evaluate their latest/year-ago pair without inheriting unrelated valuation limitations.
- **Pure company-side opportunity evidence:** Report timing, business health, ex-dividend evidence, room to buy and the five-rule verdict consume supplied dates and domain values without I/O. Confirmed last reports include today when figures have appeared; scheduled reports today count only as coming soon. Period-only evidence stays an interval or upper bound, and relaxed absence wording uses an earlier company-announced or Nasdaq-expected schedule when present. Unknown business reasons identify unavailable inputs or severe EPS disagreement; approximation markers remain on sub-tests. Dividend amounts are compared with the start close only when their currencies match. The Opportunities API and readiness tool share the company-figures and report-evidence loader and pure report and business rules, without assembling a stock-page source panel.
- **US report dates:** Nasdaq's per-symbol `api/analyst/{SYMBOL}/earnings-date` supplies dates for refresh targets matched by a suffix-free enabled Yahoo price symbol, or a suffix-free company-figures symbol for a USD listing. Only the report text is interpreted; analyst figures are ignored. The company-announced text form, including session and fiscal quarter, gives a confirmed date. Algorithmic dates are labelled **Nasdaq expected**, ahead of the app's estimate. Lookups use concurrency four and run at most once per local day: when there is no unresolved announcement, an unresolved date is within seven days or has passed, or the last successful lookup is over three days old. Confirmed dates continue to be checked for moves. A rate limit stops new Nasdaq requests for the rest of that run; remaining work reports `skipped_rate_limited`.
- **Schedule reconciliation:** A different dated answer for the same explicit or baseline-implied fiscal quarter supersedes the old announcement and records its successor; a dated symbol answer also supersedes future or same-day entries with no explicit quarter; another quarter creates a separate announcement. Missing, unknown and failed per-symbol answers never withdraw dates. The by-day calendar is only a fallback presence check of a stored unresolved announcement's own day, after a missing or unknown answer or once the day arrives. Each day is attempted at most once per local day, including failures; last-success timestamps survive later failed attempts, and successful empty results withdraw missing entries. Report-day, past-day and results-layout rows prove presence without changing the recorded kind, while filling a missing fiscal quarter. There is no horizon scan or past-calendar backfill: past days lose the company-announced mark. All refresh writes verify the lease and re-derive the US ticker; figures-link identities also check link generation.
- **Report selection and history:** The earliest active unresolved schedule stays next even after it passes, with `report_date_passed` for confirmed dates or `estimate_passed` for estimates. It keeps daily figures fetching due until closed. Resolution matches explicit or baseline-implied fiscal year and month; a later stored quarter supersedes a skipped-quarter announcement without confirming history. With no quarter baseline, resolution uses the first new quarter seen on or after the scheduled day when the fiscal quarter is absent. Confirmed last reports require a company-announced kind at resolution and figures first seen on or after the scheduled date. Nasdaq-expected schedules never become confirmed history. App estimates, including year-ago estimates, use the app's own resolved company-announced reports and observation windows of seven days or less. In the weeks after a report, US holdings show the app's estimate when no unresolved Nasdaq date remains. Announcement rows are retained through state changes. Offline demo data includes synthetic future, expected, overdue and resolved schedules, plus observed report intervals. The schedules and observations retain their source certainty.
- **Link generations and retained history:** Every company-figures write verifies both the refresh lease and the expected source-link generation and symbol in one transaction; a changed link supersedes an in-flight result. Quarters belong to the instrument. A same-company correction retains active history across symbols, while a wrong-company correction sets aside values, observations, split events and announcement history so active reads exclude them. Whole-instrument deletion removes its stored company figures with it.
- **Offline demo evidence:** Nine synthetic held instruments use deterministic price paths relative to the supplied application date, with mild drift, overlapping waves, falls and a rebound. They cover all five Opportunities verdicts and every evidence-check state except an unknown market median (enough peers always exist) and unknown room to buy (which would require a partly unvalued portfolio). Every held row has an available native price change in all three windows; every fall carries evidence and risers carry none. The seed includes short listing history, a holed window, split-stale history, recovered and still-falling lows, dividend coverage, missing coverage and a failed capture. Company figures retain their certainty examples and cover every EPS-consistency state, weakening loss-making sales and EPS, and candidate cautions. Upcoming reports retain company-confirmed, Nasdaq-expected, last-year and typical-gap labels. Conviction targets include room to buy, already at target and no target. Every holding has prices and cached exchange rates, including NOK, so holdings, gains and portfolio history stay fully valued. The demo's prices and holdings determine its totals and history. All inputs are seeded through existing repositories into the ephemeral in-memory database; read-only demo never refreshes or contacts a provider.
- **Fetch timing:** The application clock supplies calendar dates. An instrument is attempted at most once per day under its current link. A never-fetched link is due immediately; app-estimated report dates trigger daily fetching from that date through 21 days afterwards, while active Nasdaq announcements keep it due from their date until closed, with a backstop after more than seven days without success and next-day retries for rate limits or provider outages. The first company-figures rate limit skips later figures fetches for that run. Repository failures are reported per instrument and later instruments continue; lease loss or cancellation stops the work.
- **Separate outcome and FX completion:** The response and refresh-run record carry a fundamentals outcome separately from the price/FX status and counts. Its fetched, skipped, superseded and failed counts count instruments only; report-currency FX items remain in the response's fundamentals items. Stored and newly discovered report currencies in the verified ECB set receive same-run FX completion, once per currency per run, with lease and link-generation checks. Unsupported currencies remain explicit FX gaps. Fundamentals and completion failures never degrade the price/FX run status. Latest refreshes publish a data revision after price/FX work commits and again at finish, so prices become visible while figures are still updating; a failed finish gets a best-effort revision bump.

  Nasdaq lookup and fallback failures also appear in that fundamentals outcome;
  the response's additive `report_dates` items retain their specific result separately
  from the figures fetch. Persisted counts still describe instruments, with a failed
  date operation counted once even when its figures fetch succeeds. A successful
  figures fetch alongside a date failure gives a partial fundamentals outcome.
  Company-figures problems appear through certainty reasons on the figures and
  source-attempt details on the stock page, with no app-bar notice.

### Sharesight import
Sharesight's **All Trades Report** exports to XLS/Google Sheets with trade date, type (buy/sell/adjustment), quantity, price, fees, exchange rate, and trade value. Importer parses the export (convert XLS→CSV or parse XLS directly via `calamine` crate), maps rows to ledger transactions, matches/creates instruments, and tags everything with an `import_batch_id` so a bad import can be rolled back atomically. Dividends may need a second report (Taxable Income / dividend report) or manual entry — verify during Phase 0 spike.

TODO: Avanza import rows already parse and count dividends, but dividend rows are not written into the ledger yet. When dividend support lands, route them into the income/gains model instead of leaving them as import-only metadata.

### API surface (v1 sketch)
REST/JSON: `GET/POST /api/transactions`, `GET /api/holdings` (derived), `GET /api/opportunities?window=7d|14d|30d` (held assets' recent share-price change over a backend-resolved window, plus evidence and a rule-based verdict for each fall), `GET /api/instruments`, `GET /api/portfolio/value-history`, `GET /api/instruments/:id/prices` (price history, plus the instrument's running average cost segments over its whole ledger and the open position's current native average), `POST /api/import/sharesight`, `POST /api/prices/refresh`.

`GET /api/opportunities` keeps its existing `period`, ordered held `rows` and
`data_revision` cache-token contract. Each row adds `evidence`, which is `null`
unless the exact available native close-to-close change is negative (even when a
tiny fall rounds to `0.00`). Evidence loading uses stored data only; a repository
or decoding failure fails the request and logs instrument id, exchange, symbol,
window and operation.

The evidence object has `verdict` and `checks`. `verdict` contains `code`,
`deciding`, `missing` and `cautions`; check references and cautions are snake_case
strings. Verdict codes are `sentiment_dip_candidate`,
`possible_candidate_report_timing_unknown`, `not_enough_data`,
`fell_for_a_reason` and `normal_movement`. Candidate cautions are `loss_making`,
`report_coming_soon`, `already_at_target` and `no_target`; the corresponding check
carries the report's date and certainty. Unknown evidence never passes. Price
checks decide normal movement first, then a report in the period or weakening
business explains the fall, then usable company evidence (or positively identified
non-company checks) permits a candidate. Unknown report timing alone permits a
possible candidate when the business passes and price evidence is present.

Every named check contains `state`, `reasons` and the following fields. Reasons
are objects with `code` and optional `detail`; history reasons can also carry
`start` (gap), `date` (split or invalid close), `count` and `required`. Every
`unknown` check has at least one reason. Missing values are JSON `null`. Dates
are ISO calendar strings. Percentage values are strings with exactly two decimals;
closes, dividend amounts and EPS retain decimal precision.

| `checks` field | States | Additional fields |
|---|---|---|
| `unusual_drop` | `yes`, `no`, `unknown` | `change_percent`, `adjusted_change_percent`, `usual_move_percent`, `threshold_percent`, `closes`, `sample_moves` |
| `off_high` | `deep`, `not_deep`, `unknown` | `closes`, `latest_close`, `high_close`, `low_close`, `below_high_percent`, `above_low_percent`, `near_low` |
| `report_in_period` | `yes`, `no`, `unknown`, `not_applicable` | `window_start`, `window_end`, `last_report`, `scheduled_report`, `no_report_expected_before` |
| `report_coming_soon` | `yes`, `no`, `unknown`, `not_applicable` | `report` |
| `business` | `intact`, `weakening`, `loss_making`, `unknown`, `not_applicable` | `profitable`, `sales_not_shrinking`, `earnings_not_sliding`, `eps_consistency` |
| `market` | `fell_more_than_market`, `moved_with_market`, `fell_less_than_market`, `unknown` | `source`, `change_percent`, `reference_percent`, `difference_percent`, `margin_percent`, `count` |
| `stopped_falling` | `yes`, `no`, `unknown` | `latest_close`, `low_close`, `low_date`, `low_age_trading_days` |
| `ex_dividend` | `yes`, `no`, `unknown` | `events`, `amount`, `percent_of_start_close` |
| `room_to_buy` | `room_to_buy`, `already_at_target`, `no_target`, `unknown` | `target_status` |

`last_report` is a tagged object: `kind: confirmed` has `date`;
`kind: interval` has `period_end`, `absent_on`, `first_seen`;
`kind: upper_bound` has `period_end`, `first_seen`. Period-only knowledge never
becomes an exact date. `scheduled_report` and `report` use the existing report-date
shape: `certainty`, `basis`, `date`, `days_until`, `period_end`,
`observed_between`, `reasons`. Business sub-tests contain `passes` (boolean or
null), `value`, `year_ago_value`, `approximate`, `reasons`; margin and sales growth
values are percentages, while earnings values are EPS. `eps_consistency` contains
`state` (`unavailable`, `not_cross_checked`, `consistent`, `approximate`,
`inconsistent`), `gap_percent` and `compared_quarters`, independently of valuation
ratio limitations. Dividend `events` contain `ex_date`, `amount`, `currency`.
`sample_moves` is null until the historical move sample has been built; a computed
sample retains its count even when it is too small or has no price variation.
Dividend `events` is null when coverage or input validation prevents event
selection, an empty array when a covered interval has no events, and retains
selected events when a currency mismatch prevents amount comparison.
`target_status` uses the existing conviction-target status codes; target values
and gaps remain in the row's unchanged `conviction_target` block.

Price evidence uses one source (the latest close's source), fetch times, ledger
and stored provider splits, and the source's explicit split-adjustment capability.
Unusual drop requires 120 closes without internal gaps; off-high additionally
requires full-year coverage and 200 closes. Report timing explicitly uses the
relaxed no-report rule. Company checks are not applicable only for positively
identified non-companies. A current, clearly negative business sub-test means
weakening even if another sub-test is unknown; a usable loss-making business can
qualify with its caution. Market context is the median change of at least five
other held rows with available changes in the same window, excluding the row
itself. A known ex-dividend event after the start close and on or before the end
close adds its percentage back only for the unusual-drop comparison; the row's
displayed change stays close-to-close. Market, stopped-falling and target context
do not change the verdict. A verdict describes the dip and does not predict price.

Each Holdings row includes an additive `fundamentals` block with `pe`, `ps`, and
`next_report`. Its calculations reuse that row's already loaded effective close
and FX inputs, including the already loaded ledger for split checks. Its lighter
read path shares report observations, confirmed history, independent app estimates
and source state with Opportunities and the readiness tool, and assembles only the
trailing calculation quarters. Holdings retains its quarterly-only observation
read; opportunity evidence and the readiness tool also read annual observations
for the earliest absence baseline. Holdings does not decode source-panel candidates
or assemble historical display quarters. `GET /api/instruments/:id/fundamentals` returns `pe`, `ps`,
`profit_margin`, `sales_growth`, the FX conversion and share-basis state, next and
last report dates, a quarter grid with explicit missing slots between stored
quarters (never before the earliest stored quarter), and source-panel
status and attempt history. Unknown instruments return 404. The response carries
`data_revision` as a cache token; it does not promise a cross-query snapshot.

`PUT /api/instruments/:id/fundamentals-source` sets a hand-managed link with
`{provider: "YAHOO", provider_symbol}` or `{provider: null, not_a_company: true}`.
`DELETE` on the same path returns to automatic discovery, carrying `previous_link`
in the query string with no request body. PUT accepts it in JSON. The values are
`"same_company" | "wrong_company"`; it is required when active
figures exist (`400 previous_link_required`). In one immediate transaction the
repository increments the link generation, resets fetch bookkeeping, and, for
`wrong_company`, sets aside all active values, observations, split evidence and
announcement history.
`same_company` retains figures for calculations and charts until the next refresh
merges newer values. Neither correction deletes history. Mutations return 204,
use the shared API body limit, demo guard and revision-bump middleware, and do
not fetch providers. Invalid providers and blank symbols return standard error
envelopes with `invalid_provider` and `invalid_provider_symbol`.

`GET /api/instruments/:id/fundamentals-source/preview?symbol=...` searches Yahoo
and fetches its timeseries once, without storing anything. It returns `status`
(`found`, `no_match`, `provider_unavailable`), symbol, name, security type, report
currency, distinct quarter count, latest period end and `data_revision`. In demo
it returns `provider_unavailable` without network calls. Unknown instruments
return 404 on all three source endpoints.

The stock-page fundamentals source block includes `has_active_figures`,
`awaiting_figures` and `pending_update` alongside the symbol, discovery origin,
classification, candidates and attempt history. Additive `last_success_on` and
`last_attempt_on` dates come from the backend clock's local calendar; raw timestamps
remain available. A hand-managed link is never
replaced by discovery; a generation-fenced refresh may enrich its security type
and classification after verifying the same symbol. The source correction UI
previews a typed symbol, then asks how to treat existing figures before saving.
The same choice applies to "Not a company" and "Use automatic". In demo the
source details remain visible and the correction form is hidden.

Figures contain `certainty`, a decimal-string `value` or null, and stable reason
codes with optional detail. Complete inputs yield `complete`; three-quarter
scaling, derived EPS, warning-stale prices, stale source outages, moderate
consistency gaps, or an overdue report yield `approximate`. A failed source
attempt downgrades retained figures only when the last success is older than
seven calendar days; recent failures remain visible in the source panel. A
`source_unreachable` reason names the last success as a date. App-estimated
reports have a 21-day overdue grace period; company-announced and Nasdaq-derived
dates have seven days.
Positively identified funds, certificates and non-companies are `not_applicable`;
unknown classification, missing inputs, unsupported currency, unresolved share
basis, inconsistent figures and invalid denominators are `unavailable`. Missing
figures never use zero as a sentinel. Quarterly revenue and common-stockholder
net income can be reconstructed from matching annual measures; EPS cannot.
Per-share slots fetched before a subsequent split are excluded until restated.
Margin uses matching revenue/income slots, and consistency compares each quarter
on its own diluted-average share basis, with one absolute 0.02 EPS rounding
allowance across the whole window. Consistency reason details are percentages
with one decimal and a percent sign; share-basis ratios and their reason details
are decimal strings rounded to at most four decimal places. Period ends less
than 80 days apart count only the later quarter. Domain certainty and share-basis
states are enums, mapped to the JSON contract by API response types.

US next-report dates use the earliest active announcement for the current ticker:
company-announced dates are `confirmed`, algorithmic dates are Nasdaq estimates
(`nasdaq_expected`), and otherwise the app estimates (`last_year_date` or
`typical_gap`). The backend clock computes `days_until`. Passed unresolved dates
stay next and keep the daily due fetch alive. Calendar scans only check stored
unresolved dates, at most once per local day per date; there is no backfill or
horizon scan. Report-day, past-day and results-layout sightings preserve the stored
kind while supplying a missing fiscal quarter. A different dated answer for the
same explicit or implied fiscal quarter supersedes the prior entry; a fresh
successful calendar scan without the entry withdraws it. None, unknown and failed
symbol lookups retain stored dates. A later stored quarter closes a skipped
quarter's announcement without claiming a confirmed last report.

Last reports are `confirmed` only from resolved company-announced entries;
Nasdaq-expected dates never confirm last reports. An absence/first-seen window of
at most seven days gives `observed`, otherwise `period_only`. Real report history
uses resolved company-announced dates and these narrow observed windows. Reads,
resolution and scan planning use the current ticker, obsolete-symbol active entries
are superseded inside the refresh fence, and wrong-company corrections set aside
announcement history alongside figures. Set-aside history is retained but excluded
from figures, charts, observations, estimates and confirmed last reports;
an empty replacement history is `awaiting_figures` while no attempt in the current
link generation has completed or the last attempt failed. A completed attempt,
including empty, no-match or ambiguous, ends awaiting and exposes the actual reason.
Empty or failed fetches do not restore rejected history. Empty fetches preserve an
established link's status and observation baseline; only rediscovery replaces it.

`POST /api/prices/refresh` includes an additive `fundamentals` block with `status`
(`not_run`, `succeeded`, `partial`, `failed`), instrument counts (`fetched`, `skipped`,
`superseded`, `failed`) and detailed `items` (instrument, symbol, status, reason and
optional `fx_pair`). Figures outcomes stay in `items`, while Nasdaq outcomes are
reported separately in `report_dates`; per-instrument counts use the worse outcome.
`GET /api/prices/status` exposes the same block on `latest_run`,
using the five persisted fundamentals columns; its `items` is empty because item
details are not persisted. Backfills report `not_run`. This block never changes the
existing price/FX status or counts.

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
- `PriceProvider` trait + chosen implementations; FX rate fetching; SQLite caching; background refresh at application launch and on demand.
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
- Launch-time SQLite snapshots with integrity checks, tiered retention, mandatory pre-migration protection, and a documented restore drill that was executed successfully ✅ Done (2026-09-14).
- Bounded, rotating runtime logs outside the repository, plus visible health/footer status and typed startup failures for the defined fail-loud cases ✅ Done (2026-09-14).
- A deliberate, loopback-only production run model ✅ Done (2026-09-14).
- **Still open:** auto-start at logon and LAN exposure. LAN access is separate future work that must lift the loopback-only enforcement together with authentication.
- **Deliverable:** a deliberate, loopback-only production run model; unattended auto-start remains future work.

### Backlog (post-v1, prioritized later)
Deferred engineering follow-ups such as test infrastructure are tracked in [docs/TODO.md](TODO.md).

Dividend tracking & reinvestment view · performance metrics (TWR/MWR, vs. index benchmark) · stock splits & corporate actions handling · watchlist · CSV export · tax-lot (FIFO) realized-gain report · command-based undo/redo with a visible activity/stack UI (see `docs/Design.CommandUndo.md`) · auth if ever exposed beyond LAN.

## 4. Risks & open questions

| # | Risk / question | Mitigation |
|---|---|---|
| 1 | Sharesight export may not include dividends or fund transactions cleanly | Phase 0 Spike A with real data before committing import design |
| 2 | Free price providers: rate limits, Nordic fund coverage gaps, unofficial APIs breaking | Provider trait; cache aggressively; budget option (EODHD) as fallback; probe Nasdaq endpoint shape with `scripts/probe-nasdaq-nordic.ps1` |
| 3 | FX correctness (trade-date vs. valuation-date rates) is the most common bug source in multi-currency P&L | Encode rules in one tested valuation module; unit tests with known fixtures |
| 4 | Corporate actions (splits) silently corrupt derived positions if ignored | SPLIT transaction type exists in schema from day one; handling can ship post-v1 |
| 5 | Scope creep toward realtime data | Explicit non-goal; EOD only in v1 |
| 6 | `/api/health` exposes the absolute ledger path, including the Windows user name | Loopback-only binding limits the audience to processes on the host; revisit this before the mobile work lifts the boundary and adds authentication |
| 7 | Unofficial Yahoo company figures and Nasdaq report-date sources can rate-limit, fail or change shape | Retain stored evidence and show stated figures/date reasons; use the app date estimate when no active Nasdaq date exists. Price/FX status is unaffected. Re-check source terms before any distributed installer. |

## 5. Acceptance criteria (v1)

1. Full Sharesight history imported; positions match Sharesight within rounding tolerance.
2. New trades can be entered manually in < 30 seconds.
3. Portfolio value in SEK updates automatically each day without intervention.
4. Holdings table and value-history chart load in < 1 s on the host over loopback.
5. Database survives and restores from a single-file backup. **Demonstrated by the executed restore drill on 2026-09-14: aggregate diff exit code 0, with zero changed/added/removed gains rows and zero changed/added/removed value-history points.**
