# Plan: Company fundamentals (P/E, P/S, report dates) with explicit certainty

Status: draft, revised after review · Date: 2026-11-04 · Track: routine feature plan

## Goal

After a refresh, Holdings shows a P/E-over-P/S column and a next-report column for
every stock. Each stock page shows a "How the company is doing" panel. Every figure is
in exactly one certainty state: complete, approximate (amber, with a leading "~" and a
reason on hover), estimated (report dates only), not applicable (funds, ETFs,
certificates), or unavailable with a reason (a dash). Nothing is ever shown as zero,
and nothing is silently wrong. Every quarter the app has seen is kept, so history grows
past the source's five-quarter window.

The app computes the ratios itself, from reported quarterly figures plus the prices and
FX it already stores. It does not use any provider's precomputed ratios as figures.

## Decisions this plan builds on

These come from Lars (brief decisions 1-10, plus his review answers). The plan does not
reopen them:

- No paid data service for now, but the source boundary stays open so a paid provider
  can be added later.
- The app derives P/E and P/S itself and handles currency itself (the EVO.ST P/S ≈ 74
  error is the motivating case).
- Quarterly figures come from Yahoo's fundamentals-timeseries endpoint. quoteSummary is
  not used.
- Next-report dates for US listings come from Nasdaq's free US earnings calendar.
  **Refined in review:** a date is *confirmed* only when Nasdaq marks it as announced
  by the company. A Nasdaq date without that mark is a second kind of estimate,
  labelled "Nasdaq expected", and is preferred over the app's own estimate. Dates the
  app derives itself stay labelled as the app's estimate. If Phase 1 finds no
  confirmation signal at all, every US date is an estimate, with Nasdaq-expected
  preferred over app-derived. A scheduled date never becomes a confirmed past report
  date just because the quarter's figures later appear.
- Every key figure carries a certainty state. Every quarter seen is kept.
- Holdings layout: the ticker replaces the ISIN and broker tag; the gap bar (first,
  fixed width) and the conviction selector merge into one column; there is one stacked
  P/E-over-P/S column that sorts by P/E, and one next-report column.
- Stock page: four figures, report dates, and two quarterly bar charts with hatched
  placeholders for missing quarters. Analyst forecasts and P/E over time are out of
  scope.
- Each instrument has a company-figures source link of its own, separate from its
  price mappings. The link is discovered automatically and can be corrected by hand.
  Stored quarters belong to the instrument, not to the link.
- **Correcting the link (decided in review).** The correction form asks whether the
  new symbol is the *same company on a different listing* or whether the old link was
  the *wrong company*.
  - Same company: retained quarters are kept and used.
  - Wrong company: the old quarters are set aside. They stay in storage but are never
    used in any calculation or chart. The stock page shows an "awaiting figures" state
    until the next refresh brings the right ones.
  - Nothing is deleted in either case.

Decisions settled during review. These are cheap to change and open to later
challenge, but they are not open questions:

- The merged target/conviction column sorts by signed gap percent, and conviction-rank
  sorting is dropped. A stored sort on the removed column falls back to the default.
- Correction is a small preview-then-save form on the stock page.
- Currency-crossing TTM figures convert at the FX rate on the price date, and the stock
  page names the rate used.
- Yahoo's security-type label is stored on the company-figures link only. The
  instrument's own kind is not rewritten.
- There is no feature flag now. The decision-log entry records that the provider-terms
  re-check, and possibly shipping company figures off by default, must happen before
  any installer is distributed.

Settled repository commitments that constrain the design (see `docs/DecisionLog.md`):

| Entry | Constraint applied here |
|---|---|
| 2026-06-13 Primary Market Data Sources | Provider shapes stay behind a provider boundary. Deterministic tests use recorded fixtures. Mappings are per provider. Missing data is explicit with a reason, never zero. Terms must be re-checked before any distributed or hosted use. |
| 2026-06-13 Base Currency And FX Rules / `docs/CurrencyAndFxRules.md` | FX is stored as SEK per quote unit. Conversion happens at read time. Missing FX is an explicit state. |
| 2026-06-14 Backend Persistence Stack | Runtime sqlx, decimals stored as TEXT. All SQL lives in `db/`. Migrations are forward-only and additive. |
| 2026-06-16 Refresh Triggering / Provider Dispatch / Service Injection | No scheduler. Object-safe async-trait providers over reqwest with rustls, injected through the market-data service. Tests use fakes. |
| 2026-07-02, 2026-08-29, 2026-09-17 Demo mode | Demo is ephemeral, read-only and offline in both shells. The demo seed exercises user-visible provider behaviour. |
| 2026-07-06 Holdings sort persisted / stacked columns | A stored sort is validated against known columns. Only a group's headline metric is sortable. |
| 2026-07-07 Target gap diverging bar | Bar semantics and plain-sign colouring stay unchanged. The merged column still sorts by signed gap percent. |
| 2026-07-10 View settings persisted client-side | New view controls persist by default. |
| 2026-07-15 Numeric display precision | Applies to the new figures. Ratios need a stated precision class (see Phase 4). |
| 2026-08-28 Provider identity enum-typed | New provider and source codes are Rust enums. Unknown stored codes are decode errors. No CHECK lists duplicate them. |
| 2026-08-30 Migration bytes are LF-stable | New tables and columns come only through new migration files. |
| 2026-09-12 One Clock Owns Today | Days-until, freshness, due-ness and estimates use the backend state clock. The client derives no dates. |
| 2026-09-12 / 2026-09-18 Data revision and refresh lease | New valuation-style responses carry `data_revision`, which is a cache token and not a guarantee of a consistent snapshot. Refresh-derived writes go inside lease-verified write transactions. Provider calls happen outside transactions. |
| 2026-11-02 Desktop window is the only everyday shell | The desktop has no HTTP listener, so link correction must be possible in the UI. The intended single-installer distribution is what triggers the terms re-check. |

## Design overview

### Backend layers

- **Providers** (`backend/src/providers/`). There are two new object-safe traits:
  - `FundamentalsProvider`: given one symbol and the set of series types, return the
    typed period values.
  - `EarningsCalendarProvider`: given a calendar date (or a symbol, depending on the
    spike), return the report entries, each with its confirmation mark.

  Live clients are `YahooFundamentalsClient` (new file `yahoo_fundamentals.rs`, reusing
  `providers/http.rs`) and `NasdaqEarningsCalendarClient` (new file
  `nasdaq_earnings_calendar.rs`). Both have fakes alongside the existing fakes in
  `providers/mod.rs`. Source identity is a new enum, `FundamentalsSource` (`Yahoo`; a
  paid source becomes a new variant). Calendar identity is `ReportCalendarSource`
  (`NasdaqUs`). Neither is added to `PRICE_PROVIDER_PRECEDENCE`.
- **Pure domain** (`backend/src/domain/`). This layer has no sqlx, axum or provider
  types.
  - `company_figures.rs`: quarter-grid assembly, trailing-twelve-month (TTM)
    assembly, P/E, P/S, profit margin and sales growth, each with its certainty state
    and reasons. It also holds FX crossing, the share-basis rule, the split-since-report
    rule and the consistency check.
  - `report_dates.rs`: next and last report date with certainty and basis, the
    estimate rule, announcement resolution, and the pure "is a fetch due?" decision.
- **Market data** (`backend/src/market_data/`).
  - `fundamentals_source_discovery.rs`: classification and link discovery.
  - `fundamentals_refresh.rs`: the fundamentals phase of a refresh. It does its own
    selection, fetches outside transactions, writes inside fenced transactions that
    also check the link generation, completes FX for newly discovered report
    currencies, and reports its outcome separately.
  - `report_calendar_refresh.rs`: the calendar scan and announcement reconciliation,
    added with the Nasdaq-dates phase.
  - `fx_refresh.rs` gains report currencies, plus a completion entry point.
- **Persistence** (`backend/src/db/`): new repositories `fundamentals_sources.rs`,
  `fundamental_values.rs` and, later, `report_announcements.rs`. They are created by
  new migrations only.
