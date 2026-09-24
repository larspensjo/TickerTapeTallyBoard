# Plan — Range-responsive chart scaling and period-rebased gain

*Revised after external review. Every review finding is applied; the user's four
decisions are folded in and the affected open questions are closed.*

## Summary

Make the dashboard chart panel answer the selected date range instead of only
cropping it horizontally:

1. Remove the forced zero Y-axis floor from every chart (dashboard **and** the
   per-asset price chart), so short ranges are readable.
2. On the **Value** view, scale the axis to the portfolio value line only; the
   net-invested-capital reference line no longer drags the axis down, and when it
   falls outside the visible window it is announced by a labelled tag at the chart
   edge instead of silently disappearing.
3. On the **Gain** view, show gain **earned during the selected period**: the line
   starts at zero on the left edge, measured against the last complete
   observation strictly before the range start, rendered with a zero baseline
   always in view (green above, red below).
4. Add a persisted **SEK / %** toggle to the Gain view. The percent series is a
   chained daily time-weighted return, labelled in plain language so it is not
   confused with the money-weighted percentages elsewhere in the app, and marked
   as approximate for any range where the stored data cannot support exact daily
   chaining.
5. Make the panel heading period-aware, and make it impossible for the heading to
   name a period other than the one actually plotted.

Frontend-only: no backend, API, schema or migration change. The full history is
already fetched client-side by `usePortfolioValueHistory()` (no `from`/`to`
params), so the pre-range anchor point is available without a new request.

Library feasibility was confirmed against the installed lightweight-charts 5.2.1:
an autoscale provider returning `null` removes that series from scale
aggregation; price-line primitives must be folded into the primary series' range
explicitly; `BaselineSeries`, point markers, `createSeriesMarkers` and
`setVisibleRange` all support what is specified below.

## Settled decisions (do not re-open)

From the approved brief:

- Gain is rebased to the selected range start; the line begins at zero on the
  left edge; "All" is unchanged in appearance.
- The rebase **calculation anchor** is the last observation strictly before
  `start_date`, matching the 2026-06-19 "Period Reconstruction Boundary
  Convention". The period's **measured** content still begins at `start_date`.
- A SEK / percent toggle is added to the Gain view only, persisted under the
  2026-07-10 "View settings are persisted client-side" convention.
- The percent series is a chained daily time-weighted return (each observation's
  return computed after removing that observation's net cash flow).
- The forced zero Y-axis floor is removed everywhere, including the per-asset
  price chart.
- Value tab: the axis follows the portfolio value line only; the invested line
  never drags it. An out-of-window invested line gets an edge tag.
- Gain renders with `BaselineSeries` and a zero baseline always in view — not the
  same as forcing the axis floor to 0; negative gain draws below the line.
- Incomplete days are not-measured: excluded from the return chain and from the
  rebased SEK line, with the line drawn **continuously** across them. The
  existing "N days had missing inputs" chip remains the user's signal.
- "Missing data" means the backend's explicit exclusion signals, never
  calendar-day absence.
- The percent line ships as its own labelled measure with plain-language
  explanation, including the ex-dividend disclosure.
- The invested reference line and the per-asset cost-basis break-even line must
  not clip unexplained.
- Frontend-only.

From the user's review decisions:

- **Approximate percentages are computed and labelled, never blanked.** Where the
  stored observations cannot support exact daily chaining, the percentage is
  still computed, the line stays continuous, and the range carries a visible
  plain-language "approximate" marker that appears only when the condition fires.
  Rejected: blanking the percent view; fixing it in the backend by valuing
  holdings on dates the provider had no price (a larger future project that
  nothing here forecloses).
- **The Value series also skips incomplete days.** Not-measured treatment extends
  to the value and invested lines, so the two tabs never disagree about whether a
  day had a plunge. The "N days had missing inputs" chip remains the signal.
- **Lifetime gain gets no new home.** No summary tile or other surface for the
  lifetime total; it stays reachable by selecting "All".
- **The dividend caveat is conditional**, shown only when the user actually has
  dividend income in the selected period.

Rejected and not to be re-raised: cumulative gain with only axis auto-fit;
percent as period gain ÷ starting value; keeping the zero baseline on Value;
leaving the per-asset chart zero-pinned; keeping both Value lines always fully
visible; a visible break in the line at incomplete days; making the chart percent
follow the existing return-method selector; deferring the percent toggle.

## Semantics specification

This section is the contract the pure view-model implements; the phases refer
back to it.

### Observations and the window

- An **observation** is a `ValueHistoryPoint` with `incomplete === false`.
  Incomplete points are *not measured* anywhere: not plotted on either tab, not
  used as chain links, not used as the anchor.
- For a resolved range `[start, end]` (the gains `report_period`):
  - **measured points** = observations with `start <= date <= end`.
  - **anchor** = the last observation strictly before `start`. An incomplete
    point is skipped when choosing the anchor, because its `value_base` sums only
    the included positions and would inflate the whole rebased series.

### The opening: known-empty vs unknown

A missing anchor does **not** automatically mean the portfolio was empty. All
pre-range points may be incomplete, or stored valuation history may begin after
the portfolio's first transaction. The distinction uses the value-history
response's own `start_date`, which the backend sets to the earliest trade date
across the whole ledger:

- **Known empty opening** — `range.start` is null, or the history response has no
  `start_date` (no transactions at all), or `range.start <= history.start_date`.
  The portfolio genuinely held nothing before the range, so the opening is
  `value = 0, invested = 0`. This is the 2026-06-19 boundary convention applied
  to an empty opening, and it makes "All" render exactly like today's cumulative
  gain line.
- **Unknown opening** — no anchor exists but the portfolio did have transactions
  before `range.start`. The portfolio's value at the start of the period is not
  knowable from stored data. Gain for the period is **unavailable** with an
  explicit explanation; it is never reported as if the period began from zero
  (which would present inception gains as period gains).

### The opening-origin point (the line starts at zero)

The drawn gain series carries an explicit **origin point** valued at `0` (SEK) or
`0.00%`:

- The origin's date is the calendar day immediately before `range.start` (or, when
  `range.start` is null, the day before the first measured point).
