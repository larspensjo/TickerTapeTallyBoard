# Asset Detail Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a full-page, linkable per-asset detail view (`/asset/:id`) reached by clicking any instrument, composed only from existing endpoints, by introducing real client-side routing.

**Architecture:** Adopt `react-router-dom`. `App` becomes a thin global shell (app-bar + `<Routes>`); the board UI, board reducer state, board-only queries, totals band, dev status strip, and board actions move into a new `BoardView` route component so they no longer fire on other routes. A new `AssetView` route owns only the queries it needs and renders a read-only data shell derived by pure, testable helpers. The shared `InstrumentCell` becomes a router `<Link>`, making every table clickable from one change.

**Tech Stack:** React 19, TypeScript, `@tanstack/react-query`, `@tanstack/react-table`, `react-router-dom` (new), plain CSS with design tokens.

## Global Constraints

These apply to **every** task. Copied verbatim from the spec and repo instructions.

- **No backend changes.** The page is composed only from existing endpoints: `/api/instruments`, `/api/gains?include_closed=true`, `/api/holdings`, `/api/transactions`, `/api/prices/status`. `backend/Cargo.toml` version is unchanged.
- **Frontend version bump:** `frontend/package.json` `0.5.7 → 0.6.0`.
- **No unit-test runner exists.** Verification is `npm run check` (which runs `tsc --noEmit` + Biome) and `npm run fmt`, both from `frontend/`, plus manual human testing. Keep selection/state logic in **pure helpers** so it is testable if a runner is added later.
- **Freshness comes from the gains row, not prices/status.** Price/FX freshness is read from `GainsRow.latest_price.freshness` / `GainsRow.latest_fx.freshness`. `/api/prices/status` (`PriceSnapshotState`) has **no** `freshness` field and supplies only mapping state.
- **Closed-row view-model mapping** (the backend reuses the `GainsRow` shape for closed rows — relabel, never read literally):
  - Proceeds ← `market_value_base` (never "Market value" for a closed row).
  - Realized gain ← `unrealized_gain_base` / `unrealized_gain_percent`.
  - Breakdown total label ← **Realized total** (open rows: **Unrealized total**).
  - `latest_price` / `latest_fx` are `null` for closed rows → no live price, FX, or day-change.
- **Visual design:** follow `docs/VisualDesign.DarkTheme.md`. Every number uses `--font-mono`; panels are `--radius-lg` on `--surface-1` with a 1px `--hairline`; no drop shadows; up/down use `--up`/`--down`; stale/missing/unmapped use `--warning`. Use tokens, never inline hex.
- Run `npm run check` then `npm run fmt` from `frontend/` at the end of every task. Both must pass before staging.
- **Stage, do not commit.** Per `Agents.md`, plan-related changes are staged (`git add`) for review and **not** committed. Do not run `git commit` for any task unless the user explicitly asks; each task ends by staging its files.

## Decision Log

The governing decision is already recorded — **2026-06-19 "Client-Side Routing And Asset Detail Page"** in `docs/DecisionLog.md` (staged). This plan implements that decision; it introduces no new decisions, so no DecisionLog edits are required. Do **not** reference this plan or its task numbers from any runtime code or durable doc (plans are ephemeral).

## Open questions resolved during planning

Surfaced here rather than silently resolved (per repo planning rules):

1. **Where do the board action buttons (Refresh / Refresh prices / Add transaction) live now?** The spec moves them out of `App` and into `BoardView`, because keeping them in the global app-bar would require the board queries/mutations to live in `App` and fire on every route — defeating the route-scoping goal. They are rendered as a right-aligned `.board-toolbar` at the top of `BoardView`. This is a deliberate, spec-mandated deviation from the VisualDesign mockup (which shows them in the app-bar). Flag for visual review.
2. **How does Import's "View transactions" button still land on the Transactions board view, now that `boardView` is `BoardView`-local state on a different route?** `ImportView` calls `navigate("/", { state: { boardView: "transactions" } })`; `BoardView` reads `location.state.boardView` once to seed its reducer. The `onViewTransactions` prop is removed.
3. **Production deep-link reload of `/asset/:id` and `/import`.** No backend work is needed: per the 2026-06-13 "Static Frontend Serving" decision the backend already serves the SPA fallback when the static dir exists, and Vite serves the fallback in dev. Verified manually, not automated.
4. **Pure-helper home.** The spec lists only `AssetView.tsx` and `BoardView.tsx` as new component files but also requires "small testable helpers" for selection/derivation. Those helpers live in a new non-JSX module `frontend/src/components/assetViewModel.ts` so they carry no React dependency and stay unit-test-ready. This is an addition consistent with the spec, not a conflict.
5. **Board state is not preserved across navigate-away-and-back.** `BoardView` owns `boardView`, `boardFilter`, `includeClosedPositions`, and `formOpen` as local reducer state and is unmounted when navigating to `/asset/:id`. The `InstrumentCell` links carry none of that state, so returning to `/` recreates the board from defaults (Holdings, empty filter, closed form). The spec keeps these fields board-local and only requires Back/Forward to "behave correctly" (navigate), not to restore prior board state, so this is accepted for this iteration. Bookmarkable board state via URL search params (`?view=…&filter=…&closed=…`) is a possible follow-up, deliberately out of scope here to avoid expanding the surface. The plan's manual tests assert navigation works, **not** state preservation.

## File Structure

**New files**

- `frontend/src/components/BoardView.tsx` — the `/` route. Owns the board reducer (`boardView`, `boardFilter`, `includeClosedPositions`, `formOpen`), the board-only queries (`useHoldings`, `useGains(false)`, `useTransactions`, `usePriceStatus`, health, refresh/delete mutations), the totals band, the dev status strip, the board action toolbar, the add-transaction form, and the Holdings/Gains/Transactions segmented control.
- `frontend/src/components/AssetView.tsx` — the `/asset/:id` route. Owns only its five queries and composes small local sub-components (header, metric tiles, reserved chart band, gains breakdown, data & mapping, transactions). No derivation logic inline in JSX.
- `frontend/src/components/assetViewModel.ts` — pure, React-free selection and view-model helpers (`deriveAssetData`, `tilesView`, `breakdownView`, `headerStatus`, `sharesSold`, finders). Unit-test-ready.

**Modified files**

- `frontend/src/main.tsx` — wrap `<App/>` in `<BrowserRouter>`.
- `frontend/src/App.tsx` — reduce to global shell: app-bar (brand `Link` + `NavLink`s) + `<main className="workspace">` + `<Routes>`.
- `frontend/src/components/ImportView.tsx` — drop `onViewTransactions` prop; navigate via `useNavigate`.
- `frontend/src/components/TransactionsTable.tsx` — add `showToolbar`/`showActions` props (default `true`); make filter/delete props optional; accept an already-filtered list.
- `frontend/src/components/InstrumentCell.tsx` — add required `instrumentId` prop; render the primary label as a router `<Link>`.
- `frontend/src/components/HoldingsTable.tsx`, `GainsTable.tsx`, `TransactionsTable.tsx` — pass `instrumentId` to `InstrumentCell`.
- `frontend/src/styles.css` — asset-page styles (header, metric tiles, chart band, two-column panels, breakdown, data list) and the instrument-link treatment.
- `frontend/package.json` — add `react-router-dom`; bump version to `0.6.0`.

## Task Order & Dependencies

1. **Task 1** — Dependency + version bump (foundation).
2. **Task 2** — Routing refactor: `BoardView` extraction, router shell, `ImportView` navigation. App behaves exactly as before, via routes `/` and `/import`.
3. **Task 3** — `TransactionsTable` reusability props (needed by `AssetView`).
4. **Task 4** — `assetViewModel.ts` pure helpers (needed by `AssetView`).
5. **Task 5** — `AssetView` + `/asset/:id` route + asset CSS. Reachable by typing the URL.
6. **Task 6** — Entry points: `InstrumentCell` becomes a `<Link>`; the three tables pass `instrumentId`. Clicking works end-to-end.
7. **Task 7** — Final verification matrix (human testing) + format pass.

Each task ends green on `npm run check` + `npm run fmt`. Tasks 1–4 cause no user-visible change; Task 5 makes the page reachable by URL; Task 6 makes it reachable by click.

---

### Task 1: Add `react-router-dom` and bump frontend version

**Files:**
- Modify: `frontend/package.json`

