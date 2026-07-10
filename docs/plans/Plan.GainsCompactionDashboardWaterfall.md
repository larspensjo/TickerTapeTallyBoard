# Plan: Gains Page Compaction + Dashboard Portfolio Waterfall

Two user-visible changes delivered as independent, incrementally testable slices:

1. **Gains page compaction** — remove the Gains-page-only chrome so the instrument
   table moves up substantially. Shared chrome (PortfolioLayout action row,
   PortfolioSummary metric tiles) is untouched.
2. **Dashboard portfolio waterfall replaces Allocation** — remove the Allocation
   panel and add a portfolio-level gains waterfall backed by a new server-side
   aggregate block on `GET /api/gains`.

The two parts are decoupled: Part 1 is frontend-only and ships on its own; Part 2
adds a backend aggregate, a shared waterfall renderer refactor, a pure view-model,
and the Dashboard swap.

---

## Background the phases rely on

- The per-asset "Gains breakdown" waterfall (`waterfallViewModel.ts` →
  `openWaterfall` with the `hasBrokerageBreakdown` branch, rendered by
  `GainsWaterfall.tsx`) is the shape to mirror: Cost basis (held) → Price effect →
  FX effect → Market value (subtotal) → Realized gain → Brokerage costs →
  Dividend income → Total return, with the total-return bar floating as a delta
  from the cost-basis baseline.
- Its coherence comes from **one `GainsRow`**: within a single row
  `market_value = cost_basis(held) + unrealized_price_effect + unrealized_fx_effect`
  and `total_return = unrealized_gain + realized_gain + income`, so the ladder
  reconciles by construction.
- The Gains handler (`backend/src/api/gains.rs`) already loops every instrument
  with period exposure, producing per-instrument domain values (`valued_holding`
  with held cost basis / held price & FX effects / market value / unrealized gain /
  fee component, plus `performance.realized` with realized gain / realized cost
  basis / fees, plus `brokerage_total_base` and period `income_base`). The existing
  `PerformanceAccumulator` (`performance.rs`) and `summarize_holdings` are the
  precedent for aggregating over that loop with explicit availability +
  `excluded_rows` counting.

### Why the aggregate is built server-side from domain values, not by summing GainRow fields

Naively summing serialized `GainRow` fields across open **and** closed rows does
not reconcile: for a **closed** row `unrealized_price_effect_base` holds the
*realized* price effect while `market_value_base` is `0.00`, so
`cost_basis + price + fx ≠ market_value` for that row. The aggregate must therefore
be assembled in the handler from the per-instrument **domain** values (held
component from `valued_holding`, realized component from `performance.realized`),
using the open-waterfall ladder semantics, so a fully-closed position contributes
`0` to the held cost-basis anchor and its whole return through the Realized-gain
step. This is the "one coherent population, reconciles by construction" requirement
from the brief (blindspot findings 1 and 5).

### Population and the include_closed question (brief asked to investigate)

**Decision taken by this plan (confirm in Open Questions):** the aggregate is
**population-stable and independent of `include_closed`**. It aggregates over the
same set that feeds the existing period totals — *every instrument with period
exposure*, including fully-closed-in-period positions — because `include_closed`
governs only **row rendering**, not portfolio performance (decision 2026-06-20).
An instrument is included only when **all** waterfall inputs it must contribute are
available; otherwise it is excluded and counted in the block's `excluded_rows`
(mirroring `totals.excluded_rows`). Because every aggregate field then sums over
the *same* included subset, the ladder reconciles by construction.

Consequence: the Dashboard's `useGains()` call can keep passing only the date range
(no `include_closed`); the waterfall is a complete portfolio total return either
way. **Income** is the one special-cased input: when income is universally
"not tracked" (`income_not_tracked`), it contributes `0` and the block sets an
`income_not_tracked` flag so the frontend renders the "Not tracked yet" placeholder
rather than excluding every instrument (mirrors `incomeForTotalReturn`).

---

