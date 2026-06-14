import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";
import { Plus, RefreshCw } from "lucide-react";
import { type ReactNode, useReducer, useState } from "react";
import packageJson from "../package.json";
import {
  useDeleteTransaction,
  useHoldings,
  useInstruments,
  useTransactions,
} from "./api/queries";
import { AddTransactionForm } from "./components/AddTransactionForm";
import { HoldingsTable } from "./components/HoldingsTable";
import { TransactionsTable } from "./components/TransactionsTable";

const frontendVersion = packageJson.version;

type BoardView = "holdings" | "transactions";

interface UiState {
  boardView: BoardView;
  formOpen: boolean;
}

type UiAction =
  | { type: "boardViewSelected"; boardView: BoardView }
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

export function App() {
  const [uiState, dispatch] = useReducer(uiReducer, {
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
  const deleteTransaction = useDeleteTransaction();
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const instruments = instrumentsQuery.data ?? [];
  const holdingsCount = holdingsQuery.data?.length ?? 0;
  const transactionsCount = transactionsQuery.data?.length ?? 0;

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
          <a className="active" href="/">
            Board
          </a>
          <a href="/">Import</a>
          <a href="/">Settings</a>
        </nav>

        <div className="app-actions">
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
      </header>

      <main className="workspace">
        <section className="totals-band" aria-label="Portfolio summary">
          <div>
            <p className="eyebrow">Portfolio</p>
            <strong className="total-value">{holdingsCount} holdings</strong>
          </div>
          <div className="summary-metrics">
            <span>
              Holdings <strong className="number">{holdingsCount}</strong>
            </span>
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
                <HoldingsTable holdings={holdingsQuery.data ?? []} />
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
