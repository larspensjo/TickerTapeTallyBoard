# Plan: Conviction-Based Rebalance Planner (V1)

Status: Proposed plan (ephemeral — durable semantics land in
`docs/Design.RebalancePlanner.md` in the final phase; never reference this
plan's phase numbers from durable documents or code).

## Goal

A preview-only Rebalance page: the user enters a SEK offset `C` (positive =
net buying, negative = net selling, 0 = pure rebalance), the backend returns a
precomputed "ladder" of whole-share trade plans for every granularity
`N = 1..K` (K = candidate count), and a live slider indexes into that ladder
client-side. Nothing is written to the ledger; real trades happen at the
broker and arrive later via the normal import flow.

Settled decisions from the design brief are restated here only where they
shape implementation; see the brief for the full list. Key ones: preview-only,
pool+offset target math, participants = exactly the Holdings conviction-target
pool, granularity = trade count, whole shares with explicit residual, no mode
toggle, brokerage/FX costs ignored in V1, `GET /api/rebalance` (works in
read-only demo mode), precomputed ladder for a live slider, per-candidate
price freshness carried into the response.

## References

- `docs/Design.ConvictionTargets.md` — pool eligibility, weights, target math.
- `backend/src/domain/conviction.rs` — `derive_targets`, `ConvictionLevel`,
  `MarketValueState` (one source of truth for pool eligibility).
- `backend/src/api/holdings.rs` — current pool assembly to be extracted and
  shared; integration-test seeding helpers pattern (`seed_valued`).
- `backend/src/api/valuation.rs` — `load_valuation_inputs`, `money_string`,
  freshness serialization (`fresh` / `minor_stale_N_days` /
  `warning_stale_N_days`).
- `backend/src/domain/valuation.rs` — `value_position`, `ValuedHolding`
  (price/FX snapshots each carry `DataFreshness`).
- `backend/src/api/mod.rs` — router wiring; demo guard rejects non-GET.
- `frontend/src/api/{types.ts,queries.ts}`, `frontend/src/App.tsx`,
  `frontend/src/components/appModeViewModel.ts` — client and nav wiring.
- `docs/VisualDesign.DarkTheme.md` — tokens; stale display = `--warning` chip.
- DecisionLog 2026-06-16 (staleness display rules), 2026-07-02 (demo mode),
  2026-07-06 (conviction relative weights).

## Algorithm specification (pinned; the domain contract)

This section resolves the two items the brief delegated to this plan: the
direction-aware selection rule (with tie-breaking) and the
clamp-and-redistribute iteration (with the ±1-share pass). These are design
commitments to be copied into `docs/Design.RebalancePlanner.md` in the docs
phase and pinned by unit tests.

### Inputs

One `RebalanceCandidate` per member of the Holdings conviction-target pool:

- `instrument_id`
- `weight` `w` ∈ {1, 2, 4} (from `ConvictionLevel::weight()`)
- `market_value_base` `v` > 0 (SEK, `Decimal`)
- `price_base` `p` > 0 (SEK per share = native close × FX rate, FX = 1 for SEK
  instruments; same snapshots as `v`, so `v = p · q_held` by construction)
- `held_quantity` `q_held` > 0 (`i64`)

Candidates are supplied in the Holdings ordering (instrument `(exchange,
symbol)` ascending); all tie-breaks below use this input order, making the
whole ladder deterministic. All money math is `rust_decimal::Decimal`.

Offset `C` (`Decimal` SEK). `P = Σv`, `W = Σw`, `K` = candidate count.

### Unavailable states (explicit, never silently clamped)

- `K = 0` → `empty_pool`.
- `C ≤ −P` → `offset_exceeds_pool` (the brief pins `≤`, so `C = −P`, full
  liquidation, is also reported infeasible rather than planned).

### Step 1 — Ideal deltas (shared by all rungs)

`P' = P + C`; `t_i = P' · w_i / W`; `d_i = t_i − v_i`. Identity: `Σd_i = C`
exactly (pinned by test).

### Step 2 — Direction-aware selection for rung N

1. Rank candidates by `|d_i|` descending; ties by input order.
2. Initial selection `S` = top N.
3. Repairs, applied in this order (each swap replaces the lowest-ranked
   eligible member of `S` with the highest-ranked eligible non-member, keeping
   `|S| = N`):
   - **Buy presence** (`C > 0`): if `S` has no candidate with `d > 0`, swap
     the lowest-ranked member for the highest-ranked non-selected candidate
     with `d > 0`. One always exists because `Σd = C > 0`.
   - **Sell presence** (`C < 0`): symmetric — ensure at least one `d < 0`
     member. One always exists because `Σd = C < 0`.
   - **Sell capacity** (`C < 0`): while `Σ_{i∈S, d_i<0} v_i < −C` and `S`
     contains a member with `d ≥ 0` and a non-selected candidate with `d < 0`
     exists: swap the lowest-ranked `d ≥ 0` member for the highest-ranked
     non-selected `d < 0` candidate. (Capacity may still fall short when all
     sells are selected; the residual then reports the shortfall.)
   - **Two-sidedness** (`C = 0`, `N ≥ 2`): if every selected nonzero delta has
     one sign, swap the lowest-ranked member for the highest-ranked
     non-selected opposite-sign candidate (exists because `Σd = 0` unless all
     deltas are zero; an all-zero-delta pool yields an honest empty plan with
     zero residual at every rung).
4. Buys need no capacity repair (unbounded above).

Repairs are the only deviation from strict top-N nesting, so slider movement
stays near-nested; correctness beats strict nesting (per the brief).

### Steps 3+4 — Conviction-weighted redistribution with iterative clamping

- Free set `F` = selected candidates with `d ≠ 0`. Selected candidates with
  `d = 0` are fixed at `x = 0` from the start (reported as `on_target`).
- Repeat (at most `|F|` passes):
  1. `R = C − Σ_fixed x_j`; `r = R − Σ_{i∈F} d_i`;
     `x_i = d_i + r · w_i / Σ_{i∈F} w_i`.
  2. Violations: a buy (`d_i > 0`) with `x_i < 0` → bound 0; a sell
     (`d_i < 0`) with `x_i > 0` → bound 0; a sell with `x_i < −v_i` → bound
     `−v_i` (no shorting).
  3. No violations → done; `Σx = C` exactly.
  4. Otherwise fix **all** current violators at their bounds, remove them from
     `F`, repeat. `F` empty → done with a shortfall (residual reports it).
- Termination: each pass removes at least one member from `F`.
- Clamps are permanent within a rung. This one-directional scheme is
  deterministic and constraint-respecting but not guaranteed to be the exact
  weighted least-squares optimum in multi-clamp corner cases; tests pin the
  actual behavior and the design doc documents the approximation.
- **All-zero fallback**: if the loop ends with every `x_i = 0` while some
  selected `d_i ≠ 0` (this arises at `C = 0, N = 1`), discard the
  redistribution and set `x_i = d_i` for all selected members (feasible by
  construction: `d_i ≥ −v_i` and matches its own sign). This implements the
  brief's "show the single trade with its large residual — the residual is
  the signal".

### Step 5 — Whole shares and the greedy ±1 pass

- `q_i = round(x_i / p_i)` with midpoint-away-from-zero rounding
  (`RoundingStrategy::MidpointAwayFromZero`), then constrained: `sign(q_i)` ∈
  {0, `sign(d_i)`}; for sells `|q_i| ≤ q_held`.
- Residual `ρ = C − Σ q_i · p_i`. Greedy loop:
  - When `ρ > 0` the allowed adjustments are `+1` share on a buy candidate
    (always, including reviving a rounded-to-zero buy) or `+1` on a sell
    candidate while `q_i < 0` (shrinking the sell; never crossing zero).
  - When `ρ < 0`: `−1` on a buy while `q_i > 0`, or `−1` on a sell while
    `|q_i| < q_held`.
  - Eligible adjustments are those with `p_i < 2·|ρ|` (strict decrease of
    `|ρ|`). Apply the cheapest-priced eligible one (ties by input order),
    recompute `ρ`, repeat until none is eligible.
  - Terminates because `|ρ|` strictly decreases each step; guard with an
    iteration cap of `ceil(|ρ_initial| / p_min) + K` plus a `debug_assert`.
- A selected candidate ending at `q_i = 0` is reported as untraded with a
  reason: `too_small` (nonzero `d` rounded/adjusted away), `clamped`
  (redistribution clamped `x` to zero), or `on_target` (`d = 0`).

### Step 6 — Per-rung report

- `selected_count` = N (**N-contract**: N counts selected candidates, not
  executed trades; clamping/rounding can make the executed count smaller, and
  adjacent rungs can produce identical trade lists).
- `trades` (nonzero `q` only): instrument, side (buy/sell), `shares = |q|`,
  `price_base`, `amount_base = |q|·p`.
- `untraded`: instrument + reason as above.
- `effective_trade_count = trades.len()`.
- `achieved_net_base = Σ signed q_i·p_i`; `residual_base = C − net`.
- `coverage_percent`: with `G = Σ_all |d_i|` and
  `G' = Σ_all |q_i·p_i − d_i|` (unselected candidates contribute `|d_i|`),
  coverage = `100·(G − G')/G`; `None` when `G = 0`. Coverage is
  non-decreasing in N for the pre-rounding solution under nested selection;
  rounding and repair swaps can cause small decreases — tests pin monotonicity
  on exact-friendly fixtures and the design doc records the exception.

### Worked fixture (reused across tests)

Pool from the conviction design note: A(Low, 100k), B(Medium, 300k),
C(High, 300k); `P = 700k`, deltas at `C = 0`: A `0`, B `−100k`, C `+100k`.

- `N = 1`: rank ties (B, C at 100k) break by input order → B (sell).
  Redistribution zeroes it → all-zero fallback → sell 100k of B; net `−100k`,
  residual `+100k`.
- `N = 2`: {B, C}; `r = 0` → `x = d`; near-zero residual after rounding;
  coverage ≈ 100%.
- `N = 3`: adds A with `d = 0` → untraded `on_target`; identical trade list to
  `N = 2` (pins the N-contract).

## API contract

`GET /api/rebalance?amount=<decimal>`

- Missing or unparsable `amount` → `400 invalid_amount`. Any finite decimal
  (including 0 and negatives) is accepted.
- `200` response:

```json
{
  "amount_base": "10000.00",
  "base_currency": "SEK",
  "plan": {
    "status": "available",
    "pool_value_base": "700000.00",
    "candidate_count": 3,
    "rungs": [
      {
        "selected_count": 2,
        "effective_trade_count": 2,
        "trades": [
          {
            "instrument": { "...": "InstrumentResponse fields" },
            "side": "buy",
            "shares": 12,
            "price_base": "512.30",
            "amount_base": "6147.60",
            "freshness": "fresh"
          }
        ],
        "untraded": [
          { "instrument": { "...": "..." }, "reason": "too_small" }
        ],
        "achieved_net_base": "9950.10",
        "residual_base": "49.90",
        "coverage_percent": "82.50"
      }
    ]
  }
}
```

- Unavailable plan (still `200`, data-dependent):
  `"plan": { "status": "unavailable", "reasons": ["empty_pool"] }` or
  `["offset_exceeds_pool"]`.
- `freshness` per trade is the **staler** of the price and FX tri-state
  signals (fresh < minor_stale < warning_stale), serialized with the existing
  labels so `freshnessLabel`/`freshnessTone` on the frontend work unchanged.
  Freshness is attached at the API layer; the domain module stays free of it.
- `coverage_percent` is `null` when the pool is perfectly balanced (`G = 0`).
- Response size is O(K²) trades across rungs — fine at this app's scale (tens
  of instruments); noted in the design doc.