- `visibleStart` for the gain chart is the origin's date, so the visible left edge
  is exactly the zero origin and the first measured point shows the period's
  first move — including the turn of the year on YTD and the first day of a 7D
  week. The gain chart's window is therefore one calendar day wider than the
  Value tab's; that is the visible representation of "measured from the close of
  the day before the period began".
- When the anchor's own date is the day before `range.start`, the origin sits on
  the anchor's date — the most truthful placement. When the anchor is older, the
  origin carries the anchor's rebased value (zero) forward to the boundary, which
  is the same carry-forward convention the valuation endpoint already uses.
- Exactly one point per date: the origin's date is always strictly before
  `range.start`, and measured points are always on or after it, so the two can
  never collide. This matters because lightweight-charts requires unique, ordered
  timestamps.
- A known-empty opening still gets the origin at zero — the portfolio held
  nothing then, so zero gain is literally true. An unknown opening gets no origin
  and no series (unavailable state, above).

This is the reconciliation of two settled requirements that pull against each
other: the line must start at zero on the left edge, and the period's measured
content must start at `range.start`. The origin point is the opening boundary,
drawn separately from the first day's measured result.

### Rebased SEK gain

For a measured point `p`:

```
gainSek(p) = (value_p - value_anchor) - (invested_p - invested_anchor)
```

with `value_anchor = invested_anchor = 0` for a known-empty opening. A point is
included only when `invested_base` is available at both `p` and the anchor.

This is arithmetic on two endpoints only, so it is **exact regardless of skipped
observations or calendar gaps**: a day the chart could not measure, a weekend, or
a stretch when the portfolio held nothing changes nothing about the value of
`gainSek(p)`. The approximation marker below is therefore percent-only.

### Percent series — chained daily time-weighted return

Let `o_0` be the anchor (or the known-empty opening) and `o_1 … o_n` the measured
points with `invested_base` available, in date order.

```
flow_k = invested_k - invested_(k-1)
percent(o_k) = (Π_{j<=k} (1 + r_j) - 1) * 100
```

Each link's `r_k` is resolved by an explicit ladder that separates a bad
denominator from a bad numerator:

1. **Normal** — `value_(k-1) > 0` and `value_k - flow_k > 0`:
   `r_k = (value_k - flow_k) / value_(k-1) - 1`. This is the settled
   end-of-period flow convention.
2. **Flow exceeds the ending value** — `value_(k-1) > 0` and
   `value_k - flow_k <= 0`: the end-of-period flow assumption has broken down
   (a large purchase accompanied a loss, or the holdings went to nothing). The
   link instead uses the start-of-period form
   `r_k = value_k / (value_(k-1) + flow_k) - 1` when `value_(k-1) + flow_k > 0`.
   That form is always well defined here and yields a loss no worse than −100%.
   The range is marked **approximate**, because the result depended on when
   during the step the money actually moved — something the stored data does not
   record. It is never silently turned into "no change", and it never flips the
   sign of the chained product.
3. **No usable denominator either way** — `value_(k-1) <= 0` and
   `value_(k-1) + flow_k <= 0`: the link cannot be measured at all. It
   contributes factor `1`, the chain continues, and the range is marked
   **approximate** so the gap is visible rather than silently absorbed. This
   branch is defensive: the endpoint never emits a point whose portfolio value is
   zero (see the endpoint limitation below), so in practice it should not fire.

With a known-empty opening `value_0 = 0`, so the first link falls into branch 3
by construction and the first measured point reads `0.00%` — correct, because
money arriving is not performance. This specific case does **not** mark the range
approximate: there is nothing unmeasured about it.

### When the percentage is approximate

Chaining over only the measured points is not identical to chaining over every
stored point once cash flows intervene. With values/invested of
`(100,100) → (200,200) → (220,200)`, chaining all three gives 10%; dropping the
middle point gives 20%. So exactness has to be tested, not assumed.

- A link that skips one or more incomplete points is **exact** when
  `invested_base` is unchanged across every skipped point (no money moved inside
  the span); the two forms agree algebraically in that case.
- A link that skips an incomplete point across which `invested_base` changed is
  **approximate** — money moved in or out around a day the chart could not
  measure.
- Branches 2 and 3 of the link ladder also mark the range approximate.

When any link in the range is approximate, the whole range's percentage is
presented as approximate. The SEK line is unaffected and is never marked.

**Known residual, recorded deliberately:** a cash flow on a date that has *no*
stored observation at all — a trade on a day when none of the holdings had a
price, or during a stretch when the portfolio held nothing — is folded into the
next measured link and is **not** detected by this marker. Detecting it would
require either fetching the transaction ledger into the dashboard or the backend
project the user deferred (valuing holdings on dates the provider had no price).
Neither is in scope here, and nothing in this plan forecloses the backend fix.