**Interfaces:**
- Consumes: nothing.
- Produces: `react-router-dom` available to import; `frontend` version `0.6.0`.

- [ ] **Step 1: Install the dependency**

Run from `frontend/`:

```bash
npm install react-router-dom@^7
```

Expected: `package.json` `dependencies` gains `"react-router-dom": "^7.x.x"`; `package-lock.json` updates.

- [ ] **Step 2: Bump the frontend version**

Edit `frontend/package.json` line 3:

```json
  "version": "0.6.0",
```

(was `"0.5.7"`).

- [ ] **Step 3: Verify install and types resolve**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS (no type or lint errors). `react-router-dom` types resolve.

- [ ] **Step 4: Format**

Run from `frontend/`:

```bash
npm run fmt
```

- [ ] **Step 5: Stage for review**

Per the repo workflow, stage the task's files but do **not** commit (commits require explicit user approval):

```bash
git add frontend/package.json frontend/package-lock.json
```

---

### Task 2: Routing refactor — extract `BoardView`, make `App` a shell

This is one reviewable unit: after it, the app looks and behaves exactly as before, but `/` and `/import` are real routes and the board queries are scoped to `BoardView`. The asset route is added in Task 5.

**Files:**
- Create: `frontend/src/components/BoardView.tsx`
- Modify: `frontend/src/main.tsx`
- Modify: `frontend/src/App.tsx`
- Modify: `frontend/src/components/ImportView.tsx`
- Modify: `frontend/src/styles.css` (add `.board-toolbar`)

**Interfaces:**
- Consumes: existing queries from `./api/queries`; existing `valuationDisplay` helpers; `AddTransactionForm`, `HoldingsTable`, `GainsTable`, `TransactionsTable`.
- Produces:
  - `export function BoardView(): JSX.Element` — the `/` route element, no props.
  - `export function ImportView(): JSX.Element` — now prop-less (was `{ onViewTransactions }`).
  - `App` exports unchanged signature `export function App(): JSX.Element`, now route-only.

- [ ] **Step 1: Create `BoardView.tsx` with all moved board logic**

Create `frontend/src/components/BoardView.tsx`. This is the current `App` body minus the `appView` switch and minus `ImportView`, with the action buttons relocated into a `.board-toolbar` and the initial board view seeded from `location.state`:

