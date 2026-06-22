# Spec — Phase 4: Charts & Dashboard

**Status:** Draft for planning · **Owner:** Lars · **Date:** 2026-06-22

## 1. Summary

Completes Phase 4 of `docs/Design.HighLevel.md`. The per-instrument price-history
API (`GET /api/instruments/{id}/prices`) already shipped. This spec covers the four
remaining deliverables:

1. Per-instrument price chart (asset detail page).
2. Portfolio value-over-time chart (new backend series + chart).
3. Dashboard landing page (total value, day/total change, top movers).
4. Allocation breakdown (by instrument, currency, type).

The work is one combined spec implemented as four independently testable phases.

## 2. Goals & non-goals

### Goals
- Show a per-instrument price chart on the asset page using the existing endpoint.
- Add a portfolio value-over-time series, derived on the fly from the ledger, and
  chart it.
- Make the landing page a dashboard: summary, value chart, top movers, allocation.
- Keep all money/value derivation on the existing exact-decimal valuation path;
  only presentational weights use frontend float math.

### Non-goals (this spec)
- No precomputed/materialized value snapshots (no new stored "derived truth").
- No second charting library; allocation uses CSS, not a donut/pie lib.
- No dividend/income series (income is still untracked).
- No period/benchmark performance charting beyond the existing Gains date presets.
- No realtime/intraday data (EOD only, unchanged).

## 3. Architecture decisions adopted

- **Value series is derived, not stored** (Approach A). A new endpoint reconstructs
  portfolio value per date from the ledger + cached prices + cached FX at request
  time. This honors the ledger-as-source-of-truth principle and reuses the FX
  carry-forward conventions from the 2026-06-21 per-instrument price-history
  decision. No schema change.
- **Dashboard becomes the landing page.** `/` is the dashboard; the existing board
  moves to `/board`. This matches the conventional portfolio-tracker model
  (overview first, detailed tables one level in).
- **Allocation reuses display-only float math.** Allocation weights are computed in
  the frontend from `market_value_base`, under the 2026-06-18 "Display-Only Weights
  May Use Frontend Float Math" decision. They are never persisted or sent back as
  authoritative.
- **No new charting library beyond Lightweight Charts.** Lightweight Charts (already
  the chosen stack) covers the two time-series charts. Allocation is rendered as a
  CSS segmented bar + legend + table.

## 4. Phasing

Each phase ends buildable, lint-clean, and human-verifiable.

### Phase A — Charting foundation + per-instrument price chart
- Add the `lightweight-charts` dependency to `frontend/`.
- Build one reusable `<TimeSeriesChart>` wrapper that owns chart lifecycle, resize
  handling, and the dark theme from `docs/VisualDesign.DarkTheme.md`.
- Add a `useInstrumentPrices(id)` query against `GET /api/instruments/{id}/prices`.
- Replace `ReservedChartBand` in `AssetView` with an area/line chart of `close_base`
  (SEK). Pending/error/empty states reuse the `board-state` patterns; empty reads
  "No price history yet — refresh prices".
- **Unavailable `close_base`:** the endpoint can return native close points whose
  `close_base` is unavailable (FX missing on/before that date). The chart filters to
  available `close_base` points only and never passes an unavailable value into
  Lightweight Charts as a number. When some points are dropped for missing FX, surface
  a subtle missing-FX/incomplete chip; if *all* points are unavailable, show the empty
  state rather than a blank or `NaN` chart. A selector/guard test asserts unavailable
  `close_base` points are excluded from the plotted series.

**Verify:** open an asset with stored prices → chart renders; an unmapped/empty
asset → empty state, no crash; an asset with prices but missing FX → available points
chart with a missing-FX chip, no `NaN`.

### Phase B — Portfolio value-history endpoint
- New route `GET /api/portfolio/value-history` with optional `from`/`to` query
  params. Default (no params) returns the full stored history. `from > to` returns
  `400 invalid_date_range` (same code as the per-instrument endpoint).