### Endpoint limitation: dates with nothing to value are omitted

The value-history endpoint skips any date where no open position could be valued
— including dates when the portfolio held nothing at all. A full liquidation
therefore never appears as a zero point; those dates are simply absent, and no
amount of inspecting returned values can detect them. Two consequences the plan
handles explicitly:

- **The series can end before the period does.** If the last measured point is
  earlier than the range end (everything sold, or prices too stale to value
  anything), the chart says so: a note reading *"Chart ends <date> — no valued
  holdings after that date."* The final sale's realized gain is outside what this
  chart can show; the Gains page remains the surface for realized results.
- **A link can span a liquidated stretch.** Because `invested_base` nets the sale
  proceeds, such a link is close to correct rather than wild, but it carries the
  same no-valuation-boundary limitation as any other flow inside a span. It falls
  under the recorded residual above.

### Availability states (never a fabricated zero)

The gain view resolves to exactly one of these, each with its own message:

| State | Meaning | Message |
|---|---|---|
| `no_history` | No stored points in the range at all | *"No portfolio history in this interval"* (existing copy) |
| `all_incomplete` | Points exist in the range but every one is missing prices for at least one holding | *"Every day in this period is missing prices for at least one holding, so gain cannot be measured. Refreshing prices may fill this in."* |
| `unknown_opening` | The portfolio held something before the period but its value then is unknown | *"The portfolio's value at the start of this period is not known, so gain for this period cannot be measured. Choose a range that starts earlier."* |
| `missing_trade_fx` | `invested_base` is unavailable, so money put in is unknown | *"Gain cannot be shown: a trade in a foreign currency has no exchange rate for its trade date, so the money you put in is unknown from `<date>` onwards."* |
| available | Otherwise | The chart, plus any of: approximate marker, ends-early note, "unavailable after `<date>`" chip |

`missing_trade_fx` names the **first date in the whole stored history** at which
`invested_base` went null, not the first one inside the window — contamination
usually predates the selected range, and naming a window-local date would be
misleading. The window selector therefore carries that provenance out of the full
history rather than discarding it.

If `invested_base` becomes unavailable part-way through the range, the series
ends at the last available date and a warning chip states *"Gain unavailable
after `<date>` — a trade is missing its exchange rate."*

### Degenerate and edge states

| State | Behavior |
|---|---|
| **Today preset** (`start = end = today`) | The origin sits at yesterday with value 0 and the single measured point is today, so the gain chart draws a real two-point line from zero to the day's gain — not a flat zero and not a bare dot. |
| **Today, before the day's price refresh** (no measured point) | The explicit `no_history` empty state. A flat zero line is never shown in place of "no data". |
| **Today on the Value tab** | One point and no origin; the chart enables point markers when the drawn series has fewer than two points so the observation is visible rather than blank. |
| **Anchor missing, opening known empty** | Origin at zero; "All" looks exactly as it does today. |
| **Anchor missing, opening unknown** | `unknown_opening` — explicit unavailable state. |
| **Anchor exists but is incomplete** | Skipped; the anchor is the last *complete* observation before the start. |
| **Bad denominator / numerator in a link** | The ladder above; range marked approximate. |
| **Everything sold mid-range** | Series ends at the last valued date, with the ends-early note. |

### Axis and rendering rules

- **Value view**: axis autoscales to the value series only. The invested
  reference `LineSeries` gets an autoscale provider returning `null`, so it
  contributes nothing to the range, and is drawn normally when it lands inside
  the window.
- **Invested edge tag** fires on **disjoint ranges**, not point membership: take
  the min and max of the value series and of the reference series over the drawn
  window; show the tag only when `referenceMax < valueMin` (tag at the bottom
  edge) or `referenceMin > valueMax` (top edge). A reference line that crosses
  the visible band is partly on screen and gets no tag — the earlier
  "no reference point lies inside the value range" test would have wrongly
  announced a crossing line as off-screen and could not name a direction. The tag
  names the reference value at the latest drawn date in the window.
- **Gain view**: `BaselineSeries` with `baseValue: { type: "price", price: 0 }`,
  green above / red below, and an autoscale provider that *extends* the fitted
  range to include zero (`min(min, 0)`, `max(max, 0)`) rather than pinning the
  floor to it. When every value is exactly 0 the range falls back to ±1 so the
  baseline is still drawn.
- **Per-asset price chart**: zero floor removed; the series autoscale range is
  extended to include the cost-basis break-even value when one is present, so the
  break-even line participates in scaling even though `createPriceLine`
  primitives do not. This resolves the clipping limitation recorded in the
  2026-07-09 break-even decision.
- **Price-scale margins** become `{ top: 0.12, bottom: 0.12 }` on both charts, so
  a fitted axis does not draw the line onto the frame.
- **Percent price format** is its own format: a custom format whose formatter
  renders `v.toFixed(2)` followed by a percent sign, with `minMove: 0.01`.
  Reusing `compactPriceFormat` (`minMove: 1`) would put every gridline a whole
  percentage point apart and collapse a ±0.5% week to a single "0" tick,
  reproducing the exact unreadability this work removes. Two decimals match the
  2026-07-15 numeric-precision decision.

### The heading can never name a period it is not plotting

The gains query keeps previous data while a new range loads, and its key includes
the period, so immediately after a preset change the displayed response still
describes the *previous* selection. A heading driven by the selector would label
last month's numbers as "year to date". The response does not echo which preset
produced it, and deriving dates client-side is forbidden by the 2026-09-12 "One
Clock Owns Today" decision. So:

- The chart panel's range, plotted data, `visibleStart` and heading all derive
  from **one** source: the resolved `report_period` of the response currently
  being displayed.
- While that response is placeholder data from the previous selection, the
  heading **omits the period name** (reading "Portfolio gain (SEK)", today's
  wording) and an "Updating" chip appears. The previously plotted data stays on
  screen, consistent with the 2026-09-12 note that panels keep their numbers
  while refetching — it simply stops claiming a period.
- When the resolved period is **absent** — first load, or the gains query failed
  — the panel must not fall back to plotting unbounded lifetime history under a
  period-named heading. First load shows the existing skeleton; a failure shows
  an explicit state: *"Could not work out the selected period."* with a Retry
  button. This is a deliberate behavior change: today a gains failure still draws
  a lifetime chart. The Treemap view stays reachable in that state, as it is now.

## Copy

Always visible under the chart legend, `--text-muted`, body-sm — not a tooltip,
because the key distinction must be legible without hovering. No jargon: the
words "time-weighted", "money-weighted", "XIRR" and "chained return" never appear
in the UI.

**Gain view, SEK (always):**
> Gain earned since the start of the selected period. Money you added or took out
> is not counted as gain.

**Gain view, percent (always):**
> How your holdings performed over the selected period — money you added or took
> out does not move this line. The percentages in the portfolio summary and on
> the Gains page answer a different question: how your money performed.

**Dividend caveat — conditional, appended to either caption:**
> Dividends are not included here, so holdings that paid them read a little low.

Shown only when the user actually received dividend income in the selected
period. Detected frontend-only from data the dashboard already fetches: the gains
response's portfolio waterfall reports whether income is tracked at all and, when
it is, the period's income amount. The caveat appears when income is tracked and
the period's income amount is available and non-zero; with no dividends recorded
it never appears. No backend change is needed for this, and none is proposed.

**Approximation marker — conditional, percent mode only:**
- A compact warning chip beside the heading, in the same slot as the existing
  "N days had missing inputs" chip, reading `Approximate`.
- One sentence appended to the percent caption:
  > Part of this period could not be measured exactly: money moved in or out
  > around days that are missing prices, so this percentage is a close estimate.

It appears only while the condition of *When the percentage is approximate*
actually holds for the selected range, and never in SEK mode.

**Ends-early note — conditional:**
> Chart ends `<date>` — no valued holdings after that date.

**Legend labels:** SEK → `Gain this period (SEK)`; percent →
`Performance this period (%)`.

**Panel heading** (pure helper, unit-tested): `Portfolio value, <period> (SEK)` /
`Portfolio gain, <period> (SEK)` / `Portfolio gain, <period> (%)`, with
`<period>` = `today` · `last 7 days` · `last 12 months` · `year to date` ·
`all time` · the resolved `start to end` dates for a custom range (falling back to
`custom range` when either resolved date is missing). The plain-language period
names are confirmed rather than the selector's short codes. When the resolved
period is stale or absent, the period clause is dropped entirely (above).

**Unit toggle:** a `segmented-control` fieldset with legend "Gain unit" and
buttons labelled `SEK` and `%` (the percent button carries
`aria-label="Percent"`). Rendered only while the Gain view is selected.

### Defaults

The rebased gain semantics, the opening origin, the unpinned axis, the baseline
rendering, the incomplete-day exclusion and the invested edge tag are all
unconditional — the new code paths run by default, with no flag. The SEK/percent
choice is a **view preference** in the sense of the 2026-07-10 decision (one
click, no side effects), not a config flag hiding new code: both states are
covered by automated tests and by the required human pass. It defaults to `sek`
because that continues the view the user already has.

## Architecture notes

- **Split the gain chart into its own component** rather than parameterising
  `TimeSeriesChart` by series kind. Adding a Baseline series and a percent price
  format is a contract change, not a prop; splitting keeps the asset chart's
  contract (markers, cost-basis price line, tooltip) untouched. The new component
  is `PortfolioGainChart.tsx` — named for the behavior, not for a plan phase.
- Shared chart plumbing moves into two pure, unit-testable modules:
  - `chartTimeAxis.ts` — `chartDate`, `isoFromTime`, `parseIsoDate`,
    `formatIsoDate`, `calendarSpineData`, `tickMarkFormatter` (everything that
    implements the calendar-linear X axis committed in 2026-06-22).
  - `chartTheme.ts` — shared chart colors, base `createChart` options, the
    compact and percent price formats, and the autoscale providers
    (`fitToDataAutoscale` with optional extra anchor values, `zeroInViewAutoscale`,
    `noAutoscale`).
- All view derivation stays in pure selectors in `portfolioValueViewModel.ts`;
  components consume results and render. Input → action → reducer → state →
  render is preserved (the unit toggle is persisted view state through the
  existing `usePersistentSetting`).
- The superseded `filterValueHistoryPoints` and `portfolioValueSeries` are
  **kept until their caller migrates**, so no phase leaves the build broken; they
  are deleted in the same phase that migrates the dashboard.

## Phases

Plans are ephemeral: durable docs and code name the behavior, never these phase
numbers. `npm run check` already runs Vitest alongside TypeScript and lint, so
the per-phase "targeted test run" notes are for fast feedback, not extra gates.

### Phase 0 — Check what the real ledger actually produces