```tsx
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import { type ReactNode, useReducer, useState } from "react";
import { useLocation } from "react-router-dom";
import packageJson from "../../package.json";
import {
  useDeleteTransaction,
  useGains,
  useHoldings,
  useInstruments,
  usePriceStatus,
  useRefreshPrices,
  useTransactions,
} from "../api/queries";
import type { MoneyValue, PercentValue, RefreshRunSummary } from "../api/types";
import { AddTransactionForm } from "./AddTransactionForm";
import { GainsTable } from "./GainsTable";
import { HoldingsTable } from "./HoldingsTable";
import { TransactionsTable } from "./TransactionsTable";
import {
  formatGroupedNumber,
  isAvailable,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

const frontendVersion = packageJson.version;

type BoardView = "holdings" | "gains" | "transactions";

interface UiState {
  boardView: BoardView;
  boardFilter: string;
  includeClosedPositions: boolean;
  formOpen: boolean;
}

type UiAction =
  | { type: "boardViewSelected"; boardView: BoardView }
  | { type: "boardFilterChanged"; filter: string }
  | { type: "closedPositionsToggled"; includeClosedPositions: boolean }
  | { type: "formToggled"; open: boolean };

interface HealthResponse {
  status: string;
  version: string;
  build: { package: string; profile: string };
}

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case "boardViewSelected":
      return { ...state, boardView: action.boardView };
    case "boardFilterChanged":
      return { ...state, boardFilter: action.filter };
    case "closedPositionsToggled":
      return {
        ...state,
        includeClosedPositions: action.includeClosedPositions,
      };
    case "formToggled":
      return { ...state, formOpen: action.open };
  }

  return state;
}

async function fetchHealth(): Promise<HealthResponse> {
  const response = await fetch("/api/health");

  if (!response.ok) {
    throw new Error(`Health request failed: ${response.status}`);
  }

  return (await response.json()) as HealthResponse;
}

function healthLabel(healthQuery: UseQueryResult<HealthResponse, Error>) {
  if (healthQuery.isPending) {
    return "Checking API";
  }

  if (healthQuery.isError) {
    return "API offline";
  }

  return `API ${healthQuery.data.status}`;
}

function summaryMoney(value: MoneyValue | undefined) {
  return <SummaryAvailabilityValue value={value} prefix="SEK " />;
}

function summaryPercent(value: PercentValue | undefined) {
  return <SummaryAvailabilityValue value={value} suffix="%" />;
}

function priceRefreshNeedsWarning(result: RefreshRunSummary): boolean {
  return (
    result.status === "partial" ||
    result.status === "failed" ||
    result.failed_items > 0 ||
    result.unmapped_instruments > 0
  );
}

function priceRefreshLabel(result: RefreshRunSummary): string {
  if (result.status === "running") {
    return "Refreshing prices";
  }

  if (result.status === "failed") {
    return "Price refresh failed";
  }

  if (result.status === "partial") {
    return "Price refresh partial";
  }

  return result.trigger === "launch"
    ? "Launch refresh complete"
    : "Prices refreshed";
}

function priceRefreshTitle(result: RefreshRunSummary): string {
  const parts = [
    `run ${result.run_id}`,
    `trigger ${result.trigger}`,
    `mode ${result.mode}`,
    `status ${result.status}`,
    `${formatGroupedNumber(result.prices_written)} prices`,
    `${formatGroupedNumber(result.fx_rates_written)} FX`,
    `${formatGroupedNumber(result.unmapped_instruments)} unmapped`,
    `${formatGroupedNumber(result.failed_items)} failed`,
    `started ${result.started_at}`,
  ];

  if (result.finished_at) {
    parts.push(`finished ${result.finished_at}`);
  }

  if (result.message) {
    parts.push(result.message);
  }

  return parts.join(", ");
}

export function BoardView() {
  const location = useLocation();
  const initialBoardView =
    (location.state as { boardView?: BoardView } | null)?.boardView ??
    "holdings";

  const [uiState, dispatch] = useReducer(uiReducer, {
    boardView: initialBoardView,
    boardFilter: "",
    includeClosedPositions: false,
    formOpen: false,
  });

  const healthQuery = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
  });
  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const holdingsQuery = useHoldings();
  const gainsQuery = useGains(uiState.includeClosedPositions);
  const priceStatusQuery = usePriceStatus();
  const refreshPrices = useRefreshPrices();
  const deleteTransaction = useDeleteTransaction();
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const instruments = instrumentsQuery.data ?? [];
  const holdingsCount = holdingsQuery.data?.length ?? 0;
  const transactionsCount = transactionsQuery.data?.length ?? 0;
  const gainsSummary = gainsQuery.data?.summary;
  const totalValue = gainsSummary?.market_value_base;
  const refreshSummary = priceStatusQuery.data?.latest_run;
  const pricesRefreshing =
    refreshPrices.isPending || priceStatusQuery.data?.refreshing === true;

  const boardIsFetching =
    healthQuery.isFetching ||
    holdingsQuery.isFetching ||
    gainsQuery.isFetching ||
    instrumentsQuery.isFetching ||
    transactionsQuery.isFetching;

  async function handleDelete(id: number) {
    setDeleteError(null);
    try {
      await deleteTransaction.mutateAsync(id);
    } catch (error) {
      setDeleteError(
        error instanceof Error
          ? error.message
          : "Could not delete transaction.",
      );
    }
  }

  return (
    <>
      <div className="board-toolbar">
        <button
          className="button secondary"
          type="button"
          onClick={() => {
            void Promise.all([
              healthQuery.refetch(),
              holdingsQuery.refetch(),
              gainsQuery.refetch(),
              instrumentsQuery.refetch(),
              transactionsQuery.refetch(),
            ]);
          }}
          disabled={boardIsFetching}
        >
          <RefreshCw
            aria-hidden="true"
            className={boardIsFetching ? "spin" : undefined}
            size={16}
          />
          <span>Refresh</span>
        </button>
        <button
          className="button secondary"
          type="button"
          onClick={() => refreshPrices.mutate({ mode: "latest" })}
          disabled={refreshPrices.isPending}
        >
          <RefreshCw
            aria-hidden="true"
            className={refreshPrices.isPending ? "spin" : undefined}
            size={16}
          />
          <span>Refresh prices</span>
        </button>
        <button
          className="button primary"
          type="button"
          onClick={() =>
            dispatch({ type: "formToggled", open: !uiState.formOpen })
          }
        >
          <Plus aria-hidden="true" size={16} />
          <span>Add transaction</span>
        </button>
      </div>

      <section className="totals-band" aria-label="Portfolio summary">
        <div>
          <p className="eyebrow">Portfolio</p>
          <strong className="total-value">
            {isAvailable(totalValue)
              ? `SEK ${formatGroupedNumber(totalValue.value)}`
              : `${formatGroupedNumber(holdingsCount)} holdings`}
          </strong>
        </div>
        <div className="summary-metrics">
          <span>
            Holdings{" "}
            <strong className="number">
              {formatGroupedNumber(holdingsCount)}
            </strong>
          </span>
          {gainsSummary ? (
            <>
              <span>
                Unrealized {summaryMoney(gainsSummary.unrealized_gain_base)}{" "}
                {summaryPercent(gainsSummary.unrealized_gain_percent)}
              </span>
              <span>
                Day {summaryMoney(gainsSummary.day_change_base)}{" "}
                {summaryPercent(gainsSummary.day_change_percent)}
              </span>
              {gainsSummary.excluded_rows ? (
                <span className="status-chip warning">
                  {formatGroupedNumber(gainsSummary.excluded_rows)} missing
                </span>
              ) : null}
            </>
          ) : null}
          <span>
            Transactions{" "}
            <strong className="number">
              {formatGroupedNumber(transactionsCount)}
            </strong>
          </span>
        </div>
      </section>

      <section className="status-strip" aria-label="Development status">
        <span
          className={
            healthQuery.isError ? "status-chip warning" : "status-chip"
          }
        >
          {healthLabel(healthQuery)}
        </span>
        <span className="status-chip">Manual entry</span>
        <span className="status-chip">SEK base</span>
        <span className="status-chip">UI {frontendVersion}</span>
        {priceStatusQuery.isPending ? (
          <span className="status-chip">Checking prices</span>
        ) : pricesRefreshing ? (
          <span className="status-chip warning">
            <RefreshCw aria-hidden="true" className="spin" size={12} />
            Refreshing prices
          </span>
        ) : refreshSummary ? (
          <span
            className={
              priceRefreshNeedsWarning(refreshSummary)
                ? "status-chip warning"
                : "status-chip"
            }
            title={priceRefreshTitle(refreshSummary)}
          >
            {priceRefreshLabel(refreshSummary)}
          </span>
        ) : (
          <span className="status-chip">No price refresh yet</span>
        )}
        {refreshPrices.isError ? (
          <span
            className="status-chip warning"
            title={refreshPrices.error.message}
          >
            Price refresh failed
          </span>
        ) : null}
        <span className="status-chip">
          API{" "}
          {healthQuery.data?.version ??
            (healthQuery.isPending ? "checking" : "unavailable")}
        </span>
      </section>

      {uiState.formOpen ? (
        <section className="panel form-panel" aria-label="Add transaction">
          <div className="panel-header">
            <div>
              <p className="eyebrow">Manual entry</p>
              <h2>Add transaction</h2>
            </div>
          </div>
          <AddTransactionForm
            instruments={instruments}
            onClose={() => dispatch({ type: "formToggled", open: false })}
          />
        </section>
      ) : null}

      <section className="board-grid single">
        <article className="panel ledger-panel">
          <div className="panel-header">
            <div>
              <p className="eyebrow">Workspace</p>
              <h1>Portfolio Board</h1>
            </div>
            <fieldset className="segmented-control">
              <legend className="sr-only">Board view</legend>
              <button
                className={
                  uiState.boardView === "holdings" ? "active" : undefined
                }
                type="button"
                aria-pressed={uiState.boardView === "holdings"}
                onClick={() =>
                  dispatch({ type: "boardViewSelected", boardView: "holdings" })
                }
              >
                Holdings
              </button>
              <button
                className={
                  uiState.boardView === "gains" ? "active" : undefined
                }
                type="button"
                aria-pressed={uiState.boardView === "gains"}
                onClick={() =>
                  dispatch({ type: "boardViewSelected", boardView: "gains" })
                }
              >
                Gains
              </button>
              <button
                className={
                  uiState.boardView === "transactions" ? "active" : undefined
                }
                type="button"
                aria-pressed={uiState.boardView === "transactions"}
                onClick={() =>
                  dispatch({
                    type: "boardViewSelected",
                    boardView: "transactions",
                  })
                }
              >
                Transactions
              </button>
            </fieldset>
          </div>

          {uiState.boardView === "holdings" ? (
            <BoardSection
              isPending={holdingsQuery.isPending}
              isError={holdingsQuery.isError}
              isEmpty={(holdingsQuery.data?.length ?? 0) === 0}
              onRetry={() => void holdingsQuery.refetch()}
              emptyMessage="No holdings yet. Add a Buy to get started."
            >
              <HoldingsTable
                holdings={holdingsQuery.data ?? []}
                filter={uiState.boardFilter}
                onFilterChange={(filter) =>
                  dispatch({ type: "boardFilterChanged", filter })
                }
              />
            </BoardSection>
          ) : uiState.boardView === "gains" ? (
            <BoardSection
              isPending={gainsQuery.isPending}
              isError={gainsQuery.isError}
              isEmpty={(gainsQuery.data?.rows.length ?? 0) === 0}
              onRetry={() => void gainsQuery.refetch()}
              emptyMessage="No valued holdings yet. Add a Buy and refresh prices."
            >
              <GainsTable
                rows={gainsQuery.data?.rows ?? []}
                totals={gainsQuery.data?.totals}
                filter={uiState.boardFilter}
                onFilterChange={(filter) =>
                  dispatch({ type: "boardFilterChanged", filter })
                }
                includeClosedPositions={uiState.includeClosedPositions}
                onIncludeClosedPositionsChange={(includeClosedPositions) =>
                  dispatch({
                    type: "closedPositionsToggled",
                    includeClosedPositions,
                  })
                }
              />
            </BoardSection>
          ) : (
            <BoardSection
              isPending={transactionsQuery.isPending}
              isError={transactionsQuery.isError}
              isEmpty={(transactionsQuery.data?.length ?? 0) === 0}
              onRetry={() => void transactionsQuery.refetch()}
              emptyMessage="No transactions yet. Add one with the button above."
            >
              <TransactionsTable
                transactions={transactionsQuery.data ?? []}
                instruments={instruments}
                filter={uiState.boardFilter}
                onFilterChange={(filter) =>
                  dispatch({ type: "boardFilterChanged", filter })
                }
                onDelete={(id) => void handleDelete(id)}
                deletingId={
                  deleteTransaction.isPending
                    ? (deleteTransaction.variables ?? null)
                    : null
                }
                errorMessage={deleteError}
              />
            </BoardSection>
          )}
        </article>
      </section>
    </>
  );
}

function BoardSection({
  isPending,
  isError,
  isEmpty,
  onRetry,
  emptyMessage,
  children,
}: {
  isPending: boolean;
  isError: boolean;
  isEmpty: boolean;
  onRetry: () => void;
  emptyMessage: string;
  children: ReactNode;
}) {
  if (isPending) {
    return (
      <div className="board-state">
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="board-state error">
        <p className="down">Could not load data.</p>
        <button type="button" className="button outline" onClick={onRetry}>
          Retry
        </button>
      </div>
    );
  }

  if (isEmpty) {
    return <div className="board-state muted">{emptyMessage}</div>;
  }

  return <>{children}</>;
}
```

- [ ] **Step 2: Replace `App.tsx` with the global shell**

Replace the **entire** contents of `frontend/src/App.tsx` with:

```tsx
import { Link, NavLink, Route, Routes } from "react-router-dom";
import { BoardView } from "./components/BoardView";
import { ImportView } from "./components/ImportView";

function navClass({ isActive }: { isActive: boolean }) {
  return isActive ? "active" : undefined;
}

export function App() {
  return (
    <div className="app-shell">
      <header className="app-bar">
        <Link className="brand" to="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </Link>

        <nav className="app-nav" aria-label="Primary">
          <NavLink to="/" end className={navClass}>
            Board
          </NavLink>
          <NavLink to="/import" className={navClass}>
            Import
          </NavLink>
        </nav>
      </header>

      <main className="workspace">
        <Routes>
          <Route path="/" element={<BoardView />} />
          <Route path="/import" element={<ImportView />} />
        </Routes>
      </main>
    </div>
  );
}
```

