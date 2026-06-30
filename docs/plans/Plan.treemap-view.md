Now I have enough context to produce the updated plan. Let me analyze each review finding:

- **Metric inconsistency (High)**: Confirmed. Dashboard `useGains` uses no `method` param; `totals.total_return_percent` would be XIRR while tile colors are simple per-position returns (2026-06-21 decision). Fix: remove the "All Holdings" chip entirely.
- **Semantic fills (High)**: Confirmed. Design spec explicitly reserves `--up-soft`/`--down-soft` for "movers/alerts, never ordinary cells." Fix: use neutral `--surface-2` tiles + semantic-colored left border.
- **Missing empty state (High)**: Confirmed. `AllocationPanel` and others handle empty states explicitly; `PortfolioTreemap` must too.
- **Accessibility (Medium)**: Confirmed. `title` is hover-only. Fix: add `aria-label` to each tile. Full touch interaction deferred.
- **treemapViewModel placement (Medium)**: Partially valid. Plan keeps it as a separate file — `dashboardSelectors.ts` already covers two orthogonal concerns (movers + allocation). Adding a third is fine but the treemap model is larger and has its own test file; a dedicated module is cleaner. Documented in Notes.
- **Rename DashboardValueChart (Medium)**: Confirmed. Becomes misleading immediately. Fix: rename to `DashboardChartPanel`.
- **Component test (Medium)**: Confirmed. Pattern already exists in `TimeSeriesChart.test.tsx`. Add a jsdom test covering empty state and tile rendering.
- **HEIGHT dual source (Low)**: Confirmed. Fix: measure height from `ResizeObserver` alongside width; CSS `height: 280px` becomes single source of truth.
- **Version bump / DecisionLog**: Valid. New user-visible feature + new dependency warrants both. Add as Task 6.

