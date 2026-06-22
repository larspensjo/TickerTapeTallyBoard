# Gains Breakdown Waterfall Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three-row "Gains breakdown" panel on the asset view with a stacked waterfall that builds from cost basis through the price/FX effects to a subtotal (market value / proceeds) and terminates at total return, with a per-step percentage column and a visible-but-inert dividends placeholder.

**Architecture:** A pure view-model function (`waterfallView`) turns one `GainsRow` into an ordered list of typed waterfall rows with display-only geometry and percentages; a presentational component (`GainsWaterfall`) renders them. One small backend change exposes `realized_gain_base` **and `realized_cost_basis_base`** on open rows so the open-position waterfall can reach "Total return" (= unrealized + realized) and compute its "% of cost" against a coherent denominator (held cost basis + sold cost basis). Dividends remain a placeholder row driven by the existing `income_not_tracked` reason — no income plumbing in this plan.

**Percent denominator contract (resolves review finding #3):** Each waterfall row's "% of cost" is computed against the denominator that matches its own population, so partial-sell positions never divide one population's gain by another's cost:
- Price/FX effect rows → **held cost basis** (`cost_basis_base`); these are unrealized effects on currently-held shares.
- Realized row → **sold cost basis** (`realized_cost_basis_base`); realized gain belongs to sold shares. A zero realized amount renders a calm `0.00%` (numerator-zero rule), not `n/a` from a `0/0`.
- Total-return row → **total capital deployed**: open = held + sold cost basis (`cost_basis_base + realized_cost_basis_base`); closed = `cost_basis_base` alone (which already represents the full sold cost basis, so it is *not* re-added).

**Tech Stack:** Rust (axum, rust_decimal) backend; React 19 + TypeScript + Vite + Vitest frontend; plain CSS with the dark-theme `:root` tokens.

## Global Constraints

- Backend: build with `cargo build` from `backend/`; finish with `cargo clippy --all-targets -- -D warnings` then `cargo fmt` from `backend/`.
- Frontend: finish with `npm run check` then `npm run fmt` from `frontend/` (`check` = `tsc --noEmit` + Biome + Vitest).
- Money/prices/FX are exact `rust_decimal` strings end to end; never compute money of record in JS. The waterfall's **percentage column** and the open-position **total-return sum** are display-only float math, explicitly permitted by the 2026-06-18 "Display-Only Weights May Use Frontend Float Math" decision — they must never be persisted or sent back as authoritative.
- Missing data is an explicit `Unavailable` state with reasons, never zero (2026-06-14 Missing-FX Contamination Rule). A genuine `0` is distinct from `Unavailable`.
- Base currency is SEK; value cells are labeled `SEK`.
- Visual style follows `docs/VisualDesign.DarkTheme.md`: green `--up` up-steps, red `--down` down-steps, neutral greys for base/subtotal/total, every number in `--font-mono`.
- Bump the two displayed versions: `frontend/package.json` 0.9.0 → 0.10.0 and `backend/Cargo.toml` 0.6.0 → 0.7.0.
- Stage all changes; do not commit (the per-task `git add` steps stage only — a human reviews before any commit).

---

## File Structure

- `backend/src/api/gains.rs` (modify) — add `realized_gain_base` and `realized_cost_basis_base` to `GainRow`; serialize both on open and closed rows; pass `performance.realized` into `open_gain_row`. Backend test lives in the existing `#[cfg(test)] mod tests` here.
- `frontend/src/api/types.ts` (modify) — add `realized_gain_base: MoneyValue` and `realized_cost_basis_base: MoneyValue` to `GainsRow`.
- `frontend/src/components/waterfallViewModel.ts` (create) — `WaterfallView`/`WaterfallRow` types and the pure `waterfallView(gain)` selector plus display helpers.
- `frontend/src/components/waterfallViewModel.test.ts` (create) — unit tests for the selector.
- `frontend/src/components/dashboardSelectors.test.ts` (modify) — the existing `row()` fixture helper returns `GainsRow`, so it must gain the two new fields once they are added to the wire type (review finding #2).
- `frontend/src/components/GainsWaterfall.tsx` (create) — presentational component.
- `frontend/src/components/GainsWaterfall.test.tsx` (create) — render test.
- `frontend/src/components/AssetView.tsx` (modify) — swap `AssetGainsBreakdown` for `GainsWaterfall`; delete the old `AssetGainsBreakdown`/`BreakdownRow`.
- `frontend/src/components/assetViewModel.ts` (modify) — delete `BreakdownView` type and `breakdownView()`.
- `frontend/src/styles.css` (modify) — add `.gains-waterfall` styles (design tokens only, no inline hex) plus a mobile breakpoint.
- `docs/VisualDesign.DarkTheme.md` (modify) — register the four new neutral-bar tokens (design-token source of truth).
- `docs/DecisionLog.md` (modify) — record the waterfall + realized-on-open-rows decision.

---

### Task 1: Expose `realized_gain_base` on open gains rows (backend)

**Files:**
- Modify: `backend/src/api/gains.rs` (struct `GainRow` ~line 99; `open_gain_row` ~line 613; `closed_gain_row` ~line 704; call site ~line 272)
- Test: `backend/src/api/gains.rs` (`#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: JSON field `realized_gain_base: AvailabilityResponse` on every gains row. Open rows = `performance.realized.gain_base` (literal `0.00` when never sold); closed rows = the realized gain (same value as `total_return_base`).
- Produces: JSON field `realized_cost_basis_base: AvailabilityResponse` on every gains row — the cost basis of the *sold* shares. Open rows = `performance.realized.cost_basis_base` (literal `0.00` when never sold); closed rows = `realized.cost_basis_base` (same value as their `cost_basis_base`). The frontend uses this to build the broadened total-return denominator (review finding #3).

- [ ] **Step 1: Write the failing test**

Add this test inside the `mod tests` block in `backend/src/api/gains.rs`:

```rust
    #[tokio::test]
    async fn gains_open_row_exposes_realized_gain_base() {
        let state = AppState::for_tests().await;
        let sold_id = instrument(&state, "SELLER", "STO", BASE_CURRENCY).await;
        let never_id = instrument(&state, "HOLDER", "STO", BASE_CURRENCY).await;

        // SELLER: buy 10 @100, sell 4 @150 (SEK, no fees) -> realized (150-100)*4 = 200, 6 open.
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":sold_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":BASE_CURRENCY}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":sold_id,"type":"Sell","trade_date":"2026-06-05",
                   "quantity":4,"price":"150","currency":BASE_CURRENCY}),
        )
        .await;
        // HOLDER: buy only, never sold -> realized 0.
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":never_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":5,"price":"100","currency":BASE_CURRENCY}),
        )
        .await;

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);

        let rows = body["rows"].as_array().expect("rows");
        let sold = rows
            .iter()
            .find(|r| r["instrument"]["symbol"] == "SELLER")
            .expect("seller row");
        let never = rows
            .iter()
            .find(|r| r["instrument"]["symbol"] == "HOLDER")
            .expect("holder row");

        assert_eq!(sold["position_status"], "open");
        assert_eq!(sold["quantity"], 6);
        assert_available(&sold["realized_gain_base"], "200.00");
        // Sold 4 @ cost 100 -> sold cost basis 400.00.
        assert_available(&sold["realized_cost_basis_base"], "400.00");
        assert_eq!(never["position_status"], "open");
        assert_available(&never["realized_gain_base"], "0.00");
        assert_available(&never["realized_cost_basis_base"], "0.00");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path backend/Cargo.toml gains_open_row_exposes_realized_gain_base`
Expected: FAIL to compile — `GainRow` has no field `realized_gain_base` (and the JSON key is absent).

- [ ] **Step 3: Add the field to the response struct**

In `backend/src/api/gains.rs`, in `pub struct GainRow`, add the field next to the other unrealized fields (after `pub unrealized_gain_percent: AvailabilityResponse,`):

```rust
    pub unrealized_gain_percent: AvailabilityResponse,
    pub realized_gain_base: AvailabilityResponse,
    pub realized_cost_basis_base: AvailabilityResponse,