- **API** (`backend/src/api/`):
  - Holdings rows gain an additive `fundamentals` block.
  - New `GET /api/instruments/{id}/fundamentals` feeds the stock page.
  - The source link is corrected through `PUT` / `DELETE
    /api/instruments/{id}/fundamentals-source`, with a read-only `GET
    .../fundamentals-source/preview?symbol=`.
  - The shared `InstrumentResponse` gains `ticker`.
  - `GET /api/prices/status` `latest_run` gains the persisted fundamentals summary.

### Holdings fundamentals travel inside the holdings response

The brief left this open. The holdings handler already loads each row's latest
effective close and FX resolution for its market value. It passes **those already
loaded values** into the company-figures domain function. That explicit reuse, not the
`data_revision` stamp, is what guarantees that a row's P/E and P/S use the same price as
the value beside it. The revision mechanism allows reads during incremental refresh
writes, so it cannot guarantee a consistent snapshot. It also needs no second versioned
query or client-side join.

The stock page needs quarter series and source details that Holdings does not, so it
gets its own stamped endpoint. That endpoint loads its own price inputs, so during a
running refresh it can briefly differ from Holdings. This is the same accepted
behaviour 2026-09-12 records for other panels. Both responses are built by the same
pure domain function.

### Certainty model (response contract sketch)

```text
Figure {
  certainty: "complete" | "approximate" | "not_applicable" | "unavailable",
  value: decimal string | null,   // present only for complete / approximate
  reasons: [{ code, detail? }]    // empty only for complete
}
ReportDate {
  certainty: "confirmed" | "estimated" | "observed" | "period_only"
             | "not_applicable" | "unavailable",
  basis: "company_announced" | "nasdaq_expected" | "last_year_date"
         | "typical_gap" | null,   // why the date is what it is
  date | null, days_until | null,  // days_until from the backend clock
  period_end | null,               // "quarter ending ..." (no fiscal labels)
  observed_between | null,         // [earliest, latest] for observed last reports
  reasons: [{ code, detail? }]
}
```

- Reason codes are stable snake_case. The frontend maps them to plain language;
  `detail` carries, for example, a currency, a ratio or a date for the hover text.
- There is no zero sentinel anywhere. A computed zero or negative denominator becomes
  `unavailable`.