## Phases

### Phase 1 — Pure domain ladder (`backend/src/domain/rebalance.rs`)

Scope:

- New pure module `backend/src/domain/rebalance.rs` implementing the
  specification above: `RebalanceCandidate`, `RebalanceLadder`,
  `RebalanceRung`, `PlannedTrade`, `UntradedCandidate` (+ reason enum),
  `RebalanceUnavailable`, and
  `build_ladder(candidates: &[RebalanceCandidate], offset: Decimal) ->
  Result<RebalanceLadder, RebalanceUnavailable>`.
  No I/O, no HTTP/serde types; `Decimal` throughout; re-export from
  `backend/src/domain/mod.rs` (thin wrapper only).
- Behavior-preserving refactor of `backend/src/domain/conviction.rs`: extract
  the eligibility predicate used inside `derive_targets` into a shared
  `pool_membership(conviction, market_value) -> Option<(weight, value)>`
  helper, exported for the API layer's candidate assembly — one source of
  truth for pool eligibility, no second pool definition. Existing conviction
  tests must pass unchanged.

Unit tests (pin the ladder's key properties):

- Step-1 identity: ideal deltas sum exactly to `C`.
- Determinism: identical inputs → identical ladder (run twice, compare).
- Direction repairs: `C > 0` with the largest `|d|` an overweight sell still
  yields a non-degenerate buy plan at `N = 1` (the brief's naive-top-N
  failure); `C < 0` symmetric; sell-capacity swaps at very negative `C`;
  `C = 0, N ≥ 2` two-sidedness swap.
- Clamp iteration: no sign flips vs ideal delta; sells capped at `−v` (no
  shorting) and `|q| ≤ q_held`; termination within `|F|` passes; exact net
  `= C` whenever an unclamped buy remains (for `C > 0`).
- All-zero fallback: the `C = 0, N = 1` worked fixture (single trade, large
  residual).
- Whole shares: midpoint-away-from-zero rounding; ±1 pass converges toward
  `C`, respects caps/signs, revives a zero buy only within constraints, and
  terminates; a one-share-too-expensive candidate reports `too_small`.
- N-contract: `effective_trade_count ≤ selected_count`; adjacent rungs may be
  identical (worked fixture `N = 2` vs `N = 3`); `on_target`/`clamped`
  untraded reasons.
- Coverage: definition on the worked fixture; non-decreasing in N on an
  exact-friendly fixture; `None` when `G = 0`.
- Residual: `residual = C − net` at every rung, including capped sell rungs.
- Unavailable states: `empty_pool`; `offset_exceeds_pool` at `C = −P` and
  below.

Verification (from `backend/`): `cargo build`, `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt`. No user-visible
change yet, so no version bump in this phase.

### Phase 2 — Shared pool assembly + `GET /api/rebalance`

Scope:

- Extract the pool-assembly/valuation path from `holdings::list` into a new
  `backend/src/api/valued_holdings.rs` (named for the domain concept, not the
  feature): `load_valued_open_holdings(pool, valuation_date) ->
  Result<Vec<ValuedOpenHolding>, ApiError>` where each item carries the
  instrument row, derived `Position`, parsed `ConvictionLevel`, and
  `Option<ValuedHolding>` (`None` = price mapping disabled). The path is
  fallible — DB reads, `row.to_ledger()?`, `derive_position(...)`,
  `load_valuation_inputs(...).await?`, and conviction parsing each surface as
  `ApiError` today — so the extracted function must return `Result<_, ApiError>`
  and preserve the existing error handling and messages verbatim; it must not
  swallow errors into an empty/partial vector. Rebuild `holdings::list` on it —
  behavior-preserving; every existing holdings integration test must pass
  unchanged (they guard the refactor). This is the proper shared solution,
  not copy-paste.
- New `backend/src/api/rebalance.rs`: parse `amount` (400 `invalid_amount`
  otherwise), assemble candidates from `load_valued_open_holdings` (propagating
  its `ApiError`) using the shared `pool_membership` helper, compute
  `price_base` = latest close × FX rate (1 for SEK) and combined freshness from
  the `ValuedHolding` snapshots, call `domain::rebalance::build_ladder`,
  serialize per the API contract (`money_string`, the shared freshness
  serializer below, `InstrumentResponse`). Log internal inconsistencies with
  instrument context via `engine_logging`.
- Freshness reuse (do not duplicate label strings): promote the currently
  private `serialize_freshness` in `backend/src/api/valuation.rs` to
  `pub(super)`/`pub(crate)` so `rebalance.rs` emits identical labels
  (`fresh` / `minor_stale_N_days` / `warning_stale_N_days`) as every other
  surface, and add one small shared helper beside it for the "staler of price
  and FX" ranking (`fresh < minor_stale < warning_stale`). Both the label
  mapping and the worst-of ranking live in one place rather than being
  re-derived as strings in the rebalance module.
- Route `/rebalance` under `get(...)` in `backend/src/api/mod.rs` (thin
  wiring only). Being GET, it passes the demo read-only layer by design.
- Bump backend version in `backend/Cargo.toml` (0.9.0 → 0.10.0): new API
  surface.

Integration tests (reuse the `seed_valued`-style helpers from
`holdings.rs`; consider moving shared seeding helpers alongside the new
module if duplication appears):

- Happy path: seeded Low/Medium/High pool, `amount=0` → rung count = K, the
  worked-fixture trades/net/residual/coverage; `amount>0` and `amount<0`
  ladders.
- `Other`, missing-price, mapping-disabled, and closed positions are absent
  from candidates (pool parity with Holdings); `candidate_count` matches the
  Holdings eligible pool.
- `400 invalid_amount` for missing/garbage `amount`.
- `empty_pool` and `offset_exceeds_pool` unavailable payloads.
- Freshness: seed an old-dated price → trade carries
  `warning_stale_N_days`; foreign instrument combines price/FX staleness
  (staler wins).
- Demo mode: `AppState::for_tests().with_demo_mode(true)` → GET `/api/rebalance`
  returns 200 (mirrors the demo guard test style in `api/mod.rs`).

Verification (from `backend/`): `cargo build`, `cargo test`,
`cargo clippy --all-targets -- -D warnings`, `cargo fmt`. Optional manual
check: `scripts/start.ps1 -Demo` and curl `/api/rebalance?amount=10000`.

### Phase 3 — Rebalance page: end-to-end slice (external human testing recommended)

Scope:

- `frontend/src/api/types.ts`: `RebalanceResponse`, `RebalancePlan`,
  `RebalanceRung`, `RebalanceTrade`, `RebalanceUntraded` types mirroring the
  contract.
- `frontend/src/api/queries.ts`: `useRebalancePlan(amount: string | null)` —
  `apiGet` keyed by the normalized amount, `enabled` only for a valid amount,
  `placeholderData: keepPreviousData` so the slider and table do not flash
  while a new amount loads.
- New route `/rebalance` in `frontend/src/App.tsx` nested under
  `PortfolioLayout` (shares the totals band; its mutating buttons already
  hide in demo mode), lazy-loaded like the sibling pages. Nav item
  `Rebalance` added unconditionally in `appModeViewModel.ts` (page is
  read-only, demo-safe); proposed placement directly after Holdings.
- New `frontend/src/components/RebalancePage.tsx` with the logic split per
  repo architecture:
  - `rebalancePageReducer` (exported, pure): amount input text, committed
    (debounced) amount, slider position per response (clamped to
    `1..rung count`, reset policy when a new ladder arrives).
  - `rebalanceViewModel.ts` (pure): amount validation/normalization (trim;
    accept comma decimal → normalize to dot; signed), selected-rung
    derivation, formatted trade rows (side, shares, price, amount), achieved
    net / residual / coverage display fields, effective-vs-selected trade
    count label, untraded rows, unavailable-state mapping.
  - Component consumes state and renders: amount input (debounce ~400 ms,
    immediate commit on Enter/blur — proposed, see Open Questions), slider
    (`input[type=range]`, step 1, min 1, max K), trade table (theme table
    density spec; numbers mono, right-aligned; Buy/Sell as neutral chips —
    the theme forbids semantic-colored type chips), summary strip (requested
    C, achieved net, residual, coverage).
- Basic empty/prompt/loading/unavailable states (full styling polish is the
  next phase).
- Bump frontend version in `frontend/package.json` (0.14.0 → 0.15.0).

Tests (Vitest, node env; prefer reducers/view-models over render details):

- Reducer: amount editing vs committed amount, slider clamping/reset on new
  ladder, unavailable transitions.
- View-model: amount validation (signed, comma, garbage), rung selection,
  trade-row formatting, residual/coverage derivation, untraded reasons,
  N-contract label ("N selected, M executed").

Verification (from `frontend/`): `npm run check`, `npm run fmt`. Run backend
(seeded or `-Demo`) + Vite and exercise the page.

**External human testing recommended here**: slider feel and step behavior,
debounce timing on the amount input, plan plausibility against the Holdings
target-gap column (same pool, `C = 0` rung K should mirror the signed gaps),
readability of net/residual/coverage.

### Phase 4 — Staleness surfacing, edge states, visual polish (external human testing recommended)

Scope:

- Per-trade freshness: reuse `freshnessLabel`/`freshnessTone` from
  `valuationDisplay.tsx`; minor-stale → minor icon, warning-stale →
  `--warning-soft` chip on the row (existing staleness display rules,
  DecisionLog 2026-06-16).
- Plan-level banner when any trade in the *selected rung* is warning-stale —
  this page proposes concrete broker orders, the app's highest-stakes
  staleness surface (proposed both-levels approach; see Open Questions).
- Explicit unavailable/edge states styled per the theme "States" section:
  empty pool, `offset_exceeds_pool` (message explains C ≤ −pool), no valid
  amount yet, all-untraded rung ("too small to trade at this granularity"
  messaging).
- Slider styling with theme tokens (`--accent` fill, `--surface-2` track,
  focus ring); coverage presented beside the slider so the marginal benefit
  of finer granularity is visible while dragging.
- View-model tests for staleness aggregation (worst-of-rung) and edge-state
  mapping; component test by role/text only for the warning banner if that
  behavior lives nowhere else.

Verification (from `frontend/`): `npm run check`, `npm run fmt`.

**External human testing recommended here**: visual review against
`docs/VisualDesign.DarkTheme.md` (tokens, chip usage, table density), a
deliberately stale-data scenario (stop price refresh, age the DB date or seed
old prices) to confirm the banner and per-trade chips, and demo-mode
walkthrough (`scripts/start.ps1 -Demo`).

### Phase 5 — Durable documentation

Scope (no runtime code):

- New `docs/Design.RebalancePlanner.md`: the algorithm specification from
  this plan (selection rule, clamp scheme, ±1 pass, N-contract, coverage
  definition and its rounding caveat, unavailable states, V1 exclusions and
  deferrals), named after the behavior — no plan phase references.
- `docs/DecisionLog.md`: append the entries drafted below (dated at commit).
- `docs/VisualDesign.DarkTheme.md`: short "Slider" component note (track,
  fill, thumb, focus tokens) since the doc is the single source of truth for
  component appearance.
- `docs/Design.ConvictionTargets.md`: one-line cross-reference from "Future
  Planning Mechanics" to the new design note (no rewrite of the accepted
  note).