(The `/asset/:id` route is added in Task 5. `NavLink` sets `aria-current="page"` automatically when active, so no manual `aria-current` is needed.)

- [ ] **Step 3: Wrap `App` in `BrowserRouter` in `main.tsx`**

In `frontend/src/main.tsx`, add the import and wrap `<App/>`:

```tsx
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-500.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/jetbrains-mono/latin-500.css";
import "@fontsource/jetbrains-mono/latin-600.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { App } from "./App";
import "./styles.css";

const rootElement = document.getElementById("root");
const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: false,
      retry: 1,
      staleTime: 30_000,
    },
  },
});

if (!rootElement) {
  throw new Error("Root element was not found");
}

createRoot(rootElement).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <App />
      </BrowserRouter>
    </QueryClientProvider>
  </StrictMode>,
);
```

- [ ] **Step 4: Make `ImportView` navigate instead of taking a prop**

In `frontend/src/components/ImportView.tsx`:

Add `useNavigate` to the React Router import line (add a new import; there is currently no router import):

```tsx
import { useNavigate } from "react-router-dom";
```

Remove the `ImportViewProps` interface (around lines 211–213) and change the component signature from:

```tsx
export function ImportView({ onViewTransactions }: ImportViewProps) {
```

to:

```tsx
export function ImportView() {
  const navigate = useNavigate();
```

Then change the "View transactions" button's handler (around line 603) from:

```tsx
                  onClick={onViewTransactions}
```

to:

```tsx
                  onClick={() =>
                    navigate("/", { state: { boardView: "transactions" } })
                  }
```

- [ ] **Step 5: Add the `.board-toolbar` style**

In `frontend/src/styles.css`, immediately after the `.workspace { ... }` rule (ends at line 225), add:

```css
.board-toolbar {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: var(--space-2);
  padding: var(--space-4) 0 0;
}
```

- [ ] **Step 6: Verify types, lint, and behavior**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS. (Confirms `App`, `BoardView`, `ImportView`, `main` all type-check and the old `appView` references are gone.)

- [ ] **Step 7: Manual smoke test — HUMAN TESTING RECOMMENDED**

Run the app (`npm run dev` in `frontend/`, backend running). Verify:
- `/` shows the board exactly as before: totals band, status strip, action buttons (now top-right of the board area), segmented Holdings/Gains/Transactions, all data present.
- Clicking the **Import** nav link goes to `/import` (URL changes) and shows the import view; the board action buttons are **not** shown there.
- Importing (or, on the committed screen) clicking **View transactions** navigates to `/` with the **Transactions** board view selected.
- Browser Back/Forward move between `/` and `/import` correctly.
- The **Board** and **Import** nav items show the active indicator on the matching route.

- [ ] **Step 8: Format and stage for review**

Run `npm run fmt` if needed, then stage (do not commit; commits require explicit user approval):

```bash
git add frontend/src/App.tsx frontend/src/main.tsx frontend/src/components/BoardView.tsx frontend/src/components/ImportView.tsx frontend/src/styles.css
```

(Run `npm run fmt` first if `npm run check` reported formatting.)

---

### Task 3: Make `TransactionsTable` reusable (toolbar/actions optional, pre-filtered input)

`AssetView` (Task 5) reuses this table to show one instrument's transactions with no global filter and no Delete buttons. The board keeps its current behavior via defaults.

**Files:**
- Modify: `frontend/src/components/TransactionsTable.tsx`

**Interfaces:**
- Consumes: `Instrument`, `Transaction` from `../api/types`.
- Produces: `TransactionsTable` props become:
  ```ts
  {
    transactions: Transaction[];
    instruments: Instrument[];
    filter?: string;
    onFilterChange?: (filter: string) => void;
    onDelete?: (id: number) => void;
    deletingId?: number | null;
    errorMessage?: string | null;
    showToolbar?: boolean; // default true
    showActions?: boolean; // default true
  }
  ```

- [ ] **Step 1: Widen the props and default the new flags**

Replace the destructured parameter block and its type (lines 28–44) with:

```tsx
export function TransactionsTable({
  transactions,
  instruments,
  filter = "",
  onFilterChange,
  onDelete,
  deletingId = null,
  errorMessage = null,
  showToolbar = true,
  showActions = true,
}: {
  transactions: Transaction[];
  instruments: Instrument[];
  filter?: string;
  onFilterChange?: (filter: string) => void;
  onDelete?: (id: number) => void;
  deletingId?: number | null;
  errorMessage?: string | null;
  showToolbar?: boolean;
  showActions?: boolean;
}) {
```

- [ ] **Step 2: Build the actions column conditionally**

Replace the `columns` memo (lines 86–145) so the `actions` column is only present when `showActions` is true and `onDelete` exists. Use a **conditional spread inside a single array literal** (not `Array.push`): mutating an array TypeScript inferred from accessor columns would reject the display column's differing value type, whereas a single literal infers the element type as the union of all column kinds, which is assignable to the table's `columns` prop:

```tsx
  const columns = useMemo(
    () => [
      columnHelper.accessor((row) => row.transaction.trade_date, {
        id: "trade_date",
        header: "Date",
        cell: (info) => info.getValue(),
      }),
      columnHelper.accessor((row) => row.transaction.type, {
        id: "type",
        header: "Type",
        cell: (info) => <span className="type-chip">{info.getValue()}</span>,
      }),
      columnHelper.accessor((row) => row.name, {
        id: "instrument",
        header: "Instrument",
        cell: (info) => (
          <InstrumentCell
            name={info.row.original.name}
            symbol={info.row.original.symbol}
            exchange={info.row.original.exchange}
          />
        ),
      }),
      columnHelper.accessor((row) => row.transaction.quantity, {
        id: "quantity",
        header: "Qty",
        cell: (info) => formatGroupedNumber(info.getValue()),
      }),
      columnHelper.accessor((row) => row.transaction.price ?? "", {
        id: "price",
        header: "Price",
        cell: (info) => {
          const { price, currency } = info.row.original.transaction;
          return price ? (
            <FormattedNumber value={price} prefix={currency ?? ""} />
          ) : (
            "-"
          );
        },
      }),
      ...(showActions && onDelete
        ? [
            columnHelper.display({
              id: "actions",
              header: "",
              cell: (info) => {
                const id = info.row.original.transaction.id;
                return (
                  <button
                    type="button"
                    className="button outline danger"
                    onClick={() => onDelete(id)}
                    disabled={deletingId === id}
                  >
                    Delete
                  </button>
                );
              },
            }),
          ]
        : []),
    ],
    [showActions, deletingId, onDelete],
  );
```

(The `instrument` column gains `instrumentId` in Task 6, when all three tables are updated together.)

- [ ] **Step 3: Render the toolbar only when requested**

Replace the toolbar block (lines 160–172, the `<div className="table-toolbar">…</div>`) so it renders only when `showToolbar` is true:

```tsx
      {showToolbar ? (
        <div className="table-toolbar">
          <input
            className="filter-input"
            type="search"
            placeholder="Filter instrument"
            value={filter}
            onChange={(event) => onFilterChange?.(event.target.value)}
          />
          {errorMessage ? (
            <p className="form-error table-error">{errorMessage}</p>
          ) : null}
        </div>
      ) : null}
```

The table's `state.globalFilter` already reads `filter`, which now defaults to `""` — a pre-filtered list passed by `AssetView` is therefore shown unfiltered.

- [ ] **Step 4: Verify**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS. The board still passes all props, so its behavior is unchanged.

- [ ] **Step 5: Manual smoke test**

In the running app, open the **Transactions** board view. Confirm the filter input still filters and Delete buttons still work — defaults preserve old behavior.

- [ ] **Step 6: Format and stage for review**

```bash
npm run fmt
git add frontend/src/components/TransactionsTable.tsx
```

Do not commit; commits require explicit user approval.

---

### Task 4: Pure asset view-model helpers (`assetViewModel.ts`)

