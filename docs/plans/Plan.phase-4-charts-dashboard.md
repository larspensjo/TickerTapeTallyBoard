# Phase 4 Charts & Dashboard Implementation Plan

> **For agentic workers:** Implement this plan task-by-task, in order. Steps use checkbox (`- [ ]`) syntax for tracking. If your environment provides a plan-execution sub-skill (e.g. `superpowers:subagent-driven-development` or `superpowers:executing-plans`), you may use it; otherwise just follow the tasks and their per-task verification steps directly. No external skill is required.

**Goal:** Add per-instrument and portfolio value charts, turn the landing page into a dashboard (summary, value chart, top movers, allocation), all on the existing exact-decimal valuation path.

**Architecture:** Backend adds one new derived-on-the-fly endpoint (`GET /api/portfolio/value-history`) built from a pure `build_value_history` in the valuation domain; the HTTP handler and repositories stay thin. Frontend adds one reusable Lightweight Charts wrapper, two query hooks, and several pure (unit-tested) selectors that feed presentational React components. No schema change, no second charting library, no stored value snapshots.

**Tech Stack:** Rust (axum, sqlx, rust_decimal, chrono), React + TypeScript (TanStack Query, react-router-dom), `lightweight-charts` (new), Vite, Biome.

## Global Constraints

- Backend money/value output uses the 2-decimal `money_string(...)` format from `backend/src/api/valuation.rs`; native price and FX rate serialize at full precision (`precise_string`). Never zero-fill missing data.
- All value/money derivation stays on the exact-decimal `rust_decimal` path. Only presentational allocation weights may use frontend float math (per the 2026-06-18 "Display-Only Weights May Use Frontend Float Math" decision).
- Base currency is `SEK`; price provider is `YAHOO`; FX provider is `FRANKFURTER` (constants live in `backend/src/api/valuation.rs`).
- Cached prices are used **only** behind an enabled Yahoo `provider_symbols` mapping, identical to `load_valuation_inputs(...)`.
- `from > to` on any range endpoint returns `400 invalid_date_range` (shared code with `instrument_prices`).
- Backend: after each backend task run `cargo clippy --all-targets -- -D warnings` then `cargo fmt`, from `backend/`.
- Frontend: after each frontend task run `npm run check` then `npm run fmt`, from `frontend/`. As of Task A2, `npm run check` runs `tsc --noEmit`, Biome lint, **and** `vitest run` — one completion gate (mirrors the backend's clippy+tests bar). Use `npm.cmd` under `Start-Process`.
- UI must follow `docs/VisualDesign.DarkTheme.md`.
- Version bumps: `frontend/package.json` is currently `0.7.4`; `backend/Cargo.toml` is currently `0.5.5`. Bump as instructed in tasks.
- Plans are ephemeral: durable docs (DecisionLog, Design.HighLevel) must not reference this plan or "Phase A/B/C/D".
- **Staging, not committing:** Each task ends by *staging* the listed files (`git add …`) so the work can be reviewed. Do **not** run `git commit` — committing is an explicit human action taken after review. The `git add` lines below define exactly which files each task touches.

---

## File Structure

**Backend (new / modified):**
- `backend/src/domain/valuation.rs` — add pure `build_value_history(...)`, `ValueHistoryInstrument`, `ValueHistoryPoint` (sibling of `build_price_history`).
- `backend/src/domain/performance.rs` — widen `split_factor` visibility to `pub(crate)` so the value-history builder reuses it.
- `backend/src/domain/mod.rs` — re-export the new value-history items.
- `backend/src/api/portfolio.rs` — **new** HTTP handler module for `GET /api/portfolio/value-history`.
- `backend/src/api/mod.rs` — register the `portfolio` module and route.
- `backend/Cargo.toml` — version bump.

**Frontend (new / modified):**
- `frontend/src/components/TimeSeriesChart.tsx` — **new** Lightweight Charts wrapper (lifecycle/resize/theme only; not deeply tested).
- `frontend/src/components/instrumentChartViewModel.ts` — **new** pure selector for the asset price chart.
- `frontend/src/components/dashboardSelectors.ts` — **new** pure top-movers + allocation selectors.
- `frontend/src/components/Dashboard.tsx` — **new** dashboard landing page.
- `frontend/src/api/types.ts` — add price-history and value-history response types.
- `frontend/src/api/queries.ts` — add `useInstrumentPrices`, `usePortfolioValueHistory`.
- `frontend/src/components/AssetView.tsx` — replace `ReservedChartBand` with the real chart; update Back links to `/board`.
- `frontend/src/App.tsx` — routing + nav changes.
- `frontend/src/components/ImportView.tsx` — update `navigate("/", …)` handoff to `/board`.
- `frontend/src/styles.css` — dashboard + chart + allocation styles.
- `frontend/package.json` — add `lightweight-charts`, version bump.

**Tests:** co-located Rust `#[cfg(test)]` modules; frontend pure-selector tests as `*.test.ts` next to the selector, run by Vitest (established in Task A2 Step 1) and gated through `npm run check`. React Testing Library + jsdom are installed in the same step so component/reducer tests are possible later without re-litigating setup.

---

## Phase A — Charting foundation + per-instrument price chart

### Task A1: Add Lightweight Charts dependency and the reusable chart wrapper

**Files:**
- Modify: `frontend/package.json` (dependencies)
- Create: `frontend/src/components/TimeSeriesChart.tsx`
- Modify: `frontend/src/styles.css`

**Interfaces:**
- Produces: `TimeSeriesChart` React component with props
  `{ data: Array<{ time: string; value: number }>; ariaLabel: string; height?: number }`.
  `time` is an ISO `YYYY-MM-DD` date string; `value` is a finite number (caller must pre-filter unavailable points).

- [ ] **Step 1: Add the dependency**

Run from `frontend/`:
```bash
npm.cmd install lightweight-charts@^4.2.0
```
Expected: `package.json` gains `"lightweight-charts": "^4.2.0"` under `dependencies`; `package-lock.json` updates.

- [ ] **Step 2: Create the wrapper component**

Create `frontend/src/components/TimeSeriesChart.tsx`:
```tsx
import { type IChartApi, type ISeriesApi, createChart } from "lightweight-charts";
import { useEffect, useRef } from "react";

export interface TimeSeriesPoint {
  time: string; // YYYY-MM-DD
  value: number;
}

export function TimeSeriesChart({
  data,
  ariaLabel,
  height = 240,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  height?: number;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);

  // Create the chart once on mount; tear it down on unmount.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const chart = createChart(container, {
      height,
      layout: {
        background: { color: "transparent" },
        textColor: "#9aa4b2",
        attributionLogo: false,
      },
      grid: {
        vertLines: { color: "rgba(148, 163, 184, 0.08)" },
        horzLines: { color: "rgba(148, 163, 184, 0.08)" },
      },
      rightPriceScale: { borderColor: "rgba(148, 163, 184, 0.2)" },
      timeScale: { borderColor: "rgba(148, 163, 184, 0.2)" },
      handleScale: false,
      handleScroll: false,
    });
    const series = chart.addAreaSeries({
      lineColor: "#4f9cff",
      topColor: "rgba(79, 156, 255, 0.30)",
      bottomColor: "rgba(79, 156, 255, 0.02)",
      lineWidth: 2,
      priceLineVisible: false,
    });

    chartRef.current = chart;
    seriesRef.current = series;

    const resize = () =>
      chart.applyOptions({ width: container.clientWidth });
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, [height]);

  // Push data whenever it changes.
  useEffect(() => {
    seriesRef.current?.setData(data);
    chartRef.current?.timeScale().fitContent();
  }, [data]);

  return (
    <div
      ref={containerRef}
      className="time-series-chart"
      role="img"
      aria-label={ariaLabel}
    />
  );
}
```

- [ ] **Step 3: Add chart container styling**

Append to `frontend/src/styles.css`:
```css
.time-series-chart {
  width: 100%;
  min-height: 240px;
}

.chart-panel {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.chart-meta {
  display: flex;
  gap: 0.5rem;
  align-items: center;
}
```

- [ ] **Step 4: Verify it compiles**

Run from `frontend/`: `npm run check`
Expected: `tsc --noEmit` and Biome pass (the component is unused for now — that is fine; it is consumed in Task A3). If Biome flags the unused export, ignore — it is imported in Task A3 within the same branch.

- [ ] **Step 5: Format and stage**
```bash
cd frontend && npm run fmt
git add frontend/package.json frontend/package-lock.json frontend/src/components/TimeSeriesChart.tsx frontend/src/styles.css
```
Leave the changes staged for review; do not commit.

---

### Task A2: Price-history types, query hook, and the pure chart selector

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/queries.ts`
- Create: `frontend/src/components/instrumentChartViewModel.ts`
- Create: `frontend/src/components/instrumentChartViewModel.test.ts`

**Interfaces:**
- Consumes: `AvailabilityValue<string>` (`MoneyValue`) already in `types.ts`; `TimeSeriesPoint` from Task A1.
- Produces:
  - Type `PriceHistoryResponse` (`types.ts`).
  - `useInstrumentPrices(id: number)` (`queries.ts`).
  - `instrumentPriceSeries(response: PriceHistoryResponse | undefined): { points: TimeSeriesPoint[]; droppedForMissingFx: number; allUnavailable: boolean }` (`instrumentChartViewModel.ts`).

- [ ] **Step 1: Establish the Vitest + React Testing Library test runner**

The frontend currently has no test runner. Set up Vitest properly so component/reducer tests are possible later, and fold it into the existing `check` gate.

Install dev dependencies from `frontend/`:
```bash
npm.cmd install -D vitest jsdom @testing-library/react @testing-library/jest-dom
```

Create `frontend/vitest.config.ts`:
```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Pure-logic tests run in node (fast). Component tests opt in per-file with
    // `// @vitest-environment jsdom` at the top of the test file.
    environment: "node",
    globals: false,
    // Loaded for every test file so jest-dom matchers are registered. This makes
    // the matchers available to future jsdom/component tests without re-editing
    // config; it is harmless for the node-environment pure-logic tests.
    setupFiles: ["src/test/setup.ts"],
  },
});
```

Create `frontend/src/test/setup.ts` (registers jest-dom matchers; loaded via `setupFiles` above):
```ts
import "@testing-library/jest-dom/vitest";
```

In `frontend/package.json` scripts:
- Add `"test": "vitest run --passWithNoTests"`. The `--passWithNoTests` flag is required: a bare `vitest run` exits with code 1 when no test files exist, which would fail this step's verification and the `check` gate until the first test lands. (It is harmless once tests exist.)
- Update `"check"` to also run tests. It currently runs `tsc --noEmit` and Biome; append ` && vitest run --passWithNoTests` so the single gate covers types, lint, and tests. (Match the exact existing `check` command string — read it first, then append.)

Verify the runner works (no tests yet is fine): run from `frontend/` `npm.cmd run test`.
Expected: Vitest reports "no test files found" and exits 0.

Later steps refer to `npm.cmd run test` as **the frontend test command**.

- [ ] **Step 2: Add the response type**

In `frontend/src/api/types.ts`, after the `FxSnapshot` interface, add:
```ts
export interface PriceHistoryPoint {
  date: string;
  close: string;
  close_base: MoneyValue;
  fx?: { rate: string; date: string };
}

