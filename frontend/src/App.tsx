import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import { type ReactNode, useReducer, useState } from "react";
import packageJson from "../package.json";
import {
  useDeleteTransaction,
  useGains,
  useHoldings,
  useInstruments,
  useRefreshPrices,
  useTransactions,
} from "./api/queries";
import type {
  MoneyValue,
  PercentValue,
  RefreshPricesResult,
} from "./api/types";
import { AddTransactionForm } from "./components/AddTransactionForm";
import { GainsTable } from "./components/GainsTable";
import { HoldingsTable } from "./components/HoldingsTable";
import { ImportView } from "./components/ImportView";
import { TransactionsTable } from "./components/TransactionsTable";
import {
  isAvailable,
  SummaryAvailabilityValue,
} from "./components/valuationDisplay";

const frontendVersion = packageJson.version;

type BoardView = "holdings" | "gains" | "transactions";
type AppView = "board" | "import";

interface UiState {
  appView: AppView;
  boardView: BoardView;
  formOpen: boolean;
}

type UiAction =
  | { type: "appViewSelected"; appView: AppView }
  | { type: "boardViewSelected"; boardView: BoardView }
  | { type: "formToggled"; open: boolean };

interface HealthResponse {
  status: string;
  version: string;
  build: { package: string; profile: string };
}