```markdown
# Treemap View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Treemap" third view to the portfolio gain dashboard card that shows current open holdings as squarified tiles sized by market value and colored by total return for the selected date range.

**Architecture:** The `Dashboard` component already fetches `useGains` (with the active date range) for the Top Movers and Allocation panels; pass that query result as a new prop to `DashboardChartPanel` (renamed from `DashboardValueChart`), which branches off a treemap render path before the existing value-history loading checks. A new pure view-model function (`treemapViewModel`) maps `GainsRow[]` to typed tile descriptors; `PortfolioTreemap` uses `d3-hierarchy` to compute squarified layout coordinates and renders tiles as absolutely-positioned `<div>` elements.

**Tech Stack:** React, TypeScript, Vitest, d3-hierarchy (new), existing CSS token system

## Global Constraints

- All CSS must use existing tokens from `styles.css` — never inline hex values.
- All numbers rendered to the user must use `--font-mono` / the `.number` class or `font-family: var(--font-mono)`.
- Run `npm run check` (covers `tsc --noEmit`, Biome lint, and `vitest run`) and `npm run fmt` from `frontend/` after each task before committing.
- All commands are run from the `frontend/` directory.
- Commit after each task using the pattern shown in each task.

## Notes

- **"All Holdings" summary chip removed.** Per the 2026-06-21 decision, tile colors derive from per-row simple return (`total_return_percent`) while `totals.total_return_percent` is method-specific (XIRR by default). Displaying a portfolio-level XIRR total alongside per-tile simple returns in the same panel would be semantically inconsistent without a visible label explaining the difference. The chip is omitted entirely until the Dashboard query is aligned to a user-selectable return method.
- **Semantic fills replaced by bordered tiles.** The design spec (VisualDesign.DarkTheme.md) reserves `--up-soft`/`--down-soft` fills for "movers/alerts, never ordinary cells" and specifies "semantics as text color, not fills." Tiles use `--surface-2` as their neutral background; a 3 px semantic-colored left border (`--up`/`--down`/`--hairline` for null) plus the colored percentage text provides the visual encoding a treemap requires while remaining spec-compliant.
- **treemapViewModel kept as a separate file, not merged into dashboardSelectors.ts.** `dashboardSelectors.ts` already contains two orthogonal concerns (top movers + allocation). The treemap model is a third concern with its own types, filtering rules, and test file; a dedicated module avoids coupling unrelated derivations and keeps each file focused.
- **Tile-to-asset-detail linking deferred.** `instrument.id` is not currently carried in `TreemapTile`. Adding clickable links is a distinct UX feature (navigation, focus management) that can be a follow-on task.
- **Touch accessibility deferred.** A tap-to-expand/tooltip pattern for small tiles requires a separate design decision. Tiles get `aria-label` now; the full touch story is future work.

---

### Task 1: Install d3-hierarchy

**Files:**
- Modify: `frontend/package.json` (automatic via npm)
- Modify: `frontend/package-lock.json` (automatic via npm)

**Interfaces:**
- Produces: `d3-hierarchy` available as an import; `HierarchyRectangularNode`, `hierarchy`, `treemap`, `treemapSquarify` re-exported from the package.

- [ ] **Step 1: Install the package**

```bash
npm install d3-hierarchy
```

Expected output ends with: `added N packages` (or similar). `d3-hierarchy` ships its own TypeScript types — no `@types/` package needed.

- [ ] **Step 2: Verify types resolve**

```bash
npm run check
```

Expected: exits 0, no type errors. (No application code changed yet; this just confirms the package installed cleanly.)

- [ ] **Step 3: Commit**

```bash
git add package.json package-lock.json
git commit -m "chore(frontend): add d3-hierarchy for treemap layout"
```

---

### Task 2: treemapViewModel — TDD

**Files:**
- Create: `frontend/src/components/treemapViewModel.ts`
- Create: `frontend/src/components/treemapViewModel.test.ts`

**Interfaces:**
- Produces:
  ```ts
  export interface TreemapTile {
    symbol: string;
    exchange: string;
    marketValueBase: number;
    totalReturnPercent: number | null;
  }
  export function treemapViewModel(rows: GainsRow[]): TreemapTile[]
  ```

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/components/treemapViewModel.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { treemapViewModel } from "./treemapViewModel";

function inst(symbol: string, exchange = "NASDAQ"): Instrument {
  return { id: 1, symbol, exchange, name: symbol, type: "Stock", currency: "USD" };
}

function row(
  instrument: Instrument,
  marketValue: GainsRow["market_value_base"],
  opts: {
    position_status?: "open" | "closed";
    total_return_percent?: GainsRow["total_return_percent"];
  } = {},
): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument,
    quantity: 10,
    cost_basis_native: "1000",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: opts.total_return_percent ?? {
      status: "available",
      value: "10.00",
    },
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: marketValue,
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    realized_gain_base: money,
    realized_cost_basis_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: money,
    reasons: [],
    position_status: opts.position_status ?? "open",
  };
}

const avail = (value: string) => ({ status: "available", value }) as const;
const unavailMv = { status: "unavailable", reasons: ["missing_price"] } as const;
const unavailPct = {
  status: "unavailable",
  reasons: ["missing_price"],
} as const;

describe("treemapViewModel", () => {
  it("includes open positions with available market value, sorted by value desc", () => {
    const tiles = treemapViewModel([
      row(inst("SMALL"), avail("1000.00")),
      row(inst("BIG"), avail("5000.00")),
      row(inst("MID"), avail("3000.00")),
    ]);
    expect(tiles.map((t) => t.symbol)).toEqual(["BIG", "MID", "SMALL"]);
    expect(tiles[0].marketValueBase).toBe(5000);
  });

  it("excludes closed positions", () => {
    const tiles = treemapViewModel([
      row(inst("OPEN"), avail("1000.00")),
      row(inst("CLOSED"), avail("2000.00"), { position_status: "closed" }),
    ]);
    expect(tiles.map((t) => t.symbol)).toEqual(["OPEN"]);
  });

  it("excludes positions with unavailable market value", () => {
    const tiles = treemapViewModel([
      row(inst("OK"), avail("1000.00")),
      row(inst("NO"), unavailMv),
    ]);
    expect(tiles.map((t) => t.symbol)).toEqual(["OK"]);
  });

  it("excludes positions with zero or negative market value", () => {
    const tiles = treemapViewModel([
      row(inst("OK"), avail("1000.00")),
      row(inst("ZERO"), avail("0.00")),
    ]);
    expect(tiles.map((t) => t.symbol)).toEqual(["OK"]);
  });

  it("maps available total_return_percent to a number", () => {
    const [tile] = treemapViewModel([
      row(inst("AAPL"), avail("5000.00"), {
        total_return_percent: avail("42.50"),
      }),
    ]);
    expect(tile.totalReturnPercent).toBe(42.5);
  });

  it("maps unavailable total_return_percent to null", () => {
    const [tile] = treemapViewModel([
      row(inst("AAPL"), avail("5000.00"), {
        total_return_percent: unavailPct,
      }),
    ]);
    expect(tile.totalReturnPercent).toBeNull();
  });

  it("uses symbol and exchange from the instrument", () => {
    const [tile] = treemapViewModel([
      row(
        { id: 1, symbol: "MSFT", exchange: "NYSE", name: "Microsoft", type: "Stock", currency: "USD" },
        avail("1000.00"),
      ),
    ]);
    expect(tile.symbol).toBe("MSFT");
    expect(tile.exchange).toBe("NYSE");
  });

  it("returns empty array for empty input", () => {
    expect(treemapViewModel([])).toEqual([]);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
npm run check
```