All selection and open/closed/no-row/not-found derivation lives here — React-free and unit-test-ready. The component in Task 5 only renders these.

**Files:**
- Create: `frontend/src/components/assetViewModel.ts`

**Interfaces:**
- Consumes: types from `../api/types`; `reasonLabel` from `./valuationDisplay`.
- Produces (used by `AssetView` in Task 5):
  - `parseInstrumentId(raw: string | undefined): number | null`
  - `deriveAssetData(args): AssetData` where `AssetData` is `{ kind: "not-found" } | { kind: "no-row"; instrument; transactions; priceStatus } | { kind: "position"; instrument; gain; holding; transactions; priceStatus }`
  - `tilesView(gain, holding, transactions): Tiles` (`OpenTiles | ClosedTiles`)
  - `breakdownView(gain): BreakdownView`
  - `headerStatus(gain, priceStatus): HeaderStatus`
  - `sharesSold(transactions): number`

- [ ] **Step 1: Create the helper module**

Create `frontend/src/components/assetViewModel.ts`:

```ts
import type {
  GainsRow,
  Holding,
  Instrument,
  MoneyValue,
  PercentValue,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import { reasonLabel } from "./valuationDisplay";

export type AssetData =
  | { kind: "not-found" }
  | {
      kind: "no-row";
      instrument: Instrument;
      transactions: Transaction[];
      priceStatus: PriceStatusInstrument | null;
    }
  | {
      kind: "position";
      instrument: Instrument;
      gain: GainsRow;
      holding: Holding | null;
      transactions: Transaction[];
      priceStatus: PriceStatusInstrument | null;
    };

export interface OpenTiles {
  status: "open";
  marketValue: MoneyValue;
  unrealizedGain: MoneyValue;
  unrealizedPercent: PercentValue;
  dayChange: MoneyValue;
  dayChangePercent: PercentValue;
  quantity: number;
  averageCost: MoneyValue;
  costBasis: MoneyValue;
}

export interface ClosedTiles {
  status: "closed";
  realizedGain: MoneyValue;
  realizedPercent: PercentValue;
  proceeds: MoneyValue;
  costBasis: MoneyValue;
  sharesSold: number;
}

export type Tiles = OpenTiles | ClosedTiles;

export interface BreakdownView {
  priceEffect: MoneyValue;
  fxEffect: MoneyValue;
  totalLabel: string;
  total: MoneyValue;
}

export interface HeaderStatus {
  label: string;
  tone: "neutral" | "warning";
}

export function parseInstrumentId(raw: string | undefined): number | null {
  if (raw === undefined) {
    return null;
  }

  const parsed = Number(raw);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

export function findInstrument(
  instruments: Instrument[],
  id: number,
): Instrument | null {
  return instruments.find((instrument) => instrument.id === id) ?? null;
}

export function findGainsRow(rows: GainsRow[], id: number): GainsRow | null {
  return rows.find((row) => row.instrument.id === id) ?? null;
}

export function findHolding(holdings: Holding[], id: number): Holding | null {
  return holdings.find((holding) => holding.instrument.id === id) ?? null;
}

export function findPriceStatus(
  instruments: PriceStatusInstrument[],
  id: number,
): PriceStatusInstrument | null {
  return instruments.find((entry) => entry.instrument_id === id) ?? null;
}

export function instrumentTransactions(
  transactions: Transaction[],
  id: number,
): Transaction[] {
  return transactions.filter(
    (transaction) => transaction.instrument_id === id,
  );
}

export function sharesSold(transactions: Transaction[]): number {
  return transactions
    .filter((transaction) => transaction.type === "Sell")
    .reduce((sum, transaction) => sum + Math.abs(transaction.quantity), 0);
}

function averageCostBase(holding: Holding | null): MoneyValue {
  if (holding && holding.base.status === "available") {
    return { status: "available", value: holding.base.average_cost_base };
  }

  return { status: "unavailable", reasons: ["base_cost_basis_unavailable"] };
}

export function deriveAssetData(args: {
  id: number | null;
  instruments: Instrument[];
  gainsRows: GainsRow[];
  holdings: Holding[];
  transactions: Transaction[];
  priceStatus: PriceStatusInstrument[];
}): AssetData {
  if (args.id === null) {
    return { kind: "not-found" };
  }

  const instrument = findInstrument(args.instruments, args.id);
  if (!instrument) {
    return { kind: "not-found" };
  }

  const transactions = instrumentTransactions(args.transactions, args.id);
  const priceStatus = findPriceStatus(args.priceStatus, args.id);
  const gain = findGainsRow(args.gainsRows, args.id);

  if (!gain) {
    return { kind: "no-row", instrument, transactions, priceStatus };
  }

  const holding = findHolding(args.holdings, args.id);
  return { kind: "position", instrument, gain, holding, transactions, priceStatus };
}

export function tilesView(
  gain: GainsRow,
  holding: Holding | null,
  transactions: Transaction[],
): Tiles {
  if (gain.position_status === "closed") {
    return {
      status: "closed",
      realizedGain: gain.unrealized_gain_base,
      realizedPercent: gain.unrealized_gain_percent,
      proceeds: gain.market_value_base,
      costBasis: gain.cost_basis_base,
      sharesSold: sharesSold(transactions),
    };
  }

  return {
    status: "open",
    marketValue: gain.market_value_base,
    unrealizedGain: gain.unrealized_gain_base,
    unrealizedPercent: gain.unrealized_gain_percent,
    dayChange: gain.day_change_base,
    dayChangePercent: gain.day_change_percent,
    quantity: gain.quantity,
    averageCost: averageCostBase(holding),
    costBasis: gain.cost_basis_base,
  };
}

export function breakdownView(gain: GainsRow): BreakdownView {
  return {
    priceEffect: gain.price_effect_base,
    fxEffect: gain.fx_effect_base,
    totalLabel:
      gain.position_status === "closed"
        ? "Realized total"
        : "Unrealized total",
    total: gain.unrealized_gain_base,
  };
}

export function headerStatus(
  gain: GainsRow | null,
  priceStatus: PriceStatusInstrument | null,
): HeaderStatus {
  // Closed positions have no live price/FX by design, so the closed-position
  // label must outrank mapping/missing-price warnings (spec: closed rows show
  // "Closed position"). Check it before any price-status warning.
  if (gain?.position_status === "closed") {
    return { label: "Closed position", tone: "neutral" };
  }

  if (priceStatus && !priceStatus.mapping_enabled) {
    return { label: "Mapping disabled", tone: "warning" };
  }

  if (priceStatus && priceStatus.latest_price.status === "unmapped") {
    return { label: "Unmapped", tone: "warning" };
  }

  if (priceStatus && priceStatus.latest_price.status === "missing") {
    return { label: "Missing price", tone: "warning" };
  }

  if (gain) {
    if (gain.reasons.length > 0) {
      return { label: reasonLabel(gain.reasons[0]), tone: "warning" };
    }

    const priceFreshness = gain.latest_price?.freshness;
    if (priceFreshness && priceFreshness !== "fresh") {
      return { label: "Stale price", tone: "warning" };
    }

    const fxFreshness = gain.latest_fx?.freshness;
    if (fxFreshness && fxFreshness !== "fresh") {
      return { label: "Stale FX", tone: "warning" };
    }

    return { label: "Open position", tone: "neutral" };
  }

  return { label: "No position", tone: "neutral" };
}
```

- [ ] **Step 2: Verify**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS. (The module is imported nowhere yet, so this confirms it compiles and lints in isolation; Biome will flag it as unused only if re-exported — it is a standalone module, which is fine.)

- [ ] **Step 3: Format and stage for review**

```bash
npm run fmt
git add frontend/src/components/assetViewModel.ts
```

Do not commit; commits require explicit user approval.

---

### Task 5: `AssetView` component, route, and styles

Render the asset page from the helpers. After this task the page is reachable by typing `/asset/:id` in the URL bar (clicking is wired in Task 6).

**Files:**
- Create: `frontend/src/components/AssetView.tsx`
- Modify: `frontend/src/App.tsx` (add the `/asset/:id` route)
- Modify: `frontend/src/styles.css` (asset-page styles)