This is a read-only inspection of a **running** instance. Prefer the instance the
user already has open: `GET http://127.0.0.1:8480/api/portfolio/value-history`
and `GET /api/transactions`. If nothing is running, note that
`scripts/start.ps1` is not a read-only operation — it builds by default, and
application startup performs a launch-time ledger backup and a market-data
refresh. Those are normal operations, but they should be a deliberate choice
rather than a side effect of an investigation.

Record, sanitized per the private-export rule (counts and dates only, never
holdings or amounts):

1. Total point count, first and last date.
2. Count of points with `invested_base === null`, and the first such date.
3. Count of `incomplete` points and their dates.
4. **Count of incomplete points across which `invested_base` changes** (its value
   differs from the previous or the next point's). This is the population that
   makes a percentage approximate, and it decides whether the approximation
   marker is a rare footnote or a routine fixture of the UI.
5. Any point with `value_base <= 0` (expected: none).
6. Calendar gaps longer than four days that are not explained by a weekend or an
   obvious holiday — a hint that the portfolio held nothing for a stretch, since
   such dates are omitted by the endpoint entirely and cannot be detected from
   the returned values.
7. Count of Dividend transactions in the ledger — this decides whether the
   conditional dividend caveat ever appears in practice.

Outcome shapes priority, not scope. If `invested_base` is never null, the
missing-FX state is defensive and gets only unit coverage; if it fires, it is a
first-class state that must be checked by hand in Phase 5. If incomplete days
turn out to be rare, note it — the rejected "visible break at incomplete days"
option was rejected because their frequency was unknown, and this measurement is
what would re-open it as easy follow-up work.

Verify: read-only inspection; no code change, no build.

### Phase 1 — Unpin the axis (smallest end-to-end slice)

This phase alone fixes the original complaint on the Value view and the
per-asset chart. It touches no view-model contract, so the build stays green.

- Extract `chartTimeAxis.ts` and `chartTheme.ts` from `TimeSeriesChart.tsx` as a
  behavior-preserving move, plus the new autoscale providers and percent format
  (no caller uses them yet).
- In `TimeSeriesChart.tsx`:
  - Delete `zeroBaselineAutoscale`; the area series uses `fitToDataAutoscale`.
  - Extend the area series' autoscale range to include the `costBasisLine` value
    when one is set. Hold the value in a ref read by the provider closure, and
    re-apply the series options from the existing `costBasisLine` effect so a
    changed value triggers a rescale. The price-line lifecycle (dedicated effect,
    stale-handle guard) is unchanged.
  - Give the invested/reference `LineSeries` `noAutoscale` (provider returns
    `null`) so it never drags the axis.
  - Price-scale margins → `{ top: 0.12, bottom: 0.12 }`.
  - Enable point markers when the drawn series has fewer than two data points, so
    a single-observation range is visible rather than blank.
  - Add an optional `edgeTag?: { side: "above" | "below"; label: string;
    description: string }` prop, rendered inside `.time-series-chart-wrap` as an
    absolutely positioned gold tag at the named edge with `aria-label` =
    `description`.
- Add `referenceEdgeTag(value, reference)` to `portfolioValueViewModel.ts`
  implementing the **disjoint-range** rule above, returning `{ side, value }` or
  `null`.
- `Dashboard.tsx` passes the edge tag on the Value view. Nothing else in the
  dashboard changes yet.
- Styles: `.chart-edge-tag` (`--radius-pill` chip, gold `#e0b15e` text on
  `--surface-2` + `--hairline`, 11px/600) in `frontend/src/styles.css`.

Tests:
- `chartTimeAxis.test.ts` — the calendar-spine cases moved from
  `TimeSeriesChart.test.tsx`, still asserting whitespace fill and boundaries.
- `chartTheme.test.ts` — the autoscale providers as pure functions:
  `fitToDataAutoscale` preserves the base range and does not force a zero floor;
  it widens to include an extra anchor value above or below; `noAutoscale`
  returns `null`; `zeroInViewAutoscale` widens to include zero both ways and
  falls back to ±1 for an all-zero series.
- `TimeSeriesChart.test.tsx` — the reference series is created with a provider
  returning `null`; the area series' provider widens for `costBasisLine`; the
  edge tag renders by role/text with its accessible description. Existing
  price-line lifecycle and marker tests keep passing.
- `portfolioValueViewModel.test.ts` — `referenceEdgeTag`: entirely below, entirely
  above, **crossing the visible band (no tag)**, overlapping, empty reference.

Verify: from `frontend/` — `npm run check`, then `npm run fmt`.

Human testing (recommended — this is where scaling visibly changes):
- Dashboard Value view on 7D and YTD: the line fills the band vertically; the
  invested line is off-screen and the edge tag names its level and direction.
- Dashboard Value view on 12M and All: essentially unchanged.
- Asset price chart: axis fits the price window; the break-even line stays on
  screen instead of clipping; Buy/Sell/Split markers still sit on their
  transaction prices with a working crosshair tooltip. Check a split instrument
  and a long-held instrument specifically, and report any marker that now clips
  at the bottom edge.

### Phase 2 — Pure period-gain view-model (additive only)

No UI change and no removals, so the phase builds and checks on its own; the old
`filterValueHistoryPoints` and `portfolioValueSeries` stay in place for their
current caller until Phase 4 migrates it.

New exports in `portfolioValueViewModel.ts`:

- `valueHistoryWindow(points, range, historyStartDate)` → carries everything the
  downstream selectors and the UI states need, so provenance is not lost:
  `anchor`, `openingKnownEmpty`, `measured` (in-range observations), `rangePoints`
  (in-range including incomplete), `incompleteCount`,
  `investedUnavailableFrom` (first null date in the **full** history),
  `lastMeasuredDate` (last observation in the full history), and the range itself.
- `periodValueSeries(window)` → `value` and `invested` series built from
  **measured points only** (the Value tab now skips incomplete days too, so the
  two tabs never disagree about whether a day had a plunge). Invested points are
  still dropped where `invested_base` is null.
- `periodGainSeries(window)` → `{ status, sek, percent, approximate,
  endsEarlyAt, investedUnavailableAfter }`, implementing the whole **Semantics
  specification**: opening classification, origin point, rebased SEK, the link
  ladder, the approximation rule, and the availability states.
- `chartPanelHeading({ view, unit, period })` → the copy table above, with the
  period clause omitted when the resolved period is stale or absent.
- `dividendCaveatApplies(portfolioWaterfall)` → the conditional-caveat predicate.

Tests in `portfolioValueViewModel.test.ts`:
- Anchor selection: last observation strictly before the start; an incomplete
  candidate is skipped; none exists.
- Opening classification: known-empty when the range starts at or before the
  portfolio's first trade date; **unknown when pre-range holdings exist but no
  complete anchor does** (both the all-incomplete-before-start case and the
  history-starts-late case); unknown opening yields the `unknown_opening` state
  and no series.
- Origin point: present at the day before the range start with value 0; sits on
  the anchor's own date when the anchor is that day; never collides with a
  measured point; percent origin is `0.00%`.
- Rebased SEK: the first measured point carries the move **from the anchor**, not
  zero; a mid-period deposit does not appear as gain; "All" reproduces the plain
  cumulative `value − invested` series; the SEK line is unchanged by a skipped
  incomplete point (exactness of endpoint arithmetic).
- Percent chaining: three observations chain to the product of the links; a
  deposit between two observations leaves the return unchanged; the known-empty
  opening makes the first measured point `0.00%` **without** marking the range
  approximate.
- Approximation: a skipped incomplete point with **no** invested change keeps the
  range exact; a skipped incomplete point with a **purchase** across it marks it
  approximate; the same with a **sale**; Codex's worked example
  `(100,100) → (200,200) → (220,200)` with the middle point incomplete is marked
  approximate and still produces a continuous series.
- Link ladder: flow exceeding the ending value uses the start-of-period form,
  yields a loss no worse than −100%, does not flip the chained sign, and marks
  the range approximate; a zero-denominator link contributes factor 1, continues
  the chain, and marks the range approximate; neither case silently preserves the
  previous percentage.
- Incomplete days: excluded from both tabs' series and from the chain links, with
  the surrounding pair still linked.
- Weekend/holiday absence: consecutive stored dates three days apart chain
  normally and are never marked approximate.
- Invested null: across the whole range → `missing_trade_fx` naming the **full
  history's** first null date even when it predates the window; part-way →
  series truncated with `investedUnavailableAfter`.
- All in-range points incomplete → `all_incomplete`, distinct from
  `missing_trade_fx`; no points at all → `no_history`.
- Ends-early: last measured point before the range end sets `endsEarlyAt`.
- Single measured point (Today) → origin plus one point, both distinct dates.
- `chartPanelHeading` for each preset and unit, including custom with and without
  resolved dates, and the stale/absent period case.
- `dividendCaveatApplies`: income not tracked → false; tracked and zero → false;
  tracked, available and non-zero → true; unavailable income → false.

Verify: from `frontend/` — `npm run check`, then `npm run fmt`; targeted `vitest`
run of `portfolioValueViewModel.test.ts`.

### Phase 3 — `PortfolioGainChart` component

- New `frontend/src/components/PortfolioGainChart.tsx`: a presentational wrapper
  around a lightweight-charts `BaselineSeries`, built on `chartTimeAxis` and
  `chartTheme`. Props: `data`, `ariaLabel`, `visibleStart`, `unit`
  (`"sek" | "percent"`), `height`.
  - `baseValue: { type: "price", price: 0 }`; top line `#16c784` with fills
    `rgba(22,199,132,0.30)` → `rgba(22,199,132,0.02)`; bottom line `#ff4d4f`
    with fills `rgba(255,77,79,0.02)` → `rgba(255,77,79,0.30)`.
  - `autoscaleInfoProvider: zeroInViewAutoscale`.
  - `priceFormat`: compact SEK format for `unit === "sek"`, the percent format
    for `unit === "percent"`.
  - Same calendar spine and `setVisibleRange` behavior as `TimeSeriesChart`; the
    incoming series already carries its origin point, and `visibleStart` is that
    origin's date, so the left edge is the zero origin.
  - The last-value axis tag stays visible and names the period gain.
- No caller yet, so runtime behavior is unchanged.
- `PortfolioGainChart.test.tsx`, using the same lightweight-charts mock approach
  as `TimeSeriesChart.test.tsx`: a Baseline series is created with base price 0;
  the percent unit attaches the percent format and the SEK unit the compact one
  (assert formatter output, e.g. `0.5 → "0.50%"` vs `"0.5"`); the calendar spine
  and visible range match `TimeSeriesChart`'s behavior; a leading origin point at
  the visible start is preserved; negative values pass through unclamped.

Verify: from `frontend/` — `npm run check`, then `npm run fmt`; targeted `vitest`
run of the new test file.

### Phase 4 — Wire the dashboard panel

In `Dashboard.tsx` (`DashboardChartPanel`):

- Migrate to `valueHistoryWindow` / `periodValueSeries` / `periodGainSeries`, then
  **delete** `filterValueHistoryPoints` and `portfolioValueSeries` and their
  now-superseded tests. This is the phase where the old contracts go, so the
  build is never broken in between.
- Range, plotted data, `visibleStart` and heading all derive from the displayed
  response's resolved `report_period`; implement the stale-period ("Updating"
  chip, no period name) and absent-period (skeleton on first load, explicit
  *"Could not work out the selected period."* with Retry on failure) rules.
  Keep the Treemap reachable in the failure state.