export interface PriceHistoryResponse {
  instrument_id: number;
  currency: string;
  base_currency: string;
  points: PriceHistoryPoint[];
}
```

- [ ] **Step 3: Add the query hook**

In `frontend/src/api/queries.ts`, add `PriceHistoryResponse` to the type import block from `./types`, then add:
```ts
export function useInstrumentPrices(id: number | null) {
  return useQuery({
    queryKey: ["instrument-prices", id],
    queryFn: () =>
      apiGet<PriceHistoryResponse>(`/api/instruments/${id}/prices`),
    enabled: id !== null,
  });
}
```

- [ ] **Step 4: Write the failing selector test**

Create `frontend/src/components/instrumentChartViewModel.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import type { PriceHistoryResponse } from "../api/types";
import { instrumentPriceSeries } from "./instrumentChartViewModel";

function resp(points: PriceHistoryResponse["points"]): PriceHistoryResponse {
  return {
    instrument_id: 1,
    currency: "USD",
    base_currency: "SEK",
    points,
  };
}

describe("instrumentPriceSeries", () => {
  it("excludes unavailable close_base points from the plotted series", () => {
    const result = instrumentPriceSeries(
      resp([
        { date: "2026-01-02", close: "100", close_base: { status: "available", value: "1000.00" } },
        { date: "2026-01-03", close: "110", close_base: { status: "unavailable", reasons: ["missing_fx"] } },
      ]),
    );
    expect(result.points).toEqual([{ time: "2026-01-02", value: 1000 }]);
    expect(result.droppedForMissingFx).toBe(1);
    expect(result.allUnavailable).toBe(false);
  });

  it("reports allUnavailable when every point is unavailable", () => {
    const result = instrumentPriceSeries(
      resp([
        { date: "2026-01-02", close: "100", close_base: { status: "unavailable", reasons: ["missing_fx"] } },
      ]),
    );
    expect(result.points).toEqual([]);
    expect(result.allUnavailable).toBe(true);
  });

  it("treats an empty response as empty, not all-unavailable", () => {
    const result = instrumentPriceSeries(resp([]));
    expect(result.points).toEqual([]);
    expect(result.droppedForMissingFx).toBe(0);
    expect(result.allUnavailable).toBe(false);
  });

  it("returns empty for undefined input", () => {
    const result = instrumentPriceSeries(undefined);
    expect(result.points).toEqual([]);
    expect(result.allUnavailable).toBe(false);
  });
});
```

- [ ] **Step 5: Run the test to confirm it fails**

Run **the frontend test command**.
Expected: FAIL — `instrumentPriceSeries` is not defined / module not found.

- [ ] **Step 6: Implement the selector**

Create `frontend/src/components/instrumentChartViewModel.ts`:
```ts
import type { PriceHistoryResponse } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface InstrumentPriceSeries {
  points: TimeSeriesPoint[];
  droppedForMissingFx: number;
  allUnavailable: boolean;
}

/**
 * Map the price-history endpoint into a plottable series. Points whose
 * `close_base` is unavailable (missing FX on/before that date) are excluded —
 * an unavailable value is never passed to Lightweight Charts as a number.
 */
export function instrumentPriceSeries(
  response: PriceHistoryResponse | undefined,
): InstrumentPriceSeries {
  const all = response?.points ?? [];
  const points: TimeSeriesPoint[] = [];
  let dropped = 0;

  for (const point of all) {
    if (point.close_base.status === "available") {
      points.push({ time: point.date, value: Number(point.close_base.value) });
    } else {
      dropped += 1;
    }
  }

  return {
    points,
    droppedForMissingFx: dropped,
    allUnavailable: all.length > 0 && points.length === 0,
  };
}
```

- [ ] **Step 7: Run the test to confirm it passes**

Run **the frontend test command**.
Expected: PASS (4 tests).

- [ ] **Step 8: Type-check, format, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/api/types.ts frontend/src/api/queries.ts frontend/src/components/instrumentChartViewModel.ts frontend/src/components/instrumentChartViewModel.test.ts frontend/vitest.config.ts frontend/src/test/setup.ts frontend/package.json frontend/package-lock.json
```
Leave the changes staged for review; do not commit.

---

### Task A3: Render the per-instrument price chart in AssetView

**Files:**
- Modify: `frontend/src/components/AssetView.tsx` (replace `ReservedChartBand`, lines ~127, ~135, ~293-299)
- Modify: `frontend/src/styles.css` (only if a new state class is needed; reuse `board-state` patterns)

**Interfaces:**
- Consumes: `useInstrumentPrices` (A2), `instrumentPriceSeries` (A2), `TimeSeriesChart` (A1), the `id` already parsed via `parseInstrumentId` at the top of `AssetView`.

- [ ] **Step 1: Import the new pieces**

In `frontend/src/components/AssetView.tsx`, add to the imports:
```tsx
import { useInstrumentPrices } from "../api/queries"; // merge into the existing queries import
import { instrumentPriceSeries } from "./instrumentChartViewModel";
import { TimeSeriesChart } from "./TimeSeriesChart";
```
(Merge `useInstrumentPrices` into the existing destructured `../api/queries` import rather than adding a duplicate line.)

- [ ] **Step 2: Call the query inside `AssetView`**

After `const priceStatusQuery = usePriceStatus();` add:
```tsx
const pricesQuery = useInstrumentPrices(id);
```
Note: `id` may be `null` for a malformed param; the hook is `enabled` only when non-null. Do **not** add `pricesQuery` to the `isPending`/`isError` gates that block the whole page — the chart owns its own state so a price fetch never blanks the asset page.

- [ ] **Step 3: Replace the two `<ReservedChartBand />` usages**

Replace each `<ReservedChartBand />` (there are two, in the `position` and non-position branches) with:
```tsx
<AssetPriceChart query={pricesQuery} />
```

- [ ] **Step 4: Replace the `ReservedChartBand` definition with the real chart band**

Replace the `ReservedChartBand` function (lines ~293-299) with:
```tsx
function AssetPriceChart({
  query,
}: {
  query: ReturnType<typeof useInstrumentPrices>;
}) {
  if (query.isPending) {
    return (
      <section className="chart-band" aria-label="Price chart">
        <div className="skeleton-bar" />
      </section>
    );
  }

  if (query.isError) {
    return (
      <section className="chart-band error" aria-label="Price chart">
        <p className="down">Could not load price history.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => void query.refetch()}
        >
          Retry
        </button>
      </section>
    );
  }

  const series = instrumentPriceSeries(query.data);

  if (series.points.length === 0) {
    return (
      <section className="chart-band muted" aria-label="Price chart">
        <span className="chart-band-label">
          No price history yet — refresh prices
        </span>
      </section>
    );
  }

  return (
    <section className="panel chart-panel" aria-label="Price chart">
      <div className="chart-meta">
        <h2>Price history (SEK)</h2>
        {series.droppedForMissingFx > 0 ? (
          <span className="status-chip warning compact">
            {series.droppedForMissingFx} days missing FX
          </span>
        ) : null}
      </div>
      <TimeSeriesChart data={series.points} ariaLabel="Instrument price history in SEK" />
    </section>
  );
}
```

