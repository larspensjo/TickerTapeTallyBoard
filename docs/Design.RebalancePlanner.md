# Rebalance Planner

**Status:** Accepted design. This document is the durable specification for the
preview-only rebalance ladder returned by `GET /api/rebalance`.

The planner consumes the conviction-target pool already assembled by Holdings.
It does not write to the ledger, it does not model brokerage or FX costs, and
it reports whole-share trade previews plus explicit reasons when a selected
candidate does not trade.

All money math uses `Decimal`.

## Inputs

The domain input is one `RebalanceCandidate` per member of the Holdings
conviction-target pool:

- `instrument_id`
- `weight` `w` in `{1, 2, 4}` from conviction level
- `market_value_base` `v` in SEK, `Decimal`, strictly positive
- `price_base` `p` in SEK per share, `Decimal`, strictly positive
- `held_quantity` `q_held`, `i64`, strictly positive

Candidates are already filtered by the shared conviction-target pool rule: only
Low/Medium/High convictions with an available, strictly positive market value
enter the planner. The input order is the Holdings order `(exchange, symbol)`
ascending, and every tie-break in this planner uses that order.

The planner also takes the user offset `C` in SEK as a `Decimal`. Let:

- `P = Σ v`
- `W = Σ w`
- `K = candidate count`

## Unavailable States

These states are explicit and are never silently clamped:

- `K = 0` -> `empty_pool`
- `C <= -P` -> `offset_exceeds_pool`

The offset check treats `C = -P` as infeasible, so full liquidation is reported
as unavailable rather than planned.

## Ideal Targets And Deltas

Define `P' = P + C`.

Each candidate's ideal target is proportional to conviction weight:

- `t_i = P' * w_i / W`
- `d_i = t_i - v_i`

The deltas sum exactly to the requested offset: `Σ d_i = C`.

Decimal division can leave a tiny remainder, so the last candidate's target is
computed as `P' - Σ(other targets)`. The last candidate absorbs the remainder
so the delta sum stays exact.

## Direction-Aware Selection

For rung `N`, the planner first ranks candidates by `|d_i|` descending, then by
input order.

1. Select the top `N`.
2. Apply repair rules in this order, using the same ranked order for swaps.
   Each swap replaces the lowest-ranked eligible selected candidate with the
   highest-ranked eligible non-selected candidate.

Repair rules:

- **Buy presence** for `C > 0`: if the selected set has no `d_i > 0`, swap in
  the highest-ranked non-selected positive-delta candidate. A buy plan is always
  possible because positive offset is unbounded above.
- **Sell presence** for `C < 0`: symmetric. Ensure at least one selected
  `d_i < 0`.
- **Sell capacity** for `C < 0`: while the selected sell capacity
  `Σ(v_i where d_i < 0)` is still less than `-C`, and there is a selected
  `d_i >= 0` plus a non-selected `d_i < 0`, swap the lowest-ranked selected
  non-negative-delta candidate for the highest-ranked non-selected
  negative-delta candidate. If all sells are already selected, the residual
  later reports the shortfall.
- **Two-sidedness** for `C = 0` and `N >= 2`: if every selected nonzero delta
  has the same sign, swap in the opposite sign. If all selected deltas are
  zero, the rung stays an honest zero plan with zero residual.

These repairs are the only deviation from strict top-N nesting. They keep the
slider close to nested while preserving sign coverage and sell capacity.

## Conviction-Weighted Redistribution

Within a rung, redistribution works on the selected set:

- Selected candidates with `d_i = 0` are fixed at `x_i = 0` from the start and
  are reported as `on_target`.
- Selected candidates with `d_i != 0` start free.

The planner then repeats up to `|F|` passes, where `F` is the free set:

1. Compute the fixed contribution and the remaining residual:
   `R = C - Σ_fixed x_j`
   `r = R - Σ_{i in F} d_i`
2. Redistribute the remaining amount by conviction weight:
   `x_i = d_i + r * w_i / Σ_{i in F} w_i`
3. Clamp violations:
   - buy candidate (`d_i > 0`) with `x_i < 0` -> clamp to `0`
   - sell candidate (`d_i < 0`) with `x_i > 0` -> clamp to `0`
   - sell candidate (`d_i < 0`) with `x_i < -v_i` -> clamp to `-v_i`
4. If there were no violations, stop.
5. Otherwise, permanently fix all violators at their bounds, remove them from
   the free set, and repeat.

This scheme is deterministic and constraint-respecting. It is documented as an
approximation: in multi-clamp corner cases it is not guaranteed to match the
exact weighted least-squares optimum.

### All-Zero Fallback

If redistribution ends with every selected `x_i = 0` while some selected
`d_i != 0`, the planner discards the redistribution and restores
`x_i = d_i` for every selected nonzero-delta candidate.

This fallback is structural, not rounding noise. It is the behavior used for
the `C = 0, N = 1` rung where the single selected trade is the signal.

Fallback rungs also clear any stale `clamped` flags from the discarded
redistribution so the untraded reasons stay honest. They skip the greedy
plus/minus 1 residual pass entirely.

## Whole Shares And Residual Repair

Selected `x_i` values are converted to whole shares by rounding
`x_i / p_i` to an integer with midpoint-away-from-zero rounding.