- Add the persisted unit setting: key `dashboard.gainUnit`, values
  `["sek", "percent"]`, default `"sek"`, via `usePersistentSetting` + `isOneOf`.
  Render the toggle in `chart-controls` only while the Gain view is selected.
- Gain view renders `PortfolioGainChart` with the SEK or percent series and the
  gain chart's own `visibleStart` (the origin day); Value view keeps
  `TimeSeriesChart` with the invested reference line, the edge tag, and its own
  `visibleStart` (the range start).
- Render the availability states, the approximate chip and sentence (percent
  only, only when firing), the ends-early note, the "unavailable after `<date>`"
  chip, and the conditional dividend caveat. The existing "N days had missing
  inputs" chip is unchanged and keeps counting incomplete days in the window.
- Caption and legend per **Copy**; caption style `.chart-caption` added to
  `styles.css`.
- Version: `frontend/package.json` `0.25.0 → 0.26.0`. The backend is untouched,
  so `Cargo.toml` is not bumped and no cargo command is required.

Tests in `Dashboard.test.tsx` (mock `PortfolioGainChart` alongside
`TimeSeriesChart`; assert by role/text and by the props handed to the mocked
charts, never on internal state or DOM structure):
- The existing report-period test is kept and extended: the Value view receives
  only in-range measured points, and an incomplete day in range is not plotted.