- Delete this plan file after external review, per repo convention.

Verification: docs review; confirm no durable document references plan
phases.

## Version bumps

- `backend/Cargo.toml`: 0.9.0 → 0.10.0 (new `/api/rebalance` surface), in the
  API phase.
- `frontend/package.json`: 0.14.0 → 0.15.0 (new page + nav), in the first
  frontend phase.

## Proposed DecisionLog entries (drafts)

```
## 2026-07-XX - Conviction rebalance planner is preview-only and pool-scoped
Decision: The Rebalance page is a preview-only planner. GET /api/rebalance
computes a full trade-count ladder from exactly the Holdings conviction-target
pool (open Low/Medium/High holdings with usable valuations; Other-conviction
and closed positions are excluded in V1), recomputing targets against the
current pool value plus the user-supplied SEK offset. No ledger writes; real
trades execute at the broker and arrive via import. Brokerage and
currency-exchange costs are ignored in V1 (deferred). The API is GET-shaped so
the planner works in read-only demo mode.
Context: First use of the planning mechanics anticipated by the conviction-
targets design. Including closed convicted positions would make the planner's
pool diverge from Holdings, showing contradictory targets across pages.
Consequences: The planner and Holdings must keep sharing one pool-eligibility
and target-derivation source of truth. Closed-position buy candidates, cost
modeling, and a candidate roster/exclusion panel are separate future features.

## 2026-07-XX - Rebalance ladder semantics: selected-candidate granularity
Decision: Rebalance granularity N counts selected candidates, not executed
trades: clamping (no sign flips, no shorting) and whole-share rounding may
produce fewer non-zero trades, and adjacent rungs may be identical. The
remainder is redistributed across selected trades proportionally to conviction
weight; quantities are whole shares; the requested offset is never enforced —
the achieved net and residual are reported explicitly, and a coverage metric
shows how much of the total absolute target gap each rung closes. Offsets at
or below the negated pool value and an empty pool are explicit unavailable
states.
Context: Trade-count granularity gives predictable slider steps; a
minimum-trade-size threshold and buy/sell mode toggles were considered and
rejected.
Consequences: UI surfaces must present residual and effective trade count
honestly rather than implying exact execution. See
docs/Design.RebalancePlanner.md for the full heuristic.
```