function uiReducer(state: UiState, action: UiAction): UiState {
  switch (action.type) {
    case "appViewSelected":
      return { ...state, appView: action.appView };
    case "boardViewSelected":
      return { ...state, boardView: action.boardView };
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

function refreshResultNeedsWarning(result: RefreshPricesResult): boolean {
  return (
    result.status === "partial" ||
    result.status === "failed" ||
    result.failed_items > 0 ||
    result.unmapped_instruments > 0
  );
}

function refreshResultLabel(result: RefreshPricesResult): string {
  if (result.status === "failed") {
    return "Price refresh failed";
  }

  if (refreshResultNeedsWarning(result)) {
    return "Price refresh partial";
  }

  return "Prices refreshed";
}

function refreshResultTitle(result: RefreshPricesResult): string {
  const parts = [
    `status ${result.status}`,
    `${result.prices_written} prices`,
    `${result.fx_rates_written} FX`,
    `${result.unmapped_instruments} unmapped`,
    `${result.failed_items} failed`,
  ];

  if (result.message) {
    parts.push(result.message);
  }

  return parts.join(", ");
}

export function App() {
  const [uiState, dispatch] = useReducer(uiReducer, {
    appView: "board",
    boardView: "holdings",
    formOpen: false,
  });

  const healthQuery = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
  });
  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const holdingsQuery = useHoldings();
  const gainsQuery = useGains();
  const refreshPrices = useRefreshPrices();
  const deleteTransaction = useDeleteTransaction();
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const instruments = instrumentsQuery.data ?? [];
  const holdingsCount = holdingsQuery.data?.length ?? 0;
  const transactionsCount = transactionsQuery.data?.length ?? 0;
  const gainsSummary = gainsQuery.data?.summary;
  const totalValue = gainsSummary?.market_value_base;

  function showTransactions() {
    dispatch({ type: "appViewSelected", appView: "board" });
    dispatch({ type: "boardViewSelected", boardView: "transactions" });
  }

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
    <div className="app-shell">
      <header className="app-bar">
        <a className="brand" href="/" aria-label="TickerTapeTallyBoard home">
          <span className="brand-mark" aria-hidden="true" />
          <span>TickerTapeTallyBoard</span>
        </a>

        <nav className="app-nav" aria-label="Primary">
          <a
            className={uiState.appView === "board" ? "active" : undefined}
            aria-current={uiState.appView === "board" ? "page" : undefined}
            href="/"
            onClick={(event) => {
              event.preventDefault();
              dispatch({ type: "appViewSelected", appView: "board" });
            }}
          >
            Board
          </a>
          <a
            className={uiState.appView === "import" ? "active" : undefined}
            aria-current={uiState.appView === "import" ? "page" : undefined}
            href="/"
            onClick={(event) => {
              event.preventDefault();
              dispatch({ type: "appViewSelected", appView: "import" });
            }}
          >
            Import
          </a>
        </nav>

        <div className="app-actions">
          {uiState.appView === "board" ? (
            <>
              <button
                className="button secondary"
                type="button"
                onClick={() => {
                  void Promise.all([
                    healthQuery.refetch(),
                    holdingsQuery.refetch(),
                    instrumentsQuery.refetch(),
                    transactionsQuery.refetch(),
                  ]);
                }}
                disabled={
                  healthQuery.isFetching ||
                  holdingsQuery.isFetching ||
                  instrumentsQuery.isFetching ||
                  transactionsQuery.isFetching
                }
              >
                <RefreshCw
                  aria-hidden="true"
                  className={
                    healthQuery.isFetching ||
                    holdingsQuery.isFetching ||
                    instrumentsQuery.isFetching ||
                    transactionsQuery.isFetching
                      ? "spin"
                      : undefined
                  }
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
            </>
          ) : null}
        </div>
      </header>

      <main className="workspace">
        <section className="totals-band" aria-label="Portfolio summary">
          <div>
            <p className="eyebrow">Portfolio</p>
            <strong className="total-value">
              {isAvailable(totalValue)
                ? `SEK ${totalValue.value}`
                : `${holdingsCount} holdings`}
            </strong>
          </div>
          <div className="summary-metrics">
            <span>
              Holdings <strong className="number">{holdingsCount}</strong>
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
                    {gainsSummary.excluded_rows} missing
                  </span>
                ) : null}
              </>
            ) : null}
            <span>
              Transactions{" "}
              <strong className="number">{transactionsCount}</strong>
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
          {refreshPrices.isPending ? (
            <span className="status-chip warning">
              <RefreshCw aria-hidden="true" className="spin" size={12} />
              Refreshing prices
            </span>
          ) : null}
          {refreshPrices.isError ? (
            <span
              className="status-chip warning"
              title={refreshPrices.error.message}
            >
              Price refresh failed
            </span>
          ) : refreshPrices.data ? (
            <span
              className={
                refreshResultNeedsWarning(refreshPrices.data)
                  ? "status-chip warning"
                  : "status-chip"
              }
              title={refreshResultTitle(refreshPrices.data)}
            >
              {refreshResultLabel(refreshPrices.data)}
            </span>
          ) : null}
          <span className="status-chip">
            API{" "}
            {healthQuery.data?.version ??
              (healthQuery.isPending ? "checking" : "unavailable")}
          </span>
        </section>

        <div hidden={uiState.appView !== "board"}>
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
                      dispatch({
                        type: "boardViewSelected",
                        boardView: "holdings",
                      })
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
                      dispatch({
                        type: "boardViewSelected",
                        boardView: "gains",
                      })
                    }
                  >
                    Gains
                  </button>
                  <button
                    className={
                      uiState.boardView === "transactions"
                        ? "active"
                        : undefined
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
                  <HoldingsTable holdings={holdingsQuery.data ?? []} />
                </BoardSection>
              ) : uiState.boardView === "gains" ? (
                <BoardSection
                  isPending={gainsQuery.isPending}
                  isError={gainsQuery.isError}
                  isEmpty={(gainsQuery.data?.rows.length ?? 0) === 0}
                  onRetry={() => void gainsQuery.refetch()}
                  emptyMessage="No valued holdings yet. Add a Buy and refresh prices."
                >
                  <GainsTable rows={gainsQuery.data?.rows ?? []} />
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
        </div>

        <div hidden={uiState.appView !== "import"}>
          <ImportView onViewTransactions={showTransactions} />
        </div>
      </main>
    </div>
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
