# Rebalance Result Preview & Committed-Amount UX — Implementation Plan

> **For agentic workers:** If your environment provides the `superpowers` plugin, use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. In environments without it (e.g. Codex), follow the standard repo workflow from `Agents.md` instead. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show, per rebalance rung, each pool instrument's before→after target gap (with a side-flip warning), and change the amount input to start at 0 with explicit commit and persisted page state.

**Architecture:** The domain planner already computes every number needed (ideal deltas, post-trade nets, `G`/`G'`); this plan surfaces them as a per-rung `balance` report through the API, renders it as a before/after diverging gap bar table on the Rebalance page (reusing the Holdings gap-bar visual language), and replaces the debounced per-keystroke amount commit with explicit commit plus localStorage persistence (mirroring the `DateRangeSelector` pattern).

**Tech Stack:** Rust (axum, rust_decimal) backend; React + TypeScript frontend (vitest, Biome); no new dependencies.

## Why (context for the implementer)

The planner forces each rung's net to equal the requested offset `C`, so with few selected
instruments the traded names absorb the unselected names' deltas and can overshoot their own
targets — an instrument 25k under target can end 7.5k *over* target. `coverage_percent`
already aggregates this but cannot say *which* instrument ends up newly over/underweight.
The balance report makes that visible per instrument.

## Global Constraints

- Build with `cargo build` from `backend/`. When a phase is complete: `cargo clippy --all-targets -- -D warnings`, then `cargo fmt`, both from `backend/`.
- For `frontend/` changes: `npm run check` then `npm run fmt` from `frontend/`.
- All money math uses `Decimal` on the backend; the frontend receives pre-formatted strings and parses numbers only for display geometry.
- Preserve unidirectional data flow: input → action → reducer → state → render. Reducers stay pure; view derivation lives in pure view-model functions, not components.
- Sign convention (critical): the planner's internal delta is `d = target − value` (positive = buy). All **API and UI gaps use the Holdings display convention `gap = value − target`** (positive = above target = green). Do not mix them.
- Both before and after gaps are measured against the **post-offset targets** `t_i = (P + C) · w_i / W` — the targets the plan aims at. With `C = 0` these equal the Holdings targets; with `C ≠ 0` they intentionally differ from the Holdings page gaps.
- The ±5% display tolerance band has one source of truth: `TOLERANCE_PERCENT` in `backend/src/domain/conviction.rs`, exposed via the new `gap_band_status` helper.
- Version bumps: backend `backend/Cargo.toml` 0.10.0 → **0.11.0** (Phase 2), frontend `frontend/package.json` 0.16.0 → **0.17.0** (Phase 6).
- Commit after each task with a short descriptive message (repo style: plain sentence, no `feat:` prefix).

## Resolved design decisions (from brainstorming — do not re-open silently)

1. Balance rows cover **all pool candidates** of the rung (traded, selected-untraded, unselected) in input order; the separate "Untraded" section is removed and its reasons become the Action column.
2. Statuses `status_before`/`status_after` reuse the shared ±5% band; the side-flip warning fires only on `below` ↔ `above` transitions (a hair past target does not warn).
3. Backend computes everything (Decimal-exact); frontend does no gap math.
4. Persistence uses **localStorage** (matches the date-range convention; survives app restart). State persisted: committed amount + slider position. The plan itself is always recomputed against the current pool — "persisted" means parameters, not a frozen plan.
5. Amount input starts at `"0"` and the amount-0 plan loads immediately; the prompt states no longer appear on first visit and only follow an explicit commit of empty or invalid input.

---

## Phase 1 — Backend domain: per-rung balance report

Everything in this phase is pure domain code. Verify with `cargo test` from `backend/`.

### Task 1.1: Shared ±5% band helper

**Files:**
- Modify: `backend/src/domain/conviction.rs`
- Modify: `backend/src/domain/mod.rs` (re-export)

**Interfaces:**
- Produces: `pub fn gap_band_status(gap_percent: Decimal) -> TargetStatus` in `domain::conviction`, re-exported from `domain`.

- [ ] **Step 1: Write the failing test** — in the `tests` module of `conviction.rs`:

```rust
#[test]
fn gap_band_status_matches_derive_targets_band() {
    use super::gap_band_status;
    assert_eq!(gap_band_status(dec!(-5.01)), TargetStatus::Below);
    assert_eq!(gap_band_status(dec!(-5)), TargetStatus::OnTarget);
    assert_eq!(gap_band_status(dec!(0)), TargetStatus::OnTarget);
    assert_eq!(gap_band_status(dec!(5)), TargetStatus::OnTarget);
    assert_eq!(gap_band_status(dec!(5.01)), TargetStatus::Above);
}
```

- [ ] **Step 2: Run to verify it fails**

Run from `backend/`: `cargo test gap_band_status`
Expected: compile error — `gap_band_status` not found.

- [ ] **Step 3: Implement** — add above `derive_one`, and refactor `derive_one` to call it:

```rust
/// Classify a signed display-gap percent (`value − target`, as percent of
/// target) against the shared ±5% tolerance band.
pub fn gap_band_status(gap_percent: Decimal) -> TargetStatus {
    if gap_percent < -TOLERANCE_PERCENT {
        TargetStatus::Below
    } else if gap_percent > TOLERANCE_PERCENT {
        TargetStatus::Above
    } else {
        TargetStatus::OnTarget
    }
}
```

In `derive_one`, replace the `let status = if target_gap_percent < -TOLERANCE_PERCENT { ... }` block with `let status = gap_band_status(target_gap_percent);`.

