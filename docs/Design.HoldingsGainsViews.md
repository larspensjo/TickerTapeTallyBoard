# Design: Holdings / Gains / Transactions view differentiation

Status: accepted design, not yet implemented.

## Problem

The Holdings and Gains views currently overlap heavily. Both show market value
(SEK), cost basis, unrealized gain and %, and day change; Gains is effectively
Holdings with the P&L bundle unpacked into columns. The two views do not answer
distinct questions, so the second one adds little.

The application already stores native value and FX separately (see the
2026-06-13 "Keep Native Value And FX Separate For Currency-Gain Attribution"
decision), which makes a currency-aware performance breakdown possible. The
review proposes using that to give each view a single, clear purpose.

## The three-view model

Each board view answers one question:

| View         | Question              | Emphasis                              | Default sort      |
| ------------ | --------------------- | ------------------------------------- | ----------------- |
| Holdings     | Where is my money?    | Exposure, size, currency, weight      | Market value desc |
| Gains        | What made/lost money? | Performance attribution (stock vs FX) | Total gain desc   |
| Transactions | What happened?        | Buys, sells, dividends, fees, dates   | Date desc         |

This document specifies the **core attribution** scope only. Larger follow-on
ideas are listed under "Out of scope" and are intended as separate later phases.

## Scope

In scope:

- Sharpen the conceptual split between Holdings (exposure) and Gains (attribution).
- Split each holding's unrealized gain into a **price effect** and an **FX effect**.
- Add **Portfolio %** to Holdings.
- Column/sort cleanup so the two views stop duplicating each other.

Out of scope (future phases):

- Splitting *today's* day change into price vs FX effect.
- Time-period selector (Today / 1W / 1M / YTD / 1Y / since purchase).
- Dividend income, fee, and realized-gain attribution lines.
- Per-effect display toggles.

## Attribution math

For one holding with open quantity `Q`:

- `C_n` = `cost_basis_native` (native cost basis of the open position; weighted
  average, **gross of fees** — brokerage is tracked separately in SEK, never in
  native; see `position.rs::apply_buy`)
- `C_b` = `cost_basis_base` (SEK cost basis at purchase-time FX; **includes SEK
  brokerage**. Available only when every contributing buy had a known FX rate, per
  the 2026-06-14 Missing-FX Contamination Rule)
- `fee_base` = `fee_component_base` (the SEK brokerage already folded into `C_b`;
  carried alongside `C_b` in `BaseCostBasis::Available`)
- `MV_n` = `market_value_native` = `price_now * Q`
- `FX_now` = latest FX rate to base (1 for SEK holdings)
- `MV_b` = `market_value_base` = `MV_n * FX_now`
- `gross_base` = `C_b - fee_base` (SEK value of the shares at purchase, excluding
  fees)
- `avg_purchase_fx` = `gross_base / C_n` (pure cost-weighted purchase FX; correct
  across multiple buy lots and free of fee contamination)

Effects:

```
price_effect_base = (MV_n - C_n) * FX_now - fee_base
fx_effect_base    = C_n * (FX_now - avg_purchase_fx)
```

These sum exactly to the existing unrealized gain:

```
price_effect_base + fx_effect_base
  = (MV_n - C_n)*FX_now - fee_base + C_n*FX_now - C_n*avg_purchase_fx
  = MV_n*FX_now - fee_base - gross_base
  = MV_b - (gross_base + fee_base)
  = MV_b - C_b
  = unrealized_gain_base
```

### Cross-term and fee convention

The decomposition has an interaction (cross) term, and brokerage is a SEK cost
that must not appear as currency movement. Both are assigned to **price effect**
deliberately:

- The native gain is valued at *today's* FX rate, so the cross-term lands in price
  effect ("what my stock gain is worth in SEK right now"). The alternative (value
  native gain at purchase FX) is equally exact but less intuitive.
- Brokerage (`fee_base`) is subtracted from price effect, so it reads as a drag on
  the stock/return leg, not a currency loss. Because `avg_purchase_fx` is computed
  from `gross_base` (fees excluded), `fx_effect_base` is pure currency movement and
  is exactly 0 when the rate is unchanged — even with non-zero brokerage. This
  honors the 2026-06-13 rule that brokerage is a separate SEK fee, never run
  through FX.

### SEK holdings

For SEK holdings `FX_now = 1` and `avg_purchase_fx = 1`, so `fx_effect_base = 0`
and `price_effect_base = MV_n - C_n - fee_base = unrealized_gain_base`. The FX
column shows `0` for these rows.

### Availability

`price_effect_base` and `fx_effect_base` are available only when **all** of:

- `market_value_base` is available (price and FX present), and
- `cost_basis_base` is available (no missing-FX contamination), and
- `C_n != 0`.

When `C_n == 0`, reuse the existing `ZeroCostBasis` reason. Otherwise the effects
inherit the reasons that already block `market_value_base` / `cost_basis_base`.
Missing data stays an explicit unavailable state, never zero.