- **Spine:** the sorted union of stored **price and FX** dates, starting at the
  earliest date on or after the first BUY trade date. FX dates are included because
  the chart is in SEK and base-currency value can move on FX-only dates even when
  carried-forward equity closes do not — restricting the spine to price dates would
  hide those moves across holidays that differ between equities and FX. (Transaction
  dates are *not* added to the spine; a buy/sell with no price on its date is picked
  up at the next spine date via carry-forward.)
- **Per date:** reconstruct each instrument's split-adjusted position from the
  ledger (`trade_date ≤ date`), value it at the carry-forward close × carry-forward
  FX on or before that date, and sum to SEK.
- **Split adjustment:** cached closes are Yahoo split-adjusted closes. A value point
  *before* a later split must normalize the held share quantity to the adjusted-price
  convention, applying the split factors of all splits with `effective_date > date`,
  matching the period-valuation convention recorded in `docs/DecisionLog.md` on
  2026-06-19. Without this, pre-split points are over/understated. A regression test
  covers a buy, a later 2:1 split, and split-adjusted historical prices.
- **Provider-symbol gating:** value history uses cached prices **only** behind an
  enabled Yahoo mapping, identical to `load_valuation_inputs(...)` and
  `GET /api/instruments/{id}/prices`. Cached prices for a disabled/removed mapping are
  not used (the instrument is treated as having no price → excluded for that date). A
  test covers cached prices present but mapping disabled.
- **Missing inputs:** an instrument with no price or FX on/before a date is excluded
  from that point; the point reports `incomplete: true` and an `excluded_count`,
  mirroring the Gains `excluded_rows` pattern. Values are never zero-filled.
- **All-excluded dates:** a `value_base` of `"0.00"` must never be emitted as a real
  zero from a date where every active position was excluded — that would render a
  false chart drop. Each point therefore carries an `included_count`; a point with
  `included_count == 0` is **omitted from `points`** entirely (it is not a real
  observation), so the series never contains a spurious zero.
- **Response shape:**
  ```json
  {
    "base_currency": "SEK",
    "points": [
      { "date": "2026-01-02", "value_base": "12345.67", "incomplete": false, "included_count": 7, "excluded_count": 0 }
    ]
  }
  ```
  `value_base` uses the 2-decimal money format.
- **Structure:** a pure `build_value_history(...)` in the valuation domain (sibling
  of `build_price_history`); the HTTP handler and repository stay thin wrappers.

**Verify:** curl the endpoint on the real DB → monotonic dates, plausible values;
unit tests below pass.

### Phase C — Dashboard landing page
- Routing: `/` → new `Dashboard`, `/board` → existing `BoardView` (tabs unchanged),
  `/import` and `/asset/:id` unchanged. Add a "Dashboard" nav item. Update the asset
  page "← Back" target and the board-tab `location.state` handoff to `/board`.
- **Summary tiles:** total value, day change, and **current unrealized change** —
  all from the existing Gains `summary` (`summary.unrealized_gain_*`). The tile is
  labelled "unrealized change", *not* "total change": method-dependent portfolio total
  return lives under Gains `totals.total_return_*` and depends on the return-method
  decision from 2026-06-21. If a method-aware total-return tile is wanted later it must
  query `totals` with an explicit date range and return method; that is out of scope
  here and the summary tiles stay current-exposure oriented.
- **Value-over-time chart:** area chart via `usePortfolioValueHistory()` consuming
  Phase B. Incomplete points surfaced subtly (e.g. a chip noting N days had missing
  inputs).
- **Top movers:** top 3 gainers + top 3 losers by `day_change_percent`, from a pure,
  unit-tested selector over Gains rows. Selector rules: **open rows only** (closed
  positions excluded); rows whose `day_change_percent` is unavailable (missing previous
  price/FX or zero previous market value) are excluded; gainers ranked by positive
  percent descending, losers by negative percent ascending; ties broken deterministically
  by symbol then instrument name. Each row links to its asset page. Selector tests cover
  unavailable rows, closed rows, ties, and fewer-than-3 movers.

