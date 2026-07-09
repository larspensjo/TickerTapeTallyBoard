# Plan — Effective buy price marker on the Gains/asset Price history chart

## Summary

Add a horizontal break-even marker to the per-asset **Price history** card: a
dotted reference line at the position's per-share cost basis, with a number-only
price tag on the right Y-axis. It appears only for **open** positions and only
when the native average cost is available. No backend logic changes — the value
already flows to the frontend on the `Holding`.

The line is drawn with `lightweight-charts` `series.createPriceLine(...)` on the
existing area series, which produces both the dotted line and the axis tag as one
primitive. The value is `holding.average_cost_native` (native currency, matching
the chart's native-currency Y-axis), parsed to a number.

### Settled decisions (do not re-open)

- **Primitive:** `series.createPriceLine(...)` on the existing area series — not a
  flat reference series, not a custom axis primitive.
- **Value:** `average_cost_native` (native currency). Native cost basis excludes
  fees (fees are base-currency only); this is the pure price cost basis, and is
  always derivable per the 2026-06-14 Missing-FX Contamination Rule.
- **Scope:** open positions only — enforced by the guard
  `data.holding?.average_cost_native != null`, not by a branch distinction. Closed
  positions still flow through the `data.kind === "position"` branch
  (`AssetView.tsx` ~line 182); they get no line simply because `data.holding` is
  null there. The genuinely separate no-row branch is untouched.
- **Label:** number only, matching the existing last-value axis tag style (empty
  `title`, `axisLabelVisible: true`); the price line inherits the area series'
  price format, so the label matches the last-value tag automatically.
- **Line style:** dotted (`LineStyle.Dotted`).
- **Color:** `#e0b15e` — the gold already present in `TimeSeriesChart.tsx` for the
  currently-unused dashed reference series. Reused as a shared named constant so
  there is one source of truth. Gold is deliberately distinct from the blue price
  area, the green/red up/down P&L semantics, and the amber `--warning` used for
  staleness; a break-even reference is neither P&L nor a warning, so no existing
  semantic token fits. See Open Questions on whether to formalize a theme token.
- **No backend change.**

### Accepted known limitations (record, do not expand scope)

- **Split-instrument marker mismatch:** on a split instrument the cost line sits at
  the split-adjusted level (correct against the split-adjusted price series), while
  the existing Buy markers still show raw trade prices. Ship as-is; leave buy
  markers unchanged. This is a documented, accepted inconsistency and a possible
  future follow-up. See the 2026-06-22 Chart Axis / Buy-Sell Marker decisions.
- **Two avg-cost numbers:** the page's "Avg cost" metric tile is SEK
  (`average_cost_base`) while this line is native currency. Accepted with no extra
  labelling — the chart title already names the native currency.
- **Autoscale/clipping:** the area series forces a zero baseline
  (`zeroBaselineAutoscale`) and the view is windowed to `visibleStart`. Price lines
  do not participate in autoscale, so a cost basis above the visible window maximum
  can clip at the top edge. Acceptable and documented, but must be verified.

## Architecture notes

- View-derivation stays in a pure selector: a tiny `costBasisLineValue(holding)`
  helper in `assetViewModel.ts` parses `average_cost_native` and applies the null
  guard, returning `number | undefined`. Components stay thin — `AssetPriceChart`
  passes the derived value straight through.
- Unidirectional flow is preserved; the chart consumes props and renders.
- The chart's color constants stay DRY: the gold is promoted to one named constant
  reused by both the reference series and the new price line.

## Highest-risk implementation detail — price-line lifecycle

The chart series is created once in the mount effect keyed on
`[height, lineColor, topColor, bottomColor]` and is **not** remounted per
instrument (there is no React key on instrument id); data is refreshed in a
separate effect. A naive `createPriceLine` call in the data effect would either
freeze the line on the first asset or stack duplicate lines as the user navigates
asset → asset.

This is sharpened by React `StrictMode` (`frontend/src/main.tsx` ~line 31): in
dev, effects mount → clean up → remount, and the mount-effect cleanup calls
`chart.remove()` and nulls `seriesRef` (`TimeSeriesChart.tsx` ~line 244). If the
price-line cleanup skips removal but leaves a stale `IPriceLine` handle in the
ref, the next effect run can call `removePriceLine(staleHandle)` against the
freshly-created series the handle does not belong to.

The plan therefore requires a **dedicated effect** that owns the price line:

- Keep an `IPriceLine | null` ref (e.g. `costPriceLineRef`).
- Effect deps: `[costBasisLine]`.
- On run: if a previous price line ref exists, `series.removePriceLine(ref)` and
  clear the ref; then, if `costBasisLine` is a finite number and the series exists,
  `series.createPriceLine(...)` and store the returned handle. Mirror the existing
  `referenceSeries.setData([])` clear-then-set pattern.
- On cleanup: **always clear `costPriceLineRef` (set it to null)**, and only call
  `series.removePriceLine(handle)` when `seriesRef.current` is still non-null —
  i.e. the handle belongs to a live series. The mount-effect cleanup nulls
  `seriesRef` and calls `chart.remove()`, which destroys the series; when that has
  already happened the cleanup must drop the stale handle **without** calling
  `removePriceLine`, so the next mount cannot remove a stale handle against a new
  series. Declare this effect **after** the mount effect so, on unmount/remount,
  the mount cleanup runs first and this guard short-circuits removal while still
  clearing the ref.
- Because `AssetPriceChart` passes static `height`/colors, the series is never
  recreated at runtime, so this effect's `[costBasisLine]` deps are sufficient.
  (Noted as a bounded assumption — see Open Questions.)

## Testing reality

The derivation is a trivial parse-and-guard passthrough, so a pure unit test
asserts little. The behaviors that can actually break — line only for open
positions, hidden when unavailable, correct level, no stacking across navigation,
no line on closed, clipping when above the window — live in the imperative
`createPriceLine` wiring. Most of those (correct level, clipping, split-adjusted
positioning) are visual and are routed to **named manual/external test steps**
(Phase 3). One class — the price-line lifecycle across prop changes — is the
highest risk and *is* cheaply testable: `TimeSeriesChart.test.tsx` already mocks
lightweight-charts (~line 7), so a small unit test can assert `createPriceLine` /
`removePriceLine` call behavior across rerenders without browser automation. That
test complements, and does not replace, the manual steps. A single pure-helper
unit test covers the derivation.

## Phases

Plans are ephemeral: durable docs and code must name the behavior, not these phase
numbers.

### Phase 1 — Pure cost-basis derivation helper

- Add `costBasisLineValue(holding: Holding | null): number | undefined` to
  `frontend/src/components/assetViewModel.ts`: return `undefined` when the holding
  is null or `average_cost_native` is null/non-finite (use the shared
  `parseFiniteNumber`), otherwise the parsed number.
- Add a unit test in `assetViewModel.test.ts`: available value → number; null
  holding → undefined; null `average_cost_native` → undefined.

Verify:
- From `frontend/`: `npm run check`, then `npm run fmt`.
- Targeted `vitest` run of the asset view-model test.
- No UI change yet.

### Phase 2 — Price-line support in `TimeSeriesChart`

- Add optional prop `costBasisLine?: number`.
- Promote the gold `#e0b15e` to a single named module constant and use it for both
  the existing reference series and the new price line (DRY).
- Add the `costPriceLineRef` and the dedicated add/remove/cleanup effect exactly as
  described in "price-line lifecycle" above. Price line options: `price:
  costBasisLine`, gold color, `lineStyle: LineStyle.Dotted`, thin width,
  `axisLabelVisible: true`, empty `title` (number-only tag inheriting the series
  price format), `lineVisible: true`.
- No caller passes the prop yet, so runtime behavior is unchanged.
- Add a lifecycle unit test in `TimeSeriesChart.test.tsx` using the existing
  lightweight-charts mock: a price line is created when `costBasisLine` is
  provided; on a value change the old line is removed and one new line created (not
  stacked); and the line is removed when the prop becomes `undefined`. Assert on
  the mocked `createPriceLine` / `removePriceLine` call counts/arguments, not on
  DOM structure.

Verify:
- From `frontend/`: `npm run check`, then `npm run fmt`.
- Targeted `vitest` run of `TimeSeriesChart.test.tsx`.
- Buildable with no behavior change; visual verification deferred to Phase 3 where
  a real data path exists.

### Phase 3 — Wire the position branch + version bump + manual verification

- Add `costBasisLine?: number` to `AssetPriceChart`'s props and pass it through to
  `TimeSeriesChart`.
- In `AssetView.tsx`, pass `costBasisLine={costBasisLineValue(data.holding)}` on the
  **position-branch** render only. Leave the no-row branch render unchanged.
- Version bump (frontend-only feature; surfaced via `/api/health`):
  `frontend/package.json` `0.20.0 → 0.21.0`. The backend has no change and its
  manifest is not touched, so `backend/Cargo.toml` is **not** bumped and no backend
  build/clippy/fmt step is required.

Verify (automated):
- From `frontend/`: `npm run check`, then `npm run fmt`.

Verify (external/human testing — REQUIRED, this is where the real behavior lives):
- **Open position:** dotted gold line spans the chart at the average-cost level
  with a number-only right-axis tag; the tag format matches the last-value tag.
- **Closed position:** no cost line at all — the render still uses the
  `data.kind === "position"` branch, but `data.holding` is null so the guard skips
  the line. (This is not the no-row branch.)
- **No-holding / never-traded (no-row branch):** no cost line at all.
- **Unavailable value:** (defensive) no line when `average_cost_native` is null; no
  broken/zero line, no extra "unavailable" UI.
- **Split instrument:** line sits at the split-adjusted level; confirm the raw-price
  Buy markers visibly differ and that this is the accepted, documented mismatch.
- **Asset → asset navigation:** the line updates to each asset's level and does
  **not** stack or freeze (the core lifecycle risk).
- **Clipping check:** open an asset whose cost basis is above the visible window
  maximum; confirm the line clips at the top edge and that this is acceptable
  documented behavior, not a crash or layout break.

### Phase 4 — Documentation & decision log

- Add a `docs/DecisionLog.md` entry recording: the break-even cost line uses native
  `average_cost_native`, drawn split-adjusted via `createPriceLine`; open positions
  only; number-only tag; and the **accepted** raw-price Buy-marker mismatch on split
  instruments as a known scope boundary / future follow-up. (Refines the 2026-06-22
  Chart Axis and Buy/Sell Marker decisions.)
- If the Open Question on a reference-line theme token is resolved toward
  formalizing it, add the token to `docs/VisualDesign.DarkTheme.md`; otherwise note
  in the decision entry that the gold reference color is an in-module chart constant
  by design.

Verify:
- Docs review only; no build impact.

## Documents to update

- `docs/DecisionLog.md` — new entry (Phase 4).
- `docs/VisualDesign.DarkTheme.md` — only if a reference-line token is formalized
  (see Open Questions).
- No design doc under `docs/Design.*` requires changes; the asset price chart's
  durable semantics live in the DecisionLog entries.

## Open Questions

1. **Reference-line color as a theme token?** The plan picks the concrete gold
   `#e0b15e` and reuses it as an in-module chart constant (consistent with the
   other inline chart colors already in `TimeSeriesChart.tsx`). Should this instead
   become a named CSS/theme token in `VisualDesign.DarkTheme.md` (e.g.
   `--reference-line`), given the doc's "never inline hex" guidance? Charts pass JS
   color values to lightweight-charts rather than CSS, so a JS constant is the
   pragmatic fit; confirm whether formalization is wanted.
2. **Version bump magnitude.** Decided frontend-only: `frontend/package.json`
   `0.20.0 → 0.21.0`, no backend bump. Remaining minor-vs-patch magnitude for the
   frontend is a low-stakes confirm.
3. **Series-recreation robustness.** The dedicated effect assumes the chart series
   is never recreated at runtime (true for this caller, which passes static
   height/colors). If `TimeSeriesChart` later gains runtime-variable colors/height,
   the cost line would need to be re-added on series recreation. Acceptable to leave
   as a bounded assumption now?