## Phase 1 — Gains page layout compaction (frontend only)

Smallest viable end-to-end slice; independent of everything else. No API change.

**Scope (Gains page only; shared chrome untouched):**

- `GainsPage.tsx`: remove the `PORTFOLIO / Gains` panel-header block (the `eyebrow`
  + `h1` in the `.panel-header` at lines ~61–66). The top nav already marks the
  active page. Keep the `AsyncBoundary` and `GainsTable`.
- `GainsTable.tsx` `GainsTotalsBand`: collapse to one slim row.
  - Remove the separate label row (`.gains-totals-header` with the long method
    label) as its own band section; the four metrics (Capital gain, Income,
    Currency gain, Total return) become a single compact row at reduced
    size/padding.
  - Move the return-method `<select>` **out** of the totals band and **into** the
    `.table-toolbar` (alongside `DateRangeSelector`, filter input, and the
    include-closed checkbox). The `<select>` is self-describing
    ("Money-weighted (XIRR)", "Simple", "Modified Dietz (legacy)"), so it carries
    the method context by itself.
  - The long method-dependent label from `totalReturnLabel()` (e.g. "Total return
    including closed positions (money-weighted)" / "Total return (simple)" /
    Modified Dietz legacy note) becomes a `title` tooltip on the **Total return**
    metric instead of a visible band label. Preserve the `componentTitle` tooltip
    behavior on Capital/Currency metrics.
  - The "N incomplete" chip (`totals.excluded_rows`) stays, right-aligned within
    the compact totals row.
- `GainsTable` prop plumbing: `returnMethod` / `onReturnMethodChange` now drive a
  toolbar control rather than a band control; keep `saveReturnMethod` on change.
  `GainsPage` wiring is unchanged (it already passes these props).
- `styles.css`: restyle `.gains-totals`, `.gains-totals-header`,
  `.gains-totals-grid`, `.gains-total-metric`, `.panel-header`, `.table-toolbar`
  for the compacted layout per `docs/VisualDesign.DarkTheme.md` (calm chrome; vivid
  numbers; tokens only, no inline hex). Adjust the mobile overrides near styles.css
  lines ~1643–1662. Remove any now-dead totals-band label styling
  (`.gains-totals-method`, `.gains-totals-note`) if nothing else references them.

**Verification:**

- `frontend/`: `npm run check` then `npm run fmt`.
- No view-model logic changes, so no new unit tests are strictly required; if
  `totalReturnLabel` is extracted/reshaped to feed a `title`, add/adjust a small
  unit test for it (it is pure).
- **External human testing recommended:** confirm the instrument list starts far
  higher in the viewport (the ~700–800px push is gone), the four totals read
  clearly in one slim row, the method `<select>` works from the toolbar and still
  persists via `localStorage`, and the Total-return tooltip shows the method
  context. Check narrow-width reflow of the toolbar.

**Version bump:** `frontend/package.json` (user-visible change).

---

## Phase 2 — Backend portfolio waterfall aggregate block

Additive change to `GET /api/gains`. No UI wiring yet; the frontend only mirrors
the new types.

**Backend (`backend/src/api/gains/`):**

- `types.rs`: add a `PortfolioWaterfallResponse` block and a
  `portfolio_waterfall: PortfolioWaterfallResponse` field on `GainsResponse`
  (additive; every existing field stays byte-identical). Fields, all
  `AvailabilityResponse` money values in SEK unless noted, chosen to mirror exactly
  what the open-with-brokerage-breakdown ladder consumes:
  - `cost_basis_base` — Σ held (net) cost basis
  - `held_fee_component_base` — Σ held fee component (for the gross decomposition)
  - `price_effect_base` — Σ held price effect
  - `fx_effect_base` — Σ held FX effect
  - `market_value_base` — Σ market value
  - `realized_gain_base` — Σ realized gain
  - `realized_fee_base` — Σ realized fee
  - `realized_cost_basis_base` — Σ realized (sold) cost basis (for % denominators)
  - `brokerage_total_base` — Σ brokerage (raw ledger sum)
  - `income_base` — Σ period income (or `0` when `income_not_tracked`)
  - `unrealized_gain_base` — Σ unrealized gain
  - `total_return_base` — Σ (unrealized + realized + income)
  - `income_not_tracked: bool`
  - `excluded_rows: usize`