**Verify:** `/` shows summary + chart + movers; `/board` still works; deep links and
Back/Forward behave.

### Phase D — Allocation breakdown
- On the dashboard, an allocation panel with an instrument / currency / type toggle.
- Rendered as a CSS 100% segmented bar + legend + small table, computed by a pure,
  unit-tested aggregation over Gains rows' `market_value_base` (frontend float
  display math, per the 2026-06-18 decision).

**Verify:** toggling dimension repartitions; segments sum to 100%; an unavailable
market value is excluded and noted, never shown as zero.

## 5. Data flow

- Per-instrument chart: `GET /api/instruments/{id}/prices` → `useInstrumentPrices`
  → `<TimeSeriesChart>`.
- Value chart: ledger + `prices` + `fx_rates` → `build_value_history` →
  `GET /api/portfolio/value-history` → `usePortfolioValueHistory` →
  `<TimeSeriesChart>`.
- Top movers + allocation: existing `GET /api/gains` rows → pure frontend selectors
  → dashboard panels. No new endpoints.

## 6. Error handling & missing-data rules

- Missing price/FX never becomes zero. Per-instrument points are simply absent for
  dates with no close (the endpoint already does this). Value-history points stay
  present but mark `incomplete` and exclude the offending instrument's contribution.
- Charts render pending (skeleton), error (retry), and empty states using the
  existing `board-state` styles.
- `from > to` on the value-history endpoint → `400 invalid_date_range`.

## 7. Testing

- **Backend (`build_value_history` unit tests):** split-adjusted position at date
  including a buy + later 2:1 split + split-adjusted historical prices (pre-split point
  normalized correctly), price and FX carry-forward, FX-only spine date moves base value
  with closes unchanged, cached prices present but mapping disabled → instrument excluded,
  missing-input → `incomplete` point with correct `included_count`/`excluded_count`, a
  spine date where every active position lacks price or FX → point omitted (no spurious
  `"0.00"`), empty portfolio (no BUY yet) → empty series, date windowing via `from`/`to`,
  and `from > to` → 400 at the handler.
- **Frontend (pure viewmodel/selector tests):** asset-chart guard excludes unavailable
  `close_base` points from the plotted series; allocation aggregation across
  instrument/currency/type (including an unavailable market value excluded); and
  top-movers selection/ordering (open-only, unavailable rows excluded, closed rows
  excluded, ties, fewer than 3 movers, all-flat). The `<TimeSeriesChart>` wrapper stays
  thin and is not deeply unit-tested.

## 8. Documentation & versioning

- New `docs/DecisionLog.md` entries: (a) portfolio value-history endpoint
  conventions (derived-on-the-fly, spine, carry-forward, incomplete points), and
  (b) dashboard-as-landing navigation change.
- Bump `frontend/package.json` and `backend/Cargo.toml` versions.
- Mark the Phase 4 items in `docs/Design.HighLevel.md` done as they land (no
  reference to this spec from durable docs).

## 9. Open questions

- None blocking. Top-movers ranking is by `day_change_percent` (open rows,
  available values only); revisit if absolute SEK movement proves more useful in
  practice.

_Resolved during review (2026-06-22, `docs/reviews/Review.phase-4-charts-dashboard.md`):_
- _Pre-split value points are split-factor normalized to the adjusted-price convention (Phase B)._
- _All-excluded dates are omitted with an `included_count`, so no spurious `"0.00"` value point (Phase B)._
- _Asset chart filters to available `close_base` only (Phase A)._
- _Value-history spine is the union of price **and** FX dates (Phase B)._
- _Value history uses the same enabled-mapping gating as valuation (Phase B)._
- _Dashboard tile is "current unrealized change" from `summary`, not method-dependent total return (Phase C)._
- _Top-movers selector excludes closed/unavailable rows with a deterministic tie-breaker (Phase C)._