## Backend changes

All inputs already exist in `value_position` in
`backend/src/domain/valuation.rs`; no DB, provider, or refresh changes are
needed.

- `ValuedHolding`: add `price_effect_base: Availability<Decimal>` and
  `fx_effect_base: Availability<Decimal>`.
- Compute them in `value_position` from the values above, reading
  `fee_component_base` from `position.base` (already present on
  `BaseCostBasis::Available`, so no signature change), with availability rules and
  reason propagation matching the existing fields.
- `ValuationSummary`: add `price_effect_base` and `fx_effect_base` as the sum of
  the per-row effects over the rows already included in the summary (additive;
  keeps the summary internally consistent with `unrealized_gain_base`).
- API (`backend/src/api/gains.rs` / valuation serialization): expose the two new
  per-row fields on `GainsRow` and the two new summary fields, additively.

Keep the computation in the pure domain module so it stays unit-testable per
`Agents.md`.

## Frontend changes

### Types (`frontend/src/api/types.ts`)

- `GainsRow`: add `price_effect_base: MoneyValue` and `fx_effect_base: MoneyValue`.
- `GainsSummary`: add `price_effect_base: MoneyValue` and `fx_effect_base: MoneyValue`.

### Gains view (`frontend/src/components/GainsTable.tsx`)

Grouped, sortable columns (attribution-first):

| Instrument | Cost basis (SEK) | Market value (SEK) | Total gain + % | Price effect | FX effect | Day change + % | Status |

- Remove from Gains: `Qty`, `Latest close`, `Market value (native)`, and the
  redundant native cost-basis column — these are exposure/context details, not
  attribution. (`Qty` moves to Holdings; latest close is simply dropped.)
- "Total gain" is the existing `unrealized_gain_base` (+ `unrealized_gain_percent`).
- New `Price effect` and `FX effect` columns are signed and individually sortable.
- Default sort stays `unrealized_gain_base` desc.
- Unavailable effects render with the existing availability cell treatment.

### Holdings view (`frontend/src/components/HoldingsTable.tsx`)

Exposure-first columns:

| Instrument | Qty | Avg cost/share | Cost basis | Current value (SEK) | Portfolio % | Currency | P&L hint |

- `Current value (SEK)` is `market_value_base`, made the visual focus.
- `Portfolio %` = row `market_value_base` / (sum of available `market_value_base`
  across holdings). Rows with unavailable valuation are excluded from the
  denominator and show `—` for their percentage.
- `Currency` shows the instrument currency explicitly.
- Keep the existing P&L bundle but compact and secondary, not the main point.
- Default sort changes from instrument asc to `market_value_base` desc.

### Versions

Bump `frontend/package.json` and `backend/Cargo.toml` versions when implementing,
per `Agents.md`.

## Edge cases

- SEK holdings: FX effect is exactly 0; do not special-case the column away.
- Brokerage fees: subtracted from price effect via `fee_base`; FX effect stays
  pure and is 0 when the rate is unchanged, even with non-zero brokerage.
- Zero cost basis: effects unavailable with `ZeroCostBasis`.
- Missing-FX contamination: `cost_basis_base` unavailable propagates to both
  effects.
- Missing price/FX: `market_value_base` unavailable propagates to both effects.
- Holdings with no available valuation: excluded from the Portfolio % denominator
  and shown as `—`.

## Verification

- Domain unit tests in `valuation.rs`:
  - USD holding: `price_effect_base + fx_effect_base == unrealized_gain_base`.
  - SEK holding: `fx_effect_base == 0`, `price_effect_base == unrealized_gain_base`.
  - Multi-lot buys at different FX: weighted `avg_purchase_fx` gives the correct
    split.
  - Non-zero brokerage with unchanged price and unchanged FX: `fx_effect_base == 0`
    and `price_effect_base == -fee_base` (a fee never appears as FX movement).
  - Zero cost basis and missing-FX cases: effects unavailable with the right reasons.
  - Summary: per-row effects sum to the summary effects, and the two summary
    effects sum to summary `unrealized_gain_base`.
- Frontend: Gains shows the new columns and no longer shows Qty/latest close;
  Holdings shows Portfolio % summing to ~100% over valued rows.
- Recommended human check: open both views, confirm a known holding's price/FX
  split looks right (e.g. a USD holding where USD moved against SEK) and that the
  two views now read as distinct.

## Out of scope (future phases)

- Day change price/FX split (same math on previous vs latest FX).
- Time-period selector: requires historical position reconstruction, backfilled
  price/FX for the period start, and contribution handling for intra-period
  buys/sells (time-/money-weighted return). Largest follow-on lift.
- Dividend income, fees, and realized-gain lines: fees already live in cost basis
  (`fee_component_base`); dividends are a transaction type but not yet aggregated
  as income; realized gains are not computed today.