After rounding:

- the sign of the share count must still agree with the delta sign, otherwise
  it is forced back to `0`
- sell quantities are capped at `q_held`

The residual after rounding is:

`ρ = C - Σ(q_i * p_i)`

The planner then runs a greedy plus/minus 1 pass that repeatedly reduces `|ρ|`:

- if `ρ > 0`, allowed adjustments are:
  - `+1` share on a buy candidate, including a buy rounded to zero
  - `+1` share on a sell candidate while `q_i < 0` so the sell shrinks toward
    zero
- if `ρ < 0`, allowed adjustments are:
  - `-1` share on a buy candidate while `q_i > 0`
  - `-1` share on a sell candidate while `|q_i| < q_held`
- only candidates with `p_i < 2 * |ρ|` are eligible, because that guarantees a
  strict decrease in `|ρ|`
- among eligible candidates, pick the cheapest price; ties break by input order

The pass terminates because each step strictly reduces the residual magnitude.
The implementation still guards the loop with an iteration cap.

## N-Contract

`selected_count` is the number of selected candidates `N`, not the number of
executed trades.

Clamping and whole-share rounding can reduce the executed trade count, so:

- `effective_trade_count <= selected_count`
- adjacent rungs may produce identical trade lists

The selected count is still reported even when a rung collapses to fewer or no
executed trades.

## Coverage

Coverage measures how much of the total absolute target gap the rung closes.

Let:

- `G = Σ_all |d_i|`
- `a_i = q_i * p_i` for selected candidates, `0` for unselected candidates
- `G' = Σ_all |a_i - d_i|`

Then:

- `coverage = 100 * (G - G') / G`
- `coverage = None` when `G = 0`

Coverage is non-decreasing in `N` for the exact pre-rounding solution under
nested selection, but rounding and repair swaps can cause small decreases in
the shipped rung output. The tests pin the intended behavior on exact-friendly
fixtures and this monotonicity caveat is documented here.

## Per-Rung Report

Each rung reports:

- `selected_count`
- `trades` for nonzero share quantities only
- `untraded` for selected candidates that ended at zero
- `effective_trade_count`
- `achieved_net_base`
- `residual_base`
- `coverage_percent`

Trade fields are:

- `instrument`
- `side`
- `shares`
- `price_base`
- `amount_base`

Untraded reasons are:

- `too_small`
- `clamped`
- `on_target`

The selected count reflects the rung selection, not the number of executable
orders. `residual_base = C - achieved_net_base` at every rung.

## Per-Rung Balance Report

Each rung also reports one balance entry per pool candidate, in input order,
covering traded, selected-untraded, and unselected candidates alike:

- `gap_before_base` / `gap_after_base`
- `gap_before_percent` / `gap_after_percent`
- `status_before` / `status_after`

Balance gaps use the display sign convention shared with Holdings targets:
`gap = value − target`, positive above target. This is the negation of the
planner's internal delta `d = target − value`.

Both gaps are measured against the post-offset targets `t_i = P' * w_i / W`.
With a zero offset these equal the Holdings targets; with a nonzero offset
they deliberately differ, because the plan aims at the post-offset pool.

`gap_after_base = gap_before_base + q_i * p_i`. Invariants:

- `Σ gap_after_base = -residual_base` at every rung
- `total_gap_before_base = Σ|gap_before| = G`
- `total_gap_after_base = Σ|gap_after| = G′`, so coverage remains
  `100 * (G - G′) / G`

Statuses reuse the shared ±5% display tolerance band from conviction targets.
Percent fields are `None`/`null` only if a candidate's target is not strictly
positive, which the planner's feasibility checks make unreachable in practice.

The rung totals `total_gap_before_base` and `total_gap_after_base` are
serialized as money strings; per-candidate percents as two-decimal strings.

## API Contract

`GET /api/rebalance?amount=<decimal>`

- Missing or unparsable `amount` -> `400 invalid_amount`
- Any finite decimal is accepted, including `0` and negative values
- `200` responses are either:
  - `plan.status = available`
  - `plan.status = unavailable`

Available response shape:

- `amount_base`
- `base_currency`
- `plan.pool_value_base`
- `plan.candidate_count`
- `plan.rungs[]`

Unavailable response shape:

- `plan.reasons[]` with `empty_pool` or `offset_exceeds_pool`

API-layer trade fields add freshness and instrument serialization:

- freshness is the staler of the price and FX freshness states
- the serialized freshness labels remain `fresh`, `minor_stale_N_days`, and
  `warning_stale_N_days`
- freshness is attached at the API layer; the domain module stays freshness-free

Response values such as `amount_base`, `pool_value_base`, `achieved_net_base`,
and `residual_base` are serialized as money strings. `coverage_percent` is a
string with two decimals or `null`.

The total response size is `O(K^2)` across rungs, because there are `K` rungs
and each rung can include up to `K` trades. That is acceptable at this app's
scale, where the pool is expected to stay in the tens of instruments.

Because the plan must absorb the requested offset into the selected rung, a
small-N rung can concentrate the full offset into only a few traded names and
push them across their own target bands. The balance report exists to make
those overshoots visible rather than hiding them inside the aggregate coverage
number.