- [ ] **Step 5: Type-check, format**
```bash
cd frontend && npm run check && npm run fmt
```
Expected: pass. (`ReturnType<typeof useInstrumentPrices>` keeps the prop typed without exporting a new type.)

- [ ] **Step 6: Manual verification (human-recommended)**

Run the app (backend + `npm run dev`). Open an asset that has stored prices → chart renders. Open an unmapped/empty asset → "No price history yet — refresh prices", no crash. If you have a USD asset with prices but missing FX on early dates → chart shows available points plus the "N days missing FX" chip and never renders `NaN`.

- [ ] **Step 7: Stage**
```bash
git add frontend/src/components/AssetView.tsx frontend/src/styles.css
```
Leave the changes staged for review; do not commit.

---

## Phase B — Portfolio value-history endpoint

### Task B1: Pure `build_value_history` in the valuation domain

**Files:**
- Modify: `backend/src/domain/performance.rs` (widen `split_factor` to `pub(crate)`)
- Modify: `backend/src/domain/valuation.rs` (add builder + types + tests)
- Modify: `backend/src/domain/mod.rs` (re-export)

**Interfaces:**
- Consumes: `PriceCandidate`, `FxCandidate` (valuation.rs), `LedgerTransaction`, `derive_position`, `split_factor` (performance.rs).
- Produces:
  ```rust
  pub struct ValueHistoryInstrument {
      pub native_currency: String,
      pub ledger: Vec<LedgerTransaction>,   // full ledger, sorted by (trade_date, id)
      pub prices: Vec<PriceCandidate>,      // sorted by date asc; empty if mapping disabled
      pub fx_rates: Vec<FxCandidate>,       // currency->SEK, sorted asc; empty for SEK
  }
  pub struct ValueHistoryPoint {
      pub date: NaiveDate,
      pub value_base: Decimal,
      pub incomplete: bool,
      pub included_count: usize,
      pub excluded_count: usize,
  }
  pub fn build_value_history(
      instruments: &[ValueHistoryInstrument],
      from: Option<NaiveDate>,
      to: Option<NaiveDate>,
  ) -> Result<Vec<ValueHistoryPoint>, LedgerError>;
  ```
  The builder returns `Err(LedgerError)` if any stored ledger violates the derivability invariant (`derive_position`/`split_factor` failure). Such a failure is an internal data error, not "missing market data" — it must never be silently swallowed by dropping an instrument or substituting a default factor. `LedgerError` is already re-exported from `domain/mod.rs`.

- [ ] **Step 1: Make `split_factor` reusable**

In `backend/src/domain/performance.rs`, change `fn split_factor(` (line ~65) to:
```rust
pub(crate) fn split_factor(
```
Leave the body unchanged. Run `cargo build` from `backend/` to confirm it still compiles.

- [ ] **Step 2: Write the failing tests**

In `backend/src/domain/valuation.rs`, inside the existing `#[cfg(test)] mod tests`, extend the `use super::{...}` line to also import `build_value_history, ValueHistoryInstrument` and add these tests (the `buy`, `split`, `price`, `fx`, `d` helpers already exist in this module):
```rust
fn vh_instrument(
    currency: &str,
    ledger: Vec<LedgerTransaction>,
    prices: Vec<PriceCandidate>,
    fx_rates: Vec<FxCandidate>,
) -> ValueHistoryInstrument {
    ValueHistoryInstrument {
        native_currency: currency.to_owned(),
        ledger,
        prices,
        fx_rates,
    }
}

#[test]
fn value_history_sek_single_holding_uses_price_dates() {
    let inst = vh_instrument(
        "SEK",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
        vec![
            price(d(2026, 1, 2), dec!(100), "SEK"),
            price(d(2026, 1, 5), dec!(110), "SEK"),
        ],
        vec![],
    );
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].date, d(2026, 1, 2));
    assert_eq!(points[0].value_base, dec!(1000));
    assert_eq!(points[0].included_count, 1);
    assert!(!points[0].incomplete);
    assert_eq!(points[1].value_base, dec!(1100));
}

#[test]
fn value_history_carries_price_and_fx_forward() {
    // FX-only spine date (1/6) moves base value even though the close is carried forward.
    let inst = vh_instrument(
        "USD",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
        vec![price(d(2026, 1, 2), dec!(100), "USD")],
        vec![
            fx(d(2026, 1, 2), dec!(10), "USD", "SEK"),
            fx(d(2026, 1, 6), dec!(11), "USD", "SEK"),
        ],
    );
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    // Spine = union of price+fx dates >= first buy: {1/2, 1/6}.
    assert_eq!(points.len(), 2);
    assert_eq!(points[0].date, d(2026, 1, 2));
    assert_eq!(points[0].value_base, dec!(10000)); // 10 * 100 * 10
    assert_eq!(points[1].date, d(2026, 1, 6));
    assert_eq!(points[1].value_base, dec!(11000)); // carried close 100 * fx 11
}

#[test]
fn value_history_split_adjusts_pre_split_points() {
    // Buy 10 @ 120 (pre-split), 2:1 split on 1/10, split-adjusted closes (60 ≈ 120/2).
    let inst = vh_instrument(
        "SEK",
        vec![
            buy(1, d(2026, 1, 2), 10, dec!(120), Some(dec!(1)), "SEK"),
            split(2, d(2026, 1, 10), 10),
        ],
        vec![
            price(d(2026, 1, 5), dec!(60), "SEK"),  // pre-split date, adjusted price
            price(d(2026, 1, 12), dec!(60), "SEK"), // post-split date
        ],
        vec![],
    );
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    // 1/5: qty held = 10, future split factor = 2 → adjusted qty 20 * 60 = 1200.
    let p5 = points.iter().find(|p| p.date == d(2026, 1, 5)).expect("1/5");
    assert_eq!(p5.value_base, dec!(1200));
    // 1/12: qty held = 20, no future split → 20 * 60 = 1200.
    let p12 = points.iter().find(|p| p.date == d(2026, 1, 12)).expect("1/12");
    assert_eq!(p12.value_base, dec!(1200));
}

#[test]
fn value_history_excludes_instrument_with_disabled_mapping() {
    // Empty `prices` models a disabled/removed mapping: instrument has no price.
    let inst = vh_instrument(
        "SEK",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
        vec![],
        vec![],
    );
    // No prices anywhere → no spine dates → empty series.
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    assert!(points.is_empty());
}

#[test]
fn value_history_marks_incomplete_and_omits_all_excluded_dates() {
    // Two SEK holdings. `present` has a price on 1/2; `absent` never does.
    let present = vh_instrument(
        "SEK",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
        vec![price(d(2026, 1, 2), dec!(100), "SEK")],
        vec![],
    );
    let absent = vh_instrument(
        "SEK",
        vec![buy(2, d(2026, 1, 2), 5, dec!(50), Some(dec!(1)), "SEK")],
        vec![price(d(2026, 1, 9), dec!(50), "SEK")], // only a later date
        vec![],
    );
    let points = build_value_history(&[present, absent], None, None).expect("derivable ledgers");
    // 1/2: present included, absent excluded (no price on/before) → incomplete.
    let p2 = points.iter().find(|p| p.date == d(2026, 1, 2)).expect("1/2");
    assert_eq!(p2.value_base, dec!(1000));
    assert_eq!(p2.included_count, 1);
    assert_eq!(p2.excluded_count, 1);
    assert!(p2.incomplete);
    // 1/9: both have a carried price → both included.
    let p9 = points.iter().find(|p| p.date == d(2026, 1, 9)).expect("1/9");
    assert_eq!(p9.included_count, 2);
    assert!(!p9.incomplete);
}

#[test]
fn value_history_omits_points_where_every_position_is_excluded() {
    // Single USD holding with a price but no FX at all → every spine date excludes it.
    let inst = vh_instrument(
        "USD",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(10)), "USD")],
        vec![price(d(2026, 1, 2), dec!(100), "USD")],
        vec![],
    );
    let points = build_value_history(&[inst], None, None).expect("derivable ledger");
    // included_count would be 0 on 1/2 → point omitted, never a spurious 0.00.
    assert!(points.is_empty());
}

#[test]
fn value_history_empty_when_no_buy_yet() {
    let points = build_value_history(&[], None, None).expect("no ledger is Ok(empty)");
    assert!(points.is_empty());
}

#[test]
fn value_history_windows_with_from_and_to() {
    let inst = vh_instrument(
        "SEK",
        vec![buy(1, d(2026, 1, 2), 10, dec!(100), Some(dec!(1)), "SEK")],
        vec![
            price(d(2026, 1, 2), dec!(100), "SEK"),
            price(d(2026, 1, 5), dec!(110), "SEK"),
            price(d(2026, 1, 9), dec!(120), "SEK"),
        ],
        vec![],
    );
    let points =
        build_value_history(&[inst], Some(d(2026, 1, 5)), Some(d(2026, 1, 5))).expect("derivable ledger");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].date, d(2026, 1, 5));
    assert_eq!(points[0].value_base, dec!(1100));
}
```