- Switching to Gain hands the gain chart a series whose **first point is the zero
  origin, dated the day before the range start**, followed by measured points
  carrying the move from the pre-range anchor, with `visibleStart` at the origin.
- The SEK/% toggle appears only on the Gain view, switches the series and the
  heading suffix, and persists across a remount.
- **Interaction regression for the heading**: with the gains query serving
  placeholder data from a previous selection, the heading carries no period name
  and the Updating chip is present; once the matching response arrives the
  heading names the new period. With the gains query failed, the panel shows the
  explicit could-not-resolve state and does **not** plot lifetime history under a
  period-named heading, while Treemap stays reachable.
- The `unknown_opening`, `all_incomplete` and `missing_trade_fx` states each
  render their own message and no chart; `missing_trade_fx` names the historical
  first-null date.
- The approximate chip and sentence appear for a range whose data triggers them
  and are absent otherwise, and never appear in SEK mode.
- The dividend caveat appears only when the period has tracked non-zero income.
- The ends-early note appears when the last measured point precedes the range end.
- The caption distinguishing the two measures is present in percent mode without
  any hover interaction.

Verify: from `frontend/` — `npm run check`, then `npm run fmt`.

### Phase 5 — Human verification pass (required)

Mocked chart tests cannot establish visual continuity, clipping, or single-point
visibility, so this pass is not optional. On the real ledger, then in demo mode:

- **Range responsiveness**: Today / 7D / 12M / YTD / All / Custom on both Value
  and Gain; each range changes both the window and the vertical scale, and short
  ranges are readable.
- **Zero origin**: the gain line visibly starts at zero on the left edge in every
  range, one calendar day before the period, and YTD's first move reflects the
  turn of the year rather than starting from it.
- **Gain rebasing**: 7D's first measured point includes the first day's own move;
  "All" looks the same as before this change.
- **Baseline rendering**: pick a losing range and confirm gain draws red below the
  zero line without clipping, and that the zero line stays in view.
- **Percent**: the axis shows fractional percent ticks (not whole-point "0"
  gridlines); a range containing a known deposit does not show the deposit as
  performance.
- **Labelling**: the caption is readable without hovering and the two measures are
  distinguishable; the approximate chip and sentence appear only on ranges that
  warrant them; the dividend caveat appears only if dividends were received in
  the period (per Phase 0, it may never appear — confirm it does not).
- **Heading honesty**: switch presets rapidly and confirm the heading never names
  a period that does not match the plotted data.
- **Today**: with a refreshed price, the gain chart shows a two-point line from
  zero to the day's number; before the day's refresh, the explicit empty state
  appears — not a flat zero line. The Value tab's single point is visible.
- **Invested edge tag**: on a short range the tag names the invested level and
  direction; on 12M/All the invested line is drawn normally with no tag; find a
  range where the invested line crosses the band and confirm no tag appears.
- **Incomplete days**: if Phase 0 found any, open a range containing one and
  confirm both tabs draw continuously with no plunge-and-recover, and that the
  "N days had missing inputs" chip still reports it.
- **Asset page regression**: markers, crosshair tooltip, break-even line and the
  native-currency axis all still behave; the break-even line no longer clips.
- **Persistence**: the SEK/% choice survives reload and navigation, and a
  hand-corrupted `dashboard.gainUnit` value falls back to SEK.

### Phase 6 — Documentation and decision log