**Interfaces:**
- Consumes: `assetViewModel` helpers; `useInstruments`, `useGains`, `useHoldings`, `useTransactions`, `usePriceStatus`; `TransactionsTable`; `valuationDisplay` helpers (`SummaryAvailabilityValue`, `formatGroupedNumber`, `freshnessLabel`, `freshnessTone`).
- Produces: `export function AssetView(): JSX.Element` — the `/asset/:id` route element, no props.

- [ ] **Step 1: Create `AssetView.tsx`**

Create `frontend/src/components/AssetView.tsx`:

```tsx
import type { ReactNode } from "react";
import { Link, useParams } from "react-router-dom";
import {
  useGains,
  useHoldings,
  useInstruments,
  usePriceStatus,
  useTransactions,
} from "../api/queries";
import type {
  GainsRow,
  Instrument,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import {
  type BreakdownView,
  type Tiles,
  breakdownView,
  deriveAssetData,
  headerStatus,
  parseInstrumentId,
  tilesView,
} from "./assetViewModel";
import { TransactionsTable } from "./TransactionsTable";
import {
  formatGroupedNumber,
  freshnessLabel,
  freshnessTone,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

export function AssetView() {
  const { id: idParam } = useParams();
  const id = parseInstrumentId(idParam);

  const instrumentsQuery = useInstruments();
  const gainsQuery = useGains(true);
  const holdingsQuery = useHoldings();
  const transactionsQuery = useTransactions();
  const priceStatusQuery = usePriceStatus();

  const isPending =
    instrumentsQuery.isPending ||
    gainsQuery.isPending ||
    holdingsQuery.isPending ||
    transactionsQuery.isPending ||
    priceStatusQuery.isPending;

  const isError =
    instrumentsQuery.isError ||
    gainsQuery.isError ||
    holdingsQuery.isError ||
    transactionsQuery.isError ||
    priceStatusQuery.isError;

  if (isPending) {
    return (
      <div className="board-state">
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
        <div className="skeleton-bar" />
      </div>
    );
  }

  if (isError) {
    return (
      <div className="board-state error">
        <p className="down">Could not load asset data.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => {
            void instrumentsQuery.refetch();
            void gainsQuery.refetch();
            void holdingsQuery.refetch();
            void transactionsQuery.refetch();
            void priceStatusQuery.refetch();
          }}
        >
          Retry
        </button>
      </div>
    );
  }

  const data = deriveAssetData({
    id,
    instruments: instrumentsQuery.data ?? [],
    gainsRows: gainsQuery.data?.rows ?? [],
    holdings: holdingsQuery.data ?? [],
    transactions: transactionsQuery.data ?? [],
    priceStatus: priceStatusQuery.data?.instruments ?? [],
  });

  if (data.kind === "not-found") {
    return (
      <div className="board-state muted asset-not-found">
        <p>Asset not found.</p>
        <Link className="button outline" to="/">
          ← Back to board
        </Link>
      </div>
    );
  }

  const instruments = instrumentsQuery.data ?? [];
  const gain = data.kind === "position" ? data.gain : null;

  return (
    <article className="asset-page">
      <AssetHeader
        instrument={data.instrument}
        gain={gain}
        priceStatus={data.priceStatus}
      />

      {data.kind === "position" ? (
        <>
          <AssetMetricTiles
            tiles={tilesView(data.gain, data.holding, data.transactions)}
          />
          <ReservedChartBand />
          <div className="asset-two-col">
            <AssetGainsBreakdown breakdown={breakdownView(data.gain)} />
            <AssetDataMapping gain={data.gain} priceStatus={data.priceStatus} />
          </div>
        </>
      ) : (
        <>
          <ReservedChartBand />
          <AssetDataMapping gain={null} priceStatus={data.priceStatus} />
        </>
      )}

      <AssetTransactions
        transactions={data.transactions}
        instruments={instruments}
      />
    </article>
  );
}

function AssetHeader({
  instrument,
  gain,
  priceStatus,
}: {
  instrument: Instrument;
  gain: GainsRow | null;
  priceStatus: PriceStatusInstrument | null;
}) {
  const status = headerStatus(gain, priceStatus);
  const meta = [
    instrument.symbol,
    instrument.exchange,
    instrument.currency,
    instrument.type,
  ]
    .filter((part) => part && part.length > 0)
    .join(" · ");

  return (
    <header className="asset-header">
      <Link className="asset-back" to="/">
        ← Back to board
      </Link>
      <div className="asset-title-row">
        <h1>{instrument.name || instrument.symbol}</h1>
        <span
          className={
            status.tone === "warning" ? "status-chip warning" : "status-chip"
          }
        >
          {status.label}
        </span>
      </div>
      <p className="asset-meta">{meta}</p>
    </header>
  );
}

function AssetMetricTiles({ tiles }: { tiles: Tiles }) {
  if (tiles.status === "open") {
    return (
      <section className="metric-tiles" aria-label="Position metrics">
        <MetricTile label="Market value">
          <SummaryAvailabilityValue
            value={tiles.marketValue}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
        <MetricTile label="Unrealized">
          <SummaryAvailabilityValue
            value={tiles.unrealizedGain}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={tiles.unrealizedPercent}
            suffix="%"
            tone="signed"
          />
        </MetricTile>
        <MetricTile label="Day change">
          <SummaryAvailabilityValue
            value={tiles.dayChange}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={tiles.dayChangePercent}
            suffix="%"
            tone="signed"
          />
        </MetricTile>
        <MetricTile label="Quantity">
          <span className="number">{formatGroupedNumber(tiles.quantity)}</span>
        </MetricTile>
        <MetricTile label="Avg cost">
          <SummaryAvailabilityValue
            value={tiles.averageCost}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
        <MetricTile label="Cost basis">
          <SummaryAvailabilityValue
            value={tiles.costBasis}
            prefix="SEK "
            tone="plain"
          />
        </MetricTile>
      </section>
    );
  }

  return (
    <section className="metric-tiles" aria-label="Position metrics">
      <MetricTile label="Realized gain">
        <SummaryAvailabilityValue
          value={tiles.realizedGain}
          prefix="SEK "
          tone="signed"
        />{" "}
        <SummaryAvailabilityValue
          value={tiles.realizedPercent}
          suffix="%"
          tone="signed"
        />
      </MetricTile>
      <MetricTile label="Proceeds">
        <SummaryAvailabilityValue
          value={tiles.proceeds}
          prefix="SEK "
          tone="plain"
        />
      </MetricTile>
      <MetricTile label="Cost basis">
        <SummaryAvailabilityValue
          value={tiles.costBasis}
          prefix="SEK "
          tone="plain"
        />
      </MetricTile>
      <MetricTile label="Shares sold">
        <span className="number">{formatGroupedNumber(tiles.sharesSold)}</span>
      </MetricTile>
    </section>
  );
}

function MetricTile({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="metric-tile">
      <span className="metric-tile-label">{label}</span>
      <span className="metric-tile-value">{children}</span>
    </div>
  );
}

function ReservedChartBand() {
  return (
    <section className="chart-band" aria-label="Price chart placeholder">
      <span className="chart-band-label">Price chart — coming soon</span>
    </section>
  );
}

function AssetGainsBreakdown({ breakdown }: { breakdown: BreakdownView }) {
  return (
    <section className="panel asset-panel" aria-label="Gains breakdown">
      <h2>Gains breakdown</h2>
      <dl className="breakdown-list">
        <BreakdownRow
          label="Capital (price effect)"
          value={breakdown.priceEffect}
        />
        <BreakdownRow
          label="Currency (FX effect)"
          value={breakdown.fxEffect}
        />
        <BreakdownRow label={breakdown.totalLabel} value={breakdown.total} ruled />
      </dl>
    </section>
  );
}

function BreakdownRow({
  label,
  value,
  ruled = false,
}: {
  label: string;
  value: BreakdownView["total"];
  ruled?: boolean;
}) {
  return (
    <div className={ruled ? "breakdown-row ruled" : "breakdown-row"}>
      <dt>{label}</dt>
      <dd>
        <SummaryAvailabilityValue value={value} prefix="SEK " tone="signed" />
      </dd>
    </div>
  );
}

function AssetDataMapping({
  gain,
  priceStatus,
}: {
  gain: GainsRow | null;
  priceStatus: PriceStatusInstrument | null;
}) {
  return (
    <section className="panel asset-panel" aria-label="Data and mapping">
      <h2>Data &amp; mapping</h2>
      <dl className="data-list">
        {gain ? (
          <>
            <DataRow label="Latest price">{latestPriceContent(gain)}</DataRow>
            <DataRow label="Latest FX">{latestFxContent(gain)}</DataRow>
          </>
        ) : null}
        <DataRow label="Provider">{providerContent(priceStatus)}</DataRow>
      </dl>
    </section>
  );
}

function DataRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="data-row">
      <dt>{label}</dt>
      <dd>{children}</dd>
    </div>
  );
}

function latestPriceContent(gain: GainsRow) {
  const price = gain.latest_price;
  if (!price) {
    return <span className="status-chip warning">No live price</span>;
  }

  return (
    <span className="data-value">
      <span className="number">
        {price.currency} {formatGroupedNumber(price.close)}
      </span>{" "}
      <span
        className={
          freshnessTone(price.freshness) === "warning"
            ? "status-chip warning compact"
            : "status-chip compact"
        }
      >
        {freshnessLabel(price.freshness)}
      </span>
    </span>
  );
}

function latestFxContent(gain: GainsRow) {
  const fx = gain.latest_fx;
  if (!fx) {
    return <span className="asset-subtle">—</span>;
  }

  return (
    <span className="data-value">
      <span className="number">{formatGroupedNumber(fx.rate)}</span>{" "}
      <span className="asset-subtle">
        {fx.quote}→{fx.base} · {fx.date}
      </span>{" "}
      <span
        className={
          freshnessTone(fx.freshness) === "warning"
            ? "status-chip warning compact"
            : "status-chip compact"
        }
      >
        {freshnessLabel(fx.freshness)}
      </span>
    </span>
  );
}

function providerContent(priceStatus: PriceStatusInstrument | null) {
  if (!priceStatus) {
    return <span className="asset-subtle">—</span>;
  }

  if (!priceStatus.mapping_enabled) {
    return <span className="status-chip warning">Mapping disabled</span>;
  }

  if (
    priceStatus.provider_symbol === null ||
    priceStatus.latest_price.status === "unmapped"
  ) {
    return <span className="status-chip warning">Unmapped</span>;
  }

  const provider = priceStatus.latest_price.provider ?? "—";

  return (
    <span className="data-value">
      <span className="number">{priceStatus.provider_symbol}</span>{" "}
      <span className="asset-subtle">{provider}</span>
      {priceStatus.latest_price.status === "missing" ? (
        <span className="status-chip warning compact">Missing price</span>
      ) : null}
      {priceStatus.latest_fx.status === "missing" ? (
        <span className="status-chip warning compact">Missing FX</span>
      ) : null}
      {priceStatus.latest_fx.status === "unmapped" ? (
        <span className="status-chip warning compact">FX unmapped</span>
      ) : null}
    </span>
  );
}

function AssetTransactions({
  transactions,
  instruments,
}: {
  transactions: Transaction[];
  instruments: Instrument[];
}) {
  return (
    <section className="panel asset-panel" aria-label="Transactions">
      <h2>Transactions for this asset</h2>
      {transactions.length === 0 ? (
        <p className="board-state muted">
          No transactions recorded for this asset.
        </p>
      ) : (
        <TransactionsTable
          transactions={transactions}
          instruments={instruments}
          showToolbar={false}
          showActions={false}
        />
      )}
    </section>
  );
}
```

