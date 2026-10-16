# Plan: Running Average Cost Line On The Asset Price Chart

Ephemeral plan document. Durable documents and code must name the behavior
("running average cost line"), never the phase numbers used here.

## Goal

On the per-asset "Price history" chart, replace the single horizontal gold
dotted break-even line (today's native average, drawn with `createPriceLine`)
with a **running average cost line**:

- a step line of the running weighted-average native cost per share, under the
  existing weighted-average cost model (DecisionLog 2026-06-14 Cost-Basis
  Accounting Method, Ledger Ordering by `(trade_date, id)`);
- it starts at the first buy's price, stays flat until the next buy day, then
  steps to the new weighted average; sells never move it;
- when the position closes (quantity reaches 0) the line stops at the closing
  sell's date; a later buy starts a fresh segment at that buy's price, and the
  closed period shows as a visible gap;
- it is drawn for open, closed-and-reopened, and fully sold-out instruments;
- only an open position's line carries the right-axis gold tag, and that tag
  equals today's native average (the Holdings table / Opportunities native
  average, not the SEK fee-inclusive "Avg cost" tile — already accepted in
  2026-07-09);
- native currency, fees excluded; visual style unchanged (gold `#e0b15e`,
  1px dotted), now step-shaped;
- Buy-marker tooltips gain an "Avg after" row.

Done = the line traces the running weighted-average native cost per share over
time with gaps over closed periods, and for open positions its right-axis tag
equals the current native average.

## Settled constraints this plan honors

- **Backend is the source of truth.** The same fold that derives positions
  emits its intermediate states; the frontend draws only. Decimal maths, ledger
  ordering, and currency handling are identical to holdings, so the line's last
  value cannot disagree with the ledger average.
- **Split adjustment** reuses the split-factor rule already used for value
  history (`split_factor` in `backend/src/domain/performance.rs`, applied to
  "transactions dated after this date" exactly as
  `backend/src/domain/value_history.rs` does; DecisionLog 2026-06-19
  Split-Adjusted Quantities). Splits during closed periods cannot be recorded
  (`apply_split` rejects quantity 0) and are an accepted misalignment.
