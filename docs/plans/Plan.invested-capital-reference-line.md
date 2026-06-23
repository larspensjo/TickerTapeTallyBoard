# Invested-Capital Reference Line Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "net invested capital" reference line to the dashboard portfolio-value chart so the gap between the value line and the reference line reads as total profit, even across sell-offs.

**Architecture:** The backend computes the reference series on the existing `build_value_history` pass (one ledger walk, one date spine). Net invested capital at a date = cumulative buy cash-out minus sell cash-returned, in SEK, using each trade's own stored `fx_rate_to_base` and `brokerage_base`. It is a step function that moves only on trades, not on prices. The frontend maps the response into two series via a pure view-model and renders the reference as a dashed muted line layered under the existing blue value area.

**Tech Stack:** Rust (axum, rust_decimal, chrono) backend; React + TypeScript + lightweight-charts frontend; cargo tests; vitest.

## Global Constraints

- Base currency is SEK; money is rendered with `money_string` on the backend (two-decimal string) and parsed with `Number(...)` on the frontend.
- Backend: after the change run `cargo clippy --all-targets -- -D warnings` then `cargo fmt`, from `backend/`.
- Frontend: after the change run `npm run check` then `npm run fmt`, from `frontend/`. `npm run check` covers `tsc --noEmit` and Biome.
- Preserve unidirectional data flow; keep reducers/derivations pure; keep view-derivation in pure selectors/view-models, not inline in components.
- `derive_position` / ledger helpers already error via `LedgerError` on malformed input; reuse those error variants rather than inventing new ones.
- Bump UI versions: `frontend/package.json` from `0.10.4`, `backend/Cargo.toml` from `0.7.2`.
- Update `docs/DecisionLog.md` with the reason this work was done.
- Scope exclusions (do NOT include in the reference line): `Split` (no cash) and `Dividend` (income, not invested capital) transactions. Only `Buy` and `Sell` move the invested line.

## Semantics (authoritative definition)

For a date `D`, in SEK:

```
invested(D) = Σ Buy  (trade_date ≤ D) of  price·quantity·fx + brokerage_base
            − Σ Sell (trade_date ≤ D) of  price·|quantity|·fx − brokerage_base
```

- `fx` is `1` when the instrument's `native_currency` is SEK, otherwise the trade's stored `fx_rate_to_base`.
- `quantity` on a `LedgerTransaction` is signed (Buy > 0, Sell < 0); `brokerage_base` is already in SEK.
- If any contributing Buy/Sell of a **non-SEK** instrument is missing `fx_rate_to_base`, the invested value is **unavailable** (`None`) for that date and every later date. Unavailable points render as a line gap, never a fabricated number.
- The invested value is computed from **all** Buy/Sell cash flows, independent of whether an instrument's price is available on date `D` (the money was invested regardless).

---

## File Structure

- `backend/src/domain/valuation.rs` — add `invested_base: Option<Decimal>` to `ValueHistoryPoint`; compute cash-flow events and per-point cumulative invested in `build_value_history`. Domain tests live in the same file's `#[cfg(test)] mod tests`.
- `backend/src/api/portfolio.rs` — add `invested_base: Option<String>` to `ValueHistoryPointResponse`; map it in `point_response`. API test in the same file's test module.
- `frontend/src/api/types.ts` — add `invested_base: string | null` to `ValueHistoryPoint`.
- `frontend/src/components/portfolioValueViewModel.ts` — NEW pure helper mapping `ValueHistoryPoint[]` → `{ value, invested }` series.
- `frontend/src/components/portfolioValueViewModel.test.ts` — NEW unit test for the helper.
- `frontend/src/components/TimeSeriesChart.tsx` — add optional `referenceData` prop; render a dashed line series.
- `frontend/src/components/TimeSeriesChart.test.tsx` — extend mock with `addLineSeries`/`LineStyle`; assert reference rendering.
- `frontend/src/components/Dashboard.tsx` — use the view-model; pass `referenceData`; add a small two-item legend.
- `frontend/src/styles.css` — add `.chart-legend` styles near `.chart-meta` (line ~1401).
- `frontend/package.json`, `backend/Cargo.toml`, `docs/DecisionLog.md` — version bumps + decision note.