In `domain/mod.rs`, add `gap_band_status` to the existing `conviction` re-export list (alongside `pool_membership`, `TargetStatus`, etc. — `TargetStatus` is already exported for the holdings API; if not, export it too).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib domain::conviction`
Expected: all pass, including the pre-existing band-boundary tests.

- [ ] **Step 5: Commit** — `Extract shared gap_band_status helper for the ±5% display band`

### Task 1.2: `CandidateBalance` on every rung

**Files:**
- Modify: `backend/src/domain/rebalance.rs`
- Modify: `backend/src/domain/mod.rs` (re-export `CandidateBalance`)

**Interfaces:**
- Consumes: `gap_band_status`, `TargetStatus` from Task 1.1.
- Produces (used by Phase 2):

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct CandidateBalance {
    pub instrument_id: i64,
    /// Signed display gap vs the post-offset target: value − target.
    /// Positive = above target. Note: equals −ideal_delta.
    pub gap_before_base: Decimal,
    /// gap_before_base + this rung's net traded amount for the candidate.
    pub gap_after_base: Decimal,
    /// Gaps as percent of the post-offset target; None when the target is
    /// not strictly positive (defensive; not expected in practice).
    pub gap_before_percent: Option<Decimal>,
    pub gap_after_percent: Option<Decimal>,
    pub status_before: TargetStatus,
    pub status_after: TargetStatus,
}
```

`RebalanceRung` gains three fields: `balance: Vec<CandidateBalance>` (input order, one per candidate), `total_gap_before_base: Decimal` (= `G`), `total_gap_after_base: Decimal` (= `G'`).

- [ ] **Step 1: Write the failing tests** — in the `tests` module of `rebalance.rs`:

```rust
#[test]
fn balance_reports_before_after_gaps_and_side_flips() {
    // Equal weights, pool 300, target 100 each: gaps +40, −25, −15.
    let candidates = vec![
        candidate(1, dec!(1), dec!(140), dec!(0.5), 280),
        candidate(2, dec!(1), dec!(75), dec!(0.5), 150),
        candidate(3, dec!(1), dec!(85), dec!(0.5), 170),
    ];
    let ladder = build_ladder(&candidates, Decimal::ZERO).expect("ladder");

    // N = 2 selects instruments 1 and 2 (largest |delta|). The net-zero
    // constraint spreads the unselected −15 across them: sell 32.50 of 1,
    // buy 32.50 of 2 — pushing instrument 2 past its target.
    let rung = &ladder.rungs[1];
    let balance = &rung.balance;

    assert_eq!(balance[0].gap_before_base, dec!(40));
    assert_eq!(balance[1].gap_before_base, dec!(-25));
    assert_eq!(balance[2].gap_before_base, dec!(-15));
    assert_eq!(balance[0].status_before, TargetStatus::Above);
    assert_eq!(balance[1].status_before, TargetStatus::Below);

    assert_eq!(balance[0].gap_after_base, dec!(7.5));
    assert_eq!(balance[1].gap_after_base, dec!(7.5));
    assert_eq!(balance[1].status_after, TargetStatus::Above); // flipped side
    // Unselected candidate is untouched.
    assert_eq!(balance[2].gap_after_base, balance[2].gap_before_base);

    assert_eq!(balance[1].gap_before_percent, Some(dec!(-25)));
    assert_eq!(rung.total_gap_before_base, dec!(80));
    assert_eq!(rung.total_gap_after_base, dec!(30));
}

#[test]
fn balance_gap_after_sums_to_minus_residual_at_every_rung() {
    let ladder = build_ladder(&worked_fixture(), dec!(50000)).expect("ladder");
    for rung in &ladder.rungs {
        let sum_after: Decimal = rung.balance.iter().map(|b| b.gap_after_base).sum();
        assert_eq!(sum_after, -rung.residual_base);
        let total_before: Decimal =
            rung.balance.iter().map(|b| b.gap_before_base.abs()).sum();
        let total_after: Decimal =
            rung.balance.iter().map(|b| b.gap_after_base.abs()).sum();
        assert_eq!(total_before, rung.total_gap_before_base);
        assert_eq!(total_after, rung.total_gap_after_base);
    }
}
```

Add `TargetStatus` to the test module's `use super::...` list (it is re-exported through the new import in Step 3).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib domain::rebalance`
Expected: compile error — no field `balance` on `RebalanceRung`.

- [ ] **Step 3: Implement**

At the top of `rebalance.rs` add:

```rust
use super::conviction::{gap_band_status, TargetStatus};
```

Add the `CandidateBalance` struct (as specified in Interfaces above) next to `RebalanceRung`, and extend `RebalanceRung`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct RebalanceRung {
    pub selected_count: usize,
    pub trades: Vec<PlannedTrade>,
    pub untraded: Vec<UntradedCandidate>,
    pub balance: Vec<CandidateBalance>,
    pub effective_trade_count: usize,
    pub achieved_net_base: Decimal,
    pub residual_base: Decimal,
    pub coverage_percent: Option<Decimal>,
    /// Σ|gap_before| over all candidates (the planner's G).
    pub total_gap_before_base: Decimal,
    /// Σ|gap_after| over all candidates (the planner's G′).
    pub total_gap_after_base: Decimal,
}
```

In `build_rung`, inside the existing per-candidate loop (the one that accumulates `g` and `g_prime`), build the balance entry. The identities `gap_before = −ideal_delta` and `gap_after = actual_net − ideal_delta` mean `g` and `g_prime` are exactly the totals:

```rust
let mut balance = Vec::with_capacity(candidates.len());
// ... existing loop:
for idx in 0..candidates.len() {
    let candidate = &candidates[idx];
    let ideal_delta = ideal_deltas[idx];
    let actual_net = Decimal::from(quantities.quantities[idx]) * candidate.price_base;

    let gap_before = -ideal_delta;
    let gap_after = gap_before + actual_net;
    let target = candidate.market_value_base + ideal_delta;
    let (gap_before_percent, gap_after_percent, status_before, status_after) =
        if target > Decimal::ZERO {
            let hundred = Decimal::from(100);
            let before_percent = gap_before / target * hundred;
            let after_percent = gap_after / target * hundred;
            (
                Some(before_percent),
                Some(after_percent),
                gap_band_status(before_percent),
                gap_band_status(after_percent),
            )
        } else {
            (None, None, TargetStatus::Unavailable, TargetStatus::Unavailable)
        };
    balance.push(CandidateBalance {
        instrument_id: candidate.instrument_id,
        gap_before_base: gap_before,
        gap_after_base: gap_after,
        gap_before_percent,
        gap_after_percent,
        status_before,
        status_after,
    });

    achieved_net_base += actual_net;
    g += ideal_delta.abs();
    g_prime += (actual_net - ideal_delta).abs();
    // ... existing trades/untraded logic unchanged
}
```

Populate the three new `RebalanceRung` fields (`balance`, `total_gap_before_base: g`, `total_gap_after_base: g_prime`) in the struct literal at the end of `build_rung`. Note `g`/`g_prime` are moved into the rung *after* computing `coverage_percent`.

In `domain/mod.rs`, add `CandidateBalance` to the `rebalance` re-export list.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib domain::rebalance`
Expected: all pass, including all pre-existing planner tests (the change is purely additive).

