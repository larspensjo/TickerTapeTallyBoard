# Conviction Targets

Status: Accepted design note
Date: 2026-07-06

## Purpose

Conviction is a manually configured portfolio-management signal for each asset.
It is not imported from external sources and is not part of transaction, price,
or performance accounting.

The first use is to show whether an asset's current total value is below, near,
or above the value implied by its conviction level. Later work can use the same
model for buy, sell, and rebalance planning.

## Storage And Identity

Conviction is stored per instrument, keyed by `instrument_id`.

Conviction is user-managed portfolio metadata. It is not imported, not stored on
transactions, and not deleted or reset by import refresh, rollback, or ledger
edits.

New instruments default to `Other` until the user changes them.

## Conviction Levels

Each asset has one conviction level:

- `Other`
- `Low`
- `Medium`
- `High`

The target weights are relative per asset:

| Conviction | Target weight |
|---|---:|
| Other | No target |
| Low | 1 |
| Medium | 2 |
| High | 4 |

A Medium-conviction asset targets twice the value of a Low-conviction asset. A
High-conviction asset targets twice the value of a Medium-conviction asset.

## Target Denominator

Assets with conviction `Other` are excluded from the target denominator.

Only assets with `Low`, `Medium`, or `High` conviction and a usable valuation
participate in the target pool. Their current total values are summed, and each
target value is calculated from that target pool.

A lagging valuation can be used when there is a last known value. A truly
unavailable valuation excludes the asset from the target calculation while
retaining its stored conviction. The UI should make this exclusion explicit
rather than presenting the asset as if its stored conviction were `Other`.

Formula:

```text
target_pool_value = sum(current total value of eligible Low/Medium/High assets)
target_weight = sum(weights of eligible Low/Medium/High assets)
asset_target_value = target_pool_value * asset_weight / target_weight
target_gap = current_asset_value - asset_target_value
```

`Other` assets receive no target and no target gap.

`target_gap = current_asset_value - asset_target_value`. Positive gaps mean the
asset is above target / overweight. Negative gaps mean the asset is below target
/ underweight. Displays should preserve the sign so buy/sell interpretation is
unambiguous.

Targets are relative to the current eligible target pool. Changing one
instrument's conviction, adding or removing an instrument from the eligible
pool, or changing market valuations can change every eligible instrument's
target value. After any conviction edit, views must refresh target values for
the full pool, not only the edited row.

Example:

| Asset | Conviction | Current value | Weight | Target value |
|---|---|---:|---:|---:|
| A | Low | SEK 100,000 | 1 | SEK 100,000 |
| B | Medium | SEK 300,000 | 2 | SEK 200,000 |
| C | High | SEK 300,000 | 4 | SEK 400,000 |
| D | Other | SEK 500,000 | - | No target |

The target pool is SEK 700,000, excluding Asset D.

## Display Indication

V1 should use a 5% tolerance band for display.

Suggested target status:

| Status | Meaning |
|---|---|
| `below` | Current value is more than 5% below target. |
| `on_target` | Current value is within +/-5% of target. |
| `above` | Current value is more than 5% above target. |
| `no_target` | Conviction is `Other`. |
| `excluded_unavailable` | Stored conviction is `Low`, `Medium`, or `High`, but valuation is unavailable so the asset is excluded from targets. |
| `unavailable` | The target pool or target value cannot be computed. |

Useful derived display fields:

- Current value
- Conviction
- Target value
- Target gap in SEK
- Target gap as percent of target
- Target status

The tolerance is display-only in V1. Future planning logic may use a different
threshold or allow user configuration.

If the eligible target pool is empty, has no usable valuations, or sums to zero,
target values and target gaps are unavailable. Gap percent is only computed when
the asset target value is greater than zero.

## UI Placement

### Holdings

Holdings is the primary V1 surface for conviction because it already represents
current open positions.

Desired V1 additions:

- Show conviction for each holding.
- Allow changing conviction from the Holdings page.
- Require an explicit accept/apply action before saving conviction changes, to
  reduce accidental click mistakes.
- Show target indication and target gap for open holdings where valuation is
  available.
- Refresh target information for the full eligible pool after applying
  conviction changes.

### Asset Detail

Asset detail is the natural per-asset configuration surface.

Desired V1 additions:

- Show the asset's current conviction.
- Allow changing conviction for the asset.
- Show target information when the asset participates in the target pool.

On Asset Detail, conviction changes save immediately for the current instrument.
The page keeps the conviction loaded at navigation time as a local reset
baseline. If the saved conviction differs from that baseline, a Reset action is
available; Reset immediately saves the baseline conviction back to the
instrument. The baseline is discarded when leaving the page.

### Gains

Gains should not lead with conviction in V1. The page is focused on return
attribution, not allocation policy.

### Dashboard

Dashboard can stay unchanged for V1. A later enhancement could add `Conviction`
as an Allocation dimension.

### Import And Transactions

Conviction is not imported and should not be represented as a transaction.

When an import can predict that an instrument's derived quantity will become
zero while its stored conviction remains `Low`, `Medium`, or `High`, the import
flow should block until the user explicitly chooses whether to keep the
conviction for future planning or change it to `Other`. Stored conviction is
never changed implicitly.

## Closed Positions

Closed positions can have conviction metadata.

This enables future mechanics such as changing a closed asset from `Other` to
`Low`, so it can become eligible for future buy planning when cash is provided.

Closing a position never changes stored conviction automatically. Views derive
effective target eligibility from current quantity and valuation state.

Because Holdings currently focuses on open positions, closed-position conviction
configuration may need to live on Asset detail or a future broader asset
management surface.

Demo seed data should include representative convictions so target indicators
are visible in read-only demo mode.

## Future Planning Mechanics

Future buy, sell, and rebalance planning will use:

- Current total values
- Conviction target deltas
- A user-provided cash value

Cash is not modeled as part of the portfolio ledger for this feature. The user
will provide cash explicitly when asking for planning.

Planning should account for practical constraints such as:

- Whole-share quantities
- Minimum meaningful trade size
- Brokerage or transaction costs
- Whether the mode is buy-only, sell-only, or full rebalance
- Missing or stale valuations

## Important Blindspots

- Missing prices or FX can make current value, target pool value, and target gaps
  unavailable. This must be explicit, never treated as zero.
- A lagging valuation may use the last known value; a truly unavailable
  valuation excludes a convicted asset from target calculation without changing
  stored conviction.
- If all assets are `Other`, there is no target pool.
- If only one asset is eligible for the target pool, that asset targets 100% of
  the target pool and will always be on target unless valuation becomes
  unavailable.
- Target gaps across the effective eligible target pool should sum to
  approximately zero, subject to rounding. Convicted assets excluded because
  their valuation is unavailable are outside that invariant.
- Conviction is not risk modeling. It does not account for sector, country,
  currency, volatility, or correlation.
- Bulk editing from Holdings is convenient but needs an accept/apply step to
  avoid accidental portfolio metadata changes.
