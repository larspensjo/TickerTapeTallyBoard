# Plan: User-Selectable Rebalance Ranking (Amount/SEK vs Relative/percent)

**Status:** Draft for review. Ephemeral planning document — durable behavior is
recorded in `docs/Design.RebalancePlanner.md` and `docs/DecisionLog.md`, never
by reference to this plan's phase numbers.

## Goal

The rebalance planner ranks trade candidates by absolute SEK gap `|d_i|`
(`ranked_indices` in `backend/src/domain/rebalance.rs`). This structurally
buries buy-side low-conviction and newly added (`v = 0`) instruments: a newly
added Low-conviction watchlist name (real case: "Corning") only appears at
slider position `N = 4`, never at `N <= 3`, undercutting the watchlist-for-
rebalance feature (DecisionLog 2026-07-08).

Add a user-facing toggle on the Rebalance page selecting between two ranking
modes:

- **Amount (SEK)** — today's behavior: rank by `|d_i|` in SEK. Default,
  backward compatible.
- **Relative (%)** — rank by `|d_i| / t_i` (gap as a fraction of the
  post-offset target). Scale-invariant across conviction tiers, so a `v = 0`
  name (always 100% below target, the buy ceiling) surfaces immediately.

Done means: the toggle switches the ladder between the two rankings; the
ladder refetches and re-ranks; the Corning case surfaces at `N = 1` under
Relative; SEK stays the default; all gates green.

## Settled design (from the design brief — fold in, do not reopen)

1. **`RankBy` enum** `{ Sek, Percent }` in `backend/src/domain/rebalance.rs`,
   re-exported via `backend/src/domain/mod.rs`. Threaded through
   `build_ladder` (new parameter) into `build_rung`,
   `select_indices_for_rung`, `ranked_indices`, `redistribute_selected`, and
   `resolve_quantities`.
2. **Targets threaded out of `ideal_deltas`.** Percent ranking and clamping
   need `t_i`, which `ideal_deltas` already computes and currently discards.
   Return targets alongside deltas instead of recomputing.
3. **Ranking metric.** Sek: unchanged (`|d_i|` desc, then input order).
   Percent: `|d_i| / t_i` desc; tie-break conviction `weight` desc, then
   `|d_i|` SEK desc, then input order. `t_i <= 0` (unreachable in practice)
   sorts last, never divides.
4. **Repairs run on the selected mode's ranked order.** Buy/sell presence,
   sell capacity, and two-sidedness swaps use the percent-ranked vector in
   percent mode.
5. **Percent mode = converge toward targets, never crossing them.** Percent
   mode changes redistribution and quantity resolution, not just ranking:
   buys clamp to `[0, d_i]`, sells clamp to `[d_i, 0]` (sell down to target,
   which dominates the sell-to-zero clamp since `d_i >= -v_i`); unabsorbed
   residual is reported honestly; sell capacity in repair is `-d_i` instead of
   `v_i`; quantity resolution enforces strict no-crossing (round toward zero
   at the target boundary; greedy ±1 pass uses post-adjustment guards
   `(q+1)*p <= d` for buys and `(q-1)*p >= d` for sells), so a name that would
   overshoot with a single share stays untraded (`too_small`). Sek mode
   redistribution/quantities are UNCHANGED.
6. **Coverage.** Percent mode stays exactly within `[0, 100]` (no-crossing
   makes the bound exact, not just continuous); the monotonicity assertion
   stays Sek-only; document percent-mode coverage in the durable spec.
7. **API.** `GET /api/rebalance?amount=<decimal>&rank_by=sek|percent`; absent
   → `sek`; unrecognized → `400 invalid_rank` mirroring `invalid_amount`.
8. **Frontend.** `useRebalancePlan(amount, rankBy)` with `rankBy` in query key
   and URL; `rankBy` in `RebalancePageState` with a new action; a segmented
   toggle labeled **Amount** / **Relative** in the Trades/slider header;
   persisted in the existing localStorage blob; slider position preserved
   across a toggle; persistence validation defaults missing/invalid `rankBy`
   to `sek` WITHOUT discarding the committed amount or slider position
   (2026-07-06 Holdings-sort precedent: validate stored state against known
   values, fall back per-field).