- [ ] **Step 3: Run the tests to confirm they fail**

Run from `backend/`: `cargo test --lib domain::valuation::tests::value_history`
Expected: compile error / FAIL — `build_value_history` and `ValueHistoryInstrument` are undefined.

- [ ] **Step 4: Implement the builder**

In `backend/src/domain/valuation.rs`, add near `build_price_history` (after the `PricePoint`/`build_price_history` block). First extend the imports at the top of the file:
```rust
use super::performance::split_factor;
use super::{derive_position, LedgerError, LedgerTransaction, TransactionKind};
use std::collections::BTreeSet;
```
(Adjust to the file's existing `use super::{...}` line rather than duplicating; `BaseCostBasis`, `Position`, `UnavailableReason` are already imported there.)

Then add:
```rust
#[derive(Clone, Debug)]
pub struct ValueHistoryInstrument {
    pub native_currency: String,
    pub ledger: Vec<LedgerTransaction>,
    pub prices: Vec<PriceCandidate>,
    pub fx_rates: Vec<FxCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueHistoryPoint {
    pub date: NaiveDate,
    pub value_base: Decimal,
    pub incomplete: bool,
    pub included_count: usize,
    pub excluded_count: usize,
}

/// Reconstruct portfolio value per spine date from the ledger + cached prices +
/// cached FX. The spine is the sorted union of stored price and FX dates, on or
/// after the earliest BUY trade date, optionally clamped to `[from, to]`. A date
/// where every active position is excluded (no carried price/FX) is omitted so
/// the series never contains a spurious zero.
pub fn build_value_history(
    instruments: &[ValueHistoryInstrument],
    from: Option<NaiveDate>,
    to: Option<NaiveDate>,
) -> Result<Vec<ValueHistoryPoint>, LedgerError> {
    let first_buy = instruments
        .iter()
        .flat_map(|inst| inst.ledger.iter())
        .filter(|tx| tx.kind == TransactionKind::Buy)
        .map(|tx| tx.trade_date)
        .min();
    let Some(first_buy) = first_buy else {
        return Ok(Vec::new());
    };

    let mut spine: BTreeSet<NaiveDate> = BTreeSet::new();
    for inst in instruments {
        for p in &inst.prices {
            spine.insert(p.date);
        }
        for f in &inst.fx_rates {
            spine.insert(f.date);
        }
    }

    let mut points = Vec::new();
    for date in spine {
        if date < first_buy {
            continue;
        }
        if from.is_some_and(|f| date < f) || to.is_some_and(|t| date > t) {
            continue;
        }

        let mut value = Decimal::ZERO;
        let mut included = 0usize;
        let mut excluded = 0usize;

        for inst in instruments {
            // Position held as of `date` from the ledger.
            let active: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date <= date)
                .cloned()
                .collect();
            // A derivation failure is a stored-ledger invariant violation, not
            // missing market data — surface it instead of dropping the instrument.
            let position = derive_position(&active)?;
            if position.quantity == 0 {
                continue; // not an active position; neither included nor excluded
            }

            // Normalize quantity to the split-adjusted (Yahoo) price convention by
            // applying the factors of all splits with effective_date > date.
            let future: Vec<LedgerTransaction> = inst
                .ledger
                .iter()
                .filter(|tx| tx.trade_date > date)
                .cloned()
                .collect();
            // A split-factor error is likewise an invariant violation, not a
            // reason to substitute an unadjusted factor of ONE.
            let factor = split_factor(&future, position.quantity)?;
            let adjusted_qty = Decimal::from(position.quantity) * factor;

            // Carry-forward close on or before `date`.
            let close = inst
                .prices
                .iter()
                .filter(|p| p.date <= date && p.currency.eq_ignore_ascii_case(&inst.native_currency))
                .last()
                .map(|p| p.close);

            // Carry-forward FX on or before `date` (identity for SEK).
            let is_base = inst.native_currency.eq_ignore_ascii_case("SEK");
            let rate = if is_base {
                Some(Decimal::ONE)
            } else {
                inst.fx_rates
                    .iter()
                    .filter(|f| f.date <= date)
                    .last()
                    .map(|f| f.rate)
            };

            match (close, rate) {
                (Some(close), Some(rate)) => {
                    value += adjusted_qty * close * rate;
                    included += 1;
                }
                _ => excluded += 1,
            }
        }

        if included == 0 {
            continue; // every active position excluded → not a real observation
        }

        points.push(ValueHistoryPoint {
            date,
            value_base: value,
            incomplete: excluded > 0,
            included_count: included,
            excluded_count: excluded,
        });
    }

    Ok(points)
}
```
Note: `inst.prices`/`inst.fx_rates` are sorted ascending by the caller, so `.filter(...).last()` is the carry-forward value. `TransactionKind` and `derive_position` come from the sibling modules (already exported in `domain/mod.rs`).

- [ ] **Step 5: Run the tests to confirm they pass**

Run from `backend/`: `cargo test --lib domain::valuation::tests::value_history`
Expected: PASS (8 new tests).

- [ ] **Step 6: Re-export from the domain module**

In `backend/src/domain/mod.rs`, add to the `pub use valuation::{...}` block:
```rust
build_value_history, ValueHistoryInstrument, ValueHistoryPoint,
```

- [ ] **Step 7: Clippy, fmt, stage**
```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/domain/valuation.rs backend/src/domain/performance.rs backend/src/domain/mod.rs
```
Leave the changes staged for review; do not commit.

---

### Task B2: `GET /api/portfolio/value-history` handler and route

**Files:**
- Create: `backend/src/api/portfolio.rs`
- Modify: `backend/src/api/mod.rs` (register module + route)
- Modify: `backend/Cargo.toml` (version bump)

**Interfaces:**
- Consumes: `build_value_history`, `ValueHistoryInstrument`, `ValueHistoryPoint` (B1); `load`-style repo calls (`instruments::list`, `provider_symbols::find_by_instrument_provider`, `prices::list_for_instrument_in_range`, `fx_rates::list_for_pair`, `transactions::all_for_holdings`); `money_string`, `BASE_CURRENCY`, `PRICE_PROVIDER`, `FX_PROVIDER` from `valuation.rs`.
- Produces: route `GET /api/portfolio/value-history?from=&to=` returning
  `{ "base_currency": "SEK", "points": [ { "date","value_base","incomplete","included_count","excluded_count" } ] }`.

- [ ] **Step 1: Create the handler module**

Create `backend/src/api/portfolio.rs`:
```rust
use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::valuation::{money_string, BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
use crate::db::{fx_rates, instruments, prices, provider_symbols, transactions};
use crate::domain::{
    build_value_history, FxCandidate, PriceCandidate, ValueHistoryInstrument, ValueHistoryPoint,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ValueHistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ValueHistoryResponse {
    base_currency: String,
    points: Vec<ValueHistoryPointResponse>,
}

#[derive(Debug, Serialize)]
pub struct ValueHistoryPointResponse {
    date: String,
    value_base: String,
    incomplete: bool,
    included_count: usize,
    excluded_count: usize,
}

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", format!("invalid {field}: {s}")))
}

fn point_response(point: &ValueHistoryPoint) -> ValueHistoryPointResponse {
    ValueHistoryPointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        value_base: money_string(point.value_base),
        incomplete: point.incomplete,
        included_count: point.included_count,
        excluded_count: point.excluded_count,
    }
}

pub async fn value_history(
    State(state): State<AppState>,
    Query(query): Query<ValueHistoryQuery>,
) -> Result<Json<ValueHistoryResponse>, ApiError> {
    let from = query.from.as_deref().map(|s| parse_date(s, "from")).transpose()?;
    let to = query.to.as_deref().map(|s| parse_date(s, "to")).transpose()?;
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(ApiError::bad_request(
                "invalid_date_range",
                "from must not be after to",
            ));
        }
    }

    let instruments_list = instruments::list(&state.pool).await?;

    // Group full ledgers by instrument id.
    let transaction_rows = transactions::all_for_holdings(&state.pool).await?;
    let mut ledgers: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    for row in &transaction_rows {
        ledgers
            .entry(row.instrument_id)
            .or_default()
            .push(row.to_ledger()?);
    }

    let mut inputs: Vec<ValueHistoryInstrument> = Vec::new();
    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        if ledger.is_empty() {
            continue;
        }

        // Cached prices only behind an enabled Yahoo mapping (identical gating to valuation).
        let mapping =
            provider_symbols::find_by_instrument_provider(&state.pool, instrument.id, PRICE_PROVIDER)
                .await?;
        let mapping_enabled = mapping.as_ref().is_some_and(|m| m.enabled);

        let prices: Vec<PriceCandidate> = if mapping_enabled {
            prices::list_for_instrument_in_range(&state.pool, instrument.id, PRICE_PROVIDER, None, None)
                .await?
                .into_iter()
                .map(|row| price_candidate(instrument, row))
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };

        let is_base = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
        let fx_rates: Vec<FxCandidate> = if is_base {
            Vec::new()
        } else {
            fx_rates::list_for_pair(&state.pool, &instrument.currency, BASE_CURRENCY, FX_PROVIDER)
                .await?
                .into_iter()
                .map(fx_candidate)
                .collect::<Result<_, _>>()?
        };

        inputs.push(ValueHistoryInstrument {
            native_currency: instrument.currency.clone(),
            ledger,
            prices,
            fx_rates,
        });
    }

    let points = build_value_history(&inputs, from, to).map_err(|err| {
        ApiError::internal(format!("value-history derivation failed: {err}"))
    })?;

    Ok(Json(ValueHistoryResponse {
        base_currency: BASE_CURRENCY.to_string(),
        points: points.iter().map(point_response).collect(),
    }))
}

// Mirror instrument_prices.rs: a decode failure is an internal invariant
// violation surfaced as `ApiError::internal` with row/instrument/field context,
// never silently dropped as if it were missing market data.
fn price_candidate(
    instrument: &instruments::InstrumentRow,
    row: prices::PriceRow,
) -> Result<PriceCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable date in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    let close = row.close_decimal().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable close in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    Ok(PriceCandidate {
        date,
        close,
        currency: row.currency,
    })
}

fn fx_candidate(row: fx_rates::FxRateRow) -> Result<FxCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable date in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    let rate = row.rate_decimal().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable rate in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    Ok(FxCandidate {
        date,
        rate,
        base: row.base,
        quote: row.quote,
    })
}
```

- [ ] **Step 2: Register the module and route**

In `backend/src/api/mod.rs`:
- Add `mod portfolio;` to the module list (after `mod instruments;` or alphabetically near `mod prices;`).
- In `api_router()`, add:
```rust
.route("/portfolio/value-history", get(portfolio::value_history))
```

- [ ] **Step 3: Write the failing handler tests**

Append a `#[cfg(test)] mod tests` to `backend/src/api/portfolio.rs`. Model the harness on `instrument_prices.rs` tests (`send`, `instrument`, `enable_mapping`, `seed_price`, `seed_fx`, `d`), but drive transactions through the HTTP API like `gains.rs` does. Minimum tests:
```rust
#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::api::valuation::{BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
    use crate::db::{fx_rates, instruments, prices, provider_symbols};
    use crate::import::now_iso8601;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(state: &AppState, method: &str, uri: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds");
        let response = router(state.clone()).oneshot(request).await.expect("completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("body");
        let value = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).expect("json") };
        (status, value)
    }

    async fn instrument(state: &AppState, symbol: &str, currency: &str) -> i64 {
        let (row, _) = instruments::upsert(&state.pool, &instruments::NewInstrument {
            symbol: symbol.to_owned(), exchange: "STO".to_owned(), name: symbol.to_owned(),
            kind: "STOCK".to_owned(), currency: currency.to_owned(), isin: None,
        }).await.expect("instrument");
        row.id
    }

    async fn enable_mapping(state: &AppState, instrument_id: i64, enabled: bool) {
        let now = now_iso8601();
        provider_symbols::upsert(&state.pool, &provider_symbols::NewProviderSymbol {
            instrument_id, provider: PRICE_PROVIDER.to_owned(), provider_symbol: "SYM".to_owned(),
            currency: None, enabled, created_at: now.clone(), updated_at: now,
        }).await.expect("mapping");
    }

    async fn seed_price(state: &AppState, instrument_id: i64, date: NaiveDate, close: rust_decimal::Decimal, currency: &str) {
        prices::upsert(&state.pool, &prices::NewPrice {
            instrument_id, provider: PRICE_PROVIDER.to_owned(), provider_symbol: "SYM".to_owned(),
            date, close, currency: currency.to_owned(), fetched_at: now_iso8601(),
        }).await.expect("price");
    }

    async fn seed_fx(state: &AppState, date: NaiveDate, rate: rust_decimal::Decimal) {
        fx_rates::upsert(&state.pool, &fx_rates::NewFxRate {
            base: "USD".to_owned(), quote: BASE_CURRENCY.to_owned(), date, rate,
            provider: FX_PROVIDER.to_owned(), fetched_at: now_iso8601(),
        }).await.expect("fx");
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate { NaiveDate::from_ymd_opt(y, m, day).expect("date") }

    #[tokio::test]
    async fn empty_portfolio_returns_empty_points() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["base_currency"], "SEK");
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn from_after_to_is_invalid_date_range() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "GET", "/api/portfolio/value-history?from=2026-06-12&to=2026-06-11", json!({})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date_range");
    }

    #[tokio::test]
    async fn sek_holding_produces_monotonic_value_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, true).await;
        send(&state, "POST", "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-01-02","quantity":10,"price":"100","currency":BASE_CURRENCY})).await;
        seed_price(&state, id, d(2026, 1, 2), dec!(100), BASE_CURRENCY).await;
        seed_price(&state, id, d(2026, 1, 5), dec!(110), BASE_CURRENCY).await;

        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let points = body["points"].as_array().expect("points");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["date"], "2026-01-02");
        assert_eq!(points[0]["value_base"], "1000.00");
        assert_eq!(points[0]["incomplete"], false);
        assert_eq!(points[0]["included_count"], 1);
        assert_eq!(points[1]["value_base"], "1100.00");
    }

    #[tokio::test]
    async fn disabled_mapping_excludes_cached_prices() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, false).await;
        send(&state, "POST", "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-01-02","quantity":10,"price":"100","currency":BASE_CURRENCY})).await;
        seed_price(&state, id, d(2026, 1, 2), dec!(100), BASE_CURRENCY).await;

        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        // No usable prices → no spine → empty series.
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }
}
```

- [ ] **Step 4: Run the handler tests**

Run from `backend/`: `cargo test --lib api::portfolio`
Expected: PASS (4 tests). Fix any harness mismatches against the real `instruments::NewInstrument` / `provider_symbols::NewProviderSymbol` field sets (copy exact fields from `instrument_prices.rs` if a field differs).

- [ ] **Step 5: Bump the backend version**

In `backend/Cargo.toml`, change `version = "0.5.5"` to `version = "0.6.0"`.

- [ ] **Step 6: Verify against a real DB (human-recommended)**

Start the backend and curl:
```bash
curl -s "http://localhost:8787/api/portfolio/value-history" | head
```
Confirm ascending dates and plausible SEK values. (Adjust the port to your dev config.)

- [ ] **Step 7: Clippy, fmt, stage**
```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/api/portfolio.rs backend/src/api/mod.rs backend/Cargo.toml
```
Leave the changes staged for review; do not commit.

---

## Phase C — Dashboard landing page

### Task C1: Routing — dashboard at `/`, board at `/board`

**Files:**
- Create: `frontend/src/components/Dashboard.tsx` (placeholder shell this task; filled in C2–D2)
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/AssetView.tsx` (two Back links → `/board`)
- Modify: `frontend/src/components/ImportView.tsx` (`navigate("/", …)` → `navigate("/board", …)`)

**Interfaces:**
- Produces: routes `/` → `Dashboard`, `/board` → `BoardView`; nav items Dashboard / Board / Import.

- [ ] **Step 1: Create the placeholder Dashboard**

Create `frontend/src/components/Dashboard.tsx`:
```tsx
export function Dashboard() {
  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <h1>Dashboard</h1>
    </section>
  );
}
```

- [ ] **Step 2: Update routing and nav in `App.tsx`**

Edit `frontend/src/App.tsx`:
- Import `Dashboard`: `import { Dashboard } from "./components/Dashboard";`
- Replace the nav block with three items:
```tsx
<nav className="app-nav" aria-label="Primary">
  <NavLink to="/" end className={navClass}>
    Dashboard
  </NavLink>
  <NavLink to="/board" className={navClass}>
    Board
  </NavLink>
  <NavLink to="/import" className={navClass}>
    Import
  </NavLink>