- [ ] **Step 5: Clippy, fmt, commit**

Run from `backend/`: `cargo clippy --all-targets -- -D warnings && cargo fmt`
Commit: `Report per-candidate before/after balance on every rebalance rung`

**Phase 1 verification:** `cargo test` from `backend/` is green. No API or UI change yet.

---

## Phase 2 — Backend API: serialize the balance report

### Task 2.1: `balance` and totals in the rung response

**Files:**
- Modify: `backend/src/api/rebalance.rs`
- Modify: `backend/Cargo.toml` (version 0.10.0 → 0.11.0)

**Interfaces:**
- Consumes: `CandidateBalance`, `TargetStatus` from Phase 1; existing `money_string`, `InstrumentResponse`, `lookup_prepared`.
- Produces (JSON shape consumed by Phase 3): each rung gains
  `balance: [{ instrument, gap_before_base, gap_after_base, gap_before_percent, gap_after_percent, status_before, status_after }]`,
  `total_gap_before_base`, `total_gap_after_base`. Money fields are money strings; percent fields are 2-decimal strings or `null`; statuses are the existing snake_case `TargetStatus` strings.

- [ ] **Step 1: Write the failing test** — add to the `tests` module of `api/rebalance.rs`:

```rust
#[tokio::test]
async fn rungs_report_post_trade_balance() {
    let state = AppState::for_tests().await;
    seed_valued(&state, "AAA", 100, "1000", "Low").await;
    seed_valued(&state, "BBB", 300, "1000", "Medium").await;
    seed_valued(&state, "CCC", 300, "1000", "High").await;

    let (status, body) = send(&state, "GET", "/api/rebalance?amount=0", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_plan_status(&body, "available");

    // Pool 700k, weights 1/2/4 → targets 100k/200k/400k → gaps 0/+100k/−100k.
    // Rung 2 sells BBB 100k and buys CCC 100k, closing both gaps.
    let rung = &body["plan"]["rungs"][1];
    let balance = rung["balance"].as_array().expect("balance");
    assert_eq!(balance.len(), 3);

    assert_eq!(balance[0]["instrument"]["symbol"], "AAA");
    assert_eq!(balance[0]["gap_before_base"], "0.00");
    assert_eq!(balance[0]["status_before"], "on_target");

    assert_eq!(balance[1]["instrument"]["symbol"], "BBB");
    assert_eq!(balance[1]["gap_before_base"], "100000.00");
    assert_eq!(balance[1]["gap_before_percent"], "50.00");
    assert_eq!(balance[1]["status_before"], "above");
    assert_eq!(balance[1]["gap_after_base"], "0.00");
    assert_eq!(balance[1]["status_after"], "on_target");

    assert_eq!(balance[2]["instrument"]["symbol"], "CCC");
    assert_eq!(balance[2]["gap_before_base"], "-100000.00");
    assert_eq!(balance[2]["status_before"], "below");

    assert_eq!(rung["total_gap_before_base"], "200000.00");
    assert_eq!(rung["total_gap_after_base"], "0.00");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test rungs_report_post_trade_balance`
Expected: FAIL — `balance` is null / missing.

- [ ] **Step 3: Implement**

Add `CandidateBalance` to the `crate::domain` import list. Add the response struct:

```rust
#[derive(Debug, Serialize)]
pub struct RebalanceBalanceResponse {
    pub instrument: InstrumentResponse,
    pub gap_before_base: String,
    pub gap_after_base: String,
    pub gap_before_percent: Option<String>,
    pub gap_after_percent: Option<String>,
    pub status_before: &'static str,
    pub status_after: &'static str,
}
```

Extend `RebalanceRungResponse` with:

```rust
    pub balance: Vec<RebalanceBalanceResponse>,
    pub total_gap_before_base: String,
    pub total_gap_after_base: String,
```

Add a serializer (mirrors `serialize_untraded`; iterate `rung.balance`, which preserves candidate input order — do not iterate the `BTreeMap`):

```rust
fn serialize_balance(
    entry: &CandidateBalance,
    prepared_by_id: &BTreeMap<i64, &PreparedCandidate>,
) -> Result<RebalanceBalanceResponse, ApiError> {
    let prepared = lookup_prepared(prepared_by_id, entry.instrument_id)?;
    Ok(RebalanceBalanceResponse {
        instrument: InstrumentResponse::from_row(&prepared.instrument)?,
        gap_before_base: money_string(entry.gap_before_base),
        gap_after_base: money_string(entry.gap_after_base),
        gap_before_percent: entry.gap_before_percent.map(|value| format!("{value:.2}")),
        gap_after_percent: entry.gap_after_percent.map(|value| format!("{value:.2}")),
        status_before: entry.status_before.as_str(),
        status_after: entry.status_after.as_str(),
    })
}
```

In `serialize_rung`, build `balance` the same way `untraded` is built and set the two totals with `money_string(rung.total_gap_before_base)` / `money_string(rung.total_gap_after_base)`.

Bump `backend/Cargo.toml` version to `0.11.0`.

- [ ] **Step 4: Run tests**