- **Scaling:** the line keeps participating in price-chart autoscale
  (DecisionLog 2026-09-24 "The Break-Even Line Participates In Price-Chart
  Scaling"). The dashed, `noAutoscale` `referenceData` series is not reused.
- **Gap mechanism:** one Lightweight Charts `LineSeries` per held-period
  segment, `lineType: LineType.WithSteps`, each with explicit end points.
  `calendarSpineData` whitespace does not break lines, so a single series
  cannot express a gap.
- **Markers stay at raw transaction prices**; reconciling them with the
  split-adjusted line stays deferred (2026-07-09 / 2026-07-15).
- **Architecture:** input -> action -> reducer -> state -> render. Line and
  tooltip derivation live in pure view-model functions; `TimeSeriesChart`
  renders what it is given; data arrives through the shared query layer over
  `api/client.ts`.
- **No demo-seed changes.** No SQL migrations (none needed; migration bytes
  untouched).
- **Naming:** "cost basis line" / "break-even" is renamed to the behavior
  ("running average cost line") wherever touched.

## Design decisions made by this plan

These are implementation choices within the settled constraints. They are
recorded here so reviewers can challenge them; none is a user decision.

1. **Served with the price history, not a new endpoint.**
   `GET /api/instruments/{id}/prices` gains an additive, always-present
   `running_average_cost` block. Rationale: the chart already loads, pending-
   and error-handles exactly this query; the line and the price series then
   always come from one response (no second loading state, no window
   mismatch); and "the tag value comes from the same response" holds by
   construction. The block is derived from the ledger only, so it is present
   even when no price mapping is enabled, and it covers the instrument's whole
   ledger regardless of `from`/`to` (the frontend clips it to the plotted
   window). Existing fields are unchanged.
2. **Daily resolution, end-of-day state.** Transactions are folded in
   `(trade_date, id)` order and observed per trade date after the day's last
   transaction:
   - a **step** is emitted for every trade date that contains at least one Buy
     and ends with quantity > 0 (even if the average did not change, so the
     "Avg after" lookup is a plain date match);
   - a **segment opens** on the first such day after a flat state and
     **closes** on the first trade date that ends with quantity 0
     (`closed_on`);
   - split-only and dividend-only days emit nothing (after adjustment the
     line is continuous across a split);
   - consequences: a same-day close-and-reopen continues the segment with a
     step to the reopen price (no gap — accepted in the brief); a position
     opened and closed on the same day produces no segment, and a buy on a
     day that ends flat has no step and so no "Avg after" row (settled — see
     Open Questions).
3. **Split adjustment per step.** Each step's raw average
   (`cost_basis_native / quantity` at end of day) is divided by
   `split_factor(ledger rows dated after the step date, end-of-day quantity)`.
   Splits recorded in a later held period therefore also adjust earlier closed
   periods, which is correct because the provider series is adjusted for them.
4. **The open segment ends exactly on the ledger average.** The fold's final
   state is the very `Position` that `derive_position` returns; when the final
   segment is open its last step carries `final_position.average_cost_native()`
   exactly. (Mathematically identical to the adjusted fold value; using the
   final state removes Decimal rounding noise from repeating split factors such
   as 5/3.) This makes the plotted line end on the ledger average; the tag's
   displayed text is handled separately by decision 6.
5. **Open segment end point = the chart's last plotted date**, applied in the
   frontend view-model. The backend cannot know the plotted end (the chart adds
   native-currency trade-price anchors on dates without provider closes), so
   the backend reports `closed_on: null` and the view-model extends the open
   segment to the same window end the chart uses for its visible range. Closed
   segments end at `closed_on` (clipped to the window).
6. **The tag is the open segment series' own last-value label, and its text
   is the Holdings text.** `lastValueVisible: true` only on the open segment,
   `priceLineVisible: false`; closed segments never show a label, so a
   sold-out asset has no tag. The label's *position* is the open segment's
   terminal point, which is the full-precision current average (the plotted
   history is never rounded). The label's *text* must not come from the chart
   library's default formatter: that formatter rounds with `Math.round`
   (half-up), while Holdings serializes the native average through
   `money_string`, which uses `Decimal::round_dp(2)` (midpoint to even). For an
   average of 112.125 Holdings shows `112.12` but a default-formatted tag would
   show `112.13`; formatting the plotted float in JavaScript cannot fix this
   reliably either, because the plotted value is already a 10-dp rounding of
   the Decimal (double rounding). So:
   - the backend serves `current_average`, produced by the same
     `money_string(position.average_cost_native())` call Holdings uses (same
     function, same Decimal, so the strings are identical by construction),
     `null` when the position is flat;
   - the view-model attaches it to the open step line as `lastValueLabel`;
   - the chart gives the open segment series a custom `priceFormat` whose
     formatter returns `lastValueLabel` for that series' terminal value and
     falls back to fixed two-decimal formatting otherwise (`minMove: 0.01`, the
     same precision the default format uses). The step series are added after
     the existing reference and area series, so they are never the price
     scale's formatter source and the axis tick labels are unaffected.
7. **Scaling by participation.** Segment series use default autoscale, so every
   drawn step (including closed periods) is inside the fitted range. The
   anchor-value wiring on the price area series (`fitToDataAutoscale` with the
   cost value) is removed. In practice the line almost always lies inside the
   price range anyway (an average lies between the buy prices, which sit near
   that day's closes); the exception is the accepted split misalignment.
8. **Ledger errors surface as `500 internal_error`** with instrument and
   transaction context, following the value-history precedent (stored ledgers
   are write-validated; a derivation failure is an invariant violation, not
   missing data). `ApiError::into_response` already logs server errors through
   `engine_logging`.
9. **Marker tooltip rows move into the view-model.** `tradeChartMarkers` and
   `splitChartMarkers` (currently pure helpers inside `AssetView.tsx`) move to
   `instrumentChartViewModel.ts`, so the "Avg after" row is unit-testable with
   the rest of the marker derivation.

## Wire contract (additive)

Added to `PriceHistoryResponse` from `GET /api/instruments/{id}/prices`:

```json
"running_average_cost": {
  "segments": [
    {
      "steps": [
        { "date": "2025-01-10", "average": "100" },
        { "date": "2025-03-02", "average": "112.5" }
      ],
      "closed_on": "2025-06-20"
    },
    {
      "steps": [{ "date": "2025-09-01", "average": "95.1" }],
      "closed_on": null
    }
  ],
  "current_average": "95.10"
}
```

Invariants (asserted by backend tests):

- the block is always present; `segments` is empty when the ledger has no
  buys;
- segments are in date order and do not overlap; every segment has at least
  one step; step dates strictly ascend;
- `closed_on`, when present, is later than the segment's last step date;
- at most one segment has `closed_on: null`, and it is the last one; it exists
  exactly when `derive_position` reports quantity > 0, and its last `average`
  equals that position's `average_cost_native()`;
- `average` is in the instrument's native currency, split-adjusted, fees
  excluded, serialized like `close` (normalized precise string), rounded to 10
  decimal places so repeating quotients stay bounded;
- `current_average` is the display string for the right-axis tag: the
  open position's native average through `money_string` (2 dp, midpoint to
  even), byte-identical to `/api/holdings` `average_cost_native` for the same
  instrument; it is `null` exactly when there is no open segment;
- the block ignores `from`/`to`.

Frontend types (`frontend/src/api/types.ts`): `RunningAverageCostStep
{ date: string; average: string }`, `RunningAverageCostSegment { steps:
RunningAverageCostStep[]; closed_on: string | null }`, and
`PriceHistoryResponse.running_average_cost: { segments:
RunningAverageCostSegment[]; current_average: string | null }`.

---

## Phase 1 — Ledger investigation (read-only)

Purpose: confirm the accepted residuals are rare on the real ledger before
building on them.

Method:

- Read the real ledger only through read-only `GET /api/instruments` and
  `GET /api/transactions` (and `GET /api/instruments/{id}/prices` for split
  instruments), either from Lars's running production instance on its pinned
  port or from a dev instance started over a scratch copy of the newest
  launch-time backup, never the live production file:
  `.\scripts\start.ps1 -Dev -NoBrowser -NoRefresh -NoBackup -Port 8491
  -FrontendPort 5180 -DatabaseUrl <absolute path to the scratch copy>`
  (no `-InitLedger`: the copy must already exist, so a typo fails instead of
  creating an empty ledger). Coordinate any app start through the "Project
  orchestration" session.
- Analyse with a throwaway script kept in the session scratchpad, not the
  repository. Write no raw rows, symbols, quantities, or prices into the repo.

Questions to answer (aggregate counts only):

1. How many instruments have at least one Split row; how many are currently
   open vs sold out.
2. How many instruments have a closed period followed by a reopen, and for
   those, whether the stored closes or the reopen buy price show a jump around
   the closed period consistent with an unrecorded split (price ratio near a
   whole-number split ratio such as >= 1.8x or <= 0.55x across the gap).
3. For split instruments, whether stored closes before the split are
   split-adjusted (pre-split trade price / same-day close near the split
   ratio) or stale-unadjusted (ratio near 1).
4. How many Buy/Sell rows have a `currency` different from the instrument's
   currency, and how many of those belong to currently open positions.
5. How many same-day close-and-reopen days and same-day open-and-close round
   trips exist (affect gap rendering and segment creation).

Deliverable: a short "Investigation findings" section appended to this plan
with sanitized counts (DecisionLog 2026-06-12 Private Sharesight Exports).
If question 2, 3, or 4 shows the accepted residuals affect more than an
occasional instrument, stop and put it to the user before Phase 2.

Verification: documentation-only; no builds. Check `git status` shows no new
files outside this plan.

### Investigation findings (2026-10-16)

Read through the API of a dev instance over a scratch copy of the newest
launch-time backup (copy deleted afterwards). Sanitized aggregate counts:

1. 37 instruments have transactions: 21 open, 16 sold out. One instrument has
   a Split row (one row in total); it is sold out.
2. 21 closed periods in total; 4 instruments were reopened after a closed
   period. One reopen shows a price ratio of about 3.6x across a gap of
   about 13 months, but the stored closes and the trade prices agree on it.
   Because the provider series is split-adjusted, an unrecorded split would
   show in the trade prices only, so this is a genuine price move. No
   unrecorded split was found.
3. The split instrument's stored closes before the split are split-adjusted
   (the pre-split trade price differs from that day's close by about the split
   ratio).
4. No Buy/Sell row has a currency different from its instrument's currency.
5. There are no same-day close-and-reopen days, no same-day open-and-close
   round trips, and no buys on a day that ends flat.

The accepted residuals affect at most one (sold-out) instrument, so the
build continues to Phase 2 with no change to the decision log drafts.

## Phase 2 — Backend: running average cost from the position fold

Files:

- `backend/src/domain/position.rs`: extract the ordered fold into one shared
  function (e.g. `fold_positions(transactions, observe)`), which applies each
  transaction through the existing `apply` and lets the caller observe the
  state after each transaction, then returns the final `Position`.
  `derive_position` becomes a thin call to it. Keep the sortedness
  `debug_assert`. `derive_position_performance` keeps using `apply` (it needs a
  pre-apply hook for sells); adopting the shared fold there is optional.
- New `backend/src/domain/running_average_cost.rs` (named after the behavior):
  `pub fn running_average_cost(ledger: &[LedgerTransaction]) ->
  Result<RunningAverageCost, LedgerError>` with
  `RunningAverageCost { segments: Vec<AverageCostSegment>,
  current_average_native: Option<Decimal> }`,
  `AverageCostSegment { steps: Vec<AverageCostStep>, closed_on: Option<NaiveDate> }`
  and `AverageCostStep { date: NaiveDate, average_native: Decimal }`.
  `current_average_native` is the fold's final `Position::average_cost_native()`
  (unrounded; the API formats it). Implements design decisions 2–4 above via
  `fold_positions` and the existing `split_factor`. Export from
  `backend/src/domain/mod.rs`.
- The domain function is currency-blind by design, exactly like holdings
  (`LedgerTransaction` carries no currency): a trade recorded in a different
  currency than the instrument moves the line just as it moves the Holdings
  average.

Unit tests (in the new module), using the existing test-builder style from
`position.rs`:

- single buy -> one open segment with one step at the buy price;
- two buys on different days -> step at the first price, then the weighted
  average;
- partial sell -> no step, segment stays open, average unchanged;
- full close -> `closed_on` = the closing sell date, no later steps;
- close then reopen on a later day -> two segments; the second starts at the
  reopen price, not blended with the first;
- same-day close and reopen -> one continuous segment with a step to the
  reopen price on that day;
- several same-day buys, and same-day buy-then-partial-sell -> one step with
  the end-of-day weighted average (ordering by `id`);
- same-day open-and-close round trip from flat -> no segment;
- buy then closing sell on the same day as the segment's close -> no step on
  that day, segment closed that day;
- split after the last buy -> earlier steps divided by the factor; last step
  equals `derive_position(..).average_cost_native()` exactly;
- 3:2 split (repeating factor) -> last step still equals the position average
  exactly; adjusted earlier steps agree with hand-computed values to 10 dp;
- split between two buys in one segment -> pre-split step adjusted,
  post-split step not;
- sell between a buy and a split -> factor uses the running quantity at the
  split (2026-06-19 opening-quantity rule);
- split inside a later held period -> an earlier closed segment is adjusted by
  it;
- buy with missing FX and buy with a different FX rate -> native line
  unaffected (FX never enters native maths);
- dividend rows ignored;
- invariant sweep: for every fixture, an open last segment exists iff
  `derive_position` quantity > 0, and its last step and
  `current_average_native` both equal the position's native average
  (`current_average_native` is `None` iff there is no open segment);
- ledger error (sell exceeding position) propagates with the transaction id.

Existing `position.rs` tests must stay green unchanged.

Verification (repository root):

- `cargo build -p ticker-tape-tally-board-backend`
- `cargo test -p ticker-tape-tally-board-backend`
- `cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings`
- `cargo fmt`

## Phase 3 — Backend: serve the running average with the price history

Files:

- `backend/src/api/instrument_prices.rs`: load the instrument's ordered ledger
  with `transactions::ledger_for_instrument`, call `running_average_cost`, and
  add `running_average_cost: RunningAverageCostResponse { segments,
  current_average }` to `PriceHistoryResponse`, serialized per the wire
  contract (dates `%Y-%m-%d`; `average` via the module's precise-string helper
  after `round_dp(10)`; `current_average` via the same `money_string` that
  `holdings.rs` uses for `average_cost_native`, applied to the unrounded
  `current_average_native`). Map a `LedgerError` to `ApiError::internal` with a message
  naming the operation, instrument id, error code, and transaction id (e.g.
  "price-history: running average cost derivation failed for instrument 12:
  sell_exceeds_position at transaction 345"). Existing fields, gating, FX, and
  error codes are untouched.

Contract tests (alongside the existing ones in that module):

- open position with two buys -> one segment, two steps, `closed_on` null;
- **midpoint regression:** buys of 1 @ 112.00 and 1 @ 112.25 (average
  112.125) -> last step `average` is `"112.125"` (history not rounded),
  `current_average` is `"112.12"`, and it is byte-identical to the
  `/api/holdings` `average_cost_native` for the same instrument (a half-up
  rounding would give `112.13`);
- close and reopen -> two segments, first `closed_on` set, second open,
  `current_average` from the reopened position only;
- sold-out instrument -> all segments closed, `current_average` null;
- split instrument -> pre-split step adjusted;
- `from`/`to` window does not trim segments;
- instrument without transactions -> `segments: []`;
- missing/disabled price mapping -> empty `points` but segments still present;
- an invalid stored ledger (a sell exceeding the position, inserted directly
  through the repository to bypass write validation) -> `500` with
  `internal_error` and a message naming the instrument and transaction.

Documents:

- `docs/Design.HighLevel.md` API sketch: note that
  `GET /api/instruments/:id/prices` also carries the instrument's running
  average cost segments and the open position's current native average.

Verification (repository root): same four backend commands as Phase 2.

## Phase 4 — Frontend: data path and view-model (incl. "Avg after")

Files:

- `frontend/src/api/types.ts`: add the types from the wire contract.
- `frontend/src/api/queries.ts` / `api/client.ts`: no change expected — the
  existing `useInstrumentPrices` hook and `apiGet` parse path carry the new
  block.
- `frontend/src/components/chartTimeAxis.ts`: extract the chart's window rule
  (`rangeStart = visibleStart if visibleStart <= data end, else first data
  date`; `end = last data date`) into a pure `chartWindow(data, visibleStart)`
  helper. `TimeSeriesChart` and the view-model both use it, so the line is
  clipped to exactly the plotted window.
- `frontend/src/components/instrumentChartViewModel.ts`:
  - `runningAverageCostLine(response, window): ChartStepLine[]` where
    `ChartStepLine = { key: string; points: TimeSeriesPoint[]; showLastValue:
    boolean; lastValueLabel?: string }` (type exported from
    `TimeSeriesChart.tsx` alongside `ChartTradeMarker`). Parses `average` with
    `parseFiniteNumber` and skips unparsable steps; clips each segment to the
    window (left-clipping carries the prevailing value to the window start;
    segments wholly outside are dropped); appends a terminal point at
    `closed_on` (closed) or at the window end (open), skipping it when it
    coincides with the last step date; `showLastValue` only for the open
    segment, which also carries `lastValueLabel = current_average` verbatim
    (never re-formatted in the frontend).
  - `averageAfterByDate(response): Map<string, number>` from the step dates.
    Values are the split-adjusted averages the line draws (settled Open
    Question 1), so on a split instrument a pre-split buy's "Avg after" sits in
    the line's frame, not the marker's raw-price frame.
  - Move `tradeChartMarkers` and `splitChartMarkers` here from `AssetView.tsx`
    (as `priceChartMarkers(transactions, currency, averageAfter)` or similar).
    Buy markers gain an "Avg after" row (formatted with `formatUnitPrice` plus
    the currency, placed after "Avg price") when a step exists on that date;
    sells and splits never get it; a buy on a day that ends flat gets none.
- `frontend/src/components/AssetView.tsx`: use the moved marker builder and
  pass the averages map. The chart still draws the old horizontal line in this
  phase; the line swap is Phase 5.

Tests:

- `instrumentChartViewModel.test.ts`: open segment -> terminal point at window
  end, `showLastValue`, and `lastValueLabel` equal to `current_average`
  verbatim (midpoint fixture: steps ending at `"112.125"` with
  `current_average` `"112.12"` -> terminal point value 112.125, label
  `"112.12"`); closed segment -> terminal at `closed_on`, no last value, no
  label; reopen -> two lines, label only on the second; sold-out -> no line
  flagged for a last value; pre-split buy on a split fixture -> "Avg after"
  shows the adjusted step value;
  window clipping (left and right, wholly-outside segment dropped); terminal
  point not duplicated on the last step date; unparsable step skipped; empty
  segments -> `[]`; marker rows: "Avg after" on a buy day with a step, absent
  on sells, splits, and flat-ending buy days; existing marker expectations
  preserved after the move.
- `chartTimeAxis` test for `chartWindow` (visibleStart before, after, and
  absent).
- Query layer: one case in the existing hook test pattern
  (`versionedQueries.test.tsx` style, stubbed `fetch`) asserting
  `useInstrumentPrices` surfaces `running_average_cost` unchanged.

Verification (from `frontend/`): `npx vitest run` for the touched test files,
then `npm run check` and `npm run fmt`.

## Phase 5 — Frontend: draw the step line, retire the price line

Files:

- `frontend/src/components/TimeSeriesChart.tsx`:
  - replace the `costBasisLine` prop with `stepLines?: ChartStepLine[]`;
  - for each step line create a `LineSeries` with `color:
    chartColors.reference`, `lineWidth: 1`, `lineStyle: LineStyle.Dotted`,
    `lineType: LineType.WithSteps`, `priceLineVisible: false`,
    `lastValueVisible: line.showLastValue`, `crosshairMarkerVisible: false`,
    `pointMarkersVisible` only for a single-point line, the area series'
    `priceFormat` when `compactValueAxis`, and default autoscale (no
    `autoscaleInfoProvider`);
  - when a line has `lastValueLabel`, give that series a custom `priceFormat`
    (`type: "custom"`, `minMove: 0.01`) whose formatter returns
    `lastValueLabel` for the line's terminal value (compared with a tight
    relative tolerance, e.g. 1e-9, so a float that passes through the library
    unchanged or nearly unchanged still matches) and `value.toFixed(2)`
    otherwise. Put the formatter construction in a small pure helper in
    `chartTheme.ts` so it is unit-testable without the chart. Add step series
    after the reference and area series so they never become the price
    scale's formatter source; the axis tick labels stay on the existing
    format;
  - manage the series lifecycle: on a `stepLines` change remove the previous
    segment series with `chart.removeSeries` and add the new ones; recreate
    them whenever the chart itself is recreated (the chart-creation effect
    re-runs on `height`/colour/`compactValueAxis` changes, so the step-line
    effect must depend on the chart instance, not only on `stepLines`);
  - remove the `createPriceLine` effect, `costPriceLineRef`,
    `costBasisValueRef`, and the area series' break-even anchor;
  - use `chartWindow` for the visible range.
- `frontend/src/components/chartTheme.ts`: drop the break-even anchor
  wording; if `fitToDataAutoscale`'s anchor parameter has no remaining caller,
  remove it (and simplify or drop the provider on the area series), updating
  `chartTheme.test.ts`.
- `frontend/src/components/AssetView.tsx`: `AssetPriceChart` derives
  `stepLines` from `query.data` via `runningAverageCostLine` and the chart
  window; it no longer takes a `costBasisLine` prop, so the position branch and
  the no-row branch (sold-out instruments, around lines 184–225) both draw the
  line with no branch-specific wiring.
- `frontend/src/components/assetViewModel.ts`: delete `costBasisLineValue`
  and its tests.
- `frontend/src/components/PortfolioGainChart.tsx`: update the comment that
  mentions the "break-even line".

Tests (`TimeSeriesChart.test.tsx`; extend the mock with `LineType` and
`chart.removeSeries`, and distinguish step-line series from the reference
series by options):

- one step series per step line, with step type, dotted gold 1px style, and
  `lastValueVisible` only where flagged;
- **tag text midpoint regression:** for an open line whose terminal value is
  112.125 and whose `lastValueLabel` is `"112.12"`, the open series'
  `priceFormat.formatter(112.125)` returns `"112.12"` (the library default
  would render `112.13`); closed series get no custom formatter; a matching
  `chartTheme.test.ts` case covers the pure formatter helper, including the
  fallback for non-terminal values;
- step series carry no `autoscaleInfoProvider` (they participate in scaling),
  while the reference series keeps `noAutoscale`;
- rerender with different step lines removes the old series and adds the new
  ones without leaking; removing the prop removes all step series;
- no `createPriceLine` call remains;
- a crosshair callback over a buy marker renders a `tooltip` role containing
  "Avg after" and its value (user-visible row, by role/text);
- replace the old "widens the axis to keep the break-even line on screen" and
  price-line lifecycle tests.

Rendered check (recommended, headless Chrome over CDP per the project's
layout-measurement practice; coordinate app starts through "Project
orchestration"):

- Start a dev instance over an explicit, fresh scratch ledger so it is
  guaranteed to exist and to be empty:
  `.\scripts\start.ps1 -Dev -NoBrowser -NoRefresh -NoBackup -InitLedger
  -Port 8491 -FrontendPort 5180 -DatabaseUrl <absolute scratchpad path>\running-average-check-<timestamp>.sqlite`
  (a new file name per run; the path must be absolute per the 2026-09-16
  runtime-path rule; `-Port` keeps it off the production instance's pinned
  port). Through the API, create synthetic instruments and enter:
  (a) buys, a full close, and a later reopen buy; (b) buys and a full close
  only (sold out); (c) buys of 1 @ 112.00 and 1 @ 112.25 left open (midpoint
  tag case). The chart's trade-price anchors are enough to plot them.
  Optionally also a demo instance
  (`-Dev -Demo -Port 8490 -FrontendPort 5181`) for multi-buy steps and partial
  sells over real-shaped demo prices.
- Capture the price chart with `Page.captureScreenshot` and scan pixel
  columns for the gold `#e0b15e` (with antialiasing tolerance): gold present
  on both sides, absent across the closed period's proportional x-range for
  (a); right-axis gold tag present for (a) and (c); step risers visible at buy
  dates; for (b), line present and no tag.
- Read the screenshots: (c)'s tag reads `112.12`, matching that instrument's
  Holdings native average; the price-axis tick labels look as before.
- Delete the scratch ledger afterwards.
- Look at the screenshots as well. The by-eye verdict stays with Lars (human
  testing below).

Documents and versions:

- `docs/DecisionLog.md`: append the two entries drafted below, dated on the
  day they land.
- `docs/VisualDesign.DarkTheme.md`, Charts block: add the running average
  cost line mapping (gold `#e0b15e`, 1px dotted, step-shaped, one series per
  held period, last-value tag only on an open position, tag text supplied by
  the backend rather than the library's default formatter).
- Versions: frontend `frontend/package.json` 0.27.0 -> 0.28.0; workspace
  `[workspace.package].version` 0.20.0 -> 0.21.0 (the price-history response
  gained a field). Refresh `Cargo.lock`.

Verification:

- From `frontend/`: `npx vitest run` for touched files, `npm run check`,
  `npm run fmt`.
- From the repository root (the version bump touches the workspace manifest,
  so checks cover the whole workspace including the desktop crate, per the
  2026-09-16 "Cargo Workspace Checks Include The Desktop Crate" entry):
  `cargo build --workspace`, `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, then `cargo fmt`.
  The desktop crate links WebView2 and builds slowly; that cost is expected
  here.
- Rendered check above.

**Recommended human testing (Lars, real ledger; app run coordinated through
"Project orchestration"):**

- a reopened position: gap over the closed period, the new segment starts at
  the reopen buy price;
- a sold-out asset: line over each held period, no right-axis tag;
- an open position: tag text equals the Holdings table native average and the
  Opportunities average, digit for digit;
- a split instrument (if Phase 1 found one): line aligned with the
  split-adjusted price series; raw-price markers still offset (accepted);
- hovering a Buy marker shows "Avg after" matching the line level that day
  (on a split instrument, the split-adjusted level for pre-split buys).

---

## Decision log drafts (append when the work lands)

```
## YYYY-MM-DD - The Asset Price Chart Draws The Running Average Cost — reverses parts of 2026-07-09
Decision: The per-asset price chart draws the running weighted-average native cost per share as a gold dotted step line. It starts at the first buy's price, steps to the new weighted average on each later buy day, and stays flat through sells. When the position closes the line stops on the closing date, and a later buy starts a fresh line at that buy's price, leaving the closed period as a visible gap; each held period is its own chart series so a gap can never be bridged. The line is drawn for open, reopened and fully sold-out instruments; only an open position's line carries a right-axis tag, showing today's native average as the same backend-formatted text the Holdings table shows, while the plotted line keeps full precision. The series is derived in the backend by the same ordered fold that derives holdings, split-adjusted by the ledger's splits under the 2026-06-19 split-factor rule, and served with the per-instrument price history over the instrument's whole ledger regardless of the requested date window; the frontend only clips and draws it. Buy-marker tooltips show the running average after that day's trades, in the same split-adjusted frame as the line.
Context: A single level at today's average hid how the average was built and vanished for sold-out assets. Deriving the history in the frontend would duplicate the cost-basis fold and could disagree with the holdings average.
Consequences: Reverses the 2026-07-09 single-level shape, its price-line primitive, and its open-positions-only rule with its null guard; its native-currency, fee-excluded, split-adjusted level and the accepted difference from the SEK Avg cost tile remain. Resolution is daily: a same-day close and reopen draws no gap, a position opened and closed within one day draws no line, and a buy on a day that ends with nothing held has no "Avg after" row. On a split instrument a pre-split buy's tooltip shows its raw price beside a split-adjusted "Avg after". Splits that occurred while a position was closed cannot be recorded in the ledger, so an earlier held period can sit at a stale scale against the provider series; this is accepted. The fold is currency-blind like holdings, so a trade recorded in a currency other than the instrument's moves the line even though the markers skip it. Reconciling raw-price markers with the split-adjusted line remains deferred.

## YYYY-MM-DD - The Running Average Cost Line Participates In Price-Chart Scaling — refines 2026-09-24
Decision: Every drawn part of the running average cost line, including earlier steps and closed held periods, is inside the per-asset price chart's scaling range, because each segment is an ordinary chart series rather than an anchor value added to the price series. Trade markers remain excluded.
Context: Refines 2026-09-24 The Break-Even Line Participates In Price-Chart Scaling, which kept one level in view by injecting it as an anchor. A step line has many levels, and anchoring them separately would duplicate what series scaling already does.
Consequences: The line cannot clip. Because an average lies between the prices paid, it normally sits inside the price range already; the case that can widen the axis is a split-misaligned earlier held period. If that is observed squashing the price series, that is the trigger to revisit.
```

(Both Open Questions are settled and already reflected in this wording; add
any Phase 1 finding that changes a stated consequence before appending.)

## Documents to update

- `docs/DecisionLog.md` — two new entries above (durable commitments: shape
  and gap rule, sold-out display, backend derivation and serving, scaling by
  participation). Earlier entries are not edited.
- `docs/Design.HighLevel.md` — API sketch mention (Phase 3).
- `docs/VisualDesign.DarkTheme.md` — chart mapping line (Phase 5).
- Versions — `frontend/package.json`, workspace `Cargo.toml` (Phase 5).
- This plan — Phase 1 findings; delete with other plans after external review.

## Out of scope

- Reconciling raw-price markers with the split-adjusted line.
- Recording splits that happened while a position was closed.
- Demo seed changes.
- Version-stamping the price-history response with `data_revision`: the query
  key already carries the data version, so a ledger change refetches it; the
  mismatch recheck remains a property of the stamped valuation responses
  (2026-09-12).
- Any change to the SEK "Avg cost" tile, Holdings, or Opportunities.

## Open Questions

None remain open. Both questions raised by the first draft are settled (by
the orchestrator after review, not by the user); implementation does not need
to reconfirm them.

1. **Settled — "Avg after" on split instruments uses the line's frame.** The
   row stands in for the line's missing hover value, so it shows the
   split-adjusted average the line draws. On a pre-split buy this sits beside
   the marker's raw "Avg price" (e.g. raw 300, "Avg after" 75 after a 4:1
   split); that is the already accepted, deferred raw-marker mismatch. The
   backend serves no unadjusted step values.
2. **Settled — one-day held periods and closing-day buys follow daily
   resolution.** A position opened and fully closed on the same day draws no
   line segment (its markers still show), and a buy on a day that ends flat
   gets no "Avg after" row. This is consistent with the accepted same-day
   close-and-reopen limitation; Phase 1 counts occurrences.