- [ ] **Step 2: Register the `/asset/:id` route**

In `frontend/src/App.tsx`, add the import before the `BoardView` import (Biome keeps imports alphabetical by path):

```tsx
import { AssetView } from "./components/AssetView";
```

and add the route inside `<Routes>` after the `/import` route:

```tsx
          <Route path="/asset/:id" element={<AssetView />} />
```

- [ ] **Step 3: Add asset-page styles**

In `frontend/src/styles.css`, append the following block at the end of the file:

```css
.asset-page {
  display: flex;
  flex-direction: column;
  gap: var(--space-5);
  padding: var(--space-5) 0 0;
}

.asset-header {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
}

.asset-back {
  width: fit-content;
  color: var(--accent-link);
  font-size: 0.8125rem;
  font-weight: 500;
  text-decoration: none;
}

.asset-back:hover {
  text-decoration: underline;
}

.asset-title-row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: var(--space-3);
}

.asset-meta {
  margin: 0;
  color: var(--text-secondary);
  font-size: 0.8125rem;
}

.metric-tiles {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: var(--space-3);
}

.metric-tile {
  display: flex;
  flex-direction: column;
  gap: var(--space-1);
  padding: var(--space-4);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-lg);
  background: var(--surface-1);
}

.metric-tile-label {
  color: var(--text-muted);
  font-size: 0.6875rem;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}

.metric-tile-value {
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 1.125rem;
  font-weight: 600;
}

/* Tile values render through `.number`, whose global font-size would otherwise
   shrink them; inherit the larger tile sizing instead. */
.metric-tile-value .number {
  font-size: inherit;
  font-weight: inherit;
}

.chart-band {
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 200px;
  border: 1px solid var(--hairline);
  border-radius: var(--radius-lg);
  background: var(--surface-1);
  background-image:
    linear-gradient(var(--hairline-soft) 1px, transparent 1px),
    linear-gradient(90deg, var(--hairline-soft) 1px, transparent 1px);
  background-size: 32px 32px;
}

.chart-band-label {
  color: var(--text-muted);
  font-size: 0.8125rem;
  font-weight: 600;
}

.asset-two-col {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: var(--space-4);
}

.asset-panel {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  padding: var(--space-4);
}

.breakdown-list,
.data-list {
  display: flex;
  flex-direction: column;
  gap: var(--space-2);
  margin: 0;
}

.breakdown-row,
.data-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: var(--space-4);
}

.breakdown-row dt,
.data-row dt {
  color: var(--text-secondary);
  font-size: 0.8125rem;
}

.breakdown-row dd,
.data-row dd {
  margin: 0;
  text-align: right;
}

.breakdown-row.ruled {
  margin-top: var(--space-1);
  padding-top: var(--space-3);
  border-top: 1px solid var(--hairline);
}

.breakdown-row.ruled dt {
  color: var(--text-primary);
  font-weight: 600;
}

.asset-subtle {
  color: var(--text-muted);
  font-size: 0.75rem;
}

.data-value {
  display: inline-flex;
  flex-wrap: wrap;
  align-items: baseline;
  gap: var(--space-2);
  justify-content: flex-end;
}

.asset-not-found {
  display: flex;
  flex-direction: column;
  gap: var(--space-3);
  align-items: center;
}
```

(The `.status-chip.compact` rule already exists in `styles.css` at ~line 343, so it is intentionally **not** re-declared here.)

- [ ] **Step 4: Verify types and lint**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS.

- [ ] **Step 5: Manual rendering test — HUMAN TESTING RECOMMENDED**

With the app running, type these URLs directly in the address bar (clicking is wired in Task 6):
- `/asset/<id of an open position>` → header (name, `SYMBOL · EXCHANGE · CURRENCY · TYPE`, "Open position" chip), 6 metric tiles, reserved chart band, two-column **Gains breakdown** (Capital + Currency, ruled **Unrealized total**) and **Data & mapping**, then the transactions table **with no filter input and no Delete buttons**.
- `/asset/<id of a closed position>` (enable "Include closed positions" on Gains first to find one) → tiles show **Realized gain**, **Proceeds**, **Cost basis**, **Shares sold**; breakdown total is labelled **Realized total**; no day-change/live price; header chip "Closed position". Check the **labels**, not just numbers.
- `/asset/<id of an unmapped/missing-price instrument>` → `--warning` header chip and `--warning` "Unavailable"/"No live price" values; quantity and native cost basis still render.
- `/asset/<id with no gains row>` (an empty/rolled-back instrument) → identity + transactions render, plus a reduced **Data & mapping** panel (Provider/mapping row from prices/status); **no** metric tiles or gains breakdown.
- `/asset/999999` → "Asset not found" muted state with a back link.
- Reload each URL (cold cache) → renders directly. In the browser network panel confirm a `GET /api/gains?include_closed=true` fires and **no** bare `GET /api/gains` (the board-only `useGains(false)`) fires unless you visit the board.

- [ ] **Step 6: Format and stage for review**

```bash
npm run fmt
git add frontend/src/App.tsx frontend/src/components/AssetView.tsx frontend/src/styles.css
```