- Display labels for the next report:
  - `confirmed` → "confirmed";
  - `estimated` with basis `nasdaq_expected` → "Nasdaq expected";
  - `estimated` with an app basis → "est." (the app's own estimate).
- "Complete and fresh" means the inputs are complete and the company figures are not
  known to be out of date. "Out of date" means a newer report was due (from any next
  date) more than a grace period ago and has not appeared in the source. In that case
  an otherwise complete figure becomes `approximate` with the reason
  `newer_report_expected`.

Initial reason codes. The final list follows the spike.

- Not applicable: `fund_or_etf`, `not_a_company` (set by hand), `certificate`.
- Unavailable:
  - source and link: `no_fundamentals_source`, `source_ambiguous`,
    `source_unreachable` (detail: last success date), `security_type_unknown`,
    `awaiting_figures`;
  - quarter coverage: `too_few_quarters`, `year_ago_quarter_missing`,
    `latest_quarter_too_old`, `report_currency_changed`;
  - inputs and units: `loss_making` (P/E only), `non_positive_revenue`,
    `price_unavailable`, `fx_unavailable` (detail: currency),
    `share_basis_unverified`, `listing_unit_mismatch` (detail: observed ratio),
    `split_since_report`;
  - checks: `figures_inconsistent`, `implausible_ratio`.
- Approximate: `ttm_from_three_quarters`, `eps_derived_from_net_income`,
  `price_stale`, `newer_report_expected`, `figures_disagree`.
- Report-date reasons: `estimate_passed`, `report_date_passed`.

### Computation rules (spike-dependent parameters marked [S#])

- **Quarter grid.** The grid is built from active (not set aside) quarter period ends,
  in order. Two period ends 80-100 days apart are adjacent; each further multiple of
  about 91 days marks one missing quarter [S10 sets the tolerances for 52/53-week
  reporters]. Missing quarters are explicit grid slots. The stock page charts them as
  hatched placeholders, and TTM logic treats them as gaps.
- **TTM assembly.** The window is the latest four grid slots ending at the latest
  stored quarter.
  - **Additive measures (revenue, net income).** Four present slots make the figure
    complete. A missing slot may be filled exactly as annual minus the other three
    quarters, but only when S3 shows the annual and quarterly series are the same
    measure on the same basis and the annual period covers exactly those four slots.
    A slot filled this way counts as complete. Otherwise three present slots make the
    figure approximate (`ttm_from_three_quarters`, the sum scaled by 4/3).
  - **EPS is never reconstructed by subtraction.** Annual and quarterly EPS use
    different weighted-average share denominators, so annual EPS minus three quarters
    is not the missing quarter's EPS.
    - If the missing slot's net income is present or exactly derived, and its diluted
      average share count is present, its EPS is net income divided by that share
      count. The result is always approximate (`eps_derived_from_net_income`), never
      complete.
    - Otherwise the slot stays missing, and the three-of-four rule applies
      (approximate), or the figure is unavailable.
    - A provider trailing-EPS series, if S3 finds one, is used only as a cross-check
      and never stands in for a complete TTM EPS.
  - Fewer than three usable slots make the figure unavailable.
  - If the latest quarter ends more than a set age before today [S2, default 200
    days], the figure is unavailable with `latest_quarter_too_old`.
  - If the series' report currency changes inside the window, the figure is
    unavailable.
- **P/E** = latest effective close / TTM diluted EPS. EPS is converted into the
  listing currency when the two currencies differ. TTM EPS ≤ 0 means unavailable with
  `loss_making`.
- **P/S** = latest effective close / (TTM revenue / shares). Shares means the latest
  reported share count [S1/S7 choose between period-end ordinary shares and latest
  diluted average shares, and must match the basis EPS uses].
  - Dual-class rule: the source reports one total-company share count across all
    classes, and the listing's price stands in for every class. That is exact for
    equal-economic-rights classes (Volvo A/B, GOOGL/GOOG). Small price differences
    between such classes are a known approximation and are not flagged.
  - Classes with unequal economic rights are handled by the share-basis rule below.
- **Profit margin** = TTM net income / TTM revenue, over the same slots. **Sales
  growth** = latest quarter revenue / revenue of the slot four quarters earlier, minus
  1; a missing year-ago slot gives `year_ago_quarter_missing`. Both are unit-free, so
  they survive share-basis and FX gaps.
- **FX crossing.** Report currency R and listing currency L convert as
  `rate(R→L) = fx(R→SEK) / fx(L→SEK)`. Each rate is the stored rate on or before the
  price date, using the existing carry-forward rule; SEK→SEK is 1. The whole TTM sum
  converts at that one rate. A missing rate gives `fx_unavailable` with the currency
  in `detail`. The stock-page P/S notes "converted R→L at <rate> (<date>)" whenever
  R ≠ L.
- **Price.** The latest effective close comes from the same multi-source resolution
  Holdings uses. On Holdings it is exactly the value already loaded for the row. A
  warning-stale price under the 2026-06-16 staleness rule downgrades the ratio to
  approximate (`price_stale`). No price means `price_unavailable`.
- **Share basis must be established.** Without that, P/E and P/S are unavailable.
  Comparing price/EPS with (price × shares)/net income cannot detect a wrong
  listing-to-share basis, because price cancels out of the comparison. So the basis is
  established by **independent unit evidence**, and P/E and P/S are available only
  when that evidence confirms that one listing unit equals one reported share.
  - *Source.* Phase 1 must find it [S7, a blocking acceptance criterion]. The first
    candidate is a provider company market value dated near the price date (Yahoo's
    timeseries valuation types, such as a trailing market cap). It is used only as
    evidence, never displayed as a figure. Other candidates are a depositary-receipt
    marker in Yahoo search or Nasdaq listing data. A hand-entered share ratio is later
    work.
  - *Storage.* Evidence values are stored like other company figures: the metric,
    value, currency, as-of date and provider symbol. Their kind is recorded so the
    interpretation can tell them apart.
  - *Interpretation* happens at read time, in the domain. Implied units =
    evidence market value / (listing price on the evidence date × reported shares ×
    FX).
    - Within a tolerance of 1: `established`.
    - Near a clean ratio (for example 5 for TSM): `listing_unit_mismatch`, with the
      ratio in `detail`.
    - Anything else, no evidence, evidence older than its maximum age [S7], or no
      listing price on the evidence date: `share_basis_unverified`.
  - Margin and growth still show in every one of these cases.
- **Split since report.** A share split dated after the latest quarter's period end and
  on or before the price date makes P/E and P/S unavailable (`split_since_report`).
  This holds until a quarter ending after the split is stored, or until unit evidence
  dated after the split establishes the basis. The split may be a ledger `SPLIT` row,
  or a provider split event if S7 identifies a source for one. The rule covers the
  case where stale EPS and shares agree with each other but not with a post-split
  price.
- **Consistency check (data quality only).** Compute P/E both as price / TTM EPS and as
  (price × shares) / TTM net income.
  - Agreement within a tight tolerance: no effect.
  - Moderate disagreement: approximate (`figures_disagree`, with the percentage in
    `detail`).
  - Beyond that: P/E and P/S are unavailable (`figures_inconsistent`).

  This catches mismatched EPS, net-income or share series. It does not prove the unit
  basis; only the share-basis rule does that. A P/E below 1 or a P/S below 0.05 is
  unavailable (`implausible_ratio`). The spike calibrates the tolerances on the
  fixtures [S13]. Starting values are 5% and 20%.
- **Not applicable** applies only to a *positively identified* non-company:
  - a recognised fund or ETF security type (the list comes from S8);
  - a Nasdaq price mapping whose asset class is a positively non-share class;
  - a hand-set "not a company".

  An absent or unrecognised security type is not "not applicable". It is handled by
  discovery (see below) and can end as unavailable with `security_type_unknown`.
- **Revisions.** For each (instrument, period, metric), the most recently updated
  active value wins across the active rows. The first-seen record of a period is never
  moved by a revision.

### Report dates

Next report, in order of preference:

1. An active, unresolved Nasdaq announcement with the company-announced mark gives
   `confirmed` (basis `company_announced`). This arrives with the Nasdaq-dates phase.
2. An active, unresolved Nasdaq date without that mark gives `estimated` (basis
   `nasdaq_expected`).
3. Otherwise, if the app knows the real report date of the same quarter one year
   earlier, the estimate is that date plus 52 weeks (same weekday), with basis
   `last_year_date`. "Real" means a company-announced date that was then resolved, or
   an observed window no wider than 7 days.
4. Otherwise the expected next period end is the latest period end plus one quarter
   [S10 decides month-end versus 13-week step], plus a typical lag (basis
   `typical_gap`). The lag is the median of this instrument's known real lags; failing
   that, a default lag that is longer for the fiscal-year-end quarter [S3 identifies
   fiscal year ends from the annual series; S4/S5 set the defaults].
5. No stored quarters gives `unavailable`.

**Unresolved dates are kept.** A Nasdaq date stays the next report until it is
resolved or superseded, even after it has passed:

- *Resolved*: the quarter it refers to (matched by fiscal quarter ending, or failing
  that the first new quarter first seen on or after the date) is stored.
- *Superseded*: a later scan moves it to another date, or no longer lists it.

A passed, unresolved date carries `report_date_passed` (confirmed) or
`estimate_passed` (estimates) and reads as "expected <date>, figures not seen yet".
App estimates behave the same way. This keeps the fetch policy's due trigger alive on
later days.

Last report:

- `confirmed` only when the latest stored quarter resolved an announcement that carried
  the company-announced mark *when it was scheduled*, and the quarter was first seen on
  or after that date. A `nasdaq_expected` date never becomes a confirmed last report,
  even when the quarter later appears.
- `observed` when the app saw the quarter appear between two fetches no more than
  7 days apart. It is shown as "around <date>" with the window on hover.
- Otherwise `period_only` ("quarter ending <date>"). Non-US holdings normally land in
  `observed` or `period_only`. Fiscal-quarter labels such as "Q1 FY27" are not shown,
  because the app cannot verify them.

Observation record. For every stored period, the app records the provider symbol it
came from, the day it first saw the period, and the date of the latest earlier
successful fetch in which the period was absent. On an instrument's first fetch
(including the first fetch after a wrong-company correction) there is no earlier fetch,
so no window is claimed.

### Fetch policy (report-timing driven; brief left the backstop open)

The pure decision lives in `report_dates.rs`. It runs for each refresh target (the
same selection as a latest price refresh: open positions plus convicted watchlist rows)
that has a fetchable link. A fetch happens when any of these holds:

1. it has never been fetched successfully under the current link generation;
2. a report is due: the current next report date, including a passed but unresolved
   one, is on or before today, and no newer quarter has been stored since that date.
   Fetch at most once per local day, for up to 21 days after the due date;
3. the last successful fetch is older than the **7-day backstop**. This catches
   revisions, missed estimates and source lag, at a cost of at most about 44 requests
   a week;
4. the previous attempt failed with an outage or rate limit, and that attempt was not
   today.

Otherwise the instrument is skipped as `not_due`.

Two limits apply to every fetch:

- No instrument is fetched twice on one local day under the same link generation.
- The first rate-limit response ends the fundamentals phase for that run; the
  remaining instruments are reported as `skipped_rate_limited`.

Fundamentals run in latest-mode refreshes (launch and manual) only. They do not run in
backfill.

### Refresh integration, link generations and outcome separation

- The fundamentals phase runs inside the same leased run, after price and FX. Provider
  calls happen outside transactions, and the lease cancellation flag is checked
  between instruments.
- **Link generation.** Every company-figures link row carries a `generation` counter.
  It increments on every change: a manual PUT, a DELETE, or a discovery write. Before
  a provider call, the refresh reads the link and its generation. The `fenced_write`
  closure, which runs inside the lease-checked `BEGIN IMMEDIATE` transaction, then
  re-reads the link. If the generation or symbol changed, it writes nothing and
  reports the item as `superseded`. This applies to figures, observations, fetch
  bookkeeping and discovery results. The existing `fenced_write` checks only run
  ownership, so the generation check is mandatory on every fundamentals write path.
  Without it, an in-flight result under a valid lease could restore figures Lars just
  rejected.
- **Same-run FX completion.** The FX step before fundamentals already covers listing
  currencies and previously stored report currencies. After the fundamentals fetches,
  the phase collects the report currencies now needed by the targets: newly
  discovered, newly corrected, or changed. Those not fetched earlier in the run and
  inside the verified ECB set are fetched by an FX completion step through
  `fx_refresh`, fenced like the rest. Completion results and failures are reported in
  the fundamentals summary, never as FX failures, so a TWD-style gap can never turn a
  run `Partial`.
- **Outcome separation.**
  - The run's `status` (`Succeeded | Partial | Failed`) and its existing counts stay
    computed from price and FX only. `RefreshItemKind` is not widened.
  - `RefreshPricesResponse` gains an additive `fundamentals` summary: `status`,
    `fetched`, `skipped`, `superseded`, `failed`, and `items`, each with instrument,
    symbol and reason.
  - The same summary counts and status are persisted on the run record (additive
    columns), so a launch refresh's outcome, whose response nobody reads, is not lost.
    `/api/prices/status` `latest_run` exposes them.
  - **Intent, stated explicitly.** "Reported separately" protects the price-outage
    signal. It does not add a new app-bar notice. Price-run outcomes have none today:
    the refresh reducer keeps only request failures. Company-figure problems are made
    user-visible where the figures are:
    - each affected figure's hover reason names the problem and the last good date
      (`source_unreachable`);
    - the stock page's company-figures source section shows that instrument's last
      attempt outcome and last success.
- **Mid-run revision bump.** Once price and FX writes are committed, the service bumps
  the data revision, so prices appear without waiting for the fundamentals phase. It
  bumps again at finish, as today. This refines the 2026-09-18 bump rule and needs a
  decision-log entry. The refreshing flag stays true until the phase ends.
- Every provider failure is logged through `engine_logging`, naming the instrument id,
  symbol, URL or endpoint, run id, link generation and reason code.

### FX set widening

This is the plan-writer's choice, as the brief preferred. `fx_refresh::target_currencies`
adds the distinct report currencies of the targets' active stored figures. The
completion step adds newly needed ones in the same run. Both keep only currencies in a
code constant listing the ECB reference currencies, which the spike verifies against
Frankfurter's currency list [S6]. Currencies outside the set (TWD) are never requested,
so the figures that need them show `fx_unavailable`. The rule is recorded in
`docs/CurrencyAndFxRules.md`.

### Source link discovery and classification

Discovery runs inside the refresh for targets that have no link, or that have a
discovered link which has returned no quarterly data on 3 consecutive fetches or
answered "not listed". The order matters, so that classification can reach every
outcome:

1. **Search.** Search Yahoo by ISIN when the instrument has one, otherwise by symbol.
2. **Identity match.** Pick the candidates that are the instrument itself: results of
   its ISIN query, or name and symbol matches under the existing matching rules. They
   do not need to match the listing currency.
3. **Classify the identity-matched candidates before any equity filtering.**
   - Every identity-matched candidate has a recognised fund or ETF type [S8]: store a
     link with status `not_applicable` and the type, and never fetch it.
   - At least one candidate is an equity: continue with those.
   - The type is absent or unrecognised (the existing `quote_type` field is optional):
     continue with those candidates, marked `classification_unknown`. A verified fetch
     that returns quarterly company figures makes the link usable, recorded as
     "classified by the figures returned". No figures makes it unavailable with
     `security_type_unknown`, never `not_applicable`.
4. **Choose.** Prefer the enabled Yahoo price mapping's symbol when it is among the
   remaining candidates. Otherwise prefer the primary listing the spike identifies
   [S9].
5. **Verify** the chosen candidate by a timeseries fetch that returns quarterly data.
6. **Ambiguity.** More than one equally good candidate makes the link `ambiguous`,
   with the candidates stored for the correction UI. No candidates makes it
   `no_match`.

Discovery writes go through the generation-checked fenced write. A link set by hand
(or "not a company") is never overwritten by discovery.

**Correction contract.**

- `PUT /api/instruments/{id}/fundamentals-source` takes
  `{ "provider": "YAHOO", "provider_symbol": "…", "previous_link": "same_company" | "wrong_company" }`,
  or `{ "provider": null, "not_a_company": true, "previous_link": … }`.
  `previous_link` is required whenever the instrument already has active stored
  figures. The request is rejected with a stable 400 code when it is missing.
- `DELETE` (return to automatic discovery) takes the same `previous_link` choice, for
  the same reason: rediscovery might find a different company.
- **One transaction** does all of the following:
  1. writes the new link (origin `MANUAL`, or `DISCOVERED`-pending for DELETE) and
     increments its generation;
  2. for `wrong_company`, marks every active stored value and observation of the
     instrument as set aside, with a timestamp and the reason "wrong company";
  3. resets the fetch bookkeeping: last success, last attempt, consecutive-empty count
     and absent-fetch baseline.

  For `same_company`, values and observations stay active and the bookkeeping still
  resets, so the next refresh fetches immediately. Nothing is ever deleted.
  Set-aside rows are excluded from every read path, including the grid, TTM, charts,
  report-date history and FX-currency collection.
- **Before the replacement fetch.** After a wrong-company correction, Holdings figures
  and the stock page show `awaiting_figures` ("Figures for <symbol> load on the next
  refresh"). After a same-company correction, the retained figures stay visible, with
  the source section noting the pending update.
- **Preview** (read-only `GET`) runs the search plus one timeseries fetch for a typed
  symbol. It returns the name, security type, report currency, number of quarters and
  latest period end, so Lars can see he picked the right company before saving. It
  follows the instrument-lookup pattern: in demo it answers `provider_unavailable`
  without network.

### Ticker label

The ticker is derived once per request by a `db` read returning a map from instrument
id to display ticker. The rule:

1. the enabled Yahoo price mapping's symbol, with its exchange suffix stripped
   (`EVO.ST` → `EVO`, `ASML.AS` → `ASML`, `MSFT` → `MSFT`);
2. otherwise the current company-figures link's symbol, stripped the same way;
3. otherwise `instruments.symbol`, when it is neither ISIN-like nor from an `AVANZA`
   instrument;
4. otherwise none.

Nasdaq orderbook ids are never used, and neither are disabled price guesses.
`InstrumentResponse` carries `ticker: string | null` everywhere it is built (Holdings,
Gains, Transactions via `/api/instruments`, Opportunities, Rebalance).

## Phases

The sequence starts with the source spike, then the smallest backend slice that stores
real quarters, then the figures, then each UI surface, then Nasdaq dates. Every phase
leaves the app working: the default refresh exercises the new path, and demo mode stays
offline and green.

Unless a phase says otherwise, every command below runs from the repository root
`C:\Users\larsp\src\TickerTapeTallyBoard`. Frontend commands run from `frontend/`.

### Phase 1 — Source confirmation spike (no application code)

Deliverables:

- `scripts/probe-fundamentals.ps1`. It is read-only against the providers and follows
  the style of `scripts/probe-nasdaq-nordic.ps1`. It takes a private symbol list,
  calls the Yahoo timeseries, Yahoo search and Nasdaq calendar endpoints with the same
  plain request shape the Yahoo chart client uses, and writes raw outputs and per-call
  latency to `docs/spikes/private/fundamentals/` (gitignored).
- **The production ledger is never opened by this phase.**
  1. Copy the newest launch backup snapshot from the backup folder to a scratch
     location outside the repository.
  2. Start the development loop on that copy with an explicit absolute
     `-DatabaseUrl`, plus `-NoRefresh -NoBackup`. Startup writes such as migrations
     and the revision bump touch only the copy.
  3. Export the instrument and symbol list with GET requests into
     `docs/spikes/private/`.
  4. Delete the copy afterwards.

  The exported list is never committed.
- Recorded public fixtures under `backend/tests/fixtures/fundamentals/`:
  - Yahoo timeseries for `MSFT`, `ASML.AS`, `EVO.ST` (including the skipped quarter),
    one Volvo listing (`VOLV-B.ST`, plus `VOLV-A.ST` if the share-count question needs
    it), `TSM`, a company with a share split in the last two years, and an ETF
    (`IQQK.DE` for `IE00B0M63391`, expected to be empty or non-equity);
  - the unit-evidence responses for the same companies (S7);
  - Yahoo search responses for `EVO.ST`'s ISIN, `TSM`, the ETF and, if one exists in
    holdings, an instrument with an absent or unrecognised `quoteType`;
  - the trailing and annual type responses for at least MSFT and EVO.ST;
  - Nasdaq calendar responses containing at least one *company-announced* and one
    *Nasdaq-expected* entry, if both exist, plus one empty or "not found" day.
- `docs/spikes/FundamentalsSourceSpike.md` holds sanitized findings: aggregate counts
  and the public fixture companies only, never the holdings list or quantities. It ends
  with a **parameters table** that later phases consume.

Questions the spike must answer:

- **S1.** Which quarterly series types are reliably present? The candidates are
  revenue, diluted EPS, net income (and which variant), diluted and basic average
  shares, ordinary shares outstanding, and free cash flow. Their exact type names and
  value shapes (`asOfDate`, `periodType`, `currencyCode`, `reportedValue`) must be
  recorded.
- **S2.** How often are quarters missing across all holdings? Record the per-instrument
  count within the window, which quarter tends to be missing (the US Q4 pattern), and
  how far behind today the latest quarter typically ends.
- **S3.** Annual and trailing series:
  - Do `trailing*` and `annual*` types exist for revenue, net income and EPS?
  - For the additive measures only, are they the same measure and basis as the
    quarterly series, and do they cover exactly four quarters? That decides whether
    exact subtraction is allowed for revenue and net income.
  - Does annual EPS differ from the sum of quarterly EPS? Record examples, which
    confirm why EPS is never subtracted.
  - Do annual `asOfDate`s identify fiscal year ends?
- **S4.** Does any Yahoo field carry a real announcement date? The expected answer is
  no, because `asOfDate` is the period end.
- **S5.** How does the Nasdaq US calendar work?
  - Request shape and required headers. Is it indexed by date? How far ahead do dates
    appear?
  - **The confirmation signal.** Which field, flag or label marks a date as announced
    by the company, as opposed to algorithmically expected? Nasdaq describes its
    earnings dates as derived from historical reporting dates. The finding must be
    documented and fixtured with one entry of each kind. If no signal exists, record
    that, and every US date becomes `nasdaq_expected`.
  - Do past days stay retrievable, and do past entries keep their mark?
  - Which fields does it carry (symbol, fiscal quarter ending, time,
    `lastYearRptDt`)?
  - Do entries move or disappear between scans (postponement, withdrawal)?
  - What does a refresh cost in requests and latency? Is there throttling?
  - Is there a per-symbol date endpoint that is cheaper than scanning by day? Only its
    date fields would be used, because analyst figures are out of scope.
- **S6.** Which report currencies appear across holdings, and which of them Frankfurter
  (ECB) publishes. Record the verified ECB currency constant.
- **S7. Share basis (blocking acceptance criterion).** The spike must deliver all of
  the following, or Phase 2 does not start:
  - *A unit-evidence source* whose fixtures distinguish TSM (5:1) from MSFT, ASML,
    EVO.ST and Volvo (1:1), with its as-of-date behaviour and a maximum evidence age;
  - *how it is stored*: metric, as-of date, currency, provider symbol;
  - *how it is interpreted*: the implied-units formula, tolerances, and how clean
    ratios are recognised;
  - *which share series* matches the EPS basis, and whether Volvo A/B and
    GOOGL/GOOG return one total-company share count and an evidence value covering all
    classes;
  - *split detection*: whether provider split events are available (for example from
    the chart endpoint) in addition to ledger splits, and how the split fixture
    behaves between its report date and later prices.

  If no evidence source passes, stop and put it to Lars. Under the share-basis rule,
  P/E and P/S would otherwise be unavailable for every holding.
- **S8.** Classification: the Yahoo `quoteType` values across holdings (how many are
  equities, funds or ETFs, absent or unrecognised), the list of values treated as
  positively fund or ETF, and the Nasdaq asset classes that are positively non-share.
- **S9.** Discovery: for each holding, does an ISIN or symbol search yield an
  identity-matched candidate with quarterly data? Count no-match, ambiguous and
  classification-unknown results. For Nasdaq-priced Nordic instruments, record which
  Yahoo listing carries company figures and how to recognise the primary listing.
- **S10.** Period-end regularity: are there 52/53-week reporters, and what spacing
  tolerances keep the quarter grid correct?
- **S11.** Latency and volume: what does a first refresh cost (about 44 searches,
  about 44 timeseries calls, the evidence calls, and calendar calls), and what does a
  typical refresh cost?
- **S12.** Re-check the endpoints' terms or usage notes so the decision-log entry can
  record the personal-use basis.
- **S13.** Calibration:
  - Compute P/E, P/S, margin and growth by hand from the fixtures with the planned
    rules.
  - Record the consistency-check figures and set its tolerances and bounds.
  - Confirm that EVO.ST P/S lands at about 6-7 and not about 74.
  - Confirm that TSM ends as `listing_unit_mismatch`.

Verification:

- This phase is documentation, a probe script and fixtures, so no application build or
  test is needed.
- Check that no file under `docs/spikes/private/` is staged and no database copy is
  inside the repository.
- Check that committed fixtures contain only the named public tickers.

**External human testing recommended.** Lars spot-checks the S13 hand-computed P/E and
P/S for three or four fixture companies against a reference he trusts. Agreement is a
by-eye verdict.

**Gates:**

- S7 failing stops the work and goes to Lars.
- Any finding that contradicts a brief or review decision also goes to Lars before
  Phase 2. Examples: the timeseries lacks EPS or share counts, or the Nasdaq calendar
  is unusable even for expected dates.
- S5 finding no confirmation signal is *not* a gate. Lars's answer already covers it:
  every US date is then an estimate.

### Phase 2 — Company figures stored by the refresh (backend)

Builds:

- Migration `backend/migrations/0010_create_company_fundamentals.sql`, a new file with
  LF line endings. Existing migrations are not edited. Enum-like columns carry no CHECK
  lists; they are typed in Rust per 2026-08-28. Contents:
  - `instrument_fundamentals_sources`: one row per instrument. It holds:
    - the source and the provider symbol (null for "not a company");
    - the provider security type and the classification outcome;
    - the origin (`DISCOVERED`/`MANUAL`), the discovery status and candidate detail;
    - **`generation`**;
    - the attempt and success bookkeeping (last attempt time, status and reason; last
      success time; consecutive-empty count).
  - `fundamental_values`: (instrument, period type `QUARTER`/`ANNUAL`/`TRAILING`
    [per S3] or an evidence period, period end or as-of date, metric, value TEXT
    decimal, currency, source, **provider symbol**, first seen, updated, **set-aside
    time and reason**). Unique per (instrument, source, provider symbol, period type,
    period end, metric), so rows retained under another symbol never collide. Upserts
    update only active rows.
  - `fundamental_period_observations`: (instrument, period type, period end, **provider
    symbol**, first seen on, latest earlier fetch in which the period was absent,
    **set-aside time and reason**).
  - Additive columns on `market_data_refresh_runs` for the fundamentals summary
    (status, fetched, skipped, superseded, failed).
- `db/fundamentals_sources.rs` and `db/fundamental_values.rs`, with typed `Metric`,
  `PeriodType`, `FundamentalsSource`, `LinkOrigin`, `LinkStatus` and
  `Classification` enums. They provide:
  - generation-checked write helpers that take the expected generation and return
    `Superseded` on mismatch;
  - active-only read queries;
  - the atomic correction operation used by Phase 5.
- `providers/yahoo_fundamentals.rs`: the client and parser, behind `FundamentalsProvider`
  with a fake. One request per instrument carries every needed type, including the
  unit-evidence type chosen in S7 if it comes from the same endpoint.
- `market_data/fundamentals_source_discovery.rs`: classification-then-selection
  discovery as designed, reusing `providers::SymbolSearchMatch.quote_type`.
- `market_data/fundamentals_refresh.rs` covers:
  - the fetch policy (the pure decision in `domain/report_dates.rs`, using only the
    app estimate steps in this phase);
  - generation-checked fenced writes;
  - observation recording;
  - the rate-limit short-circuit;
  - the same-run FX completion step;
  - the separate `fundamentals` summary on `RefreshPricesResponse` and on the run
    record.
- `refresh_execution.rs` calls the phase after FX. `refresh.rs` performs the mid-run
  revision bump. `/api/prices/status` exposes the persisted summary.
- `fx_refresh.rs` adds stored report currencies from the verified ECB constant, plus
  the completion entry point. Report-currency failures go to the fundamentals summary.
- Logging is added on every new provider, discovery and completion path.

Depends on spike: S1 (types and parser), S3 (stored series and subtraction
eligibility), S6 (ECB set), S7 (evidence type), S8/S9 (classification and discovery),
S10 (step used by the estimate), S11 (whether sequential fetching is acceptable).

Tests:

- Parser tests on every Phase 1 fixture, including the EVO.ST gap, TSM TWD, an ETF
  that is empty or non-equity, and an absent `quoteType`.
- Repository tests against the real migrations:
  - the most recently updated active value wins, and first-seen does not move on a
    revision;
  - set-aside rows are excluded from every active read;
  - rows under two provider symbols coexist without collision;
  - the observation's absent-fetch date is recorded only after an earlier successful
    fetch;
  - a manual link survives discovery;
  - a generation-mismatched write is refused.
- Pure fetch-policy tests: never fetched under the current generation, due, a passed
  but unresolved due date, the 21-day due window, the backstop, the failed-retry rule,
  and once-per-day.
- Discovery tests with fakes:
  - an all-ETF identity match stores `not_applicable` and never fetches;
  - a mixed result keeps the equities;
  - an absent `quoteType` with figures becomes usable;
  - an absent `quoteType` without figures becomes `security_type_unknown`, not
    `not_applicable`;
  - ambiguous and no-match outcomes.
- Refresh tests with fakes:
  - fundamentals failures leave the run status `Succeeded`;
  - stored report-currency FX is requested for EUR and never for TWD;
  - **same-run FX completion**: an instrument whose report currency appears in
    neither stored figures nor listing currencies (listing SEK, newly reporting EUR)
    gets EUR/SEK requested in the same run, and the results are reported in the
    fundamentals summary;
  - the first 429 stops the phase;
  - the revision is bumped after price and FX and again at finish;
  - the fundamentals summary is persisted on the run record.
- Concurrency tests, using separate pools over one file per 2026-09-18:
  - lease loss: a holder suspended inside a fundamentals provider call cannot write;
  - **link change during provider I/O**: suspend the fundamentals provider call;
    change the link through the repository's correction operation on another pool
    (wrong company); release the call. Assert that no values, observations or
    bookkeeping are written, the item is `superseded`, and the set-aside state stands.
    Repeat for a discovery write racing a manual link.

Verification:

1. `cargo build -p ticker-tape-tally-board-backend`
2. `cargo test -p ticker-tape-tally-board-backend`
3. `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`, then
   `cargo fmt`
4. If the backend version is bumped in this phase (`[workspace.package].version` in the
   root `Cargo.toml` is the workspace manifest), widen to
   `cargo clippy --workspace --all-targets -- -D warnings`.

Agent end-to-end check:

- Start the development loop (`.\scripts\start.ps1 -Dev`) on the development ledger
  with a few instruments, or on a copied backup snapshot.
- Trigger `POST /api/prices/refresh {"mode":"latest"}`. Confirm:
  - the `fundamentals` summary, and the run status unaffected by fundamentals;
  - the persisted summary in `/api/prices/status`;
  - rows in the new tables;
  - `engine.log` lines with context.
- A second refresh on the same day reports `not_due` for every instrument.

### Phase 3 — Figures and report dates served by the API, plus demo seed (backend)

Builds:

- `domain/company_figures.rs` and the completed `domain/report_dates.rs`, implementing
  the computation, certainty, share-basis, split-since-report and consistency rules,
  plus the report-date rules without Nasdaq inputs.
- `api/` changes:
  - an additive `fundamentals` block on each holdings row: `pe`, `ps`, `next_report`.
    The handler passes each row's already loaded close and FX inputs explicitly into
    the domain function.
  - new `GET /api/instruments/{id}/fundamentals`, stamped with `data_revision`. It
    returns:
    - the four figures (`pe`, `ps`, `profit_margin`, `sales_growth`);
    - the FX conversion note and the share-basis state;
    - `next_report` and `last_report`;
    - the quarter grid for sales and EPS in the report currency, with explicit
      `missing` slots;
    - the source panel data: symbol, origin, security type, classification, status,
      candidates, last success, last attempt outcome, and "awaiting figures" when
      applicable.
  - routing in `api/mod.rs`.
- Inputs are loaded through active-only `db` reads. The domain stays pure.
- The response DTOs serialize decimals as strings and never emit `0` for a missing
  figure.
- `backend/src/demo/mod.rs` seeds company figures, unit evidence and observations
  relative to the demo date. The seed covers:
  - a complete same-currency case (NOVA);
  - a cross-currency case reporting in EUR while listed in SEK (NORD);
  - an approximate three-of-four case with a hatched missing quarter, and a derived
    EPS slot (SKOG);
  - a loss-making unavailable P/E (ALBA);
  - a not-applicable ETF (GLBL);
  - an instrument with no source found (VARV);
  - a share-basis case: an existing demo instrument given `listing_unit_mismatch`
    evidence, or a new fictional depositary-receipt instrument if no existing one fits;
  - app estimates, both future and passed.

  The seed stays offline, query-only and read-only.

Depends on spike: S2/S3 (TTM and subtraction rules, latest-quarter age), S7
(share-basis interpretation, evidence age, split source), S10 (grid tolerances), S13
(consistency tolerances and bounds).

Tests:

- Domain unit tests:
  - P/E, P/S, margin and growth on fixture-derived inputs, including EVO.ST P/S pinned
    to the calibrated value;
  - TTM: three of four, exact subtraction allowed for revenue and net income only,
    **an EPS slot never subtracted, derived from net income and shares as
    approximate**, too few quarters, a currency change inside the window, and a latest
    quarter that is too old;
  - FX crossing via SEK at the price-date rate, and missing FX;
  - share basis: established; TSM `listing_unit_mismatch` with ratio 5; no evidence,
    stale evidence and no price on the evidence date all `share_basis_unverified`;
    margin and growth still available in each case;
  - **split since report**: a split between the latest period end and the price date
    gives unavailable, and a quarter after the split clears it;
  - consistency agreement, disagreement and inconsistency, and implausible bounds;
  - loss-making EPS and non-positive revenue;
  - not applicable versus `security_type_unknown`;
  - a stale price, `newer_report_expected` and `awaiting_figures`;
  - set-aside rows excluded;
  - the never-zero property;
  - next-report app estimate steps, `estimate_passed` retained after the date, and
    last-report observed versus period-only.
- API contract tests:
  - the holdings rows carry the fundamentals block, and the existing fields are
    byte-identical;
  - Holdings P/E uses exactly the row's loaded close (an injected price table makes
    any second read detectable);
  - the fundamentals endpoint returns the right shape for each demo case and 404 for
    an unknown instrument;
  - `data_revision` is present;
  - `days_until` follows a pinned clock.
- Demo test: the seeded read endpoints return each certainty state.

Verification: the same four backend commands as Phase 2. The agent's curl check
against `-Dev` and `-Demo` confirms that every state is reachable.

### Phase 4 — Holdings and the shared instrument cell (backend ticker + frontend)

Builds:

- Backend: the ticker map read and the `ticker` field on `InstrumentResponse` at every
  construction site.
- Frontend:
  - `api/types.ts` gains the figure, report-date (including `basis`) and ticker types.
  - `api/client.ts` parses the new fields.
  - New `components/companyFiguresViewModel.ts` (pure) maps a figure or report date to
    its display: text, tone (`normal | approximate | muted`), the "~" prefix, the
    confirmed / "Nasdaq expected" / "est." tag, the plain-language title built from the
    reasons, and an accessible label (for example "approximately 24.3; built from three
    of four quarters").
  - Number formatting: ratios show one decimal (a new precision class, recorded as a
    refinement of 2026-07-15), and percentages keep their two-decimal contract.
- Frontend: `InstrumentCell.tsx` takes `ticker` and shows the name link with the ticker
  beneath it. The ISIN and broker tag are no longer shown; with no ticker, only the
  name appears. Every caller passes `ticker`, with TransactionsTable taking it from
  `/api/instruments`.
- Frontend: `HoldingsTable.tsx`.
  - The Conviction and Target gap columns merge into one column, keeping the
    `target` id: the fixed-width gap bar comes first, then the conviction selector
    with the target SEK value beneath it. The column sorts by signed gap percent.
    Conviction-rank sorting is dropped; the filter box still matches conviction text.
  - New column `valuation`: P/E with P/S stacked underneath. It sorts by P/E. Complete
    and approximate values sort by value; not-applicable and unavailable rows sort
    last.
  - New column `report`: the next report date with its tag and a days-away hint. It
    sorts by `days_until` ascending, with no-date rows last.
  - Column header descriptions explain the certainty markers and the three date
    labels.
  - Sort accessors and cell views come from a pure helper (`holdingsFundamentals.ts`),
    not from logic inside the component.
  - `SORTABLE_COLUMN_IDS` gains `valuation` and `report` and drops `conviction`. A
    stored `holdings.sorting` naming `conviction` falls back to the default through the
    existing validation.
  - The search text includes the ticker.
  - Column order follows the agreed mockup (see Open Questions). The fallback order is
    Instrument, Qty, Cost, Value, P&L, Target & conviction, P/E·P/S, Next report.
  - Unavailable shows a dash with its reason on hover. Not applicable shows a muted
    "n/a" label styled differently from the dash. Approximate shows in `--warning`
    text with "~".
- `frontend/src` CSS: fixed gap-bar width, the approximate tone and the n/a tone, all
  using tokens per `docs/VisualDesign.DarkTheme.md`.

Tests:

- View-model unit tests for every certainty state, the three next-report labels, every
  reason mapping and the precision classes.
- Pure sort-field tests for both new columns, including the ordering of n/a and
  unavailable rows.
- A persistence test showing a stored `conviction` sort falls back to the default.
- `client.ts` parse tests for the new fields.
- Backend ticker-rule unit and API tests: suffix stripping, an Avanza ISIN instrument,
  a Nasdaq-only instrument with no ticker, and no use of disabled guesses.
- Component tests by role and text:
  - the Holdings row shows the ticker and no exchange chip;
  - an approximate cell exposes "~" with its explanation through its accessible name
    or title;
  - n/a and the dash are distinguishable by text.
- Update the existing Holdings, Gains, Transactions, Opportunities and Rebalance tests
  that asserted the exchange tag. Do not use snapshots.

Verification:

- Backend: the four commands as in Phase 2.
- Frontend: from `frontend/`, run `npm run check` (which includes Vitest), then
  `npm run fmt`.
- The agent measures layout in the development loop with headless Chrome over CDP. At
  the usual desktop width, Holdings must have no horizontal scrollbar and the gap bars
  must line up down the table.

**External human testing recommended.** Lars opens the desktop window
(`.\scripts\start.ps1`, production ledger, after a launch refresh) and the demo
(`-Demo`) and checks:

- the layout matches the agreed mockup;
- the "~" hover reasons and date labels read sensibly;
- sorting by P/E and by next report behaves as expected;
- the sort persists across navigation.

### Phase 5 — Stock page panel and source correction (backend endpoints + frontend)

Builds:

- Backend: `api/fundamentals_source.rs`, implementing the correction contract above.
  - `PUT` and `DELETE` call the atomic repository correction operation: link write,
    generation increment, set-aside on `wrong_company`, and bookkeeping reset, all in
    one transaction.
  - `previous_link` is required whenever active figures exist.
  - `GET …/fundamentals-source/preview?symbol=` is read-only and live; in demo it
    answers `provider_unavailable`.
  - Validation errors use the standard envelope with stable codes (`invalid_provider`,
    `invalid_provider_symbol`, `previous_link_required`). Body limits come from the
    shared definition. The demo guard and the revision bump come from the existing
    middleware.
  - The demo-mutation test list gains the PUT and the DELETE.
- Frontend:
  - New `CompanyFiguresPanel.tsx`, placed under the price chart on `AssetView.tsx`.
    It shows the four figures with the same markers, the FX conversion note, and the
    next report (with its label and days away) and last report (date or observed
    window, and the quarter ending). With no figures yet it shows an "awaiting figures"
    state.
  - New `QuarterBarChart.tsx` draws two small DOM bar charts, sales and EPS per
    quarter in the report currency. A missing slot is a hatched placeholder using
    `--wf-hatch-stripe`, with an accessible label "No figures reported for the quarter
    ending …". Negative EPS draws below an axis. Set-aside quarters never appear.
  - The panel view-model extends `companyFiguresViewModel.ts` or a sibling
    `companyFiguresPanelViewModel.ts`.
  - `api/queries.ts` gains `useInstrumentFundamentals(id)` on the versioned query layer,
    plus correction and preview mutations that invalidate only the version.
- Frontend: the "Data & mapping" panel gains a "Company figures source" section.
  - It shows the symbol, whether the link was found automatically or set by hand, the
    security type or "type unknown", the last successful update, the last attempt
    outcome, and an ambiguity or no-match reason with its candidates.
  - A small correction form, driven by an exported, unit-tested reducer, runs:
    type symbol → Preview (name, type, report currency, quarters found) → choose
    "Same company, different listing" or "Wrong company — set the old figures aside"
    (asked only when figures exist) → Save. It also offers "Not a company" and "Use
    automatic", which ask the same choice.
  - The form is hidden in demo.
  - After saving, the page shows "Figures load on the next refresh": with the retained
    figures for a same-company change, or the awaiting state for a wrong-company one.

Tests:

- Backend API tests:
  - PUT, preview and DELETE round trips, and a manual link surviving a refresh with
    fake discovery;
  - **wrong company**: values and observations set aside atomically, the generation
    incremented, the bookkeeping reset, the fundamentals endpoint showing
    `awaiting_figures`, and the next fake refresh storing the new company's quarters
    with the old ones still excluded;
  - **same company**: retained values remain used, and the next refresh merges by
    latest update;
  - a missing `previous_link` with figures present gives `400 previous_link_required`,
    and nothing changes;
  - an unknown instrument gives 404, and a blank symbol gives a 400 code;
  - demo gives 403 for the mutations and `provider_unavailable` for the preview with
    no network.
- Frontend:
  - panel view-model tests: each certainty state, the FX note shown only when the
    currencies differ, the missing-slot mapping, and the awaiting state;
  - correction reducer transitions, including that the same/wrong-company choice is
    required when figures exist;
  - `client.ts` tests for the PUT, DELETE and preview error bodies;
  - component tests by role and text: the panel heading "How the company is doing",
    four labelled figures, a placeholder bar's accessible label, and the correction
    flow's visible states.

Verification: the backend four commands, then from `frontend/` `npm run check` and
`npm run fmt`. The agent checks the panel and both correction paths in the development
loop, and the panel in demo.

**External human testing recommended.** Lars opens EVO.ST, MSFT, TSM, a Volvo listing
and an ETF in the desktop window and checks:

- each figure's state and hover text;
- the bar charts and the hatched placeholder;
- the correction flow: preview a wrong symbol, then the right one; save as wrong
  company; see the awaiting state; refresh; the right figures appear.

### Phase 6 — US report dates from the Nasdaq calendar

Builds:

- Migration `0011_record_report_announcements.sql` (new file, LF endings):
  - `report_announcements`: (instrument, source, scheduled date, **kind**
    `company_announced` / `expected` as marked at each sighting, fiscal quarter ending
    as given, time of day, first seen, last seen in a scan, **state** `active` /
    `superseded` / `withdrawn` / `resolved`, resolving period end, superseded-by);
  - `report_calendar_scans`: (source, calendar date, scanned at).

  Only entries matching held instruments are stored. They are matched by US ticker,
  meaning the enabled Yahoo price symbol or the suffix-free company-figures symbol of a
  USD listing.
- `providers/nasdaq_earnings_calendar.rs` (client, parser with the S5 confirmation
  signal, and fake) and `market_data/report_calendar_refresh.rs`. They run inside the
  fundamentals phase with lease-fenced writes.
- **Scan cadence [S5/S11].**
  - Scan whenever at least one US company-figures target exists. Having confirmed
    dates does *not* stop scanning, because re-scans are how postponements and
    withdrawals are found.
  - Cover the next H days, skipping weekends.
  - Re-scan a day when its last scan is older than R days, or when it falls within the
    next 7 days and was last scanned before today. Also re-scan the day of every
    active, unresolved announcement, even a passed one, until it resolves.
  - If S5 shows past days are retrievable, also backfill past dates at no more than K
    past days per refresh, until one year is covered. Past entries become `confirmed`
    history only if they carry the company-announced mark, and otherwise only feed the
    `last_year_date` estimate basis as Nasdaq-expected history.
  - If S5 finds a per-symbol date endpoint that is cheaper, use it instead, reading
    date fields only. Reconciliation is the same.
- **Announcement reconciliation** (pure planning in `report_dates.rs`, writes in the
  calendar refresh):
  - *Postponement or move*: an entry for the same instrument and fiscal quarter on a
    different day supersedes the old one.
  - *Withdrawal*: an active entry missing from a fresh scan of its day becomes
    `withdrawn`.
  - *Mark change*: an entry's kind follows its latest sighting.
  - *Resolution*: the matching quarter arrives (fiscal quarter ending, or failing that
    the first new quarter first seen on or after the date). The entry becomes
    `resolved` and keeps the kind it had when scheduled. A confirmed last report
    requires `company_announced` at that point.
  - An active, unresolved entry stays the next-report source after its date has
    passed, which keeps the due trigger alive.
- `domain/report_dates.rs` gains the Nasdaq inputs: `confirmed` and `nasdaq_expected`
  next dates, confirmed last reports, and `last_year_date` estimates from resolved
  history.
- Demo seed extension (offline): a confirmed future date, a Nasdaq-expected date, a
  confirmed date that has passed but is unresolved ("figures not seen yet"), and a
  resolved confirmed last report.

Tests:

- Parser tests on the calendar fixtures, including both kinds of mark.
- Ticker-matching tests.
- Pure scan-planning tests: the horizon, the re-scan age, re-scanning unresolved past
  days, the backfill budget, and scanning continuing when every target has a confirmed
  date.
- Reconciliation tests: **postponement**, **removal**, a mark change from expected to
  company-announced, and resolution.
- Domain tests:
  - confirmed beats Nasdaq-expected, which beats the app's estimate;
  - **an overdue confirmed date is retained and keeps triggering a fetch on later days
    within the window**;
  - a Nasdaq-expected date never becomes a confirmed last report when the quarter
    appears;
  - year-ago estimates use resolved history.
- Refresh tests with fakes: a calendar outage leaves the run status and the
  fundamentals figures intact, and is reported in the fundamentals summary.
- Lease-loss test for calendar writes.

Verification: the backend four commands, then from `frontend/` `npm run check` (only if
types or the view-model changed) and `npm run fmt`. The agent compares calendar-derived
dates and marks for two or three US fixture companies against the raw calendar response.

**External human testing recommended.** Lars checks two US holdings' "confirmed" next
dates against the companies' investor-relations pages, checks that a Nasdaq-expected
date is labelled as such, and confirms that a non-US holding still reads as the app's
estimate.

### Phase 7 — Documentation, decision log and release

This can be folded into each phase's commit as it lands. It is listed separately so
nothing is missed.

- Version bumps:
  - `frontend/package.json` minor bump in Phases 4 and 5. A patch bump in Phase 6 is
    needed only if the frontend changed.
  - `[workspace.package].version` in the root `Cargo.toml` gets a minor bump in Phase 2
    (refresh response and status), Phase 3 (new API), Phase 5 (correction endpoints)
    and Phase 6. Batching to one bump per merged release is acceptable.
  - A bump touches the workspace manifest, so that phase runs
    `cargo clippy --workspace --all-targets -- -D warnings`.
- Documents (see the list below), updated in the phase that changes the behaviour.
- Plan retirement after external review, per Agents.md. Durable documents and code name
  behaviours, never the phases of this plan.

## Documents to update

| Document | Change | When |
|---|---|---|
| `docs/spikes/FundamentalsSourceSpike.md` (new) | Sanitized findings, including the confirmation signal and share-basis evidence, and the parameters table. Cross-referenced from `docs/spikes/PriceProviderSpike.md`'s "Sources Checked" section. | Phase 1 |
| `docs/DecisionLog.md` | Entries listed below, appended at the end. Earlier entries are not edited. | As each commitment settles |
| `docs/Design.HighLevel.md` | Market-data section: the company-figures source and link generations, the Nasdaq US calendar and its confirmation rule, the fetch policy, the separate and persisted outcome, FX completion. Data model: the new tables and run columns. API surface: the new endpoints and the `latest_run` addition. Risks table: unofficial sources and their outage behaviour. | Phases 2, 3, 5, 6 |
| `docs/CurrencyAndFxRules.md` | Cross-currency ratio conversion via SEK at the price-date rate. The FX set includes report currencies in the ECB set, completed within the run that discovers them. Currencies outside it make dependent figures unavailable. | Phases 2-3 |
| `docs/VisualDesign.DarkTheme.md` | Certainty markers (amber "~" in `--warning` text, the dash, the muted n/a label). The three next-report labels. The fundamentals bar charts and hatched placeholders. The ticker replacing the exchange tag in the instrument cell (the density section mentions the exchange tag). The merged target and conviction column. | Phases 4-5 |
| `frontend/package.json`, root `Cargo.toml` | Version bumps | Phases 2-6 |

## Decision-log entries this work warrants

Each entry is recorded when the commitment is settled, the spike-dependent ones after
Phase 1. Each is a commitment, not a work summary.

1. **Company figures are computed by the app from reported quarters.**
   - Sources: Yahoo fundamentals-timeseries for quarters and share-basis evidence;
     Nasdaq US calendar for US report dates.
   - quoteSummary and providers' precomputed ratios are not used as figures, and no
     paid source is used for now. The source boundary is enum-typed so a paid provider
     can be added.
   - Both sources are unofficial and used on a personal-use basis. Before any installer
     is distributed (the 2026-11-02 direction), the 2026-06-13 terms re-check must
     happen, possibly together with shipping company figures off by default.
2. **Company-figures source link per instrument**, separate from price mappings.
   - It is discovered (classification before equity selection, unknown type never
     "not applicable") and is hand-correctable. A hand-set link is never overwritten.
   - It carries the provider's security type and is versioned by a generation that
     every refresh write must match.
   - Stored quarters belong to the instrument and carry their provider symbol. The
     latest active value wins.
   - A correction states "same company" (figures kept) or "wrong company" (figures set
     aside, never used, never deleted).
3. **Certainty states and rules for company figures.**
   - The five states, and never zero.
   - TTM assembly, with exact subtraction only for additive measures and EPS never
     subtracted.
   - FX crossing at the price-date rate.
   - P/E and P/S only when independent evidence establishes the share basis.
   - The split-since-report rule, and the consistency check as data quality only.
   - Not applicable only for positively identified non-companies.
4. **Fundamentals refresh.**
   - Part of launch and manual refresh, driven by report timing with a 7-day backstop.
   - FX for newly needed report currencies is completed in the same run.
   - The outcome is reported and persisted separately from price/FX run status;
     user-visible on the figures and the stock page, with no app-bar notice.
   - Refines 2026-06-16 Refresh Triggering, and refines 2026-09-18 with the revision
     bump after the price/FX writes.
5. **FX includes report currencies the ECB publishes.** This refines 2026-06-13 Base
   Currency And FX Rules.
6. **Report-date certainty.**
   - *Confirmed* only when Nasdaq marks the date as company-announced. This refines
     Lars's brief decision 5, under which confirmed means the company actually
     announced it.
   - Nasdaq dates without the mark are a labelled "Nasdaq expected" estimate,
     preferred over the app's own labelled estimate.
   - Scheduled dates never become confirmed history because figures later appear.
   - Unresolved dates are retained until resolved or superseded.
7. **Holdings layout.**
   - The ticker replaces the ISIN and broker tag in the shared instrument cell.
   - The gap bar and conviction share one column that sorts by gap, and conviction-rank
     sorting is dropped.
   - P/E over P/S sorts by P/E.
   - There is a next-report column.

   This refines 2026-07-06 stacked columns and 2026-07-07 target gap column.
8. **Ratio display precision** (one decimal). This refines 2026-07-15.
9. **The demo seed exercises company-figure certainty states and report-date kinds.**
   This refines 2026-08-29 Demo data.

## Risks

- **Refresh duration.** The first refresh after upgrade adds about 90 or more requests
  (searches, timeseries and evidence), and the Refresh action stays busy until the
  phase ends. Prices still appear early thanks to the mid-run bump. If S11 shows a
  typical (non-first) refresh gaining more than about 15 seconds, Phase 2 adds bounded
  concurrency for fundamentals fetches. That decision is made in Phase 2, not deferred.
- **Unofficial endpoints may change or throttle.** Mitigations: the provider boundary,
  recorded fixtures, the rate-limit short-circuit, last-good figures kept and marked
  `source_unreachable` when stale, and the outcome kept separate from the price signal.
- **Share-basis evidence may be thin.** If S7's evidence is only available for some
  listings, P/E and P/S are unavailable for the rest. That is honest, but it may hide
  many ratios. S7 reports the expected coverage before Phase 2 starts.
- **Pre-migration snapshot.** The first launch after Phase 2 and Phase 6 takes the
  mandatory pre-migration snapshot; this is expected.
- **Calendar-day due rules.** "Due" and "overdue" use calendar days against the
  backend clock, and holidays are not modelled. An estimate may read as passed a day or
  two early.

## Open Questions

None. The three items below were settled by Claude after review (cheap to change; Lars
may overturn any of them):

1. **Holdings column order.** Use Instrument, Qty, Cost, Value, P&L, Target &
   conviction, P/E·P/S, Next report, which matches the agreed mockup. Lars checks it
   by eye in Phase 4's human testing.
2. **Yahoo's market value as share-basis evidence.** It is allowed as evidence only.
   It is never displayed, and it is checked against the app's price × shares × FX,
   provided S7 shows it is in a known currency and stays consistent on the fixtures.
   This does not reverse brief decision 2, because every displayed ratio is still
   computed by the app.
3. **Ticker suffix.** Strip the exchange suffix, so EVO.ST is shown as EVO.