---

## Task 1: Backend — compute invested capital in `build_value_history`

**Files:**
- Modify: `backend/src/domain/valuation.rs` (struct `ValueHistoryPoint` ~line 169; `build_value_history` ~lines 189-292)
- Test: `backend/src/domain/valuation.rs` (`#[cfg(test)] mod tests`, near existing `build_value_history` tests ~line 960)

**Interfaces:**
- Consumes: `ValueHistoryInstrument { native_currency: String, ledger: Vec<LedgerTransaction>, prices, fx_rates }`; `LedgerTransaction { trade_date, kind, quantity: i64 (signed), price: Option<Decimal>, fx_rate_to_base: Option<Decimal>, brokerage_base: Decimal }`; `TransactionKind::{Buy, Sell, Split, Dividend}`; `LedgerError::{BuyMissingPrice { transaction_id }, SellMissingPrice { transaction_id }}`.
- Produces: `ValueHistoryPoint` gains a public field `invested_base: Option<Decimal>`. Consumed by Task 2.

- [ ] **Step 1: Write the failing tests**

Add these tests inside `mod tests` in `backend/src/domain/valuation.rs`. They reuse the existing helpers in that module (`d(...)` for dates, the `ValueHistoryInstrument` builders used by neighbouring `build_value_history` tests, `dec!`). Match the surrounding tests' construction style for ledgers and price/fx candidates.

```rust
#[test]
fn invested_capital_tracks_buy_cost_including_brokerage() {
    // SEK instrument: buy 10 @ 100 with 9 SEK brokerage on 2026-01-02.
    let mut buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
    buy.brokerage_base = dec!(9);
    let inst = ValueHistoryInstrument {
        native_currency: "SEK".to_string(),
        ledger: vec![buy],
        prices: vec![price(d(2026, 1, 2), dec!(100), "SEK")],
        fx_rates: vec![],
    };
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    assert_eq!(points.len(), 1);
    // 10*100 + 9 = 1009
    assert_eq!(points[0].invested_base, Some(dec!(1009)));
}

#[test]
fn invested_capital_drops_by_sell_proceeds_net_of_brokerage() {
    // Buy 10 @ 100 (2026-01-02), sell 4 @ 150 with 5 SEK brokerage (2026-01-05).
    let buy = ledger_buy(1, d(2026, 1, 2), 10, dec!(100));
    let mut sell = ledger_sell(2, d(2026, 1, 5), 4, dec!(150));
    sell.brokerage_base = dec!(5);
    let inst = ValueHistoryInstrument {
        native_currency: "SEK".to_string(),
        ledger: vec![buy, sell],
        prices: vec![
            price(d(2026, 1, 2), dec!(100), "SEK"),
            price(d(2026, 1, 5), dec!(150), "SEK"),
        ],
        fx_rates: vec![],
    };
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    // Day 1: invested = 1000. Day 2: 1000 - (4*150 - 5) = 1000 - 595 = 405.
    assert_eq!(points[0].invested_base, Some(dec!(1000)));
    assert_eq!(points[1].invested_base, Some(dec!(405)));
}

#[test]
fn invested_capital_uses_trade_time_fx_for_non_sek() {
    // USD instrument: buy 10 @ 100 USD at fx 10 on 2026-01-02.
    let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)));
    let inst = ValueHistoryInstrument {
        native_currency: "USD".to_string(),
        ledger: vec![buy],
        prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
        fx_rates: vec![fx(d(2026, 1, 2), dec!(10))],
    };
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    // 10*100*10 = 10000, no brokerage.
    assert_eq!(points[0].invested_base, Some(dec!(10000)));
}

#[test]
fn invested_capital_unavailable_when_non_sek_trade_lacks_fx() {
    // USD buy missing fx_rate_to_base => invested unavailable from that date.
    let buy = ledger_buy_fx(1, d(2026, 1, 2), 10, dec!(100), None);
    let inst = ValueHistoryInstrument {
        native_currency: "USD".to_string(),
        ledger: vec![buy],
        prices: vec![price(d(2026, 1, 2), dec!(100), "USD")],
        fx_rates: vec![fx(d(2026, 1, 2), dec!(10))],
    };
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    assert_eq!(points[0].invested_base, None);
}
```