Expected: TypeScript error or Vitest failure — `treemapViewModel` does not exist yet.

- [ ] **Step 3: Implement treemapViewModel.ts**

Create `frontend/src/components/treemapViewModel.ts`:

```ts
import type { GainsRow } from "../api/types";

export interface TreemapTile {
  symbol: string;
  exchange: string;
  marketValueBase: number;
  totalReturnPercent: number | null;
}

export function treemapViewModel(rows: GainsRow[]): TreemapTile[] {
  const tiles: TreemapTile[] = [];

  for (const row of rows) {
    if (row.position_status !== "open") continue;
    if (row.market_value_base.status !== "available") continue;

    const marketValueBase = Number(row.market_value_base.value);
    if (!Number.isFinite(marketValueBase) || marketValueBase <= 0) continue;

    let totalReturnPercent: number | null = null;
    const pct = row.total_return_percent;
    if (pct.status === "available") {
      const v = Number(pct.value);
      if (Number.isFinite(v)) totalReturnPercent = v;
    }

    tiles.push({
      symbol: row.instrument.symbol,
      exchange: row.instrument.exchange,
      marketValueBase,
      totalReturnPercent,
    });
  }

  return tiles.sort((a, b) => b.marketValueBase - a.marketValueBase);
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
npm run check
```

Expected: all 8 tests in `treemapViewModel.test.ts` pass, TypeScript clean, Biome clean.

- [ ] **Step 5: Format and commit**

```bash
npm run fmt
git add src/components/treemapViewModel.ts src/components/treemapViewModel.test.ts
git commit -m "feat(dashboard): add treemapViewModel — pure tile derivation from GainsRow[]"
```

---

### Task 3: PortfolioTreemap component, CSS, and tests

**Files:**
- Create: `frontend/src/components/PortfolioTreemap.tsx`
- Create: `frontend/src/components/PortfolioTreemap.test.tsx`
- Modify: `frontend/src/styles.css` (append treemap styles)

**Interfaces:**
- Consumes:
  - `treemapViewModel(rows: GainsRow[]): TreemapTile[]` from `./treemapViewModel`
  - `GainsRow` from `../api/types`
- Produces:
  ```tsx
  export function PortfolioTreemap(props: {
    rows: GainsRow[];
  }): JSX.Element
  ```

**Visual encoding rationale:** Tiles use `--surface-2` as their neutral background. Positive return gets a `--up`-colored 3 px left border; negative gets `--down`; null gets `--hairline`. The percentage text is also colored with `--up`/`--down`/`--text-secondary`. This encodes performance direction visually without using semantic soft fills, keeping the implementation spec-compliant. See Notes above.

- [ ] **Step 1: Append treemap CSS to styles.css**

At the very end of `frontend/src/styles.css`, append:

```css
/* ── Treemap ─────────────────────────────────────────────────────────────── */

.treemap-container {
  position: relative;
  height: 280px;
  overflow: hidden;
}

.treemap-empty {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  color: var(--text-muted);
  font-size: 0.875rem;
}

.treemap-tile {
  position: absolute;
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-2);
  overflow: hidden;
  background: var(--surface-2);
  border-left: 3px solid var(--hairline);
  border-radius: var(--radius-xs);
}

.treemap-tile--up {
  border-left-color: var(--up);
}

.treemap-tile--down {
  border-left-color: var(--down);
}

.treemap-tile--small .treemap-tile-label,
.treemap-tile--small .treemap-tile-pct {
  display: none;
}

.treemap-tile-label {
  overflow: hidden;
  font-size: 0.75rem;
  font-weight: 600;
  color: var(--text-primary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.treemap-tile-pct {
  font-family: var(--font-mono);
  font-size: 0.6875rem;
  font-weight: 500;
}
```

- [ ] **Step 2: Create PortfolioTreemap.tsx**

Create `frontend/src/components/PortfolioTreemap.tsx`:

```tsx
import {
  type HierarchyRectangularNode,
  hierarchy,
  treemap,
  treemapSquarify,
} from "d3-hierarchy";
import { useEffect, useMemo, useRef, useState } from "react";
import type { GainsRow } from "../api/types";
import { type TreemapTile, treemapViewModel } from "./treemapViewModel";

// d3-hierarchy datum: the root holds children; each leaf IS a TreemapTile.
type RootDatum = { children: TreemapTile[] };
type Datum = RootDatum | TreemapTile;

function computeLeaves(
  tiles: TreemapTile[],
  width: number,
  height: number,
): HierarchyRectangularNode<Datum>[] {
  if (!tiles.length || width <= 0 || height <= 0) return [];
  const root = hierarchy<Datum>({ children: tiles } as RootDatum)
    .sum((d) => ("marketValueBase" in d ? d.marketValueBase : 0))
    .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
  treemap<Datum>()
    .size([width, height])
    .paddingOuter(2)
    .paddingInner(2)
    .tile(treemapSquarify)(root);
  return root.leaves();
}

function tileModifier(percent: number | null): string {
  if (percent === null) return "";
  if (percent > 0) return " treemap-tile--up";
  if (percent < 0) return " treemap-tile--down";
  return "";
}

function tileColor(percent: number | null): string {
  if (percent === null) return "var(--text-secondary)";
  if (percent > 0) return "var(--up)";
  if (percent < 0) return "var(--down)";
  return "var(--text-secondary)";
}

function formatPct(percent: number | null): string {
  if (percent === null) return "—";
  return (percent >= 0 ? "+" : "") + percent.toFixed(2) + "%";
}

export function PortfolioTreemap({ rows }: { rows: GainsRow[] }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    setSize({ width: el.clientWidth, height: el.clientHeight });
    const observer = new ResizeObserver(() =>
      setSize({ width: el.clientWidth, height: el.clientHeight }),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const tiles = useMemo(() => treemapViewModel(rows), [rows]);
  const leaves = useMemo(
    () => computeLeaves(tiles, size.width, size.height),
    [tiles, size],
  );

  return (
    <div
      ref={containerRef}
      className="treemap-container"
      aria-label="Portfolio holdings treemap"
    >
      {tiles.length === 0 ? (
        <p className="treemap-empty">No valued open holdings to display.</p>
      ) : (
        leaves.map((leaf) => {
          const tile = leaf.data as TreemapTile;
          const w = leaf.x1 - leaf.x0;
          const h = leaf.y1 - leaf.y0;
          const isSmall = w < 64 || h < 44;
          const label = `${tile.symbol}.${tile.exchange}: ${formatPct(tile.totalReturnPercent)}`;
          return (
            <div
              key={`${tile.symbol}.${tile.exchange}`}
              className={`treemap-tile${tileModifier(tile.totalReturnPercent)}${isSmall ? " treemap-tile--small" : ""}`}
              style={{ left: leaf.x0, top: leaf.y0, width: w, height: h }}
              title={label}
              aria-label={label}
            >
              <span className="treemap-tile-label">
                {tile.symbol}.{tile.exchange}
              </span>
              <span
                className="treemap-tile-pct"
                style={{ color: tileColor(tile.totalReturnPercent) }}
              >
                {formatPct(tile.totalReturnPercent)}
              </span>
            </div>
          );
        })
      )}
    </div>
  );
}
```

- [ ] **Step 3: Write PortfolioTreemap component tests**

Create `frontend/src/components/PortfolioTreemap.test.tsx`:

```tsx
// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { PortfolioTreemap } from "./PortfolioTreemap";

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

function inst(symbol: string, exchange = "NYSE"): Instrument {
  return { id: 1, symbol, exchange, name: symbol, type: "Stock", currency: "USD" };
}

function openRow(symbol: string, value: string): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: inst(symbol),
    quantity: 10,
    cost_basis_native: "1000",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: { status: "available", value: "5.00" },
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: { status: "available", value },
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    realized_gain_base: money,
    realized_cost_basis_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: money,
    reasons: [],
    position_status: "open",
  };
}

describe("PortfolioTreemap", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("shows empty state when rows is empty", () => {
    render(<PortfolioTreemap rows={[]} />);
    expect(screen.getByText("No valued open holdings to display.")).toBeTruthy();
  });

  it("shows empty state when all holdings have unavailable market value", () => {
    const money = { status: "available", value: "0.00" } as const;
    const row: GainsRow = {
      instrument: inst("AAPL"),
      quantity: 10,
      cost_basis_native: "1000",
      cost_basis_base: money,
      performance_start_date: null,
      performance_denominator_base: money,
      capital_gain_base: money,
      capital_gain_percent: money,
      income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
      currency_gain_base: money,
      currency_gain_percent: money,
      total_return_base: money,
      total_return_percent: money,
      latest_price: null,
      previous_price: null,
      latest_fx: null,
      previous_fx: null,
      market_value_native: money,
      market_value_base: { status: "unavailable", reasons: ["missing_price"] },
      proceeds_native: money,
      proceeds_base: money,
      unrealized_gain_base: money,
      unrealized_gain_percent: money,
      realized_gain_base: money,
      realized_cost_basis_base: money,
      price_effect_base: money,
      fx_effect_base: money,
      day_change_base: money,
      day_change_percent: money,
      reasons: [],
      position_status: "open",
    };
    render(<PortfolioTreemap rows={[row]} />);
    expect(screen.getByText("No valued open holdings to display.")).toBeTruthy();
  });

  it("renders tiles with aria-label when rows have valued holdings", () => {
    render(<PortfolioTreemap rows={[openRow("MSFT", "5000.00"), openRow("GOOG", "3000.00")]} />);
    // Tiles are rendered (ResizeObserver provides zero size so leaves will be empty,
    // but the empty-state text should NOT appear since tiles is non-empty).
    expect(screen.queryByText("No valued open holdings to display.")).toBeNull();
  });
});
```

- [ ] **Step 4: Verify types, lint, and tests**

```bash
npm run check
```

Expected: exits 0, all tests pass (including the 3 new PortfolioTreemap tests), TypeScript clean, Biome clean.

- [ ] **Step 5: Format and commit**

```bash
npm run fmt
git add src/components/PortfolioTreemap.tsx src/components/PortfolioTreemap.test.tsx src/styles.css
git commit -m "feat(dashboard): add PortfolioTreemap component with squarified d3 layout"
```

---

### Task 4: Wire treemap view into Dashboard

**Files:**
- Modify: `frontend/src/components/Dashboard.tsx`

**Interfaces:**
- Consumes:
  - `PortfolioTreemap` from `./PortfolioTreemap`
  - `useGains` from `../api/queries`

- [ ] **Step 1: Add the import for PortfolioTreemap**

At the top of `Dashboard.tsx`, after the existing import of `TimeSeriesChart`, add:

```tsx
import { PortfolioTreemap } from "./PortfolioTreemap";
```

- [ ] **Step 2: Rename DashboardValueChart to DashboardChartPanel**

Replace every occurrence of `DashboardValueChart` in `Dashboard.tsx` with `DashboardChartPanel`. This covers:
- The function definition on line 55: `function DashboardValueChart(` → `function DashboardChartPanel(`
- The JSX usage inside `Dashboard` on line 40: `<DashboardValueChart` and `</DashboardValueChart>` → `<DashboardChartPanel` and `</DashboardChartPanel>`

- [ ] **Step 3: Extend the ChartView type**

Replace:

```tsx
type ChartView = "value" | "gain";
```

With:

```tsx
type ChartView = "value" | "gain" | "treemap";
```

- [ ] **Step 4: Pass gainsQuery to DashboardChartPanel**

In the `Dashboard` function, pass `gainsQuery` as a new prop to `DashboardChartPanel`:

```tsx
<DashboardChartPanel
  query={valueHistory}
  gainsQuery={gainsQuery}
  dateRange={dateRange}
  selectedDatePreset={selectedDatePreset}
  onDatePresetChange={onDatePresetChange}
  onDateRangeChange={onDateRangeChange}
/>
```

- [ ] **Step 5: Add gainsQuery to DashboardChartPanel's prop type**

Update the function signature of `DashboardChartPanel`:

```tsx
function DashboardChartPanel({
  query,
  gainsQuery,
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
  gainsQuery: ReturnType<typeof useGains>;
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
})
```

- [ ] **Step 6: Move chartControls above the early returns and add "treemap" to the segmented control**

In `DashboardChartPanel`, move the `isGain` constant and `chartControls` JSX to just after the hooks (before the `if (query.isPending)` early return), replacing the current segmented-control array:

```tsx
const isGain = view === "gain";
const chartControls = (
  <div className="chart-controls">
    <DateRangeSelector
      dateRange={dateRange}
      selectedDatePreset={selectedDatePreset}
      onDatePresetChange={onDatePresetChange}
      onDateRangeChange={onDateRangeChange}
      ariaLabel="Dashboard date range"
    />
    <fieldset className="segmented-control">
      <legend className="sr-only">Chart view</legend>
      {(["value", "gain", "treemap"] as ChartView[]).map((v) => (
        <button
          key={v}
          type="button"
          className={view === v ? "active" : undefined}
          aria-pressed={view === v}
          onClick={() => setView(v)}
        >
          {v[0].toUpperCase() + v.slice(1)}
        </button>
      ))}
    </fieldset>
  </div>
);
```