</nav>
```
- Replace the routes:
```tsx
<Routes>
  <Route path="/" element={<Dashboard />} />
  <Route path="/board" element={<BoardView />} />
  <Route path="/import" element={<ImportView />} />
  <Route path="/asset/:id" element={<AssetView />} />
</Routes>
```

- [ ] **Step 3: Point AssetView Back links at `/board`**

In `frontend/src/components/AssetView.tsx`, change both `to="/"` Back links (the not-found `<Link className="button outline" to="/">` and the header `<Link className="asset-back" to="/">`) to `to="/board"`. Leave the brand link in `App.tsx` pointing at `/`.

- [ ] **Step 4: Update the ImportView handoff**

In `frontend/src/components/ImportView.tsx` (~line 602), change `navigate("/", { state: { boardView: "transactions" } })` to `navigate("/board", { state: { boardView: "transactions" } })`.

- [ ] **Step 5: Type-check, format, verify (human-recommended)**
```bash
cd frontend && npm run check && npm run fmt
```
Run the app: `/` shows the Dashboard placeholder; `/board` shows the existing board; the asset page "← Back" returns to `/board`; an import "View transactions" handoff lands on `/board` with the transactions tab active. Browser Back/Forward behave.

- [ ] **Step 6: Stage**
```bash
git add frontend/src/components/Dashboard.tsx frontend/src/App.tsx frontend/src/components/AssetView.tsx frontend/src/components/ImportView.tsx
```
Leave the changes staged for review; do not commit.

---

### Task C2: Value-history query + value-over-time chart on the dashboard

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/queries.ts`
- Modify: `frontend/src/components/Dashboard.tsx`
- Modify: `frontend/src/styles.css`