Do not commit; commits require explicit user approval.

---

### Task 6: Entry points — make `InstrumentCell` a link, wire all tables

After this task, clicking any instrument in Holdings, Gains, or Transactions opens its asset page.

**Files:**
- Modify: `frontend/src/components/InstrumentCell.tsx`
- Modify: `frontend/src/components/HoldingsTable.tsx`
- Modify: `frontend/src/components/GainsTable.tsx`
- Modify: `frontend/src/components/TransactionsTable.tsx`
- Modify: `frontend/src/styles.css` (link treatment)

**Interfaces:**
- Consumes: `react-router-dom` `Link`.
- Produces: `InstrumentCell` props become `{ instrumentId: number; name: string; symbol: string; exchange: string }` (`instrumentId` required).

- [ ] **Step 1: Make `InstrumentCell` render a `<Link>`**

Replace the entire contents of `frontend/src/components/InstrumentCell.tsx`:

```tsx
import { Link } from "react-router-dom";

interface InstrumentCellProps {
  instrumentId: number;
  name: string;
  symbol: string;
  exchange: string;
}

export function InstrumentCell({
  instrumentId,
  name,
  symbol,
  exchange,
}: InstrumentCellProps) {
  const trimmedName = name.trim();
  const trimmedSymbol = symbol.trim();
  const primary = trimmedName || trimmedSymbol;
  const showSymbol = trimmedSymbol.length > 0 && trimmedSymbol !== primary;

  return (
    <div className="instrument-cell">
      <Link className="instrument-link" to={`/asset/${instrumentId}`}>
        {primary}
      </Link>
      {showSymbol ? <span>{trimmedSymbol}</span> : null}
      {exchange ? <em>{exchange}</em> : null}
    </div>
  );
}
```

- [ ] **Step 2: Pass `instrumentId` from `HoldingsTable`**

In `frontend/src/components/HoldingsTable.tsx`, in the `instrument` column cell (around lines 162–167), pass the id:

```tsx
      cell: (info) => {
        const { id, symbol, name, exchange } =
          info.row.original.holding.instrument;
        return (
          <InstrumentCell
            instrumentId={id}
            name={name}
            symbol={symbol}
            exchange={exchange}
          />
        );
      },
```

- [ ] **Step 3: Pass `instrumentId` from `GainsTable`**

In `frontend/src/components/GainsTable.tsx`, in the `instrument` column cell (around lines 127–131):

```tsx
    cell: (info) => {
      const { id, symbol, name, exchange } = info.row.original.gain.instrument;
      return (
        <InstrumentCell
          instrumentId={id}
          name={name}
          symbol={symbol}
          exchange={exchange}
        />
      );
    },
```

- [ ] **Step 4: Pass `instrumentId` from `TransactionsTable`**

In `frontend/src/components/TransactionsTable.tsx`, in the `instrument` column cell (defined in Task 3 Step 2), pass the transaction's instrument id:

```tsx
        cell: (info) => (
          <InstrumentCell
            instrumentId={info.row.original.transaction.instrument_id}
            name={info.row.original.name}
            symbol={info.row.original.symbol}
            exchange={info.row.original.exchange}
          />
        ),
```

- [ ] **Step 5: Style the instrument link**

In `frontend/src/styles.css`, replace the `.instrument-cell strong { ... }` rule (lines 583–591) so the link inherits the same look and gains a hover accent:

```css
.instrument-cell strong,
.instrument-cell .instrument-link {
  min-width: 0;
  overflow: hidden;
  color: var(--text-primary);
  font-size: 0.875rem;
  font-weight: 600;
  text-decoration: none;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.instrument-cell .instrument-link:hover {
  color: var(--accent-link);
  text-decoration: underline;
}
```

- [ ] **Step 6: Verify**

Run from `frontend/`:

```bash
npm run check
```

Expected: PASS. (All three tables now satisfy the required `instrumentId` prop.)

- [ ] **Step 7: Manual end-to-end test — HUMAN TESTING RECOMMENDED**

With the app running:
- In **Holdings**, **Gains**, and **Transactions**, click an instrument name → lands on its `/asset/:id` page.
- Row hover still highlights with `--surface-2`; the instrument name is now a link that underlines/accents on hover.
- `← Back to board` and browser Back/Forward return to `/` and the board renders. Note: board view state (selected segment, filter text, include-closed, open form) is **not** preserved across the round-trip — it resets to defaults (Holdings, empty filter). This is accepted for this iteration; see open question #5.

- [ ] **Step 8: Format and stage for review**

```bash
npm run fmt
git add frontend/src/components/InstrumentCell.tsx frontend/src/components/HoldingsTable.tsx frontend/src/components/GainsTable.tsx frontend/src/components/TransactionsTable.tsx frontend/src/styles.css
```

Do not commit; commits require explicit user approval.

---

### Task 7: Final verification matrix and format pass

**Files:** none changed (verification only; fix-ups staged if needed).

- [ ] **Step 1: Full check + format**

Run from `frontend/`:

```bash
npm run check
npm run fmt
```

Expected: both PASS clean.

- [ ] **Step 2: Run the full manual matrix — HUMAN TESTING REQUIRED**

From the spec's Verification section, confirm each:
- Click an instrument in Holdings, Gains, and Transactions → lands on its asset page.
- `← Back to board` and browser Back/Forward behave correctly.
- Deep-link and reload `/asset/:id` renders from a cold cache; the network panel shows a `GET /api/gains?include_closed=true` and **no** bare `GET /api/gains` unless the board is visited.
- An **open**, a **closed** (Gains "include closed"), and an **unmapped/missing-price** asset each render their expected variant. For the closed asset, check the **labels** (Proceeds, Realized total — not "Market value"/"Unrealized total").
- The asset transactions table shows **no global filter input and no Delete buttons**.
- An existing instrument id with **no gains row** (seed one locally if needed — e.g. an empty instrument left by an import rollback) shows identity + transactions with no metric tiles or gains breakdown.
- Invalid `/asset/999999` shows the not-found state.
- Production-style deep-link reload: build (`npm run build`) and run the backend serving `frontend/dist`; reload `/asset/:id` and `/import` directly → both render via the existing SPA fallback (no backend change).
- Visual pass against `docs/VisualDesign.DarkTheme.md` (panels, mono numbers, up/down colors, warning treatment, no drop shadows). Review the relocated board action buttons placement (open question #1).

- [ ] **Step 3: Stage any fix-ups**

If the matrix surfaced issues, fix them, re-run `npm run check` + `npm run fmt`, and stage them with `git add`. Do not commit; commits require explicit user approval. Otherwise nothing to stage.

---

## Spec coverage map

| Spec requirement | Task |
|---|---|
| Add `react-router-dom`, bump `0.5.7 → 0.6.0` | 1 |
| `BrowserRouter` in `main.tsx`; `App` reduced to shell + `<Routes>` | 2 |
| Routes `/`→BoardView, `/import`→ImportView | 2 |
| Route `/asset/:id`→AssetView | 5 |
| Nav links become `NavLink`; app-bar global | 2 |
| Board state/queries/totals/status/actions move to `BoardView`; `appView` removed | 2 |
| Import "View transactions" still selects Transactions view | 2 |
| Back/Forward, deep link, reload work | 2, 5, 7 |
| `InstrumentCell` → `<Link>` with `instrumentId`; three tables pass it | 6 |
| Data sourced from existing endpoints, selected by id; `useGains(true)` | 4, 5 |
| Freshness from gains row, mapping from prices/status | 4, 5 |
| Header / metric tiles / reserved chart band / two-column / transactions layout | 5 |
| Open / closed / no-row / unmapped / not-found / loading / error states | 4, 5 |
| Closed-row view-model mapping (Proceeds, Realized total, null price/fx) | 4, 5 |
| `TransactionsTable` `showToolbar`/`showActions`, optional props, pre-filtered list | 3, 5 |
| Reuse `valuationDisplay` helpers | 4, 5 |
| Selection/derivation in pure testable helpers | 4 |
| `npm run check` + `npm run fmt`; manual matrix | every task, 7 |

## Follow-ups (separate specs/plans — do not implement here)

1. Price-history endpoint `GET /api/instruments/{id}/prices` + Lightweight Charts component filling the reserved band.
2. Per-asset income and total return once dividend/income accounting exists.
3. Edit/manipulation actions on the asset page.