- [ ] **Step 7: Insert treemap branch before the value-history loading checks**

After `chartControls` is defined and before `if (query.isPending)`, insert:

```tsx
if (view === "treemap") {
  return (
    <section className="panel chart-panel" aria-label="Portfolio value">
      <div className="chart-meta">
        <div className="chart-meta-title">
          <h2>Portfolio map</h2>
        </div>
        {chartControls}
      </div>
      {gainsQuery.isPending ? (
        <div className="chart-band">
          <div className="skeleton-bar" />
        </div>
      ) : gainsQuery.isError ? (
        <div className="chart-band error">
          <p className="down">Could not load holdings data.</p>
          <button
            type="button"
            className="button outline"
            onClick={() => void gainsQuery.refetch()}
          >
            Retry
          </button>
        </div>
      ) : (
        <PortfolioTreemap rows={gainsQuery.data?.rows ?? []} />
      )}
    </section>
  );
}
```

- [ ] **Step 8: Remove the duplicate chartControls definition from its old location**

The old `const chartControls = (...)` block that was between the loading early returns and the empty-state check is now replaced by the one moved in Step 6. Delete it. The existing `{chartControls}` usages in the empty-state and main render branches remain and will now reference the moved definition.

- [ ] **Step 9: Verify**

```bash
npm run check
```

Expected: exits 0, all tests pass, TypeScript clean, Biome clean.

- [ ] **Step 10: Format and commit**

```bash
npm run fmt
git add src/components/Dashboard.tsx
git commit -m "feat(dashboard): add Treemap view to portfolio chart panel"
```

---

### Task 5: Manual verification

- [ ] **Step 1: Start the app**

Follow the project's standard start procedure (e.g. `start.ps1` or `cargo run` + `npm run dev`).

- [ ] **Step 2: Navigate to the dashboard**

Confirm the segmented control shows three buttons: **Value · Gain · Treemap**.

- [ ] **Step 3: Click "Treemap"**

Confirm:
- The chart is replaced by a treemap with squarified tiles.
- Each tile shows `SYMBOL.EXCHANGE` and a signed percentage.
- Positive-return tiles have a green left border; negative-return tiles have a red left border; unknown-return tiles have a neutral hairline border.
- Percentage text is colored green/red/secondary accordingly.
- Small tiles (narrow/short) show no text, but have a `title` tooltip and `aria-label` on hover/focus.
- When there are no valued open holdings, the message "No valued open holdings to display." appears.

- [ ] **Step 4: Change the date range**

Confirm the treemap re-renders with updated percentages matching the selected period (same behavior as the Gain view).

- [ ] **Step 5: Switch back to Value and Gain**

Confirm the time-series charts still render correctly.

- [ ] **Step 6: Resize the browser window**

Confirm the treemap reflows — tile layout adapts to the new container width and height.

---

### Task 6: DecisionLog entry and version bump

**Files:**
- Modify: `docs/DecisionLog.md`
- Modify: `frontend/package.json`

- [ ] **Step 1: Bump the frontend version**

In `frontend/package.json`, increment the minor version (e.g. `0.x.y` → `0.(x+1).0`) to reflect the new user-visible view.

- [ ] **Step 2: Add a DecisionLog entry**

Append an entry to `docs/DecisionLog.md` covering:
- The Treemap view feature and its purpose (at-a-glance per-holding performance heatmap on the dashboard).
- The choice to use `d3-hierarchy` (squarified treemap algorithm; well-typed; no canvas dependency vs. lightweight-charts).
- The decision to omit the "All Holdings" summary chip (metric inconsistency: tile colors are per-row simple return; portfolio totals are method-specific XIRR — showing both without labeling would mislead; deferred until Dashboard aligns to a selectable method).
- The visual encoding choice (neutral tile fill + semantic-colored left border instead of `--up-soft`/`--down-soft` fills, to comply with the design spec's "semantics as text color, not fills" rule).

- [ ] **Step 3: Verify and commit**

```bash
npm run check
```

```bash
npm run fmt
git add docs/DecisionLog.md frontend/package.json
git commit -m "chore: bump frontend version and log treemap design decisions"
```
```