**Interfaces:**
- Consumes: `TimeSeriesChart` (A1).
- Produces: `ValueHistoryResponse` type; `usePortfolioValueHistory()`; a value chart section in `Dashboard`.

- [ ] **Step 1: Add the response type**

In `frontend/src/api/types.ts` add:
```ts
export interface ValueHistoryPoint {
  date: string;
  value_base: string;
  incomplete: boolean;
  included_count: number;
  excluded_count: number;
}

export interface ValueHistoryResponse {
  base_currency: string;
  points: ValueHistoryPoint[];
}
```

- [ ] **Step 2: Add the query hook**

In `frontend/src/api/queries.ts`, add `ValueHistoryResponse` to the `./types` import block, then:
```ts
export function usePortfolioValueHistory() {
  return useQuery({
    queryKey: ["portfolio-value-history"],
    queryFn: () => apiGet<ValueHistoryResponse>("/api/portfolio/value-history"),
  });
}
```

- [ ] **Step 3: Render the chart in Dashboard**

Replace `frontend/src/components/Dashboard.tsx` with:
```tsx
import { usePortfolioValueHistory } from "../api/queries";
import { TimeSeriesChart } from "./TimeSeriesChart";

export function Dashboard() {
  const valueHistory = usePortfolioValueHistory();

  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <DashboardValueChart query={valueHistory} />
    </section>
  );
}

function DashboardValueChart({
  query,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
}) {
  if (query.isPending) {
    return (
      <section className="chart-band" aria-label="Portfolio value">
        <div className="skeleton-bar" />
      </section>
    );
  }
  if (query.isError) {
    return (
      <section className="chart-band error" aria-label="Portfolio value">
        <p className="down">Could not load portfolio value.</p>
        <button type="button" className="button outline" onClick={() => void query.refetch()}>
          Retry
        </button>
      </section>
    );
  }

  const points = (query.data?.points ?? []).map((p) => ({
    time: p.date,
    value: Number(p.value_base),
  }));
  const incompleteDays = (query.data?.points ?? []).filter((p) => p.incomplete).length;

  if (points.length === 0) {
    return (
      <section className="chart-band muted" aria-label="Portfolio value">
        <span className="chart-band-label">
          No portfolio history yet — add a Buy and refresh prices
        </span>
      </section>
    );
  }

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
      <TimeSeriesChart data={points} ariaLabel="Portfolio value over time in SEK" height={280} />
    </section>
  );
}
```

- [ ] **Step 4: Add dashboard layout styles**

Append to `frontend/src/styles.css`:
```css
.dashboard {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}
```

- [ ] **Step 5: Type-check, format, verify (human-recommended)**
```bash
cd frontend && npm run check && npm run fmt
```
`/` renders the value chart (or the empty state). If any day is incomplete, the chip shows.

- [ ] **Step 6: Stage**
```bash
git add frontend/src/api/types.ts frontend/src/api/queries.ts frontend/src/components/Dashboard.tsx frontend/src/styles.css
```
Leave the changes staged for review; do not commit.

---

### Task C3: Summary tiles (total value, day change, unrealized change)

**Files:**
- Modify: `frontend/src/components/Dashboard.tsx`
- Modify: `frontend/src/styles.css` (reuse existing `metric-tiles`/`metric-tile`)

**Interfaces:**
- Consumes: `useGains` (existing), `SummaryAvailabilityValue`, `isAvailable` from `valuationDisplay`.

- [ ] **Step 1: Query gains and add a summary tiles section**

In `frontend/src/components/Dashboard.tsx`, import:
```tsx
import { useGains } from "../api/queries";
import { SummaryAvailabilityValue } from "./valuationDisplay";
```
Call `const gainsQuery = useGains();` in `Dashboard` and render `<DashboardSummary summary={gainsQuery.data?.summary} />` above the value chart.

- [ ] **Step 2: Implement the tiles**

Add to `Dashboard.tsx`:
```tsx
import type { GainsSummary } from "../api/types";

function DashboardSummary({ summary }: { summary: GainsSummary | undefined }) {
  return (
    <section className="metric-tiles" aria-label="Portfolio summary">
      <div className="metric-tile">
        <span className="metric-tile-label">Total value</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue value={summary?.market_value_base} prefix="SEK " tone="plain" />
        </span>
      </div>
      <div className="metric-tile">
        <span className="metric-tile-label">Day change</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue value={summary?.day_change_base} prefix="SEK " tone="signed" />{" "}
          <SummaryAvailabilityValue value={summary?.day_change_percent} suffix="%" tone="signed" />
        </span>
      </div>
      <div className="metric-tile">
        <span className="metric-tile-label">Unrealized change</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue value={summary?.unrealized_gain_base} prefix="SEK " tone="signed" />{" "}
          <SummaryAvailabilityValue value={summary?.unrealized_gain_percent} suffix="%" tone="signed" />
        </span>
      </div>
    </section>
  );
}
```
Note: the third tile is labelled "Unrealized change" and reads `summary.unrealized_gain_*` — **not** method-dependent total return. Do not wire `totals.total_return_*` here.

- [ ] **Step 3: Type-check, format, verify, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/Dashboard.tsx
```
Leave the changes staged for review; do not commit.

---

### Task C4: Top-movers selector + panel

**Files:**
- Create: `frontend/src/components/dashboardSelectors.ts`
- Create: `frontend/src/components/dashboardSelectors.test.ts`
- Modify: `frontend/src/components/Dashboard.tsx`

**Interfaces:**
- Consumes: `GainsRow` from `types.ts`.
- Produces: `topMovers(rows: GainsRow[]): { gainers: MoverRow[]; losers: MoverRow[] }` where
  `MoverRow = { instrument: Instrument; percent: number }`.

- [ ] **Step 1: Write the failing selector test**

Create `frontend/src/components/dashboardSelectors.test.ts`:
```ts
import { describe, expect, it } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { topMovers } from "./dashboardSelectors";

function inst(id: number, symbol: string, name = symbol): Instrument {
  return { id, symbol, exchange: "STO", name, type: "Stock", currency: "SEK" };
}

function row(
  instrument: Instrument,
  status: "open" | "closed",
  dayChangePercent: GainsRow["day_change_percent"],
): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument,
    quantity: status === "open" ? 1 : 0,
    cost_basis_native: "0.00",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: money,
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: money,
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    day_change_base: money,
    day_change_percent: dayChangePercent,
    reasons: [],
    position_status: status,
  };
}

const avail = (v: string) => ({ status: "available", value: v }) as const;
const unavail = { status: "unavailable", reasons: ["zero_previous_market_value"] } as const;

describe("topMovers", () => {
  it("ranks open gainers desc and losers asc, excluding unavailable and closed rows", () => {
    const rows = [
      row(inst(1, "AAA"), "open", avail("5.0")),
      row(inst(2, "BBB"), "open", avail("-3.0")),
      row(inst(3, "CCC"), "open", avail("1.0")),
      row(inst(4, "DDD"), "open", avail("-8.0")),
      row(inst(5, "EEE"), "open", unavail), // excluded: unavailable
      row(inst(6, "FFF"), "closed", avail("99.0")), // excluded: closed
    ];
    const { gainers, losers } = topMovers(rows);
    expect(gainers.map((m) => m.instrument.symbol)).toEqual(["AAA", "CCC"]);
    expect(losers.map((m) => m.instrument.symbol)).toEqual(["DDD", "BBB"]);
  });

  it("breaks ties by symbol then name", () => {
    const rows = [
      row(inst(1, "ZZZ"), "open", avail("4.0")),
      row(inst(2, "AAA"), "open", avail("4.0")),
    ];
    const { gainers } = topMovers(rows);
    expect(gainers.map((m) => m.instrument.symbol)).toEqual(["AAA", "ZZZ"]);
  });

  it("caps each side at three and tolerates fewer", () => {
    const rows = [
      row(inst(1, "A"), "open", avail("1")),
      row(inst(2, "B"), "open", avail("2")),
      row(inst(3, "C"), "open", avail("3")),
      row(inst(4, "D"), "open", avail("4")),
    ];
    const { gainers, losers } = topMovers(rows);
    expect(gainers).toHaveLength(3);
    expect(losers).toHaveLength(0); // no negatives
  });

  it("returns empty sides for all-flat / empty input", () => {
    expect(topMovers([])).toEqual({ gainers: [], losers: [] });
    const { gainers, losers } = topMovers([row(inst(1, "A"), "open", avail("0"))]);
    expect(gainers).toEqual([]);
    expect(losers).toEqual([]);
  });
});
```

- [ ] **Step 2: Run to confirm failure**

Run **the frontend test command**.
Expected: FAIL — `topMovers` not defined.

- [ ] **Step 3: Implement the selector**

Create `frontend/src/components/dashboardSelectors.ts`:
```ts
import type { GainsRow, Instrument } from "../api/types";