- `performance.rs` (or a new `waterfall.rs` sibling module): add a
  `PortfolioWaterfallAccumulator` mirroring `PerformanceAccumulator`'s pattern. It
  ingests the per-instrument domain values already computed in the handler loop
  (`valued_holding` held component + `performance.realized` realized component +
  `brokerage_total_base` + row `income_base`). An instrument contributes only when
  **all** its required inputs are available; otherwise `excluded_rows += 1` and it
  contributes to nothing (guaranteeing all fields sum over the same subset →
  reconciliation by construction). Income is the special case: `income_not_tracked`
  contributes `0` and does not exclude the instrument, and the accumulator records
  that at least one instrument was `income_not_tracked` so the response flag is set
  appropriately. Each output field is `Available` only if the population is
  non-empty and that field's inputs were all available; otherwise `Unavailable`
  with merged, de-duplicated reasons.
- `gains.rs` handler: instantiate the accumulator alongside `perf_accum`, feed it
  inside the existing per-instrument loop (both the open-position branch and the
  closed-position branch — a closed position contributes `0` held cost basis / held
  effects / market value and its realized gain), and serialize the block into the
  response. Do **not** gate it on `query.include_closed`.
- `tests.rs`: add backend tests asserting:
  - **Reconciliation (required):** with a coherent multi-instrument population,
    `market_value == cost_basis + price_effect + fx_effect` and
    `total_return == unrealized_gain + realized_gain + income`, i.e. the ladder
    sums from the held-cost-basis anchor to the total-return terminus. Include a
    case with a fully-closed-in-period position so its return flows only through
    the realized step and still reconciles.
  - **Exclusion semantics:** an instrument with a missing FX/price input is
    excluded and counted in `portfolio_waterfall.excluded_rows`, and the remaining
    population still reconciles.
  - **include_closed independence:** the block is identical for `include_closed`
    true vs false given the same date range.
  - **income_not_tracked:** when income is not tracked, `income_base` is `0`,
    `income_not_tracked` is `true`, and instruments are not excluded for it.

**Frontend (`frontend/src/api/types.ts`):** add a `PortfolioWaterfall` interface
mirroring the block and a `portfolio_waterfall: PortfolioWaterfall` field on
`GainsResponse`. No UI consumes it yet.

**Verification:**

