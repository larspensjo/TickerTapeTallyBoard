# Concept: Gains Breakdown as a Waterfall

Status: investigation / concept (not yet planned or implemented)

## Goal

Replace the current two-line "Gains breakdown" panel on the asset view with a
stacked **waterfall** layout that shows, step by step, how the numbers add and
subtract to reach a final result. The user wants to *see* the buildup — cost
basis, the effects that move it, and (where relevant) realized gains — rather
than just a single unrealized total.

## Current state

The asset view renders a minimal breakdown in
[AssetGainsBreakdown](../frontend/src/components/AssetView.tsx) with three rows:

- Capital (price effect)
- Currency (FX effect)
- Unrealized total (ruled)

Data is shaped by [breakdownView()](../frontend/src/components/assetViewModel.ts),
which pulls `price_effect_base`, `fx_effect_base`, and `unrealized_gain_base`
from a `GainsRow`.

## Key identities (what makes a waterfall sound)

For an **open** position, the per-instrument data already satisfies:

```
market value = cost basis + price effect + FX effect
unrealized total = price effect + FX effect
```

Confirmed against a live example (Arista Networks, SEK base):

- 265,582.94 (cost basis) + 53,546.54 (price) + 9,418.19 (FX) = 328,547.67 (market value)
- 53,546.54 + 9,418.19 = 62,964.73 (unrealized total)

These identities come from the domain calculation in
[domain/valuation.rs](../backend/src/domain/valuation.rs):

- `market_value_base = market_value_native * latest_fx`
- `unrealized_gain_base = market_value_base - cost_basis_base`
- `price_effect_base = (market_value_native - cost_basis_native) * fx - fee_component_base`
- `fx_effect_base = cost_basis_native * fx - (cost_basis_base - fee_component_base)`

Note: **fees are baked into `cost_basis_base`** today (via `fee_component_base`),
so the price/FX effects are computed net of fees. Splitting fees out as their own
waterfall step would require exposing `fee_component_base` per row.

## Realized gains — verified available, but reframes the total

`RealizedGain` is computed for **every** instrument in
[derive_position_performance()](../backend/src/domain/position.rs), not just
closed ones. It carries decomposed fields:

- `proceeds_native` / `proceeds_base`
- `cost_basis_native` / `cost_basis_base`
- `price_effect_base`, `fx_effect_base`
- `gain_base`
- `sold_quantity`

It defaults to literal zeros via `RealizedGain::zero()`, so "show 0 when there
are no sells" falls out naturally.

Today it is only **serialized** as a separate closed row when
`include_closed && sold_quantity > 0` (see
[api/gains.rs](../backend/src/api/gains.rs), around the realized-row branch).
Surfacing realized gains on an open position is therefore a **serialization
change, not new math** — low risk.

### Important framing caveat

Realized gains come from shares **already sold**; market value reflects shares
**still held**. They do not flow into the "market value" total — the arithmetic
does not close if you append realized after market value while still calling the
end "market value".

The fix: make the waterfall **terminate at total return / total P&L**, not at
market value. Market value becomes an intermediate subtotal.

```
Cost basis (held)        265,583   ████████████
  + Price effect          +53,547             ░░░
  + FX effect              +9,418               ░
  = Market value         328,548   ███████████████   (subtotal)
  + Realized gain              +0                     (sold shares)
  ( + Dividends )              +0                     (later phase)
  ─────────────────────────────────
  Total return             62,965+                    (unrealized + realized [+ income])
```

## Fields available now vs. needs backend work

Available today on `GainsRow` (see [api/types.ts](../frontend/src/api/types.ts)),
no backend change:

| Field | Role in stack |
|---|---|
| `cost_basis_base` | base/foundation bar |
| `price_effect_base` | +/- step |
| `fx_effect_base` | +/- step |
| `unrealized_gain_base` | subtotal of the two effects |
| `market_value_base` | intermediate subtotal |
| `proceeds_base` | meaningful for closed positions only |
| `day_change_base` | **not additive** — a 1-day slice of price effect; keep as context, not a stack layer |

Needs backend serialization/plumbing:

1. **Realized gain on open rows** — `RealizedGain` already computed; expose
   `realized_gain_base` (and ideally its price/FX split) on the open `GainsRow`.
   Low risk.
2. **Fees as a discrete step** — `fee_component_base` exists in the domain but is
   folded into cost basis; expose it to show "clean cost → − fees".
3. **Dividends / income per instrument** — exists only at portfolio totals
   (`income_base` in `GainsTotals`/`GainsSummary`), **not per row**. Real backend
   work required. Highest-value addition for a true "total return" stack.

## Layout concepts considered

- **A — Waterfall (recommended).** Running total from cost basis up; each step a
  floating bar starting where the previous ended; negative steps float down and
  render red. Most literal answer to "show how everything adds and removes."
- **B — Stacked rows + running subtotal column.** Cheapest; reuses the existing
  `dl` list, adds a cumulative column. Good fallback / first increment.
- **C — Single segmented horizontal bar.** Compact, good for mobile, but harder
  to label precisely.

## Edge cases / decisions to settle

- **Open vs closed positions.** Closed positions have no market value; the stack
  should pivot to `proceeds − cost basis = realized`. Reuse one component with two
  modes; the view model already branches on `position_status`.
- **Availability, not just zero.** Every field is an `Availability<Decimal>` and
  can be `Unavailable` (e.g. missing FX) — distinct from a genuine `0`. Render
  unavailable steps distinctly; do not collapse to 0.
- **Always-zero realized noise.** Most positions never sold; realized is 0 the
  vast majority of the time. Lean toward showing it for layout consistency but
  de-emphasizing (muted, no colored bar) when zero.
- **Sign/color convention.** Align with the dark-theme spec
  ([VisualDesign.DarkTheme.md](VisualDesign.DarkTheme.md)): green up-steps, red
  down-steps, neutral base/subtotals/totals.
- **Scope.** Concept A using only existing fields delivers ~90% of the visual
  payoff with zero backend changes. Fees and dividends are natural follow-on
  phases.

## Suggested phasing (for a future plan)

1. **Phase 1 — Waterfall with existing fields.** cost basis → price → FX →
   market value, open + closed modes. Frontend only.
2. **Phase 2 — Realized gains.** Expose `realized_gain_base` on open rows; extend
   the stack to terminate at total return. Backend serialization + frontend.
3. **Phase 3 — Fees split.** Expose `fee_component_base` per row; add a "− fees"
   step.
4. **Phase 4 — Dividends/income per instrument.** Real backend plumbing; complete
   the total-return stack.

Each phase is independently testable; backend changes follow the unidirectional
flow and keep reducers pure (see [Agents.md](../Agents.md)).

## Key source references

- Frontend panel: [AssetView.tsx](../frontend/src/components/AssetView.tsx)
- View model: [assetViewModel.ts](../frontend/src/components/assetViewModel.ts)
- Wire types: [api/types.ts](../frontend/src/api/types.ts)
- Effect math: [domain/valuation.rs](../backend/src/domain/valuation.rs)
- Realized gains: [domain/position.rs](../backend/src/domain/position.rs)
- Gains serialization: [api/gains.rs](../backend/src/api/gains.rs)