- `docs/DecisionLog.md`, appended (never rewriting committed entries):
  1. **Reversal** of the zero-baseline half of 2026-06-22 "Chart Axis And
     Price-Series Semantics": charts fit their visible window instead of pinning
     the Y axis to zero, on the dashboard and the per-asset price chart. That
     entry's calendar-linear X-axis and whitespace-spacing commitments and its
     native-currency per-asset rule remain intact and are restated as unchanged.
  2. **Period-rebased gain semantics**: gain is measured over the selected range;
     the calculation anchor is the last complete observation strictly before the
     start, per the 2026-06-19 boundary convention; the drawn line carries an
     explicit zero opening-origin point at the period boundary so it starts at
     zero while the measured content still starts at the range start; a missing
     anchor means an empty opening only when the range reaches back to the
     portfolio's first transaction, and otherwise means the opening valuation is
     unknown and gain is unavailable; the zero baseline is always in view;
     not-measured days are excluded from **both** the gain and the value series
     and the lines stay continuous; calendar-day absence is never missing data.
  3. **Chart percent measure and its honesty rules**: the Gain chart's percent
     line is a chained daily time-weighted return, deliberately different from
     the user-selectable money-weighted/simple/Modified Dietz totals of
     2026-06-21, coexisting on the condition that the UI names in plain language
     what each percentage measures. Where stored observations cannot support
     exact daily chaining the percentage is computed and **labelled approximate**
     rather than blanked; the undetectable residual (a cash flow on a date with no
     stored observation) is recorded as a known limitation whose proper fix is a
     backend valuation project, deliberately not started here. The chart series is
     ex-dividend while the Gains totals include income since 2026-06-23, and the
     caveat is shown only when dividend income exists in the period.
  4. **Value-axis scope**: the dashboard value axis follows the portfolio value
     line only; the invested reference line never scales it and announces itself
     with an edge tag when the two ranges are disjoint. Refines the 2026-06-23
     "Net-Invested-Capital Reference Line On Dashboard" entry.
  5. **Break-even line participates in scaling**: refines the clipping limitation
     recorded in 2026-07-09 "Break-Even Cost Line On The Asset Price Chart".
     Trade markers are deliberately **not** added as autoscale anchors, because a
     split instrument's raw pre-split marker prices would squash the price series
     (the raw-price / split-adjusted mismatch that entry already documents);
     revisit if the human rendering pass shows markers clipping in practice.
  6. **The chart panel never names a period it is not plotting**: range, data and
     heading come from one resolved period; a stale period drops the period name,
     and an unresolved period is an explicit state rather than a silent fallback
     to lifetime history. Follows from the 2026-09-12 "One Clock Owns Today" and
     "Data Is Served And Requested By Revision" entries.
  7. **Lifetime total gain has no dedicated surface**: it stays reachable by
     selecting the "All" range. A lifetime tile beside the summary band's
     "Unrealized change" tile — which is narrower, covering current holdings only
     and excluding realized gain — would never agree with it, reproducing the
     two-disagreeing-figures problem this work's labelling exists to avoid.
     Revisit if the user finds themselves switching to "All" routinely just to
     read the number.
- `docs/VisualDesign.DarkTheme.md`:
  - Add the gain baseline area as a **third reserved exception** to the
    "semantics as text color, not fills" rule, alongside the soft-tinted chips and
    the treemap — a gain chart's whole point is above/below zero.
  - Extend the Charts (Lightweight Charts) token mapping with the baseline series
    colors, the percent axis format, and the edge-tag chip.
- `docs/Design.HighLevel.md` needs no change (its chart entries are a phase
  history, not a behavior contract) — confirm during the pass.

Verify: documentation review only; no build impact.

## Documents to update

- `docs/DecisionLog.md` — seven appended entries (Phase 6). This work **does**
  establish durable commitments: it reverses a committed rule, introduces new
  measurement semantics, and closes a question about where a figure lives.
- `docs/VisualDesign.DarkTheme.md` — semantic-fill exception, chart token mapping,
  edge-tag chip (Phase 6).
- `frontend/package.json` — version `0.25.0 → 0.26.0` (Phase 4), surfaced in the
  UI footer alongside the backend version.
- No backend, API, schema, migration or `docs/CurrencyAndFxRules.md` change.

## Closed questions (recorded, not carried forward)

- **Lifetime gain's home** — closed: no new surface; "All" is the route to it.
  Rationale and revisit trigger are in the decision-log entry above.
- **Should the Value line also skip incomplete days?** — closed: yes. Both tabs
  treat an incomplete day as not-measured, so they can never disagree about
  whether a day had a plunge, and the existing chip stays the signal.
- **Trade markers as autoscale anchors** — closed: the break-even line
  participates in scaling, markers do not, because a split instrument's raw
  pre-split marker prices would squash the price series. Phase 1's human
  verification is the trigger for revisiting.
- **Heading period names** — closed: plain-language names ("year to date",
  "all time", "last 7 days") confirmed.
- **Approximate percentages** — closed: computed and labelled, never blanked;
  line stays continuous; scope stays frontend-only.
- **Dividend caveat** — closed: conditional on the period actually containing
  dividend income, detected from data the dashboard already fetches; no backend
  change needed.

## Open Questions

None outstanding. Two items are deliberately recorded as accepted limitations
rather than questions, and would only reopen on evidence from Phase 0 or Phase 5:

1. A cash flow on a date with no stored observation is folded into the next
   measured link and is not caught by the approximation marker. The proper fix is
   the deferred backend valuation project.
2. A fully liquidated stretch is invisible to this chart, which ends at the last
   date with a valued open position and says so. Realized results stay on the
   Gains page.