## Documents to update (summary)

- New: `docs/Design.RebalancePlanner.md`.
- Append: `docs/DecisionLog.md` (two entries above).
- Amend: `docs/VisualDesign.DarkTheme.md` (slider note),
  `docs/Design.ConvictionTargets.md` (cross-reference line).

## Open questions (to confirm before or during the frontend phases)

The algorithm-level questions the brief left open (selection rule,
tie-breaking, clamp iteration, ±1 pass) are resolved by the specification
above. Still genuinely open, with proposed defaults:

1. **Amount-input debounce/UX** — proposed: ~400 ms debounce plus immediate
   commit on Enter/blur; the query uses `keepPreviousData` so the table never
   blanks. Confirm the timing and whether typing should show a subtle
   "updating…" hint.
2. **Slider default position when a ladder loads** — proposed: `N = K`
   (finest granularity, closest to ideal targets), preserved across amount
   changes when the rung count is unchanged, clamped otherwise. Alternative:
   remember last position in `localStorage` like other view preferences.
3. **Slider labeling and count presentation** — proposed: label "Trades" with
   the selected N, plus an adjacent "M executed of N selected" note when they
   differ (the N-contract made visible). Confirm wording.
4. **Coverage and residual presentation** — proposed: a summary strip
   (requested / achieved net / residual) above the table and a coverage
   percent beside the slider. Confirm whether coverage also belongs in a
   per-rung tooltip on the slider.
5. **Stale-price flagging placement** — proposed: both per-trade (minor icon /
   warning chip per the 2026-06-16 display rules) and a page-level warning
   banner when the selected rung contains any warning-stale trade. Confirm
   the both-levels approach.
6. **Nav placement** — proposed: "Rebalance" directly after "Holdings"
   (planner is holdings-adjacent). Cosmetic; confirm.