export interface MoverRow {
  instrument: Instrument;
  percent: number;
}

interface Candidate extends MoverRow {}

function tieBreak(a: Candidate, b: Candidate): number {
  const bySymbol = a.instrument.symbol.localeCompare(b.instrument.symbol);
  if (bySymbol !== 0) return bySymbol;
  return a.instrument.name.localeCompare(b.instrument.name);
}

/**
 * Top 3 gainers + top 3 losers by available day_change_percent. Open rows only;
 * unavailable percentages excluded; zero is neither a gainer nor a loser.
 */
export function topMovers(rows: GainsRow[]): {
  gainers: MoverRow[];
  losers: MoverRow[];
} {
  const candidates: Candidate[] = [];
  for (const row of rows) {
    if (row.position_status !== "open") continue;
    if (row.day_change_percent.status !== "available") continue;
    const percent = Number(row.day_change_percent.value);
    if (!Number.isFinite(percent)) continue;
    candidates.push({ instrument: row.instrument, percent });
  }

  const gainers = candidates
    .filter((c) => c.percent > 0)
    .sort((a, b) => b.percent - a.percent || tieBreak(a, b))
    .slice(0, 3);

  const losers = candidates
    .filter((c) => c.percent < 0)
    .sort((a, b) => a.percent - b.percent || tieBreak(a, b))
    .slice(0, 3);

  return { gainers, losers };
}
```

- [ ] **Step 4: Run to confirm passing**

Run **the frontend test command**.
Expected: PASS.

- [ ] **Step 5: Render the panel**

In `Dashboard.tsx`, import `Link` from `react-router-dom`, `topMovers` from `./dashboardSelectors`, and `formatGroupedNumber` from `./valuationDisplay`. Call `topMovers(gainsQuery.data?.rows ?? [])` and render below the value chart:
```tsx
function TopMoversPanel({ rows }: { rows: GainsRow[] }) {
  const { gainers, losers } = topMovers(rows);
  if (gainers.length === 0 && losers.length === 0) {
    return null;
  }
  return (
    <section className="panel" aria-label="Top movers">
      <h2>Top movers</h2>
      <div className="movers-grid">
        <MoverList title="Gainers" movers={gainers} />
        <MoverList title="Losers" movers={losers} />
      </div>
    </section>
  );
}

function MoverList({ title, movers }: { title: string; movers: MoverRow[] }) {
  return (
    <div className="mover-list">
      <h3>{title}</h3>
      {movers.length === 0 ? (
        <p className="asset-subtle">—</p>
      ) : (
        <ul>
          {movers.map((m) => (
            <li key={m.instrument.id}>
              <Link className="instrument-link" to={`/asset/${m.instrument.id}`}>
                {m.instrument.symbol}
              </Link>
              <span className={m.percent >= 0 ? "up number" : "down number"}>
                {m.percent >= 0 ? "+" : ""}
                {formatGroupedNumber(m.percent.toFixed(2))}%
              </span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
```
Import the `MoverRow` type and `GainsRow` type as needed. Add minimal `.movers-grid { display:grid; grid-template-columns:1fr 1fr; gap:1rem; }` and `.mover-list ul { list-style:none; margin:0; padding:0; }`, `.mover-list li { display:flex; justify-content:space-between; padding:0.25rem 0; }` to `styles.css`.

- [ ] **Step 6: Type-check, format, verify, stage**
```bash
cd frontend && npm run check && npm run fmt
git add frontend/src/components/dashboardSelectors.ts frontend/src/components/dashboardSelectors.test.ts frontend/src/components/Dashboard.tsx frontend/src/styles.css
```
Leave the changes staged for review; do not commit.

---

## Phase D — Allocation breakdown

### Task D1: Allocation aggregation selector

**Files:**
- Modify: `frontend/src/components/dashboardSelectors.ts`
- Modify: `frontend/src/components/dashboardSelectors.test.ts`

**Interfaces:**
- Produces:
  ```ts
  export type AllocationDimension = "instrument" | "currency" | "type";
  export interface AllocationSlice { key: string; label: string; valueBase: number; weightPercent: number; }
  export interface Allocation { slices: AllocationSlice[]; excludedCount: number; }
  export function allocationBreakdown(rows: GainsRow[], dimension: AllocationDimension): Allocation;
  ```

- [ ] **Step 1: Write the failing test**

Append to `frontend/src/components/dashboardSelectors.test.ts`:
```ts
import { allocationBreakdown } from "./dashboardSelectors";

describe("allocationBreakdown", () => {
  function mvRow(
    instrument: Instrument,
    marketValue: GainsRow["market_value_base"],
  ): GainsRow {
    const r = row(instrument, "open", { status: "available", value: "0.00" });
    return { ...r, market_value_base: marketValue };
  }

  it("aggregates by instrument and computes weights summing to 100", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "750.00" }),
      mvRow(inst(2, "BBB"), { status: "available", value: "250.00" }),
    ];
    const { slices, excludedCount } = allocationBreakdown(rows, "instrument");
    expect(excludedCount).toBe(0);
    expect(slices.map((s) => s.label)).toEqual(["AAA", "BBB"]);
    expect(slices.map((s) => s.weightPercent)).toEqual([75, 25]);
    expect(slices.reduce((a, s) => a + s.weightPercent, 0)).toBe(100);
  });

  it("groups by currency and type", () => {
    const usd: Instrument = { ...inst(1, "AAA"), currency: "USD" };
    const sek: Instrument = { ...inst(2, "BBB"), currency: "SEK" };
    const etf: Instrument = { ...inst(3, "CCC"), type: "Etf" };
    const rows = [
      mvRow(usd, { status: "available", value: "100.00" }),
      mvRow(sek, { status: "available", value: "100.00" }),
      mvRow(etf, { status: "available", value: "200.00" }),
    ];
    expect(allocationBreakdown(rows, "currency").slices.map((s) => s.label).sort()).toEqual(["SEK", "USD"]);
    expect(allocationBreakdown(rows, "type").slices.some((s) => s.label === "Etf")).toBe(true);
  });

  it("excludes unavailable market values, never counting them as zero", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "100.00" }),
      mvRow(inst(2, "BBB"), { status: "unavailable", reasons: ["missing_fx"] }),
    ];
    const { slices, excludedCount } = allocationBreakdown(rows, "instrument");
    expect(excludedCount).toBe(1);
    expect(slices).toHaveLength(1);
    expect(slices[0].weightPercent).toBe(100);
  });

  it("returns empty allocation for no available rows", () => {
    expect(allocationBreakdown([], "instrument")).toEqual({ slices: [], excludedCount: 0 });
  });
});
```

- [ ] **Step 2: Run to confirm failure**

Run **the frontend test command**. Expected: FAIL — `allocationBreakdown` not defined.

- [ ] **Step 3: Implement**

Append to `frontend/src/components/dashboardSelectors.ts`:
```ts
export type AllocationDimension = "instrument" | "currency" | "type";

export interface AllocationSlice {
  key: string;
  label: string;
  valueBase: number;
  weightPercent: number;
}

export interface Allocation {
  slices: AllocationSlice[];
  excludedCount: number;
}

function bucketKey(row: GainsRow, dimension: AllocationDimension): string {
  switch (dimension) {
    case "instrument":
      return row.instrument.symbol;
    case "currency":
      return row.instrument.currency;
    case "type":
      return row.instrument.type;
  }
}

/**
 * Aggregate open + closed rows' available market_value_base into weighted slices.
 * Display-only float math (per the 2026-06-18 decision); never persisted.
 * Unavailable market values are excluded and counted, never treated as zero.
 */