## Affected files

- Backend: `backend/src/domain/rebalance.rs`, `backend/src/domain/mod.rs`,
  `backend/src/api/rebalance.rs`, `backend/Cargo.toml` (version).
- Frontend: `frontend/src/api/types.ts`, `frontend/src/api/queries.ts`,
  `frontend/src/components/RebalancePage.tsx`,
  `frontend/src/components/rebalanceViewModel.ts` (toggle option list),
  `frontend/src/components/RebalancePage.reducer.test.ts`,
  `frontend/src/components/RebalancePage.test.tsx`,
  `frontend/src/components/rebalanceViewModel.test.ts`,
  `frontend/package.json` (version).
- Docs: `docs/Design.RebalancePlanner.md`, `docs/DecisionLog.md`.

---

## Phase 1 — Domain: RankBy ranking, percent-mode target clamps

All work in `backend/src/domain/rebalance.rs` plus the re-export in
`backend/src/domain/mod.rs`. Pure, no API/DB changes; `build_ladder` gains a
parameter and the API caller is updated mechanically (`RankBy::Sek`) so the
crate still builds — the query parameter itself lands in the next phase.

### 1a. Types and target threading

- Add `pub enum RankBy { Sek, Percent }` (`Clone, Copy, Debug, PartialEq,
  Eq`). Re-export `RankBy` from `domain/mod.rs`.
- Change `ideal_deltas` to also return the per-candidate targets it already
  computes (e.g. return a small struct such as
  `IdealAllocation { pool_value_base: Decimal, targets: Vec<Decimal>, deltas: Vec<Decimal> }`,
  or targets-and-deltas as parallel vectors — implementer's choice, but one
  computation, no recomputation downstream). The final-candidate
  remainder-absorption behavior is unchanged; `Σ d_i = C` must still hold
  exactly.
- `build_rung` currently reconstructs `target = market_value_base + delta`
  for the balance report; switch it to the threaded target (one source of
  truth, identical value by construction).