Run: `cargo test` (from `backend/`)
Expected: all pass, including the pre-existing `happy_path_...` test (additive fields don't break it).

- [ ] **Step 5: Clippy, fmt, commit**

`cargo clippy --all-targets -- -D warnings && cargo fmt`
Commit: `Serialize per-rung balance report in /api/rebalance`

**Phase 2 verification:** `cargo test` green. Optionally `cargo run` and `curl "http://localhost:PORT/api/rebalance?amount=0"` to inspect the new fields by eye.

---

## Phase 3 — Frontend: balance table with before→after gap bars

Verify with `npm run check` from `frontend/` after each task.

### Task 3.1: API types + bar-geometry helper extraction

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/components/holdingsConviction.ts`
- Test: `frontend/src/components/holdingsConviction.test.ts`

**Interfaces:**
- Produces:

```ts
// types.ts
export interface RebalanceBalanceEntry {
  instrument: Instrument;
  gap_before_base: string;
  gap_after_base: string;
  gap_before_percent: string | null;
  gap_after_percent: string | null;
  status_before: TargetStatus;
  status_after: TargetStatus;
}
// RebalanceRung gains:
//   balance: RebalanceBalanceEntry[];
//   total_gap_before_base: string;
//   total_gap_after_base: string;

// holdingsConviction.ts
export interface GapBarGeometry {
  side: "above" | "below" | "on_target";
  widthPercent: number; // 0..50, percent of the full track
}
export function gapBarGeometry(gapPercent: number): GapBarGeometry;
```

- [ ] **Step 1: Write the failing test** — in `holdingsConviction.test.ts`:

```ts
import { gapBarGeometry, TARGET_GAP_BAR_CLAMP_PERCENT } from "./holdingsConviction";

describe("gapBarGeometry", () => {
  it("maps sign to side and clamps magnitude to the half-track", () => {
    expect(gapBarGeometry(0)).toEqual({ side: "on_target", widthPercent: 0 });
    expect(gapBarGeometry(25)).toEqual({ side: "above", widthPercent: 25 });
    expect(gapBarGeometry(-25)).toEqual({ side: "below", widthPercent: 25 });
    expect(gapBarGeometry(TARGET_GAP_BAR_CLAMP_PERCENT * 3)).toEqual({
      side: "above",
      widthPercent: 50,
    });
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run from `frontend/`: `npx vitest run src/components/holdingsConviction.test.ts`
Expected: FAIL — `gapBarGeometry` is not exported.

- [ ] **Step 3: Implement**

In `holdingsConviction.ts`, extract the geometry math out of `targetGapBar` (keep `targetGapBar`'s signature and tooltip behavior identical):

```ts
export interface GapBarGeometry {
  side: "above" | "below" | "on_target";
  widthPercent: number;
}

/** Geometry of the diverging gap bar for a signed gap percent: side by sign,
 * width clamped to `TARGET_GAP_BAR_CLAMP_PERCENT` and mapped onto one half of
 * the track (0..50). Shared by Holdings targets and the rebalance balance. */
export function gapBarGeometry(gapPercent: number): GapBarGeometry {
  const magnitude = Math.min(Math.abs(gapPercent), TARGET_GAP_BAR_CLAMP_PERCENT);
  const widthPercent = (magnitude / TARGET_GAP_BAR_CLAMP_PERCENT) * 50;
  const side = gapPercent === 0 ? "on_target" : gapPercent > 0 ? "above" : "below";
  return { side, widthPercent };
}
```

Refactor `targetGapBar` to `return { ...gapBarGeometry(gapPercent), tooltip };`.

In `types.ts`, add `RebalanceBalanceEntry` and the three new `RebalanceRung` fields as specified in Interfaces.

- [ ] **Step 4: Run checks**

Run: `npm run check`
Expected: PASS (existing `targetGapBar` tests still green).

- [ ] **Step 5: Commit** — `Extract shared gap bar geometry and add rebalance balance types`

### Task 3.2: Balance-row view model

**Files:**
- Modify: `frontend/src/components/rebalanceViewModel.ts`
- Test: `frontend/src/components/rebalanceViewModel.test.ts`

**Interfaces:**
- Consumes: `gapBarGeometry`, `GapBarGeometry` (Task 3.1); existing `formatRebalanceMoney`, `rebalanceUntradedReasonLabel`, `parseFiniteNumber`, `formatGroupedNumber`.
- Produces:

```ts
export interface RebalanceBalanceBarViewModel {
  before: GapBarGeometry;
  after: GapBarGeometry;
  tooltip: string;
}
export interface RebalanceBalanceRowViewModel {
  instrument: Instrument;
  actionKind: "trade" | "untraded" | "unselected";
  actionLabel: string; // "Buy SEK 32,500" | "Too small" | "—"
  bar: RebalanceBalanceBarViewModel | null;
  afterGapLabel: string; // "SEK 7,500 (7.50%)"
  flipsSide: boolean; // below ↔ above only
}
export function buildRebalanceBalanceRows(rung: RebalanceRung): RebalanceBalanceRowViewModel[];
// RebalancePageViewModel gains:
//   balanceRows: RebalanceBalanceRowViewModel[];
//   balanceTotalLabel: string | null;  // "SEK 80,000 → SEK 30,000"
// RebalancePageViewModel drops: untradedRows (reasons fold into balanceRows).
```

- [ ] **Step 1: Write the failing test** — in `rebalanceViewModel.test.ts` (adjust the instrument factory to whatever helper the file already uses; if none, build a minimal `Instrument` literal):

```ts
import { buildRebalanceBalanceRows } from "./rebalanceViewModel";
import type { RebalanceRung } from "../api/types";

function balanceRung(): RebalanceRung {
  const instrument = (id: number, symbol: string) =>
    ({ id, symbol, name: symbol, exchange: "STO" }) as never;
  return {
    selected_count: 2,
    effective_trade_count: 1,
    trades: [
      {
        instrument: instrument(1, "AAA"),
        side: "sell",
        shares: 65,
        price_base: "0.50",
        amount_base: "32.50",
        freshness: "fresh",
      },
    ],
    untraded: [{ instrument: instrument(2, "BBB"), reason: "too_small" }],
    balance: [
      {
        instrument: instrument(1, "AAA"),
        gap_before_base: "40.00",
        gap_after_base: "7.50",
        gap_before_percent: "40.00",
        gap_after_percent: "7.50",
        status_before: "above",
        status_after: "above",
      },
      {
        instrument: instrument(2, "BBB"),
        gap_before_base: "-25.00",
        gap_after_base: "7.50",
        gap_before_percent: "-25.00",
        gap_after_percent: "7.50",
        status_before: "below",
        status_after: "above",
      },
      {
        instrument: instrument(3, "CCC"),
        gap_before_base: "-15.00",
        gap_after_base: "-15.00",
        gap_before_percent: "-15.00",
        gap_after_percent: "-15.00",
        status_before: "below",
        status_after: "below",
      },
    ],
    achieved_net_base: "0.00",
    residual_base: "0.00",
    coverage_percent: "62.50",
    total_gap_before_base: "80.00",
    total_gap_after_base: "30.00",
  };
}

describe("buildRebalanceBalanceRows", () => {
  it("joins trades and untraded reasons onto balance rows in balance order", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows.map((row) => row.actionKind)).toEqual([
      "trade",
      "untraded",
      "unselected",
    ]);
    expect(rows[0].actionLabel).toBe("Sell SEK 32.50");
    expect(rows[1].actionLabel).toBe("Too small");
    expect(rows[2].actionLabel).toBe("—");
  });

  it("flags only below↔above flips and builds before/after bar geometry", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows.map((row) => row.flipsSide)).toEqual([false, true, false]);
    expect(rows[1].bar?.before).toEqual({ side: "below", widthPercent: 25 });
    expect(rows[1].bar?.after).toEqual({ side: "above", widthPercent: 7.5 });
  });

  it("renders an after-gap label with amount and percent", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows[1].afterGapLabel).toBe("SEK 7.50 (7.50%)");
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/rebalanceViewModel.test.ts`
Expected: FAIL — `buildRebalanceBalanceRows` not exported.

- [ ] **Step 3: Implement** — in `rebalanceViewModel.ts`:

Add imports: `gapBarGeometry, type GapBarGeometry` from `./holdingsConviction`; `parseFiniteNumber` from `./valuationDisplay`; `RebalanceBalanceEntry` type.

```ts
export interface RebalanceBalanceBarViewModel {
  before: GapBarGeometry;
  after: GapBarGeometry;
  tooltip: string;
}

export interface RebalanceBalanceRowViewModel {
  instrument: Instrument;
  actionKind: "trade" | "untraded" | "unselected";
  actionLabel: string;
  bar: RebalanceBalanceBarViewModel | null;
  afterGapLabel: string;
  flipsSide: boolean;
}

function balanceBar(entry: RebalanceBalanceEntry): RebalanceBalanceBarViewModel | null {
  if (entry.gap_before_percent === null || entry.gap_after_percent === null) {
    return null;
  }
  const beforePercent = parseFiniteNumber(entry.gap_before_percent);
  const afterPercent = parseFiniteNumber(entry.gap_after_percent);
  if (beforePercent === null || afterPercent === null) {
    return null;
  }
  return {
    before: gapBarGeometry(beforePercent),
    after: gapBarGeometry(afterPercent),
    tooltip: [
      `Before ${formatRebalanceMoney(entry.gap_before_base)} (${formatGroupedNumber(entry.gap_before_percent)}%)`,
      `After ${formatRebalanceMoney(entry.gap_after_base)} (${formatGroupedNumber(entry.gap_after_percent)}%)`,
    ].join("\n"),
  };
}

export function buildRebalanceBalanceRows(
  rung: RebalanceRung,
): RebalanceBalanceRowViewModel[] {
  const tradesById = new Map(rung.trades.map((trade) => [trade.instrument.id, trade]));
  const untradedById = new Map(
    rung.untraded.map((candidate) => [candidate.instrument.id, candidate]),
  );

  return rung.balance.map((entry) => {
    const trade = tradesById.get(entry.instrument.id);
    const untraded = untradedById.get(entry.instrument.id);
    const actionKind = trade ? "trade" : untraded ? "untraded" : "unselected";
    const actionLabel = trade
      ? `${trade.side === "buy" ? "Buy" : "Sell"} ${formatRebalanceMoney(trade.amount_base)}`
      : untraded
        ? rebalanceUntradedReasonLabel(untraded.reason)
        : "—";
    const afterGapLabel =
      entry.gap_after_percent === null
        ? formatRebalanceMoney(entry.gap_after_base)
        : `${formatRebalanceMoney(entry.gap_after_base)} (${formatGroupedNumber(entry.gap_after_percent)}%)`;

    return {
      instrument: entry.instrument,
      actionKind,
      actionLabel,
      bar: balanceBar(entry),
      afterGapLabel,
      flipsSide:
        (entry.status_before === "above" && entry.status_after === "below") ||
        (entry.status_before === "below" && entry.status_after === "above"),
    };
  });
}
```

Wire into the page view model:

- `RebalancePageViewModel`: add `balanceRows: RebalanceBalanceRowViewModel[]` and `balanceTotalLabel: string | null`; **remove** `untradedRows` and the `RebalanceUntradedRowViewModel` plumbing (`untradedRows()` helper stays only if `emptyTradeRowsMessage` still needs `rung.untraded` — it uses the raw rung data, so the helper and interface can be deleted).
- In `buildAvailableViewModel`: `balanceRows: buildRebalanceBalanceRows(rung)` and
  ``balanceTotalLabel: `${formatRebalanceMoney(rung.total_gap_before_base)} → ${formatRebalanceMoney(rung.total_gap_after_base)}` ``.
- In every empty/prompt/loading/error view-model literal, add `balanceRows: [], balanceTotalLabel: null` and drop `untradedRows`.

- [ ] **Step 4: Run tests**

Run: `npx vitest run src/components/rebalanceViewModel.test.ts`
Expected: new tests PASS; fix any existing view-model tests that asserted `untradedRows` (they now assert `balanceRows` action kinds/labels instead — same behavioral coverage, new surface).

- [ ] **Step 5: Commit** — `Add before/after balance rows to the rebalance view model`

### Task 3.3: Render the balance table

**Files:**
- Modify: `frontend/src/components/RebalancePage.tsx`
- Modify: `frontend/src/styles.css`
- Test: `frontend/src/components/RebalancePage.test.tsx` (update: untraded section is gone)

**Interfaces:**
- Consumes: `viewModel.balanceRows`, `viewModel.balanceTotalLabel` (Task 3.2); existing `.target-gap-track` / `.target-gap-axis` / `.target-gap-fill` CSS.

- [ ] **Step 1: Replace the untraded section** — in `RebalancePage.tsx`, delete the whole `viewModel.untradedRows.length > 0 ? ... : null` block and render instead (still inside the `status === "available"` fragment, after the trades table):

```tsx
<div className="rebalance-balance">
  <div className="panel-header compact">
    <div>
      <p className="eyebrow">Resulting balance</p>
      <h2>Gap vs plan target</h2>
    </div>
    {viewModel.balanceTotalLabel ? (
      <span className="status-chip compact">
        Total gap {viewModel.balanceTotalLabel}
      </span>
    ) : null}
  </div>
  <div className="table-wrap">
    <table aria-label="Post-trade balance">
      <thead>
        <tr>
          <th>Instrument</th>
          <th>Action</th>
          <th>Balance</th>
          <th className="number-head">After gap</th>
        </tr>
      </thead>
      <tbody>
        {viewModel.balanceRows.map((row) => (
          <tr key={row.instrument.id}>
            <td>
              <InstrumentCell
                instrumentId={row.instrument.id}
                name={row.instrument.name}
                symbol={row.instrument.symbol}
                exchange={row.instrument.exchange}
              />
            </td>
            <td>
              {row.actionKind === "trade" ? (
                <span className="type-chip">{row.actionLabel}</span>
              ) : row.actionKind === "untraded" ? (
                <span className="status-chip">{row.actionLabel}</span>
              ) : (
                <span>{row.actionLabel}</span>
              )}
            </td>
            <td>{row.bar ? balanceBarCell(row.bar) : null}</td>
            <td className="number">
              {row.afterGapLabel}
              {row.flipsSide ? (
                <span className="status-chip warning compact rebalance-flip-chip">
                  Flips target band
                </span>
              ) : null}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  </div>
</div>
```

with this helper at the bottom of the file (next to `tradeFreshnessCell`):

```tsx
function balanceBarCell(bar: RebalanceBalanceBarViewModel) {
  return (
    <div className="target-gap-track" title={bar.tooltip}>
      <span className="target-gap-axis" />
      {bar.before.side !== "on_target" ? (
        <span
          className={`target-gap-fill ghost ${bar.before.side}`}
          style={{ width: `${bar.before.widthPercent}%` }}
        />
      ) : null}
      {bar.after.side !== "on_target" ? (
        <span
          className={`target-gap-fill ${bar.after.side}`}
          style={{ width: `${bar.after.widthPercent}%` }}
        />
      ) : null}
    </div>
  );
}
```

(Import `RebalanceBalanceRowViewModel`/`RebalanceBalanceBarViewModel` types as needed. The solid *after* fill is rendered last so it draws over the *before* ghost.)

- [ ] **Step 2: Add the ghost style** — in `styles.css` right after the existing `.target-gap-fill.below` rule:

```css
/* Rebalance balance preview: the pre-trade gap renders as a hollow ghost
 * behind the solid post-trade fill. */
.target-gap-fill.ghost {
  background: transparent;
  border: 1px solid;
  opacity: 0.65;
}

.target-gap-fill.ghost.above {
  border-color: var(--up);
  background: transparent;
}

.target-gap-fill.ghost.below {
  border-color: var(--down);
  background: transparent;
}

.rebalance-flip-chip {
  margin-left: 8px;
}
```

- [ ] **Step 3: Update `RebalancePage.test.tsx`** — remove/replace assertions about the "Untraded" section; assert instead (by role/text, no DOM-structure snapshots) that the balance table renders an instrument's action chip and the "Flips target band" chip when the view model flags a flip. Keep this minimal: the row-building logic is already covered by Task 3.2's pure tests.

- [ ] **Step 4: Run checks**

Run: `npm run check` then `npm run fmt` from `frontend/`.
Expected: PASS.

- [ ] **Step 5: Commit** — `Show post-trade balance table with before/after gap bars on Rebalance`

**Phase 3 verification (external human testing recommended):** run backend + frontend, open Rebalance, enter an amount, drag the slider across rungs. Check: (a) the balance table updates instantly with the slider; (b) ghost=before / solid=after bars read correctly against Holdings colors; (c) a low-N rung on a real portfolio shows the overshoot the feature exists for; (d) "Flips target band" appears only on genuine side flips; (e) dark-theme contrast of the ghost outline per docs/VisualDesign.DarkTheme.md.

---

## Phase 4 — Amount input: start at 0, explicit commit

### Task 4.1: Default committed amount 0, remove debounce, add Apply button

**Files:**
- Modify: `frontend/src/components/RebalancePage.tsx`
- Test: `frontend/src/components/RebalancePage.reducer.test.ts`, `frontend/src/components/rebalanceViewModel.test.ts`

**Interfaces:**
- Produces: `initialState` = `{ amountInput: "0", committedAmount: "0", sliderPosition: 1, lastAvailableRungCount: null }`. Commit happens only via Enter, blur, or the Apply button (`commitNow`). No time-based commit exists anymore.

- [ ] **Step 1: Update reducer test** — in `RebalancePage.reducer.test.ts`, update/add the initial-state expectation:

```ts
it("starts committed at amount 0 so the default plan loads immediately", () => {
  expect(initialState.amountInput).toBe("0");
  expect(initialState.committedAmount).toBe("0");
});
```

This test must assert the **production** initial state: export `initialState` from `RebalancePage.tsx` (the reducer is already exported), import it in `RebalancePage.reducer.test.ts`, and delete the local `initialState` fixture the test file currently defines at the top — otherwise the test can pass against the fixture while `RebalancePage.tsx` still starts at `""`. Scenario-specific state literals inside individual tests may stay (they get the new fields in Phase 5), but the default-state expectation must target the export.

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/RebalancePage.reducer.test.ts`
Expected: FAIL — amountInput is `""`.

- [ ] **Step 3: Implement** — in `RebalancePage.tsx`:

1. `initialState`: `amountInput: "0"`, `committedAmount: "0"` (export `initialState`).
2. Delete the debounce: remove `debounceRef`, the entire `useEffect` on `[state.amountInput]`, and the `clearTimeout` lines inside `commitNow` (it becomes just the dispatch).
3. Add the Apply button inside `.rebalance-controls`, right after the amount `<label>`:

```tsx
<button
  type="button"
  className="button outline rebalance-apply-button"
  onClick={commitNow}
>
  Apply
</button>
```

4. Typing no longer triggers fetches: `useRebalancePlan(state.committedAmount)` only changes on commit — no other change needed.

If the button needs alignment with the input, add to `styles.css`:

```css
.rebalance-apply-button {
  align-self: flex-end;
}
```

- [ ] **Step 4: Update view-model expectations** — with a default committed amount, no prompt appears on first visit (that state is now `loading` → `available`); update any test that asserted the first-visit prompt. Both prompt branches remain reachable after an explicit commit: committing empty input shows "No valid amount entered yet...", committing garbage shows "Enter a valid decimal amount." — keep both tested.

- [ ] **Step 5: Run checks**

Run: `npm run check`
Expected: PASS. Manually: typing must NOT refetch (watch the Updating chip / network tab); Enter, blur, and Apply must.

- [ ] **Step 6: Commit** — `Start rebalance at amount 0 with explicit Apply instead of debounced commits`

---

## Phase 5 — Persist rebalance parameters across navigation

### Task 5.1: localStorage round trip + restored-slider clamping

**Files:**
- Modify: `frontend/src/components/RebalancePage.tsx`
- Test: `frontend/src/components/RebalancePage.reducer.test.ts`

**Interfaces:**
- Consumes: the `DateRangeSelector` persistence pattern (`storage()` try/catch, load in `useReducer` lazy init, save in `useEffect`).
- Produces:

```ts
export interface RebalancePageState {
  amountInput: string;
  committedAmount: string | null;
  sliderPosition: number;
  lastAvailableRungCount: number | null;
  sliderRestored: boolean; // true when sliderPosition came from storage
}
export function loadRebalancePageState(): RebalancePageState;
export function saveRebalancePageState(state: RebalancePageState): void;
```

Persisted JSON under key `"rebalance-page-state"`: `{ committedAmount: string, sliderPosition: number }`.

`saveRebalancePageState` is a no-op for an invalid commit (`committedAmount: null`) **and** for a pristine first visit (`sliderRestored: false` with `lastAvailableRungCount: null`) — otherwise the save effect could persist the default `sliderPosition: 1` before the first available plan arrives, and the next mount would restore it instead of defaulting to all assets.

- [ ] **Step 1: Write the failing tests** — in `RebalancePage.reducer.test.ts`:

```ts
import {
  loadRebalancePageState,
  rebalancePageReducer,
  saveRebalancePageState,
} from "./RebalancePage";

describe("rebalance page persistence", () => {
  beforeEach(() => localStorage.clear());

  it("round trips committed amount and slider position", () => {
    saveRebalancePageState({
      amountInput: "25000",
      committedAmount: "25000",
      sliderPosition: 3,
      lastAvailableRungCount: 8,
      sliderRestored: false,
    });
    const restored = loadRebalancePageState();
    expect(restored.committedAmount).toBe("25000");
    expect(restored.amountInput).toBe("25000");
    expect(restored.sliderPosition).toBe(3);
    expect(restored.lastAvailableRungCount).toBeNull();
    expect(restored.sliderRestored).toBe(true);
  });

  it("does not persist an un-restored default before the first available plan", () => {
    saveRebalancePageState({
      amountInput: "0",
      committedAmount: "0",
      sliderPosition: 1,
      lastAvailableRungCount: null,
      sliderRestored: false,
    });
    expect(localStorage.getItem("rebalance-page-state")).toBeNull();
    // A later visit therefore still defaults the slider to all assets.
    const later = rebalancePageReducer(loadRebalancePageState(), {
      type: "planChanged",
      rungCount: 8,
    });
    expect(later.sliderPosition).toBe(8);
  });

  it("falls back to the initial state on missing or corrupt storage", () => {
    expect(loadRebalancePageState().committedAmount).toBe("0");
    localStorage.setItem("rebalance-page-state", "{not json");
    expect(loadRebalancePageState().committedAmount).toBe("0");
  });

  it("clamps a restored slider position instead of jumping to max", () => {
    const restored = {
      amountInput: "0",
      committedAmount: "0",
      sliderPosition: 3,
      lastAvailableRungCount: null,
      sliderRestored: true,
    };
    const afterPlan = rebalancePageReducer(restored, {
      type: "planChanged",
      rungCount: 8,
    });
    expect(afterPlan.sliderPosition).toBe(3); // kept, not reset to 8

    const shrunkPool = rebalancePageReducer(restored, {
      type: "planChanged",
      rungCount: 2,
    });
    expect(shrunkPool.sliderPosition).toBe(2); // clamped into range
  });

  it("still defaults a first visit to all assets", () => {
    const first = rebalancePageReducer(
      {
        amountInput: "0",
        committedAmount: "0",
        sliderPosition: 1,
        lastAvailableRungCount: null,
        sliderRestored: false,
      },
      { type: "planChanged", rungCount: 8 },
    );
    expect(first.sliderPosition).toBe(8);
  });
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npx vitest run src/components/RebalancePage.reducer.test.ts`
Expected: FAIL — `loadRebalancePageState` not exported / `sliderRestored` missing.

- [ ] **Step 3: Implement** — in `RebalancePage.tsx`:

1. Add `sliderRestored: boolean` to `RebalancePageState`; `initialState` gets `sliderRestored: false`.
2. In the `planChanged` reducer arm, replace the first-load branch:

```ts
case "planChanged": {
  if (action.rungCount === null) {
    return state;
  }
  const rungCount = Math.max(1, Math.trunc(action.rungCount));
  const sliderPosition =
    state.lastAvailableRungCount === null && !state.sliderRestored
      ? rungCount
      : clampInteger(state.sliderPosition, 1, rungCount);
  if (
    state.lastAvailableRungCount === rungCount &&
    state.sliderPosition === sliderPosition
  ) {
    return state;
  }
  return { ...state, lastAvailableRungCount: rungCount, sliderPosition };
}
```

3. Add the persistence trio (mirror `DateRangeSelector.tsx`):

```ts
const REBALANCE_PAGE_STATE_KEY = "rebalance-page-state";

interface PersistedRebalancePageState {
  committedAmount: string;
  sliderPosition: number;
}

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

export function loadRebalancePageState(): RebalancePageState {
  const saved = storage()?.getItem(REBALANCE_PAGE_STATE_KEY);
  if (!saved) return initialState;
  try {
    const parsed = JSON.parse(saved) as Partial<PersistedRebalancePageState>;
    const committedAmount = normalizeRebalanceAmount(parsed.committedAmount ?? null);
    if (committedAmount === null || typeof parsed.sliderPosition !== "number") {
      return initialState;
    }
    return {
      amountInput: committedAmount,
      committedAmount,
      sliderPosition: Math.max(1, Math.trunc(parsed.sliderPosition)),
      lastAvailableRungCount: null,
      sliderRestored: true,
    };
  } catch {
    return initialState;
  }
}

export function saveRebalancePageState(state: RebalancePageState): void {
  if (state.committedAmount === null) {
    return; // keep the last good parameters on an invalid commit
  }
  if (!state.sliderRestored && state.lastAvailableRungCount === null) {
    // Never persist a pristine first visit before the first available plan:
    // saving { sliderPosition: 1 } here would make the next mount treat it as
    // a restored choice and skip the all-assets default.
    return;
  }
  storage()?.setItem(
    REBALANCE_PAGE_STATE_KEY,
    JSON.stringify({
      committedAmount: state.committedAmount,
      sliderPosition: state.sliderPosition,
    } satisfies PersistedRebalancePageState),
  );
}
```

4. In the component: `useReducer(rebalancePageReducer, undefined, loadRebalancePageState)` and

```ts
useEffect(() => {
  saveRebalancePageState(state);
}, [state]);
```

- [ ] **Step 4: Run checks**

Run: `npm run check` then `npm run fmt`
Expected: PASS (fix any reducer tests constructing state literals — they need the new `sliderRestored` field).

- [ ] **Step 5: Commit** — `Persist rebalance amount and trade-count selection across navigation`

**Phase 5 verification (external human testing recommended):** set amount 25000, slider to 3, go to Holdings, come back → same amount, same slider, same plan. Restart the app → still restored. Change a conviction on Holdings so the pool shrinks below the saved slider → slider clamps without errors. Clear the field, press Apply → the prompt message appears while the stored parameters survive a reload.

---

## Phase 6 — Documentation, versions, final checks

### Task 6.1: Durable docs, decision log, version bump

**Files:**
- Modify: `docs/Design.RebalancePlanner.md`
- Modify: `docs/DecisionLog.md`
- Modify: `frontend/package.json` (0.16.0 → 0.17.0)

- [ ] **Step 1: Extend `docs/Design.RebalancePlanner.md`** — add a `## Per-Rung Balance Report` section after `## Per-Rung Report`, and add the new fields to the API contract list. Content to cover (durable spec, name behavior, no plan-phase references):

```markdown
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
- `total_gap_after_base = Σ|gap_after| = G'`, so coverage remains
  `100 * (G - G') / G`

Statuses reuse the shared ±5% display tolerance band from conviction targets.
Percent fields are `None`/`null` only if a candidate's target is not strictly
positive, which the planner's feasibility checks make unreachable in practice.

The rung totals `total_gap_before_base` and `total_gap_after_base` are
serialized as money strings; per-candidate percents as two-decimal strings.
```

Also document (one paragraph in the API Contract section) that a small-N rung
absorbs the full offset in few names and can push traded candidates past their
targets; the balance report exists to make that visible.

- [ ] **Step 2: Append to `docs/DecisionLog.md`** (end of file, entry template):

```markdown
## 2026-07-07: Rebalance result preview and committed-amount UX
Decision: Every rebalance rung reports per-instrument before/after target gaps measured against post-offset targets, plus total-gap sums, and the Rebalance page renders them as a before/after balance view with a warning when a trade pushes an instrument across its target band. The rebalance amount starts at 0, commits only on explicit action (Apply/Enter/blur), and the committed amount and trade-count selection persist locally across navigation.
Context: A rung that trades few instruments must absorb the entire requested offset, so traded names can overshoot their own targets and swing to the other side; the page previously gave no per-instrument feedback and recomputed on every keystroke.
Consequences: The rebalance API carries balance data per rung; the ±5% display band has a single shared definition used by both Holdings targets and rebalance statuses; rebalance page parameters are client-persisted state, while plans themselves are always recomputed against the current pool.
```

- [ ] **Step 3: Bump the frontend version** — `frontend/package.json` `"version": "0.17.0"` (run `npm install --package-lock-only` to sync `package-lock.json`). Backend was bumped in Phase 2.

- [ ] **Step 4: Full verification**

- From `backend/`: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt`
- From `frontend/`: `npm run check && npm run fmt`
- Launch the app and confirm the footer/health versions read 0.17.0 / 0.11.0.

- [ ] **Step 5: Commit** — `Document rebalance balance report and record the decision`

**Phase 6 verification / final external human test:** one pass over the whole feature on real data — default 0-amount plan on first visit, slider sweep with the balance table, a deliberate low-N + large-amount plan to see "Flips target band", navigation round trip, restart round trip.

---

## Open questions (surfaced, with chosen defaults)

None blocking. Two chosen defaults worth a conscious veto before implementation:

1. **Post-offset "before" gaps:** with a nonzero amount, `gap_before` on the Rebalance page will not equal the Holdings gap column (different denominator pool). The table header says "Gap vs plan target" to signal this. Alternative (rejected): report Holdings-basis before-gaps, which would break the `Σ gap_after = −residual` invariant and misstate what the plan aims at.
2. **localStorage over sessionStorage:** parameters survive an app restart, matching the date-range convention. Flip to sessionStorage in `storage()` if that feels wrong in use.