```

- [ ] **Step 4: Populate it on open rows**

Change `open_gain_row` to accept the realized gain. Update its signature:

```rust
fn open_gain_row(
    instrument: &instruments::InstrumentRow,
    valued_holding: &ValuedHolding,
    realized: &RealizedGain,
    performance_start_date: Option<NaiveDate>,
) -> Result<GainRow, ApiError> {
```

Add the field to the returned struct, right after the `unrealized_gain_percent` line in `open_gain_row`:

```rust
        unrealized_gain_percent: serialize_availability(
            &valued_holding.unrealized_gain_percent,
            |v| format!("{:.2}", v),
        ),
        realized_gain_base: serialize_base_amount(&realized.gain_base),
        realized_cost_basis_base: serialize_base_amount(&realized.cost_basis_base),
```

Update the call site (in `list`, the open branch ~line 272):

```rust
        gain_rows.push(open_gain_row(
            instrument,
            &valued_holding,
            &performance.realized,
            performance_start_date,
        )?);
```

- [ ] **Step 5: Populate it on closed rows**

In `closed_gain_row`, add the field right after the `unrealized_gain_percent` line (reusing the already-computed `gain_base`):

```rust
        unrealized_gain_percent: serialize_availability(&gain_percent, |v| format!("{:.2}", v)),
        realized_gain_base: serialize_availability(&gain_base, |v| money_string(*v)),
        realized_cost_basis_base: serialize_availability(&cost_basis_base, |v| money_string(*v)),
```

(`cost_basis_base` is the realized cost-basis availability already bound at the top of `closed_gain_row`.)

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test --manifest-path backend/Cargo.toml gains_open_row_exposes_realized_gain_base`
Expected: PASS.

- [ ] **Step 7: Run the full gains test module to catch regressions**

Run: `cargo test --manifest-path backend/Cargo.toml --lib api::gains`
Expected: PASS (all existing gains tests still green).

- [ ] **Step 8: Lint, format, commit (staged)**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt && cd ..
git add backend/src/api/gains.rs
git commit -m "feat(gains): expose realized_gain_base on open rows

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Add `realized_gain_base` to the wire type and waterfall view model (frontend)

**Files:**
- Modify: `frontend/src/api/types.ts` (`GainsRow` ~line 152)
- Create: `frontend/src/components/waterfallViewModel.ts`
- Create: `frontend/src/components/waterfallViewModel.test.ts`

**Interfaces:**
- Consumes: `GainsRow` including the new `realized_gain_base: MoneyValue` and `realized_cost_basis_base: MoneyValue`.
- Produces:
  - `WaterfallKind = "base" | "effect" | "subtotal" | "total" | "placeholder"`
  - `WaterfallDirection = "up" | "down" | "flat"`
  - `WaterfallRow = { key: string; label: string; kind: WaterfallKind; value: MoneyValue; direction: WaterfallDirection | null; percent: PercentValue | null; span: { from: number; to: number } | null }`
  - `WaterfallView = { mode: "open" | "closed"; currency: string; rows: WaterfallRow[]; minValue: number; maxValue: number }` — `minValue`/`maxValue` define the normalized geometry domain so bars that cross below zero render correctly (review finding #4).
  - `waterfallView(gain: GainsRow): WaterfallView`

- [ ] **Step 1: Add the field to the wire type**

In `frontend/src/api/types.ts`, inside `interface GainsRow`, add after `unrealized_gain_percent: PercentValue;`:

```ts
  unrealized_gain_percent: PercentValue;
  realized_gain_base: MoneyValue;
  realized_cost_basis_base: MoneyValue;
```

- [ ] **Step 1b: Fix the existing `GainsRow` fixtures (review finding #2)**

Adding two required fields to `GainsRow` makes every existing `GainsRow` object literal fail `tsc`. The `row()` helper in `frontend/src/components/dashboardSelectors.test.ts` (and the `mvRow` wrapper that spreads it) returns a `GainsRow`, so add both fields to the `row()` helper's returned object, next to `unrealized_gain_percent`:

```ts
    unrealized_gain_percent: money,
    realized_gain_base: money,
    realized_cost_basis_base: money,
```

Then grep the frontend for any other `GainsRow` literals before running `npm run check`:

```bash
cd frontend && grep -rn "GainsRow" src --include=*.ts --include=*.tsx
```

Expected: the only object-literal producers are `dashboardSelectors.test.ts` (fixed above) and the new `waterfallViewModel.test.ts` (written in Step 2). The production files (`Dashboard.tsx`, `GainsTable.tsx`, `PortfolioSummary.tsx`, `assetViewModel.ts`, `AssetView.tsx`, `dashboardSelectors.ts`, `api/types.ts`) only consume the type and need no change.

- [ ] **Step 2: Write the failing tests**

Create `frontend/src/components/waterfallViewModel.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { GainsRow, MoneyValue } from "../api/types";
import { waterfallView } from "./waterfallViewModel";

const money = (value: string): MoneyValue => ({ status: "available", value });
const missing = (...reasons: string[]): MoneyValue => ({
  status: "unavailable",
  reasons,
});

function openGain(overrides: Partial<GainsRow> = {}): GainsRow {
  return {
    instrument: {
      id: 1,
      symbol: "ANET",
      exchange: "NYSE",
      name: "Arista",
      type: "Stock",
      currency: "USD",
    },
    quantity: 100,
    cost_basis_native: "0",
    cost_basis_base: money("265582.94"),
    performance_start_date: null,
    performance_denominator_base: money("265582.94"),
    capital_gain_base: money("53546.54"),
    capital_gain_percent: money("20.16"),
    currency_gain_base: money("9418.19"),
    currency_gain_percent: money("3.55"),
    total_return_base: money("62964.73"),
    total_return_percent: money("23.71"),
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money("0"),
    market_value_base: money("328547.67"),
    proceeds_native: missing(),
    proceeds_base: missing(),
    unrealized_gain_base: money("62964.73"),
    unrealized_gain_percent: money("23.71"),
    realized_gain_base: money("0.00"),
    realized_cost_basis_base: money("0.00"),
    price_effect_base: money("53546.54"),
    fx_effect_base: money("9418.19"),
    day_change_base: money("0"),
    day_change_percent: money("0"),
    reasons: [],
    position_status: "open",
    ...overrides,
  };
}

describe("waterfallView (open)", () => {
  it("builds the open ladder ending at total return = unrealized + realized", () => {
    const view = waterfallView(openGain());
    expect(view.mode).toBe("open");
    expect(view.rows.map((r) => [r.key, r.kind, r.label])).toEqual([
      ["cost-basis", "base", "Cost basis (held)"],
      ["price", "effect", "Price effect"],
      ["fx", "effect", "FX effect"],
      ["market-value", "subtotal", "Market value"],
      ["realized", "effect", "Realized gain"],
      ["dividends", "placeholder", "Dividends"],
      ["total-return", "total", "Total return"],
    ]);

    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "62964.73" });
    // delta bar floats from cost basis to cost basis + total return
    expect(total?.span).toEqual({ from: 265582.94, to: 328547.67 });
    expect(total?.direction).toBe("up");
  });

  it("colors steps by sign and computes percent against cost basis", () => {
    const view = waterfallView(openGain({ price_effect_base: money("-1000.00") }));
    const price = view.rows.find((r) => r.key === "price");
    expect(price?.direction).toBe("down");
    expect(price?.percent).toEqual({ status: "available", value: "-0.38" });
  });

  it("treats zero realized as flat with a 0.00 percent (de-emphasized, not unavailable)", () => {
    const realized = waterfallView(openGain()).rows.find((r) => r.key === "realized");
    expect(realized?.direction).toBe("flat");
    expect(realized?.value).toEqual({ status: "available", value: "0.00" });
    expect(realized?.percent).toEqual({ status: "available", value: "0.00" });
  });

  it("sums realized into total return when there were sells", () => {
    const view = waterfallView(openGain({ realized_gain_base: money("200.00") }));
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "63164.73" });
  });

  it("uses population-matched denominators for a partial sell (review finding #3)", () => {
    // Sold 4 shares at cost 400; the realized row's % is gain / sold cost basis.
    // The total-return row's % is total return / (held cost basis + sold cost basis).
    const view = waterfallView(
      openGain({
        realized_gain_base: money("200.00"),
        realized_cost_basis_base: money("400.00"),
      }),
    );
    const realized = view.rows.find((r) => r.key === "realized");
    expect(realized?.percent).toEqual({ status: "available", value: "50.00" });

    const total = view.rows.find((r) => r.key === "total-return");
    // 63164.73 / (265582.94 + 400) * 100 = 23.75
    expect(total?.percent).toEqual({ status: "available", value: "23.75" });
    // Price/FX effect rows keep the held-cost denominator.
    const price = view.rows.find((r) => r.key === "price");
    expect(price?.percent).toEqual({ status: "available", value: "20.16" });
  });

  it("exposes a domain that drops below zero when total return wipes out cost basis", () => {
    // A partially-sold position with a realized loss larger than the held cost basis:
    // cost basis 1000, total return -1500 -> total span ends at -500.
    const view = waterfallView(
      openGain({
        cost_basis_base: money("1000.00"),
        unrealized_gain_base: money("-1500.00"),
        realized_gain_base: money("0.00"),
        realized_cost_basis_base: money("0.00"),
        market_value_base: money("-500.00"),
        price_effect_base: money("-1500.00"),
        fx_effect_base: money("0.00"),
      }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.span).toEqual({ from: 1000, to: -500 });
    expect(view.minValue).toBeLessThanOrEqual(-500);
  });

  it("renders the dividends placeholder as inert and not contributing", () => {
    const dividends = waterfallView(openGain()).rows.find((r) => r.key === "dividends");
    expect(dividends?.kind).toBe("placeholder");
    expect(dividends?.span).toBeNull();
    expect(dividends?.percent).toBeNull();
    expect(dividends?.value).toEqual({
      status: "unavailable",
      reasons: ["income_not_tracked"],
    });
  });

  it("renders an unavailable effect with no bar and an unavailable percent", () => {
    const view = waterfallView(openGain({ fx_effect_base: missing("missing_fx") }));
    const fx = view.rows.find((r) => r.key === "fx");
    expect(fx?.span).toBeNull();
    expect(fx?.direction).toBeNull();
    expect(fx?.percent).toEqual({ status: "unavailable", reasons: ["missing_fx"] });
  });
});

describe("waterfallView (closed)", () => {
  it("pivots to proceeds and terminates at realized total return", () => {
    const view = waterfallView(
      openGain({
        position_status: "closed",
        market_value_base: money("0.00"),
        proceeds_base: money("13195.00"),
        cost_basis_base: money("10020.00"),
        price_effect_base: money("2175.00"),
        fx_effect_base: money("1000.00"),
        unrealized_gain_base: money("3175.00"),
        realized_gain_base: money("3175.00"),
        realized_cost_basis_base: money("10020.00"),
        total_return_base: money("3175.00"),
      }),
    );
    expect(view.mode).toBe("closed");
    expect(view.rows.map((r) => [r.key, r.kind, r.label])).toEqual([
      ["cost-basis", "base", "Cost basis (sold)"],
      ["price", "effect", "Price effect"],
      ["fx", "effect", "FX effect"],
      ["proceeds", "subtotal", "Proceeds"],
      ["dividends", "placeholder", "Dividends"],
      ["total-return", "total", "Total return"],
    ]);
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "3175.00" });
    expect(total?.span).toEqual({ from: 10020, to: 13195 });
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd frontend && npx vitest run src/components/waterfallViewModel.test.ts`
Expected: FAIL — `Cannot find module './waterfallViewModel'`.

- [ ] **Step 4: Implement the view model**

Create `frontend/src/components/waterfallViewModel.ts`:

```ts
import type { GainsRow, MoneyValue, PercentValue } from "../api/types";

export type WaterfallKind = "base" | "effect" | "subtotal" | "total" | "placeholder";
export type WaterfallDirection = "up" | "down" | "flat";

export interface WaterfallRow {
  key: string;
  label: string;
  kind: WaterfallKind;
  value: MoneyValue;
  /** Coloring hint for effect/total rows; null for base/subtotal/placeholder. */
  direction: WaterfallDirection | null;
  /** Display-only percent vs cost basis; null when the row has no meaningful percent. */
  percent: PercentValue | null;
  /** Display-only bar geometry in base-currency units; null when there is no bar. */
  span: { from: number; to: number } | null;
}

export interface WaterfallView {
  mode: "open" | "closed";
  currency: string;
  rows: WaterfallRow[];
  /** Normalized geometry domain (base-currency units); minValue ≤ 0 ≤ maxValue. */
  minValue: number;
  maxValue: number;
}

const CURRENCY = "SEK";

function toNumber(value: MoneyValue): number | null {
  if (value.status !== "available") {
    return null;
  }
  const parsed = Number(value.value);
  return Number.isFinite(parsed) ? parsed : null;
}

function directionOf(n: number): WaterfallDirection {
  if (n > 0) return "up";
  if (n < 0) return "down";
  return "flat";
}

// Display-only percentage vs a population-matched cost basis (decision 2026-06-18,
// denominator contract from review finding #3). Never money of record.
function displayPercent(value: MoneyValue, costBasis: MoneyValue): PercentValue {
  if (value.status !== "available") {
    return { status: "unavailable", reasons: value.reasons };
  }
  const v = Number(value.value);
  // A zero numerator is a calm 0.00%, even when the matching cost basis is also zero
  // (e.g. a never-sold position's realized row): never surface 0/0 as "n/a".
  if (Number.isFinite(v) && v === 0) {
    return { status: "available", value: "0.00" };
  }
  if (costBasis.status !== "available") {
    return { status: "unavailable", reasons: costBasis.reasons };
  }
  const cb = Number(costBasis.value);
  if (!Number.isFinite(v) || !Number.isFinite(cb)) {
    return { status: "unavailable", reasons: ["unavailable"] };
  }
  if (cb === 0) {
    return { status: "unavailable", reasons: ["zero_cost_basis"] };
  }
  return { status: "available", value: ((v / cb) * 100).toFixed(2) };
}

// Display-only sum for the open-position total-return terminus (decision 2026-06-18).
function displaySum(a: MoneyValue, b: MoneyValue): MoneyValue {
  if (a.status !== "available") return a;
  if (b.status !== "available") return b;
  return { status: "available", value: (Number(a.value) + Number(b.value)).toFixed(2) };
}

function levelRow(
  key: string,
  label: string,
  kind: "base" | "subtotal",
  value: MoneyValue,
): WaterfallRow {
  const n = toNumber(value);
  return {
    key,
    label,
    kind,
    value,
    direction: null,
    percent: null,
    span: n === null ? null : { from: 0, to: n },
  };
}

function placeholderRow(key: string, label: string): WaterfallRow {
  return {
    key,
    label,
    kind: "placeholder",
    value: { status: "unavailable", reasons: ["income_not_tracked"] },
    direction: null,
    percent: null,
    span: null,
  };
}

// Pushes an effect row, advances the running total, and returns the new running total.
// An unavailable effect renders without a bar and does not move the running total.
function pushEffect(
  rows: WaterfallRow[],
  key: string,
  label: string,
  value: MoneyValue,
  costBasis: MoneyValue,
  running: number,
): number {
  const n = toNumber(value);
  if (n === null) {
    rows.push({
      key,
      label,
      kind: "effect",
      value,
      direction: null,
      percent: displayPercent(value, costBasis),
      span: null,
    });
    return running;
  }
  const to = running + n;
  rows.push({
    key,
    label,
    kind: "effect",
    value,
    direction: directionOf(n),
    percent: displayPercent(value, costBasis),
    span: { from: running, to },
  });
  return to;
}

// `denominator` drives the display-only % (population-matched, finding #3); `baseline`
// is the held cost basis the delta bar floats from. They differ for an open partial sell.
function totalRow(
  label: string,
  value: MoneyValue,
  denominator: MoneyValue,
  baseline: MoneyValue,
): WaterfallRow {
  const amount = toNumber(value);
  const base = toNumber(baseline);
  const span =
    amount === null || base === null ? null : { from: base, to: base + amount };
  return {
    key: "total-return",
    label,
    kind: "total",
    value,
    direction: amount === null ? null : directionOf(amount),
    percent: displayPercent(value, denominator),
    span,
  };
}

// Normalized geometry domain. Tracks both a minimum and a maximum so a span that crosses
// below zero (a realized loss larger than the held cost basis) still renders in-track
// (review finding #4). `minValue` is clamped at 0 or below so the baseline stays anchored.
function computeDomain(rows: WaterfallRow[]): { minValue: number; maxValue: number } {
  let min = 0;
  let max = 0;
  for (const row of rows) {
    if (row.span) {
      min = Math.min(min, row.span.from, row.span.to);
      max = Math.max(max, row.span.from, row.span.to);
    }
  }
  if (max <= min) {
    return { minValue: min, maxValue: min + 1 };
  }
  // Pad the high end slightly so the tallest bar never touches the track edge.
  return { minValue: min, maxValue: max + (max - min) * 0.02 };
}

function openWaterfall(gain: GainsRow): WaterfallView {
  const costBasis = gain.cost_basis_base;
  const rows: WaterfallRow[] = [];
  let running = toNumber(costBasis) ?? 0;

  rows.push(levelRow("cost-basis", "Cost basis (held)", "base", costBasis));
  running = pushEffect(rows, "price", "Price effect", gain.price_effect_base, costBasis, running);
  running = pushEffect(rows, "fx", "FX effect", gain.fx_effect_base, costBasis, running);
  rows.push(levelRow("market-value", "Market value", "subtotal", gain.market_value_base));
  // Realized gain belongs to sold shares: its % is vs the sold cost basis.
  pushEffect(
    rows,
    "realized",
    "Realized gain",
    gain.realized_gain_base,
    gain.realized_cost_basis_base,
    running,
  );
  rows.push(placeholderRow("dividends", "Dividends"));

  const totalReturn = displaySum(gain.unrealized_gain_base, gain.realized_gain_base);
  // Total-return % is vs total capital deployed = held + sold cost basis; the delta bar
  // still floats from the held cost basis baseline.
  const totalCostBasis = displaySum(costBasis, gain.realized_cost_basis_base);
  rows.push(totalRow("Total return", totalReturn, totalCostBasis, costBasis));

  const { minValue, maxValue } = computeDomain(rows);
  return { mode: "open", currency: CURRENCY, rows, minValue, maxValue };
}

function closedWaterfall(gain: GainsRow): WaterfallView {
  const costBasis = gain.cost_basis_base;
  const rows: WaterfallRow[] = [];
  let running = toNumber(costBasis) ?? 0;

  rows.push(levelRow("cost-basis", "Cost basis (sold)", "base", costBasis));
  running = pushEffect(rows, "price", "Price effect", gain.price_effect_base, costBasis, running);
  running = pushEffect(rows, "fx", "FX effect", gain.fx_effect_base, costBasis, running);
  rows.push(levelRow("proceeds", "Proceeds", "subtotal", gain.proceeds_base));
  rows.push(placeholderRow("dividends", "Dividends"));
  // Closed: cost_basis_base already represents the full sold cost basis, so it serves as
  // both the denominator and the baseline (do not re-add realized_cost_basis_base).
  rows.push(totalRow("Total return", gain.total_return_base, costBasis, costBasis));

  const { minValue, maxValue } = computeDomain(rows);
  return { mode: "closed", currency: CURRENCY, rows, minValue, maxValue };
}

export function waterfallView(gain: GainsRow): WaterfallView {
  return gain.position_status === "closed" ? closedWaterfall(gain) : openWaterfall(gain);
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd frontend && npx vitest run src/components/waterfallViewModel.test.ts`
Expected: PASS (all cases).

- [ ] **Step 6: Commit (staged)**

```bash
git add frontend/src/api/types.ts frontend/src/components/waterfallViewModel.ts frontend/src/components/waterfallViewModel.test.ts frontend/src/components/dashboardSelectors.test.ts
git commit -m "feat(asset): add gains waterfall view model

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Waterfall component and CSS (frontend)

**Files:**
- Create: `frontend/src/components/GainsWaterfall.tsx`
- Create: `frontend/src/components/GainsWaterfall.test.tsx`
- Modify: `frontend/src/styles.css` (new `:root` tokens, `.gains-waterfall` styles, mobile breakpoint)
- Modify: `docs/VisualDesign.DarkTheme.md` (register the four new neutral-bar tokens)

**Interfaces:**
- Consumes: `WaterfallView`, `WaterfallRow` from `waterfallViewModel`; `SummaryAvailabilityValue` from `valuationDisplay`.
- Produces: `export function GainsWaterfall({ view }: { view: WaterfallView })`.

- [ ] **Step 1: Write the failing render test**

Create `frontend/src/components/GainsWaterfall.test.tsx`:

```tsx
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { GainsWaterfall } from "./GainsWaterfall";
import type { WaterfallView } from "./waterfallViewModel";

const view: WaterfallView = {
  mode: "open",
  currency: "SEK",
  minValue: 0,
  maxValue: 335000,
  rows: [
    {
      key: "cost-basis",
      label: "Cost basis (held)",
      kind: "base",
      value: { status: "available", value: "265582.94" },
      direction: null,
      percent: null,
      span: { from: 0, to: 265582.94 },
    },
    {
      key: "price",
      label: "Price effect",
      kind: "effect",
      value: { status: "available", value: "53546.54" },
      direction: "up",
      percent: { status: "available", value: "20.16" },
      span: { from: 265582.94, to: 319129.48 },
    },
    {
      key: "dividends",
      label: "Dividends",
      kind: "placeholder",
      value: { status: "unavailable", reasons: ["income_not_tracked"] },
      direction: null,
      percent: null,
      span: null,
    },
    {
      key: "total-return",
      label: "Total return",
      kind: "total",
      value: { status: "available", value: "62964.73" },
      direction: "up",
      percent: { status: "available", value: "23.71" },
      span: { from: 265582.94, to: 328547.67 },
    },
  ],
};

describe("GainsWaterfall", () => {
  it("renders each row label, the currency header, and the dividends placeholder", () => {
    render(<GainsWaterfall view={view} />);
    expect(screen.getByText("Cost basis (held)")).toBeInTheDocument();
    expect(screen.getByText("Total return")).toBeInTheDocument();
    expect(screen.getByText("SEK")).toBeInTheDocument();
    expect(screen.getByText("% of cost")).toBeInTheDocument();
    // Dividends placeholder is a calm "not tracked" note, not a warning chip.
    expect(screen.getByText("Not tracked yet")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run src/components/GainsWaterfall.test.tsx`
Expected: FAIL — `Cannot find module './GainsWaterfall'`.

- [ ] **Step 3: Implement the component**

Create `frontend/src/components/GainsWaterfall.tsx`:

```tsx
import { SummaryAvailabilityValue } from "./valuationDisplay";
import type { WaterfallRow, WaterfallView } from "./waterfallViewModel";

function barGeometry(
  span: { from: number; to: number },
  minValue: number,
  maxValue: number,
): { left: number; width: number } {
  const domain = maxValue - minValue || 1;
  const lo = Math.min(span.from, span.to);
  const hi = Math.max(span.from, span.to);
  return {
    left: ((lo - minValue) / domain) * 100,
    width: Math.max(((hi - lo) / domain) * 100, 0.6),
  };
}

function barClass(row: WaterfallRow): string {
  if (row.kind === "base") return "wf-bar base";
  if (row.kind === "subtotal") return "wf-bar subtotal";
  if (row.kind === "total") return "wf-bar total";
  if (row.direction === "up") return "wf-bar up";
  if (row.direction === "down") return "wf-bar down";
  return "wf-bar flat";
}

function Track({
  row,
  minValue,
  maxValue,
}: {
  row: WaterfallRow;
  minValue: number;
  maxValue: number;
}) {
  if (!row.span) {
    if (row.kind === "placeholder") {
      return <div className="wf-track" />;
    }
    return (
      <div className="wf-track">
        <div className="wf-bar unavailable" />
      </div>
    );
  }
  const { left, width } = barGeometry(row.span, minValue, maxValue);
  return (
    <div className="wf-track">
      <div className={barClass(row)} style={{ left: `${left}%`, width: `${width}%` }} />
    </div>
  );
}

function ValueCell({ row }: { row: WaterfallRow }) {
  if (row.kind === "placeholder") {
    return <span className="wf-placeholder">Not tracked yet</span>;
  }
  const tone = row.kind === "base" || row.kind === "subtotal" ? "plain" : "signed";
  return <SummaryAvailabilityValue value={row.value} tone={tone} />;
}

function PercentCell({ row }: { row: WaterfallRow }) {
  if (row.percent === null) {
    return <span className="wf-pct-empty" />;
  }
  if (row.percent.status !== "available") {
    return <span className="wf-pct muted">n/a</span>;
  }
  const sign = Number(row.percent.value) > 0 ? "+" : "";
  const tone =
    Number(row.percent.value) > 0 ? "up" : Number(row.percent.value) < 0 ? "down" : "flat";
  return (
    <span className={`wf-pct ${tone}`}>
      {sign}
      {row.percent.value}%
    </span>
  );
}

export function GainsWaterfall({ view }: { view: WaterfallView }) {
  return (
    <section className="panel asset-panel gains-waterfall" aria-label="Gains breakdown">
      <h2>Gains breakdown</h2>
      <div className="wf-head">
        <span className="wf-col-amount">{view.currency}</span>
        <span className="wf-col-pct">% of cost</span>
      </div>
      <div className="wf-rows">
        {view.rows.map((row) => (
          <div
            key={row.key}
            className={`wf-row kind-${row.kind}${row.kind === "placeholder" ? " is-muted" : ""}`}
          >
            <span className="wf-label">{row.label}</span>
            <Track row={row} minValue={view.minValue} maxValue={view.maxValue} />
            <span className="wf-value">
              <ValueCell row={row} />
            </span>
            <PercentCell row={row} />
          </div>
        ))}
      </div>
    </section>
  );
}
```

- [ ] **Step 4: Add the neutral-bar tokens to `:root`**

`docs/VisualDesign.DarkTheme.md` forbids inline hex; the waterfall introduces three neutral bar fills and a hatch stripe that have no existing token, so add them as an intentional token extension. In `frontend/src/styles.css`, inside the existing `:root { … }` block (next to `--surface-2`/`--hairline`), add:

```css
  /* Gains waterfall neutral bars (base < subtotal < total, progressively lighter) */
  --wf-bar-base: #2b3138;
  --wf-bar-subtotal: #39414b;
  --wf-bar-total: #4a525d;
  --wf-hatch-stripe: #353b44;
```

Then record the four new tokens in `docs/VisualDesign.DarkTheme.md` so the token list stays the single source of truth.

- [ ] **Step 5: Add the CSS**

Append to `frontend/src/styles.css`:

```css
/* Gains breakdown waterfall (asset view) */
.gains-waterfall .wf-head {
  display: grid;
  grid-template-columns: 110px 1fr 96px 64px;
  column-gap: var(--space-3);
  margin-bottom: var(--space-1);
}
.gains-waterfall .wf-head .wf-col-amount,
.gains-waterfall .wf-head .wf-col-pct {
  grid-column: auto;
  text-align: right;
  font-size: 11px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-muted);
}
.gains-waterfall .wf-head .wf-col-amount {
  grid-column: 3;
}
.gains-waterfall .wf-head .wf-col-pct {
  grid-column: 4;
}
.gains-waterfall .wf-rows {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}
.gains-waterfall .wf-row {
  display: grid;
  grid-template-columns: 110px 1fr 96px 64px;
  align-items: center;
  column-gap: var(--space-3);
  min-height: 30px;
}
.gains-waterfall .wf-label {
  font-size: 13px;
  color: var(--text-secondary);
}
.gains-waterfall .wf-row.kind-subtotal .wf-label,
.gains-waterfall .wf-row.kind-total .wf-label {
  color: var(--text-primary);
  font-weight: 600;
}
.gains-waterfall .wf-row.kind-subtotal,
.gains-waterfall .wf-row.kind-total {
  border-top: 1px solid var(--hairline);
  padding-top: var(--space-2);
}
.gains-waterfall .wf-row.is-muted {
  opacity: 0.7;
}
.gains-waterfall .wf-track {
  position: relative;
  height: 22px;
}
.gains-waterfall .wf-bar {
  position: absolute;
  top: 3px;
  height: 16px;
  border-radius: 3px;
}
.gains-waterfall .wf-bar.base {
  background: var(--wf-bar-base);
}
.gains-waterfall .wf-bar.subtotal {
  background: var(--wf-bar-subtotal);
}
.gains-waterfall .wf-bar.total {
  background: var(--wf-bar-total);
}
.gains-waterfall .wf-bar.up {
  background: var(--up);
}
.gains-waterfall .wf-bar.down {
  background: var(--down);
}
.gains-waterfall .wf-bar.flat {
  background: var(--wf-bar-base);
  opacity: 0.5;
}
.gains-waterfall .wf-bar.unavailable {
  left: 0;
  width: 24px;
  background: repeating-linear-gradient(
    45deg,
    var(--wf-hatch-stripe),
    var(--wf-hatch-stripe) 4px,
    var(--surface-1) 4px,
    var(--surface-1) 8px
  );
  border: 1px dashed var(--text-muted);
}
.gains-waterfall .wf-value {
  text-align: right;
}
.gains-waterfall .wf-placeholder {
  font-size: 12px;
  color: var(--text-muted);
  font-style: italic;
}
.gains-waterfall .wf-pct {
  text-align: right;
  font-family: var(--font-mono);
  font-weight: 500;
  font-size: 13px;
}
.gains-waterfall .wf-pct.up {
  color: var(--up);
}
.gains-waterfall .wf-pct.down {
  color: var(--down);
}
.gains-waterfall .wf-pct.flat,
.gains-waterfall .wf-pct.muted {
  color: var(--text-muted);
}

/* The 4-column grid reserves ~306px before the track gets any width; on narrow panels
   (the asset two-column layout collapses at minmax(280px, 1fr)) shrink the fixed
   columns and gaps so the track never overflows. (review finding #5) */
@media (max-width: 640px) {
  .gains-waterfall .wf-head,
  .gains-waterfall .wf-row {
    grid-template-columns: 84px 1fr 72px 52px;
    column-gap: var(--space-2);
  }
  .gains-waterfall .wf-label {
    font-size: 12px;
  }
  .gains-waterfall .wf-pct {
    font-size: 12px;
  }
}
```

Verify the layout at the 640px mobile breakpoint and at roughly a 280px panel width (the narrowest the `.asset-two-col` grid produces before it stacks): the track must keep a positive width and no column should overflow its cell.

- [ ] **Step 6: Run the render test to verify it passes**

Run: `cd frontend && npx vitest run src/components/GainsWaterfall.test.tsx`
Expected: PASS.

- [ ] **Step 7: Commit (staged)**

```bash
git add frontend/src/components/GainsWaterfall.tsx frontend/src/components/GainsWaterfall.test.tsx frontend/src/styles.css docs/VisualDesign.DarkTheme.md
git commit -m "feat(asset): add gains waterfall component and styles

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Wire the waterfall into AssetView and remove the old breakdown

**Files:**
- Modify: `frontend/src/components/AssetView.tsx` (imports; render site ~line 129; delete `AssetGainsBreakdown` ~line 407 and `BreakdownRow` ~line 427)
- Modify: `frontend/src/components/assetViewModel.ts` (delete `BreakdownView` type ~line 52 and `breakdownView()` ~line 186)

**Interfaces:**
- Consumes: `waterfallView` from `waterfallViewModel`, `GainsWaterfall` from `GainsWaterfall`.

- [ ] **Step 1: Swap the import**

In `frontend/src/components/AssetView.tsx`, update the `assetViewModel` import block to drop `BreakdownView`/`breakdownView`:

```ts
import {
  deriveAssetData,
  headerStatus,
  parseInstrumentId,
  type Tiles,
  tilesView,
} from "./assetViewModel";
```

Add two new imports near the other component imports:

```ts
import { GainsWaterfall } from "./GainsWaterfall";
import { waterfallView } from "./waterfallViewModel";
```

- [ ] **Step 2: Swap the render site**

Replace the `AssetGainsBreakdown` usage (inside the `asset-two-col` div):

```tsx
          <div className="asset-two-col">
            <GainsWaterfall view={waterfallView(data.gain)} />
            <AssetDataMapping gain={data.gain} priceStatus={data.priceStatus} />
          </div>
```

- [ ] **Step 3: Delete the dead components**

In `frontend/src/components/AssetView.tsx`, delete the entire `AssetGainsBreakdown` function and the entire `BreakdownRow` function (the two functions spanning roughly lines 407-444).

- [ ] **Step 4: Delete the dead view model**

In `frontend/src/components/assetViewModel.ts`, delete the `BreakdownView` interface (lines ~52-57) and the `breakdownView` function (lines ~186-194).

- [ ] **Step 5: Verify the build and tests are green**

Run: `cd frontend && npm run check`
Expected: PASS — no `tsc` errors (no remaining references to `breakdownView`/`BreakdownView`/`AssetGainsBreakdown`), Biome clean, all Vitest tests pass.

- [ ] **Step 6: Commit (staged)**

```bash
git add frontend/src/components/AssetView.tsx frontend/src/components/assetViewModel.ts
git commit -m "feat(asset): replace gains breakdown list with waterfall

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Versions, decision log, and final verification

**Files:**
- Modify: `frontend/package.json` (`version`)
- Modify: `backend/Cargo.toml` (`version`)
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Bump versions**

In `frontend/package.json` set `"version": "0.10.0"`. In `backend/Cargo.toml` set `version = "0.7.0"`.

- [ ] **Step 2: Add the decision-log entry**

Append to `docs/DecisionLog.md`:

```markdown
## 2026-06-22 - Gains Breakdown Is A Waterfall; Realized Gain On Open Rows
Decision: The asset view's "Gains breakdown" panel is a stacked waterfall built from one `GainsRow`: cost basis → price effect → FX effect → subtotal (market value for open, proceeds for closed) → realized gain (open only) → a dividends placeholder → total return. The total-return bar is a delta floating from the cost-basis baseline so its height equals the return. To support the open-position terminus and a coherent percent column, `GET /api/gains` now serializes `realized_gain_base` and `realized_cost_basis_base` on open rows (= `performance.realized.gain_base` / `.cost_basis_base`, literal `0.00` when never sold); closed rows carry both too (realized gain, and the sold cost basis = their `cost_basis_base`). The per-step "% of cost" column uses population-matched denominators: price/FX effects vs held cost basis, the realized step vs the sold cost basis, and total return vs total capital deployed (held + sold cost basis for open; the closed row's `cost_basis_base` already is the full sold cost basis). A zero numerator renders `0.00%`, never `0/0`. The geometry exposes a `minValue`/`maxValue` domain so a realized loss larger than the held cost basis still renders in-track. The "% of cost" column and the open-position total-return amount (unrealized + realized) are display-only frontend float math under the 2026-06-18 decision, never money of record. Dividends remain an inert placeholder driven by the existing `income_not_tracked` reason — no per-instrument income plumbing yet.
Context: The prior two-line breakdown showed only the unrealized split. The waterfall makes the buildup legible and was chosen from three mockups (`docs/Design.GainsBreakdownWaterfall.md`). The population-matched denominator and below-zero geometry resolve review findings #3 and #4. Fees-as-a-step and real dividend income are deferred follow-on phases.
Consequences: `GainsRow` gains `realized_gain_base` and `realized_cost_basis_base` fields (additive). The asset breakdown reads realized and renders a total-return terminus; the Gains table's existing `total_return_base` open-row semantics (current-position unrealized, 2026-06-21) are unchanged. The waterfall adds four neutral-bar design tokens to the dark theme. Fees split and per-instrument dividend income remain separate future work; when income lands, the dividends placeholder becomes a contributing step.
```

- [ ] **Step 3: Full backend verification (required workflow commands)**

Run: `cd backend && cargo clippy --all-targets -- -D warnings && cargo test --lib api::gains && cargo fmt && cd ..`
Expected: clippy clean, gains tests PASS, `cargo fmt` applies cleanly (per Agents.md the completion step is `cargo fmt`, not `--check`).

- [ ] **Step 4: Full frontend verification (required workflow commands)**

Run: `cd frontend && npm run check && npm run fmt && cd ..`
Expected: `npm run check` (tsc + Biome + Vitest) PASS, then `npm run fmt` applies cleanly.

- [ ] **Step 4b: Confirm formatting left nothing unexpected**

Run: `git status --short` (and optionally `git diff --check`).
Expected: only the intended plan files are modified; if `cargo fmt`/`npm run fmt` reformatted anything, stage those changes with the relevant task's `git add` before committing.

- [ ] **Step 5: Manual visual check (recommended — external human testing)**

Run the app (Vite + backend), open an asset detail page, and confirm against `docs/Design.GainsBreakdownWaterfall.md` / the mockup:
- Open position: cost basis → price → FX → market value → realized (0, de-emphasized) → dividends ("Not tracked yet") → total return; total bar is a small delta, value and "% of cost" columns aligned and colored.
- A partially-sold open position shows a non-zero realized step and a total return that includes it.
- A closed position (enable "Include closed positions" or open a fully-sold asset) shows the proceeds pivot and no separate realized step.
- An instrument with missing FX shows the FX step hatched with "n/a" percent — not zero.

- [ ] **Step 6: Commit (staged)**

```bash
git add frontend/package.json backend/Cargo.toml docs/DecisionLog.md
git commit -m "chore: bump versions and log gains waterfall decision

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Notes for the implementer

- **Open questions already resolved during planning:** realized gains are included now (the user chose the full waterfall over a frontend-only first cut); dividends are an inert placeholder only.
- **Why a new `waterfallViewModel.ts` file** rather than extending `assetViewModel.ts`: keeps the selector focused and independently testable, and `assetViewModel.ts` already carries the tiles/header logic. This follows the existing one-view-model-per-concern pattern (e.g. `instrumentChartViewModel.ts`).
- **Display-only math boundary:** only the `% of cost` column and the open total-return sum use JS floats. Every other amount is the exact API string passed straight to `SummaryAvailabilityValue`. Do not route these display values back into any request.
- **`assert_available` / `assert_unavailable`** helpers already exist in the gains test module — reuse them; do not redefine.