- The per-rung pipeline now threads `candidates`, `deltas`, `targets`,
  `offset`, and `rank_by` through four functions. To keep signatures readable
  (and clippy's `too_many_arguments` quiet), prefer a small borrowed context
  struct (e.g. `RungContext<'a> { candidates, deltas, targets, offset,
  rank_by }`) over ever-longer positional argument lists. This is an internal
  refactor; public domain types (`RebalanceRung`, `RebalanceLadder`, etc.) are
  unchanged.

### 1b. Ranking

Extend `ranked_indices` to take the mode (plus targets and candidate weights):

- `RankBy::Sek`: exactly today's comparator — `|d_i|` desc, then index asc.
  Byte-for-byte identical ordering to current behavior.
- `RankBy::Percent`: primary key `|d_i| / t_i` desc for `t_i > 0`; any
  candidate with `t_i <= 0` sorts after all `t_i > 0` candidates (defensive —
  feasibility checks make it unreachable; the division is never evaluated for
  such a candidate). Ties (notably all `v = 0` candidates at exactly 100%)
  break by conviction `weight` desc, then `|d_i|` SEK desc, then index asc.

Implementation note: compare ratios without constructing NaN-prone values —
either compute `Decimal` ratios only for `t_i > 0` (safe), or compare
cross-multiplied `|d_l| * t_r` vs `|d_r| * t_l` to avoid division entirely;
either is acceptable, cross-multiplication avoids any precision question at
the comparator level.

### 1c. Repairs over the mode's ranked order

`select_indices_for_rung` uses the single `ranked` vector for top-N selection
and every repair swap (`lowest_ranked_selected` / `highest_ranked_nonselected`).
Passing the mode-ranked vector through changes which candidates are evicted or
admitted in percent mode — that is intended and gets its own test.

Sell-capacity repair: the capacity sum in the while-loop condition becomes
mode-dependent:

- Sek: `Σ (v_i where d_i < 0)` — unchanged.
- Percent: `Σ (-d_i where d_i < 0)` (sell down to target only).

### 1d. Percent-mode redistribution clamps

In `redistribute_selected`, the violation checks become mode-dependent. Both
modes keep: buy (`d > 0`) with `x < 0` → clamp to `0` (sets
`clamped_to_zero`); sell (`d < 0`) with `x > 0` → clamp to `0` (sets
`clamped_to_zero`).

- Sek additionally keeps: sell with `x < -v` → clamp to `-v` (sell to zero
  value). Buys stay unbounded above. Unchanged.
- Percent instead clamps at the target bound:
  - buy (`d > 0`) with `x > d` → fix at `x = d` (violator, becomes fixed; NOT
    `clamped_to_zero` — a target-capped candidate still trades and must not
    report a `Clamped` untraded reason).
  - sell (`d < 0`) with `x < d` → fix at `x = d`. This dominates the `-v`
    clamp because `t_i >= 0` implies `d_i >= -v_i`, so the sell-to-zero check
    is not needed in percent mode.

The clamp-and-refix loop is otherwise unchanged: violators are fixed at their
bounds, removed from the free set, residual redistributed among the remaining
free candidates; termination argument (`passes <= selected_free_count`)
holds identically. When every selected candidate is fixed at its target cap
and residual remains, the residual is simply reported (achieved net <
requested offset) — this intentionally replaces "a single selected buy at
N = 1 absorbs the entire offset" in percent mode; the user slides to more
names to absorb more.

All-zero fallback: restoring `x_i = d_i` equals the percent-mode target cap,
so the fallback path needs no mode branch and stays consistent in both modes.

### 1e. Percent-mode quantity resolution — strict no-crossing

`resolve_quantities` changes only in percent mode, and percent mode enforces
that no trade ever crosses its target (not even by one share):

- Initial whole-share rounding is clamped **toward zero** at the target
  boundary in percent mode (round `x_i / p_i` toward zero, not
  midpoint-away-from-zero), so a name never *starts* past its target. Because
  `x_i` is already within `[0, d_i]` / `[d_i, 0]`, round-toward-zero keeps
  `q_i * p_i` inside the target gap by construction. Sek mode keeps
  midpoint-away-from-zero rounding unchanged.
- Greedy ±1 pass uses **post-adjustment** guards so an added share never
  results in crossing the target:
  - `+1` share on a buy candidate is allowed only while
    `(q_i + 1) * p_i <= d_i` (the resulting traded amount stays at or under
    the target gap);
  - `-1` share on a sell candidate is allowed only while
    `(q_i - 1) * p_i >= d_i` (the resulting sell amount stays at or above the
    target gap, i.e. no deeper than target), in addition to the existing
    `|q_i| < q_held` cap (keep both: the target cap is the no-crossing
    invariant, the held-quantity cap is the hard share-count invariant).
  - Adjustments that move a candidate *toward* its target (shrinking a buy,
    shrinking a sell toward zero) stay allowed as today.
  Sek mode keeps today's conditions untouched.
- Consequence: a tiny-target name whose single share would overshoot the
  target simply cannot trade — it lands at zero shares and reports the honest
  `too_small` untraded reason. This is intended.
- The all-zero-fallback path (`skip_greedy_pass`) is unchanged.

Because percent-mode trades never cross a target, `|a_i − d_i| <= |d_i|` holds
exactly for every candidate (not just the continuous solution), so `G' <= G`
and percent-mode coverage is **exactly** within `[0, 100]` — no rounding
caveat.

### 1f. Coverage

- No formula change. Keep the existing test
  `coverage_matches_the_definition_and_is_none_for_balanced_pools` as-is: its
  monotonicity assertion (`coverages.windows(2).all(...)`) exercises Sek mode
  and stays there; do NOT add a percent monotonicity assertion. Add a
  percent-mode test asserting coverage is within `[0, 100]` at every rung.
  Because strict no-crossing (1e) makes the bound exact, the assertion does
  not need exact-friendly (whole-share-divisible) fixtures — any valid
  percent-mode ladder must satisfy `0 <= coverage <= 100` at every rung.

### 1g. Mechanical caller update

`backend/src/api/rebalance.rs` `handler` passes `RankBy::Sek` to
`build_ladder` so this phase compiles and every existing API test passes
unchanged.

### Phase 1 tests (domain, `#[cfg(test)]` in rebalance.rs)

- **Corning case:** a fixture with established holdings plus one `v = 0`
  Low-conviction candidate whose SEK `|d|` ranks last, with `C > 0`. Assert:
  Sek ranks it last (absent from rung 1..K-1 trades / selection), Percent
  surfaces it at `N = 1` (rung 1 contains a buy for it).
- **Percent tie-break:** two `v = 0` candidates (both at exactly 100%): the
  higher-conviction one ranks first; with equal conviction, larger SEK `|d|`
  first; with both equal, input order.
- **Percent buy clamp:** single-name `N = 1` rung in percent mode buys only
  up to target; `achieved_net_base < offset` and `residual_base` reports the
  remainder; the same fixture in Sek mode absorbs the full offset at `N = 1`
  (regression pin on the unchanged Sek behavior).
- **Percent sell clamp:** a sell stops at its target (`x = d_i`), not at zero
  value; `gap_after` for that candidate is on-target rather than flipped.
- **Repairs on percent order:** a `C < 0` (or presence-repair) fixture where
  the percent-ranked order picks a different swap victim/entrant than the SEK
  order would; assert the percent-mode selection differs accordingly.
- **Percent sell capacity:** repair loop keeps swapping while
  `Σ(-d_i) < -C` even when `Σ v_i` would have sufficed under the old rule.
- **Greedy pass never crosses target:** a percent-mode fixture where the SEK
  greedy pass would add a share past a target; assert it does not — the name
  either stays at its capped share count or, if a single share would
  overshoot, lands at zero shares with the `too_small` untraded reason, and
  the residual is reported honestly instead.
- **Round-toward-zero at the boundary:** a percent-mode fixture where
  midpoint-away rounding would push a name one share past target; assert
  round-toward-zero keeps it at or under target (no crossing).
- **Percent coverage bounds:** coverage within `[0, 100]` at every rung; the
  no-crossing guarantee makes this exact, so the fixture need not be
  whole-share-divisible.
- **Determinism:** `identical_inputs_produce_identical_ladders` extended (or
  duplicated) for `RankBy::Percent`.
- **All existing Sek tests unchanged and green,** including the monotonicity
  assertion.

### Phase 1 verification

From `backend/`:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

No behavior change is observable through the API yet (handler pins
`RankBy::Sek`). No human testing needed.

---

## Phase 2 — API: `rank_by` query parameter

All work in `backend/src/api/rebalance.rs`. Bump `backend/Cargo.toml`
`version` (0.12.0 → 0.13.0; API surface change, visible via `/api/health`).

- Add `rank_by: Option<String>` to `RebalanceQuery`.
- Add `parse_rank_by(rank_by: Option<&str>) -> Result<RankBy, ApiError>`
  mirroring `parse_amount`: `None` → `RankBy::Sek`; `"sek"` → `Sek`;
  `"percent"` → `Percent`; anything else — including `SEK`, `Percent`, and
  empty string —
  `ApiError::bad_request("invalid_rank", "rank_by must be sek or percent")`.
  Matching is **case-sensitive, lowercase-only**, consistent with the Gains
  `method` param precedent and the existing `invalid_amount` pattern. (Whether
  to trim surrounding whitespace as `parse_amount` does is an implementer
  detail; the accepted values themselves are exactly `sek` and `percent`.)
- Thread the parsed mode into `build_ladder`.
- No response-shape change: the client always knows which mode it requested,
  and the query key carries it. `rank_by` is deliberately NOT echoed in the
  response, keeping the shape backward compatible.
- No new logging is required — the existing error path and the
  `log_internal_inconsistency` context remain sufficient; `ApiError`
  bad-request responses are client errors, matching how `invalid_amount` is
  handled today.

### Phase 2 tests (API, `#[cfg(test)]` in api/rebalance.rs)

- `rank_by=percent` returns a percent-ranked ladder — with an explicitly
  **Corning-like** fixture, NOT the existing
  `rebalance_marks_open_and_watchlist_candidates_and_proposes_a_watchlist_buy`
  fixture (whose High-conviction zero-value name already has the largest SEK
  gap, so SEK mode would rank it first too, and the test would not distinguish
  the modes). Seed larger established holdings near their targets plus one
  **Low-conviction, zero-value watchlist** name whose small conviction weight
  gives it a small target — hence a small SEK `|d|` that ranks last under
  Amount, but a 100% relative gap that ranks first under Relative. Concretely:
  two or more larger open holdings (higher conviction, market value close to
  their post-offset targets so their SEK gaps stay modest but still exceed the
  small name's SEK gap) and one `Low` watchlist name with `quantity = 0`,
  `market_value = 0`, a positive price, and the smallest conviction weight,
  under a positive `amount`. Assert **both** directions:
  - `rank_by=sek`: rung 1 does NOT contain a trade for the watchlist name.
  - `rank_by=percent`: rung 1 DOES contain a buy for the watchlist name
    (`is_new = true`).
- Absent `rank_by` behaves as `sek`: response for `?amount=X` equals the
  response for `?amount=X&rank_by=sek` (compare the JSON bodies).
- `rank_by=bogus`, `rank_by=SEK` (case-sensitivity pin), and `rank_by=` (empty)
  each → `400` with `error.code == "invalid_rank"`.
- Existing tests stay green unchanged.

### Phase 2 verification

From `backend/`:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Optional manual check: run the backend and hit
`/api/rebalance?amount=10000&rank_by=percent` in a browser; not a substitute
for the UI check in the next phase.

---

## Phase 3 — Frontend: toggle, state, persistence

Bump `frontend/package.json` `version` (0.18.1 → 0.19.0; user-visible
feature).

### 3a. Shared type (one source of truth)

In `frontend/src/api/types.ts`:

```ts
export const REBALANCE_RANK_BY_VALUES = ["sek", "percent"] as const;
export type RebalanceRankBy = (typeof REBALANCE_RANK_BY_VALUES)[number];
```

The wire values live here once; the UI labels (**Amount** / **Relative**)
live once in the view-model option list (3d). No component re-declares
either.

### 3b. Query hook

`frontend/src/api/queries.ts`:

```ts
export function useRebalancePlan(amount: string | null, rankBy: RebalanceRankBy)
```

- Query key: `["rebalance", normalizedAmount, rankBy]` (so the existing
  broad `["rebalance"]` invalidations keep matching both modes).
- URL: `/api/rebalance?amount=...&rank_by=${rankBy}`.
- `placeholderData: keepPreviousData` stays: toggling briefly renders the
  previous ranking's ladder under the preserved slider position while the
  refetch runs — accepted UX; the "Updating..." chip already covers it.

### 3c. Page state, reducer, persistence

`frontend/src/components/RebalancePage.tsx`:

- `RebalancePageState` gains `rankBy: RebalanceRankBy`
  (`initialState.rankBy = "sek"`).
- New action `{ type: "rankByChanged"; rankBy: RebalanceRankBy }`. The
  reducer updates `rankBy` only; `sliderPosition`, `committedAmount`,
  `amountInput`, and `lastAvailableRungCount` are preserved (same pool → same
  rung count, so no clamp is expected; the existing `planChanged` clamping
  still protects against a pool change racing the toggle).
- `PersistedRebalancePageState` gains `rankBy`.
  `loadRebalancePageState` treats `rankBy` as an independently-validated
  field: a value found in `REBALANCE_RANK_BY_VALUES` is restored, anything
  missing or unrecognized defaults to `"sek"` **without** discarding a valid
  `committedAmount`/`sliderPosition` (an old blob without `rankBy` must
  restore amount and slider exactly as before). The existing all-or-nothing
  fallback to `initialState` remains only for invalid amount/slider — do not
  extend it to `rankBy`. `saveRebalancePageState` writes `rankBy` alongside
  the existing fields under the same `rebalance-page-state` key.
- The component calls `useRebalancePlan(state.committedAmount, state.rankBy)`.

### 3d. Toggle UI and view model

- Add a small exported option list in
  `frontend/src/components/rebalanceViewModel.ts` so labels stay out of the
  component:

  ```ts
  export const rankByOptions: ReadonlyArray<{ value: RebalanceRankBy; label: string }> = [
    { value: "sek", label: "Amount" },
    { value: "percent", label: "Relative" },
  ];
  ```

  No other view-model change is expected — ranking happens server-side and
  the view model renders whatever ladder arrives. If implementation finds the
  toggle needs more derivation (e.g. tooltips), it goes here, not inline.
- Render a `fieldset.segmented-control` in the existing
  `rebalance-slider-header` (next to the Trades label and the trade-count /
  coverage chips), following the established pattern from `Dashboard.tsx` /
  `ImportView.tsx` (`sr-only` legend, `aria-pressed`, `.active` class). This
  reuses the existing `segmented-control` styling in `styles.css` and stays
  within `docs/VisualDesign.DarkTheme.md` (existing component vocabulary; no
  new colors or fills). Legend/aria-label: "Rebalance ranking". Buttons:
  Amount, Relative.
- The toggle renders whenever the slider block renders; it must remain
  visible while a refetch is in flight (the previous ladder is shown via
  `keepPreviousData`).

### Phase 3 tests

- `RebalancePage.reducer.test.ts`:
  - `rankByChanged` updates `rankBy` and preserves `sliderPosition`,
    `committedAmount`, `amountInput`, and `lastAvailableRungCount` — with a
    comment noting the query uses `keepPreviousData`, so the preserved slider
    position briefly applies to the previous ranking's ladder while
    refetching (accepted behavior, pinned intentionally).
  - Persistence round-trips `rankBy` = `"percent"`.
  - A legacy blob without `rankBy` restores `committedAmount` and
    `sliderPosition` and defaults `rankBy` to `"sek"`.
  - A blob with `rankBy: "bogus"` defaults to `"sek"` without losing
    amount/slider.
  - Existing tests stay green unchanged.
- `RebalancePage.test.tsx` (component, user-visible behavior only): clicking
  **Relative** issues a fetch whose URL contains `rank_by=percent` (assert on
  the mocked fetch/apiGet call), and the pressed state moves to the Relative
  button. Keep assertions role/text-based; no DOM-structure or snapshot
  assertions.
- `rebalanceViewModel.test.ts`: `rankByOptions` covers exactly the
  `REBALANCE_RANK_BY_VALUES` wire values (guards the one-source-of-truth
  coupling).

### Phase 3 verification

From `frontend/`:

```
npm run check
npm run fmt
```

**External human testing recommended (primary UI checkpoint):**

1. Open the Rebalance page against the real portfolio; confirm the
   Amount/Relative toggle renders in the Trades header per the dark theme.
2. With the real Corning-style setup (newly added Low-conviction watchlist
   instrument), set a positive amount, switch to Relative, slide to `N = 1`:
   the new instrument's buy must appear at `N = 1`; switch back to Amount:
   prior behavior (appears only at `N = 4`).
3. Toggle while a ladder is shown: slider position is preserved, the
   "Updating..." chip appears, no layout jump.
4. In Relative at small `N`, confirm the residual is reported (achieved net
   below the requested amount) instead of one name absorbing everything, and
   that no "Flips target band" warnings appear for traded names (targets are
   never deliberately crossed).
5. Navigate away and back, and reload: mode, amount, and slider persist.
   Clear the stored key or use an old blob to confirm the sek default.
6. Demo mode: the GET-shaped endpoint keeps working with the toggle.

---

## Phase 4 — Durable docs and decision log

- `docs/Design.RebalancePlanner.md`:
  - **Inputs / API Contract:** document `rank_by=sek|percent`, default `sek`,
    `400 invalid_rank` alongside `invalid_amount`.
  - **Direction-Aware Selection:** describe both ranking metrics; the percent
    tie-break chain (percent desc → conviction weight desc → `|d|` SEK desc →
    input order); the defensive `t <= 0` last-place rule; and one explicit
    sentence that repair swaps operate over the selected mode's ranked order,
    so percent mode can evict/admit different candidates than SEK mode.
  - **Conviction-Weighted Redistribution:** add the percent-mode clamp set
    (buy `[0, d]`, sell `[d, 0]`, target bound dominating the sell-to-zero
    clamp), the honest-residual consequence at small `N`, the redefined sell
    capacity (`-d_i`), and that the all-zero fallback is mode-consistent.
  - **Whole Shares And Residual Repair:** percent mode never crosses a
    target — initial rounding is toward zero at the target boundary and the
    greedy pass uses post-adjustment guards (`(q+1)*p <= d` for buys,
    `(q-1)*p >= d` for sells), so a name that would overshoot with a single
    share stays untraded and reports `too_small`. Sek mode keeps
    midpoint-away rounding and its existing guards.
  - **Coverage:** percent mode keeps coverage **exactly** within `[0, 100]`
    because trades never cross targets, so `|a_i − d_i| <= |d_i|` holds for
    every candidate (whole-share result, not just the continuous solution).
    Do NOT carry over the whole-share rounding caveat into the percent-mode
    coverage language. The non-decreasing-in-`N` property remains documented
    and asserted for SEK mode only, with its existing rounding/repair caveat
    unchanged.
- `docs/DecisionLog.md`: one new entry (this plan's reason for existing),
  e.g. "2026-07-08 - User-selectable rebalance ranking mode": ranking between
  absolute SEK gap (default) and relative-to-target percent is a user choice
  persisted client-side; percent mode converges toward post-offset targets
  and never deliberately crosses them, reporting unabsorbed residual honestly;
  unrecognized API values are rejected, absent values default to SEK for
  backward compatibility. Name behaviors, not plan phases.
- Confirm both version bumps landed (backend in the API phase, frontend in
  the frontend phase).

### Phase 4 verification

Docs-only phase: re-run both gate sets once as a final sweep
(`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt` from
`backend/`; `npm run check`, `npm run fmt` from `frontend/`), and proofread
that the design doc references behaviors, not this plan.

---

## Risks and notes

- **Strict no-crossing means some small names stay untraded:** in percent
  mode a name whose single whole share would overshoot its (small) target
  cannot trade; it reports the honest `too_small` reason and its share of the
  offset stays in the residual. This is the intended trade-off for the exact
  `[0, 100]` coverage guarantee and for never producing a target-band flip on
  a traded name. The user slides to more names (or switches to Amount mode) to
  absorb more.
- **`keepPreviousData` flash:** toggling shows the previous ranking's ladder
  for a moment. Accepted; covered by the "Updating..." chip and pinned in the
  reducer test comment.
- **No config flag:** the feature defaults to `sek` (backward compatible),
  but the new percent code path is exercised by default in tests at every
  layer (domain, API, frontend), satisfying the "defaults must exercise the
  new code path" rule via test coverage rather than a runtime flag.

## Resolved decisions (previously open questions)

1. **`rank_by` is NOT echoed in the API response** — the response shape stays
   backward compatible; the client tracks the requested mode in its query key.
2. **Percent mode enforces strict no-crossing in quantity resolution** —
   round-toward-zero at the target boundary plus post-adjustment greedy guards
   (`(q+1)*p <= d` for buys, `(q-1)*p >= d` for sells). A name that would
   overshoot with one share stays untraded (`too_small`); percent-mode
   coverage is therefore exactly within `[0, 100]`.
3. **`rank_by` parsing is case-sensitive and lowercase-only** — only `sek` and
   `percent` are accepted; `SEK`, `Percent`, empty, and any other value return
   `400 invalid_rank`; absent defaults to `sek`.

No open questions remain.