If the helpers `ledger_buy`, `ledger_sell`, `ledger_buy_fx`, `price`, `fx` do not already exist in the test module under those exact names, add thin wrappers next to the existing ledger/price/fx builders so these tests compile. They must produce `LedgerTransaction` values whose `quantity` is signed (positive for buy, **negative** for sell) and whose `fx_rate_to_base`/`brokerage_base` match the arguments. Example wrappers (adapt field names to the module's existing builders):

```rust
fn ledger_buy(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: date,
        kind: TransactionKind::Buy,
        quantity: qty,
        price: Some(price),
        dividend_per_share: None,
        fx_rate_to_base: None,
        brokerage_base: Decimal::ZERO,
    }
}

fn ledger_sell(id: i64, date: NaiveDate, qty: i64, price: Decimal) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: date,
        kind: TransactionKind::Sell,
        quantity: -qty,
        price: Some(price),
        dividend_per_share: None,
        fx_rate_to_base: None,
        brokerage_base: Decimal::ZERO,
    }
}

fn ledger_buy_fx(
    id: i64,
    date: NaiveDate,
    qty: i64,
    price: Decimal,
    fx_rate_to_base: Option<Decimal>,
) -> LedgerTransaction {
    LedgerTransaction {
        fx_rate_to_base,
        ..ledger_buy(id, date, qty, price)
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `backend/`): `cargo test invested_capital`
Expected: FAIL — `ValueHistoryPoint` has no field `invested_base` (compile error).

- [ ] **Step 3: Add the field to `ValueHistoryPoint`**

In `backend/src/domain/valuation.rs`, extend the struct (~line 169):

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ValueHistoryPoint {
    pub date: NaiveDate,
    pub value_base: Decimal,
    pub invested_base: Option<Decimal>,
    pub incomplete: bool,
    pub included_count: usize,
    pub excluded_count: usize,
}
```

- [ ] **Step 4: Compute the cash-flow events and per-point invested**

In `build_value_history`, add a private cash-flow helper above the function:

```rust
struct InvestedCashFlow {
    date: NaiveDate,
    delta: Decimal,
}

/// Collect SEK cash-flow deltas from Buy/Sell trades and the earliest date from
/// which invested capital becomes unavailable because a non-SEK trade lacks FX.
/// Buys add cash out (price·qty·fx + brokerage); sells subtract cash returned
/// (price·|qty|·fx − brokerage). Splits and dividends are ignored.
fn invested_cash_flows(
    instruments: &[ValueHistoryInstrument],
) -> Result<(Vec<InvestedCashFlow>, Option<NaiveDate>), LedgerError> {
    let mut events = Vec::new();
    let mut unavailable_from: Option<NaiveDate> = None;
    let mut mark_unavailable = |date: NaiveDate, slot: &mut Option<NaiveDate>| {
        *slot = Some(match *slot {
            Some(current) => current.min(date),
            None => date,
        });
    };

    for inst in instruments {
        let is_base = inst.native_currency.eq_ignore_ascii_case("SEK");
        for tx in &inst.ledger {
            let (price, signed_qty) = match tx.kind {
                TransactionKind::Buy => (
                    tx.price.ok_or(LedgerError::BuyMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Sell => (
                    tx.price.ok_or(LedgerError::SellMissingPrice {
                        transaction_id: tx.id,
                    })?,
                    // Sell quantity is negative; cash returned reduces invested.
                    Decimal::from(tx.quantity),
                ),
                TransactionKind::Split | TransactionKind::Dividend => continue,
            };

            let fx = if is_base {
                Some(Decimal::ONE)
            } else {
                tx.fx_rate_to_base
            };
            let Some(fx) = fx else {
                mark_unavailable(tx.trade_date, &mut unavailable_from);
                continue;
            };

            // Both buys and sells *add* brokerage_base. Buy: +(price·qty·fx) +
            // brokerage raises net invested. Sell: signed_qty is negative, so
            // price·signed_qty·fx is already the (negative) cash returned;
            // adding brokerage reduces that cash returned, i.e. keeps net
            // invested higher. Matches the authoritative formula above:
            //   −(price·|qty|·fx − brokerage) = price·signed_qty·fx + brokerage.
            let delta = price * signed_qty * fx + tx.brokerage_base;
            events.push(InvestedCashFlow {
                date: tx.trade_date,
                delta,
            });
        }
    }

    events.sort_by_key(|event| event.date);
    Ok((events, unavailable_from))
}
```

Then, inside `build_value_history`, after `first_buy` is resolved and before the spine loop, compute the events and initialise a cursor plus a running accumulator. The cash-flow events are sorted by date and the date spine ascends, so a single forward cursor advances over the events exactly once across the whole loop — `O(points + cash_flows)` rather than re-scanning every event at every point:

```rust
    let (cash_flows, invested_unavailable_from) = invested_cash_flows(instruments)?;
    let mut next_flow = 0usize;
    let mut invested_total = Decimal::ZERO;
```

And at the point-push site (currently `points.push(ValueHistoryPoint { ... })` ~line 282), advance the cursor for every cash flow on or before this date, then compute `invested_base` and include it:

```rust
        while next_flow < cash_flows.len() && cash_flows[next_flow].date <= date {
            invested_total += cash_flows[next_flow].delta;
            next_flow += 1;
        }
        let invested_base = match invested_unavailable_from {
            Some(unavailable) if date >= unavailable => None,
            _ => Some(invested_total),
        };

        points.push(ValueHistoryPoint {
            date,
            value_base,
            invested_base,
            incomplete: excluded_count > 0,
            included_count,
            excluded_count,
        });
```

- [ ] **Step 5: Fix existing `ValueHistoryPoint` literals**

Any existing test in this module that constructs `ValueHistoryPoint { .. }` directly now fails to compile. Search the file for `ValueHistoryPoint {` and add `invested_base: <expected>,` to each (use the correct expected value, or `None` where the test does not care and asserts other fields). Do not use `..Default::default()` — the struct has no `Default`.

- [ ] **Step 6: Run the new and existing tests**

Run (from `backend/`): `cargo test valuation`
Expected: PASS — the four new `invested_capital_*` tests pass and all pre-existing `build_value_history` / valuation tests still pass.

- [ ] **Step 7: Lint, format, commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/domain/valuation.rs
git commit -m "feat(valuation): compute net invested capital per value-history point

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Backend — expose `invested_base` in the value-history API

**Files:**
- Modify: `backend/src/api/portfolio.rs` (`ValueHistoryPointResponse` ~line 29; `point_response` ~line 43)
- Test: `backend/src/api/portfolio.rs` (`#[cfg(test)] mod tests`, extend `sek_holding_produces_monotonic_value_points` ~line 347 or add a sibling test)

**Interfaces:**
- Consumes: `ValueHistoryPoint.invested_base: Option<Decimal>` (Task 1); `money_string(Decimal) -> String` (already imported).
- Produces: JSON field `points[].invested_base: string | null`. Consumed by Task 3.

- [ ] **Step 1: Write the failing assertion**

Extend the existing test `sek_holding_produces_monotonic_value_points` in `backend/src/api/portfolio.rs` by adding, after the existing `value_base` assertions:

```rust
        assert_eq!(points[0]["invested_base"], "1000.00");
        assert_eq!(points[1]["invested_base"], "1000.00");
```

(The test buys 10 @ 100 SEK once, so invested is 1000.00 on both days.)

- [ ] **Step 2: Run to verify it fails**

Run (from `backend/`): `cargo test sek_holding_produces_monotonic_value_points`
Expected: FAIL — `points[0]["invested_base"]` is `Null` (field absent).

- [ ] **Step 3: Add the response field and map it**

In `ValueHistoryPointResponse` (~line 29):

```rust
#[derive(Debug, Serialize)]
pub struct ValueHistoryPointResponse {
    date: String,
    value_base: String,
    invested_base: Option<String>,
    incomplete: bool,
    included_count: usize,
    excluded_count: usize,
}
```

In `point_response` (~line 43):

```rust
fn point_response(point: &ValueHistoryPoint) -> ValueHistoryPointResponse {
    ValueHistoryPointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        value_base: money_string(point.value_base),
        invested_base: point.invested_base.map(money_string),
        incomplete: point.incomplete,
        included_count: point.included_count,
        excluded_count: point.excluded_count,
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run (from `backend/`): `cargo test sek_holding_produces_monotonic_value_points`
Expected: PASS.

- [ ] **Step 5: Lint, format, commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/api/portfolio.rs
git commit -m "feat(api): expose invested_base in value-history response

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Frontend — type + pure view-model for the two series

**Files:**
- Modify: `frontend/src/api/types.ts` (`ValueHistoryPoint` ~line 94)
- Create: `frontend/src/components/portfolioValueViewModel.ts`
- Test: `frontend/src/components/portfolioValueViewModel.test.ts`

**Interfaces:**
- Consumes: `ValueHistoryPoint { date: string, value_base: string, invested_base: string | null, ... }`; `TimeSeriesPoint { time: string, value: number }` from `./TimeSeriesChart`.
- Produces: `portfolioValueSeries(points: ValueHistoryPoint[]): { value: TimeSeriesPoint[]; invested: TimeSeriesPoint[] }`. Consumed by Task 5.

- [ ] **Step 1: Add the type field**

In `frontend/src/api/types.ts` (~line 94):

```ts
export interface ValueHistoryPoint {
  date: string;
  value_base: string;
  invested_base: string | null;
  incomplete: boolean;
  included_count: number;
  excluded_count: number;
}
```

- [ ] **Step 2: Write the failing test**

Create `frontend/src/components/portfolioValueViewModel.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { ValueHistoryPoint } from "../api/types";
import { portfolioValueSeries } from "./portfolioValueViewModel";

function point(
  date: string,
  value: string,
  invested: string | null,
): ValueHistoryPoint {
  return {
    date,
    value_base: value,
    invested_base: invested,
    incomplete: false,
    included_count: 1,
    excluded_count: 0,
  };
}

describe("portfolioValueSeries", () => {
  it("maps value_base and invested_base into parallel numeric series", () => {
    const { value, invested } = portfolioValueSeries([
      point("2026-01-02", "1000.00", "1000.00"),
      point("2026-01-05", "1100.00", "405.00"),
    ]);

    expect(value).toEqual([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-05", value: 1100 },
    ]);
    expect(invested).toEqual([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-05", value: 405 },
    ]);
  });

  it("omits invested points when invested_base is null so the line shows a gap", () => {
    const { value, invested } = portfolioValueSeries([
      point("2026-01-02", "1000.00", null),
      point("2026-01-05", "1100.00", "900.00"),
    ]);

    expect(value).toHaveLength(2);
    expect(invested).toEqual([{ time: "2026-01-05", value: 900 }]);
  });
});
```

- [ ] **Step 3: Run to verify it fails**

Run (from `frontend/`): `npm run test -- portfolioValueViewModel`
Expected: FAIL — cannot resolve `./portfolioValueViewModel`.

- [ ] **Step 4: Implement the view-model**

Create `frontend/src/components/portfolioValueViewModel.ts`:

```ts
import type { ValueHistoryPoint } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface PortfolioValueSeries {
  value: TimeSeriesPoint[];
  invested: TimeSeriesPoint[];
}

/**
 * Split the value-history response into the total-value series and the net
 * invested-capital reference series. Invested points are dropped when the
 * backend could not derive them (`invested_base === null`) so the reference
 * line renders a gap instead of a fabricated value.
 */
export function portfolioValueSeries(
  points: ValueHistoryPoint[],
): PortfolioValueSeries {
  const value: TimeSeriesPoint[] = [];
  const invested: TimeSeriesPoint[] = [];

  for (const point of points) {
    value.push({ time: point.date, value: Number(point.value_base) });

    if (point.invested_base !== null) {
      const investedValue = Number(point.invested_base);
      if (Number.isFinite(investedValue)) {
        invested.push({ time: point.date, value: investedValue });
      }
    }
  }

  return { value, invested };
}
```

- [ ] **Step 5: Run to verify it passes**

Run (from `frontend/`): `npm run test -- portfolioValueViewModel`
Expected: PASS (both tests).

- [ ] **Step 6: Check, format, commit**

```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/api/types.ts frontend/src/components/portfolioValueViewModel.ts frontend/src/components/portfolioValueViewModel.test.ts
git commit -m "feat(dashboard): add portfolio value/invested view-model

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: Frontend — render the reference line in `TimeSeriesChart`

**Files:**
- Modify: `frontend/src/components/TimeSeriesChart.tsx`
- Test: `frontend/src/components/TimeSeriesChart.test.tsx`

**Interfaces:**
- Consumes: `TimeSeriesPoint[]` for `referenceData`; existing `calendarSpineData(data, rangeStart)` helper.
- Produces: `TimeSeriesChart` accepts optional `referenceData?: TimeSeriesPoint[]`. Consumed by Task 5.

**Note on autoscale:** No change to `zeroBaselineAutoscale` is needed. The reference line is a separate series on the same (right) price scale; lightweight-charts unions every series' autoscale info, so a negative invested value pulls the visible floor below zero on its own, while the area series keeps filling from zero. The existing "pins the y-axis floor to zero" test (which inspects the area series provider in isolation) stays valid.

- [ ] **Step 1: Extend the test mock and write the failing test**

In `frontend/src/components/TimeSeriesChart.test.tsx`, extend the hoisted mock so the chart exposes a line series and the module exports `LineStyle`:

In the `vi.hoisted` block, add a `setData` for the line series and an `addLineSeries` mock:

```ts
  const setReferenceData = vi.fn();
  const addLineSeries = vi.fn(() => ({ setData: setReferenceData }));
```

Add `addLineSeries` to the object returned by `createChart`:

```ts
  const createChart = vi.fn(() => ({
    addAreaSeries,
    addLineSeries,
    applyOptions,
    remove,
    subscribeCrosshairMove,
    timeScale,
  }));
```

Return `addLineSeries` and `setReferenceData` from the hoisted block (add them to the returned object).

In the `vi.mock("lightweight-charts", ...)` factory, add the `LineStyle` enum so the component can import it:

```ts
vi.mock("lightweight-charts", () => ({
  TickMarkType: {
    Year: 0,
    Month: 1,
    DayOfMonth: 2,
    Time: 3,
    TimeWithSeconds: 4,
  },
  LineStyle: {
    Solid: 0,
    Dotted: 1,
    Dashed: 2,
    LargeDashed: 3,
    SparseDotted: 4,
  },
  createChart: chartMocks.createChart,
}));
```

Add a new test:

```ts
  it("renders the invested reference as a dashed line on the same calendar spine", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        visibleStart="2026-01-02"
        data={[
          { time: "2026-01-02", value: 1000 },
          { time: "2026-01-04", value: 1100 },
        ]}
        referenceData={[
          { time: "2026-01-02", value: 1000 },
          { time: "2026-01-04", value: 1000 },
        ]}
      />,
    );

    type LineSeriesOptions = { lineStyle: number };
    const lineCalls = chartMocks.addLineSeries.mock.calls as unknown as Array<
      [LineSeriesOptions]
    >;
    expect(lineCalls[0]?.[0].lineStyle).toBe(2); // LineStyle.Dashed

    // Reference data is gap-filled onto the same daily spine as the value line.
    expect(chartMocks.setReferenceData).toHaveBeenLastCalledWith([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-03" },
      { time: "2026-01-04", value: 1000 },
    ]);
  });
```

- [ ] **Step 2: Run to verify it fails**

Run (from `frontend/`): `npm run test -- TimeSeriesChart`
Expected: FAIL — `addLineSeries` is not called / `referenceData` prop unused.

- [ ] **Step 3: Implement the reference series**

In `frontend/src/components/TimeSeriesChart.tsx`:

Add `LineStyle` and `ISeriesApi` (already imported) usage. Update the import from `lightweight-charts` to include `LineStyle`:

```ts
import {
  type AreaData,
  type AutoscaleInfoProvider,
  createChart,
  type IChartApi,
  type ISeriesApi,
  LineStyle,
  type SeriesMarker,
  TickMarkType,
  type Time,
  type WhitespaceData,
} from "lightweight-charts";
```

Add `referenceData` to the props type and signature:

```ts
export function TimeSeriesChart({
  data,
  ariaLabel,
  visibleStart,
  markers = [],
  referenceData,
  height = 240,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  visibleStart?: string;
  markers?: ChartTradeMarker[];
  referenceData?: TimeSeriesPoint[];
  height?: number;
}) {
```

Add a ref next to `seriesRef`:

```ts
  const referenceSeriesRef = useRef<ISeriesApi<"Line"> | null>(null);
```

In the create-chart effect, after the area series is created, add the line series:

```ts
    const referenceSeries = chart.addLineSeries({
      color: "#e0b15e",
      lineWidth: 2,
      lineStyle: LineStyle.Dashed,
      priceLineVisible: false,
      lastValueVisible: false,
      crosshairMarkerVisible: false,
    });

    chartRef.current = chart;
    seriesRef.current = series;
    referenceSeriesRef.current = referenceSeries;
```

In the cleanup of that effect, null the ref:

```ts
      chartRef.current = null;
      seriesRef.current = null;
      referenceSeriesRef.current = null;
```

In the data effect (the one keyed on `[data, visibleStart]`), after `seriesRef.current?.setData(chartData);`, set the reference data on the same spine and add `referenceData` to the dependency array:

```ts
    seriesRef.current?.setData(chartData);

    const referenceSeries = referenceSeriesRef.current;
    if (referenceSeries) {
      referenceSeries.setData(
        referenceData && referenceData.length > 0
          ? calendarSpineData(referenceData, rangeStart)
          : [],
      );
    }
```

Change the effect dependency array to `[data, visibleStart, referenceData]`.

- [ ] **Step 4: Run to verify it passes**

Run (from `frontend/`): `npm run test -- TimeSeriesChart`
Expected: PASS — the new dashed-reference test passes and all existing chart tests still pass.

- [ ] **Step 5: Check, format, commit**

```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/TimeSeriesChart.tsx frontend/src/components/TimeSeriesChart.test.tsx
git commit -m "feat(chart): support a dashed reference line series

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Frontend — wire the reference line and legend into the Dashboard

**Files:**
- Modify: `frontend/src/components/Dashboard.tsx` (`DashboardValueChart` ~lines 27-97)
- Modify: `frontend/src/styles.css` (add `.chart-legend` near `.chart-meta` ~line 1406)

**Interfaces:**
- Consumes: `portfolioValueSeries(points)` from `./portfolioValueViewModel`; `TimeSeriesChart`'s `referenceData` prop (Task 4).
- Produces: user-visible chart with value area + invested reference line + a two-item legend.

- [ ] **Step 1: Replace the inline mapping with the view-model**

In `frontend/src/components/Dashboard.tsx`, add the import:

```ts
import { portfolioValueSeries } from "./portfolioValueViewModel";
```

In `DashboardValueChart`, replace the `points` `useMemo` (the inline `.map(...)`) with the view-model:

```ts
  const history = query.data?.points;
  const series = useMemo(
    () => portfolioValueSeries(history ?? []),
    [history],
  );
  const incompleteDays = useMemo(
    () => (history ?? []).filter((point) => point.incomplete).length,
    [history],
  );
```

Update the empty-state guard (was `points.length === 0`) to use the value series:

```ts
  if (series.value.length === 0) {
```

- [ ] **Step 2: Pass the reference data and render the legend**

Replace the returned chart markup's `<TimeSeriesChart .../>` and chart-meta block:

```tsx
  return (
    <section className="panel chart-panel" aria-label="Portfolio value">
      <div className="chart-meta">
        <h2>Portfolio value (SEK)</h2>
        {incompleteDays > 0 ? (
          <span className="status-chip warning compact">
            {incompleteDays} days had missing inputs
          </span>
        ) : null}
      </div>
      <div className="chart-legend" aria-hidden="true">
        <span className="chart-legend-item value">Value</span>
        <span className="chart-legend-item invested">Invested capital</span>
      </div>
      <TimeSeriesChart
        data={series.value}
        referenceData={series.invested}
        ariaLabel="Portfolio value over time in SEK, with net invested capital reference line"
        visibleStart={query.data?.start_date ?? undefined}
        height={280}
      />
    </section>
  );
```

- [ ] **Step 3: Add legend styles**

In `frontend/src/styles.css`, after the `.chart-meta { ... }` block (~line 1406), add:

```css
.chart-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  font-size: 0.75rem;
  color: var(--text-secondary);
}

.chart-legend-item {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
}

.chart-legend-item::before {
  content: "";
  width: 14px;
  height: 0;
  border-top-width: 2px;
  border-top-style: solid;
}

.chart-legend-item.value::before {
  border-top-color: #4f9cff;
}

.chart-legend-item.invested::before {
  border-top-style: dashed;
  border-top-color: #e0b15e;
}
```

- [ ] **Step 4: Verify the build and existing dashboard tests**

Run (from `frontend/`): `npm run check && npm run test -- Dashboard portfolioValueViewModel TimeSeriesChart`
Expected: PASS (type-check clean; existing dashboard/chart tests still pass).

- [ ] **Step 5: Manual verification (recommended — external human testing)**

Start the app and open the dashboard. Confirm: (a) the blue value area renders as before; (b) a dashed gold line sits under it tracking invested capital; (c) the legend shows "Value" and "Invested capital"; (d) on a portfolio with a significant sell-off, the invested line steps down at the sell and the gap to the value line still reads as profit; (e) if you have a non-SEK holding with a trade missing its FX, the invested line shows a gap rather than dropping to zero.

- [ ] **Step 6: Format, commit**

```bash
cd frontend && npm run fmt
git add frontend/src/components/Dashboard.tsx frontend/src/styles.css
git commit -m "feat(dashboard): show invested-capital reference line and legend

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Version bumps and decision log

**Files:**
- Modify: `frontend/package.json:3`, `backend/Cargo.toml:3`, `docs/DecisionLog.md`

- [ ] **Step 1: Bump versions**

`frontend/package.json` line 3: `"version": "0.10.5",`
`backend/Cargo.toml` line 3: `version = "0.7.3"`

- [ ] **Step 2: Add a decision-log entry**

Append to `docs/DecisionLog.md` (follow the file's existing entry format and date `2026-06-23`): a short note that the dashboard portfolio chart gained a net-invested-capital reference line (cumulative buy cash-out minus sell cash-returned, trade-time FX), chosen so the value-vs-reference gap reads as total profit even across significant sell-offs; cost-basis-of-held was rejected because it hides realized gains at sell events.

- [ ] **Step 3: Verify both builds one last time**

Run: `cd backend && cargo build` then `cd ../frontend && npm run check`
Expected: both succeed.

- [ ] **Step 4: Commit**

```bash
git add frontend/package.json backend/Cargo.toml docs/DecisionLog.md
git commit -m "chore: bump versions and log invested-capital reference line

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review Notes

- **Spec coverage:** trade-time-FX step semantics (Task 1 + tests); minor-slope/no-spine-expansion (no spine change is made — Task 1 only adds a field, spine untouched); dashed muted line + legend (Tasks 4–5); negative "house money" values render (Task 4 autoscale note); missing-FX → gap (Tasks 1, 3, 4). All covered.
- **Excluded by design:** Split and Dividend transactions never move the invested line (Task 1 `continue` arms).
- **Type consistency:** `invested_base` is `Option<Decimal>` (Rust) → `Option<String>` (JSON) → `string | null` (TS) → dropped-or-`number` in `portfolioValueSeries`. `portfolioValueSeries` and `referenceData` names are used identically across Tasks 3–5.