- `backend/`: `cargo build`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt`, and the new `cargo test` cases.
- `frontend/`: `npm run check` then `npm run fmt` (types compile; nothing renders
  the block yet).

**Version bump:** `backend/Cargo.toml` (additive API surface change).

---

## Phase 3 — Shared waterfall renderer refactor + portfolio view-model

Makes the renderer reusable and derives a `WaterfallView` from the new block, with
the reconciliation unit test. Keeps the asset page green throughout.

**Renderer refactor (`GainsWaterfall.tsx`):**

- Parameterize the currently hard-coded bits so both the asset page and the
  Dashboard can use it:
  - Accept a `title` prop instead of the hard-coded `"Gains breakdown"` `<h2>` and
    `aria-label`.
  - Accept the wrapper `className` (or a small variant flag) so the asset page
    keeps `panel asset-panel gains-waterfall` while the Dashboard can use its panel
    class; keep `gains-waterfall` as the shared style hook.
  - The percent column stays (population-matched display-only percent is coherent
    now); no need to make it optional for this work. If trivially cheap, gate the
    `% of cost` column behind an optional `showPercent` (default `true`) to keep
    the component honest — optional, not required.
- `AssetView.tsx` (line ~194): pass `title="Gains breakdown"` and the existing
  class so behavior is unchanged.

**Portfolio view-model:**

- Add `portfolioWaterfallView(block: PortfolioWaterfall): WaterfallView` producing
  the same `WaterfallView` shape. It mirrors `openWaterfall`'s
  `hasBrokerageBreakdown` branch: gross cost basis / gross price effect / gross
  realized via the fee components, Market value subtotal, Brokerage costs negative
  step, Dividend income (real step, or the `Not tracked yet` placeholder when
  `income_not_tracked`), and a Total-return delta bar floating from the gross
  cost-basis baseline. Unavailable aggregate steps render the standard unavailable
  bar. Percent column uses population-matched denominators (held cost basis for
  price/FX; realized cost basis for realized; held+sold total for total return),
  identical to the per-asset rules.
- **DRY (Agents.md):** factor the shared open-ladder construction so
  `waterfallView(GainsRow)` and `portfolioWaterfallView(block)` build the ladder
  through one internal helper taking a normalized input, rather than duplicating
  the ladder logic. Prefer this refactor over a copy-paste second builder.
- **Reconciliation unit test (required, blindspot finding 5):** in
  `waterfallViewModel.test.ts` (or a sibling), assert that for a fully-available
  aggregate block the effect steps sum from the anchor to the terminus — i.e. the
  Total-return bar's `to` equals cost-basis `+ price + fx + realized + brokerage
  (net 0 in gross scheme) + income`, and the Market-value subtotal equals
  `cost_basis + price + fx`. A bar-count assertion alone is explicitly insufficient.
  Add cases for an unavailable step (unavailable bar, no NaN) and for
  `income_not_tracked` (placeholder row).

**Verification:**

- `frontend/`: `npm run check`, new Vitest cases, then `npm run fmt`.
- Asset page must remain visually unchanged (renderer refactor is behavior-preserving).

---

## Phase 4 — Dashboard swap: waterfall replaces Allocation

Wires the view-model and renderer into the Dashboard and removes Allocation
entirely.

**`Dashboard.tsx`:**

- Remove `AllocationPanel` (component + its render slot).
- Add a portfolio waterfall panel in its place, driven by
  `portfolioWaterfallView(gainsQuery.data?.portfolio_waterfall)` and rendered via
  the refactored `GainsWaterfall` with a Dashboard-appropriate `title`
  (e.g. "Portfolio gains breakdown"). It respects the Dashboard date range (the
  gains query already takes `dateRange`); the Dashboard `useGains()` call stays as
  is (date range only — see population decision above).
- Handle excluded rows: render the standard "N incomplete" warning chip on the
  panel when `portfolio_waterfall.excluded_rows > 0` (reuse the existing
  `status-chip warning compact` convention).
- Keep loading/error/empty handling consistent with the other Dashboard panels.

**`dashboardSelectors.ts` + `dashboardSelectors.test.ts`:**

- Remove `allocationBreakdown`, `AllocationDimension`, `AllocationSlice`,
  `Allocation`, and their helpers (`bucketKey`, `bucketLabels`). Keep `topMovers`.
- Remove the `allocationBreakdown` describe block and imports from the test file;
  keep the `topMovers` tests.

**`styles.css`:** remove the now-unused allocation classes (`.allocation-panel`,
`.allocation-body`, `.allocation-bar`, `.allocation-segment`, `.allocation-table`,
`.allocation-label`, `.allocation-isin`, `.allocation-swatch`, and related). Add
any minimal panel styling the Dashboard waterfall needs, reusing the shared
`gains-waterfall` styles.

**Accepted consequence (user-confirmed):** the currency and type allocation
breakdowns disappear from the app for now. A possible future home is treemap
grouping dimensions — mention as follow-on only; do not implement.

**Verification:**

- `frontend/`: `npm run check`, updated Vitest, then `npm run fmt`.
- **External human testing recommended:** on `/`, the Allocation panel is gone and
  the portfolio waterfall renders in its place; the ladder reconciles visually
  (Total return bar height equals the buildup); the date-range selector reshapes
  the waterfall; the "N incomplete" chip appears when instruments are excluded;
  income shows a real step or the "Not tracked yet" placeholder as appropriate.
  Confirm the asset page waterfall is unchanged.

**Version bump:** `frontend/package.json` (user-visible change), kept aligned with
the backend bump from Phase 2 for the release.

---

## Phase 5 — Documentation and Decision Log

Add the three DecisionLog entries the brief calls for (append to
`docs/DecisionLog.md`; do not renumber older entries), each naming behavior rather
than plan phases:

1. **Dashboard composition change** — the portfolio gains waterfall replaces the
   Allocation panel, superseding the earlier `/` = tiles/chart/movers/allocation
   composition (2026-06-22 "Dashboard is the landing page"); the currency/type
   allocation breakdown is intentionally dropped for now, with treemap grouping
   dimensions noted as a possible future home.
2. **Gains header compaction** — the Gains page removes its page-local eyebrow/h1
   header and collapses the totals band to one slim row, moving the return-method
   selector into the table toolbar and the long method label into a tooltip;
   shared chrome is unchanged.
3. **Portfolio waterfall aggregates are one-population, server-aggregated,
   display-only decomposition** — `GET /api/gains` exposes a portfolio waterfall
   block aggregated server-side over one coherent population (all period-exposed
   instruments whose inputs are fully available; others counted as excluded),
   independent of `include_closed`, reconciling by construction; percentages and
   the brokerage gross-decomposition are display-only and never change money of
   record (consistent with 2026-06-18, 2026-06-22, 2026-07-09).

Also review whether any durable design doc under `docs/` describing the Dashboard
composition or the Gains view needs a light update; update as needed. (Per the
project convention, ephemeral `Design.*.md` briefs may already have been deleted;
the DecisionLog is the durable record.)

**Verification:** documentation review only.

---

## Cross-cutting notes

- **Architecture:** aggregation stays a pure backend accumulator (unit-tested via
  the handler tests / accumulator); the frontend derivation stays a pure
  view-model with unit tests; components remain presentational. Entry points
  unchanged. DRY is preserved by sharing one ladder builder between the per-asset
  and portfolio view-models and one renderer between the two surfaces.
- **No config flag/threshold** is introduced, so the "defaults must exercise the
  new path" rule is satisfied trivially: the new block ships always-on and the
  Dashboard renders it by default.
- **Money of record is untouched** — every new number is a display aggregate or a
  display-only percentage/gross decomposition.

## Documents this work will touch

- `docs/DecisionLog.md` — three new entries (Phase 5).
- Any durable Dashboard/Gains design doc under `docs/` still present — light
  update if it describes the replaced Allocation panel or the old Gains header.
- Version manifests — `frontend/package.json`, `backend/Cargo.toml`.

## Open Questions

1. **Population / `include_closed` for the aggregate (brief asked to investigate).**
   This plan takes the aggregate as **population-stable and independent of
   `include_closed`** (all period-exposed instruments; excluded ones counted),
   matching the 2026-06-20 "totals include closed period activity" precedent, so
   the Dashboard call stays date-range-only. Confirm this is the intended semantics
   versus tying the aggregate to the rendered-rows population (which would make the
   Dashboard number depend on a toggle the Dashboard does not even expose).
2. **Dashboard waterfall panel title.** Proposed "Portfolio gains breakdown".
   Confirm wording (vs "Gains breakdown", which the asset page already uses).
3. **Placeholder vs real zero for income.** With dividends now trackable
   (2026-06-23), portfolio income is typically `Available(0.00)` and renders a real
   `0.00` step; the "Not tracked yet" placeholder appears only when
   `income_not_tracked`. Confirm this matches the desired presentation (it mirrors
   the per-asset waterfall exactly).
4. **Optional `showPercent` prop on the renderer.** The percent column stays for
   both surfaces; adding an unused `showPercent` toggle is optional. Confirm
   whether to add it now for honesty or defer until a caller needs it.