export function allocationBreakdown(
  rows: GainsRow[],
  dimension: AllocationDimension,
): Allocation {
  const totals = new Map<string, number>();
  let total = 0;
  let excludedCount = 0;

  for (const row of rows) {
    if (row.market_value_base.status !== "available") {
      excludedCount += 1;
      continue;
    }
    const value = Number(row.market_value_base.value);
    if (!Number.isFinite(value) || value === 0) {
      continue;
    }
    const key = bucketKey(row, dimension);
    totals.set(key, (totals.get(key) ?? 0) + value);
    total += value;
  }

  if (total === 0) {
    return { slices: [], excludedCount };
  }

  const slices: AllocationSlice[] = [...totals.entries()]
    .map(([key, valueBase]) => ({
      key,
      label: key,
      valueBase,
      weightPercent: (valueBase / total) * 100,
    }))
    .sort((a, b) => b.valueBase - a.valueBase || a.label.localeCompare(b.label));

  return { slices, excludedCount };
}
```

- [ ] **Step 4: Run to confirm passing**

Run **the frontend test command**. Expected: PASS.
Note: the `weightPercent` exact-100 assertions hold for the chosen fixtures (75/25, 100); the component must not assume weights are pre-rounded.

- [ ] **Step 5: Format, stage**
```bash
cd frontend && npm run fmt
git add frontend/src/components/dashboardSelectors.ts frontend/src/components/dashboardSelectors.test.ts
```
Leave the changes staged for review; do not commit.

---

### Task D2: Allocation panel (segmented bar + legend + table)

**Files:**
- Modify: `frontend/src/components/Dashboard.tsx`
- Modify: `frontend/src/styles.css`

**Interfaces:**
- Consumes: `allocationBreakdown`, `AllocationDimension` (D1); `useGains` rows already in `Dashboard`.

- [ ] **Step 1: Add the allocation panel with a dimension toggle**

In `Dashboard.tsx`, add `useState` from `react`, import `allocationBreakdown`, type `AllocationDimension`. Render an `<AllocationPanel rows={gainsQuery.data?.rows ?? []} />` below top movers:
```tsx
function AllocationPanel({ rows }: { rows: GainsRow[] }) {
  const [dimension, setDimension] = useState<AllocationDimension>("instrument");
  const { slices, excludedCount } = allocationBreakdown(rows, dimension);

  const palette = ["#4f9cff", "#46c39a", "#e6a93b", "#d96c6c", "#9b8cff", "#5fb0c9"];

  return (
    <section className="panel allocation-panel" aria-label="Allocation">
      <div className="panel-header">
        <h2>Allocation</h2>
        <fieldset className="segmented-control">
          <legend className="sr-only">Allocation dimension</legend>
          {(["instrument", "currency", "type"] as AllocationDimension[]).map((dim) => (
            <button
              key={dim}
              type="button"
              className={dimension === dim ? "active" : undefined}
              aria-pressed={dimension === dim}
              onClick={() => setDimension(dim)}
            >
              {dim[0].toUpperCase() + dim.slice(1)}
            </button>
          ))}
        </fieldset>
      </div>

      {slices.length === 0 ? (
        <p className="board-state muted">No valued holdings to allocate.</p>
      ) : (
        <>
          <div className="allocation-bar" role="img" aria-label="Allocation segments">
            {slices.map((slice, i) => (
              <span
                key={slice.key}
                className="allocation-segment"
                style={{ width: `${slice.weightPercent}%`, background: palette[i % palette.length] }}
                title={`${slice.label} ${slice.weightPercent.toFixed(1)}%`}
              />
            ))}
          </div>
          <table className="allocation-table">
            <tbody>
              {slices.map((slice, i) => (
                <tr key={slice.key}>
                  <td>
                    <span className="allocation-swatch" style={{ background: palette[i % palette.length] }} />
                    {slice.label}
                  </td>
                  <td className="number">SEK {formatGroupedNumber(slice.valueBase.toFixed(2))}</td>
                  <td className="number">{slice.weightPercent.toFixed(1)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
          {excludedCount > 0 ? (
            <span className="status-chip warning compact">
              {excludedCount} excluded (no market value)
            </span>
          ) : null}
        </>
      )}
    </section>
  );
}
```

- [ ] **Step 2: Add allocation styles**

Append to `frontend/src/styles.css`:
```css
.allocation-bar {
  display: flex;
  width: 100%;
  height: 14px;
  border-radius: 7px;
  overflow: hidden;
  background: rgba(148, 163, 184, 0.12);
}
.allocation-segment { height: 100%; }
.allocation-table { width: 100%; border-collapse: collapse; margin-top: 0.75rem; }
.allocation-table td { padding: 0.25rem 0; }
.allocation-table td:not(:first-child) { text-align: right; }
.allocation-swatch {
  display: inline-block;
  width: 10px;
  height: 10px;
  border-radius: 2px;
  margin-right: 0.5rem;
}
```

- [ ] **Step 3: Type-check, format, verify (human-recommended)**
```bash
cd frontend && npm run check && npm run fmt
```
Toggling Instrument / Currency / Type repartitions the bar; segment widths sum to 100%; an instrument with an unavailable market value is excluded and noted, never shown as a zero slice.

- [ ] **Step 4: Stage**
```bash
git add frontend/src/components/Dashboard.tsx frontend/src/styles.css
```
Leave the changes staged for review; do not commit.

---

## Final Task: Documentation, version bump, Design.HighLevel updates

**Files:**
- Modify: `docs/DecisionLog.md`
- Modify: `docs/Design.HighLevel.md`
- Modify: `frontend/package.json` (version)

- [ ] **Step 1: Add DecisionLog entries**

Append two entries to `docs/DecisionLog.md` (match the existing dated-heading style). Entry (a) records the value-history endpoint conventions; entry (b) records the dashboard-as-landing navigation change:
```markdown
## 2026-06-22 - Portfolio value-history endpoint conventions
Decision: `GET /api/portfolio/value-history` reconstructs portfolio value per date on the fly from the ledger + cached prices + cached FX (no stored snapshots). The spine is the sorted union of stored price and FX dates on or after the earliest BUY trade date; FX-only dates are included so base-currency value can move on FX-only days. Each instrument's split-adjusted position is valued at the carry-forward close × carry-forward FX on or before the date, normalizing held quantity by the factors of all splits with effective_date > date (the 2026-06-19 split-adjusted convention). Cached prices are used only behind an enabled Yahoo mapping. A date where every active position lacks price/FX is omitted (carries `included_count`); points with missing inputs are kept and marked `incomplete` with an `excluded_count`. `value_base` uses the 2-decimal money format. `from > to` returns `400 invalid_date_range`. Stored-data invariant violations (an underivable ledger, an undecodable price/FX row) are surfaced as `500 internal` with row/instrument context, never silently dropped as if they were missing market data — consistent with the per-instrument price-history endpoint.
Context: Second slice of Phase 4 charts; feeds the dashboard value-over-time chart while honoring ledger-as-source-of-truth and reusing valuation gating and carry-forward conventions.
Consequences: No schema change and no materialized derived truth. Series never contains a spurious zero from an all-excluded date. Allocation and top movers continue to derive from `GET /api/gains`, not this endpoint.

## 2026-06-22 - Dashboard is the landing page
Decision: `/` renders the dashboard (summary tiles, portfolio value chart, top movers, allocation); the portfolio board moved to `/board`. Asset "Back" links and the import board-tab handoff target `/board`. Top-movers rank open rows by available `day_change_percent` (gainers desc, losers asc, ties by symbol then name); allocation weights use display-only frontend float math (2026-06-18) over `market_value_base`.
Context: Conventional portfolio-tracker model — overview first, detailed tables one level in.
Consequences: Deep links to `/board` are stable; the unrealized-change summary tile is current-exposure oriented (`summary.unrealized_gain_*`), not method-dependent total return.
```

- [ ] **Step 2: Mark Phase 4 items done in Design.HighLevel.md**

In `docs/Design.HighLevel.md`, mark the four delivered Phase 4 items (per-instrument chart, portfolio value chart, dashboard landing, allocation breakdown) as done in the existing style. Do not reference this plan or "Phase A/B/C/D".

- [ ] **Step 3: Bump the frontend version**

In `frontend/package.json`, bump `"version": "0.7.4"` to `"0.8.0"`. `package-lock.json` also carries the root package version, so refresh it too: run `npm.cmd install --package-lock-only` from `frontend/` (or edit the two `version` fields in `package-lock.json` by hand) so the manifest and lockfile do not drift. Stage both files in Step 5.

- [ ] **Step 4: Full verification**

Run from `backend/`: `cargo test` then `cargo clippy --all-targets -- -D warnings && cargo fmt`.
Run from `frontend/`: `npm run check`, **the frontend test command**, then `npm run fmt`.
Expected: all green.

- [ ] **Step 5: Stage**
```bash
git add docs/DecisionLog.md docs/Design.HighLevel.md frontend/package.json frontend/package-lock.json
```
Leave the changes staged for review; do not commit.

---

## Self-Review notes (coverage map)

- Spec Phase A (per-instrument chart, unavailable-close_base guard, missing-FX chip, empty state) → Tasks A1–A3; guard test in A2.
- Spec Phase B (endpoint, spine = price∪FX dates, carry-forward, split adjustment, mapping gating, incomplete/excluded, all-excluded omission, response shape, pure builder + thin handler, `from>to` 400) → Tasks B1–B2 with the eight builder tests + four handler tests.
- Spec Phase C (routing `/`→dashboard & `/board`, nav, Back/handoff updates, summary tiles from `summary` labelled "unrealized change", value chart with incomplete chip, top-movers pure selector with the documented rules) → Tasks C1–C4.
- Spec Phase D (instrument/currency/type toggle, CSS segmented bar + legend + table, pure aggregation over `market_value_base`, unavailable excluded) → Tasks D1–D2.
- Spec §8 (DecisionLog entries a+b, version bumps, Design.HighLevel done marks) → Final Task.

**Resolved during review (2026-06-22):** the frontend had no test runner. Task A2 Step 1 establishes Vitest + jsdom + React Testing Library, a `vitest.config.ts`, and folds `vitest run` into `npm run check` so types, lint, and tests are one completion gate — matching the backend's clippy+tests bar and the pure/unit-testable architecture in `Agents.md`. RTL/jsdom are installed now (not just bare Vitest) so later component/reducer tests need no setup re-litigation.
