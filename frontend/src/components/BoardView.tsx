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
import type {
  DateRange,
  MoneyValue,
  PercentValue,
  RefreshRunSummary,
} from "../api/types";
import { AddTransactionForm } from "./AddTransactionForm";
import { type DatePreset, GainsTable } from "./GainsTable";
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
  datePreset: DatePreset;
  dateRange: DateRange;
}

type UiAction =
  | { type: "boardViewSelected"; boardView: BoardView }
  | { type: "boardFilterChanged"; filter: string }
  | { type: "closedPositionsToggled"; includeClosedPositions: boolean }
  | { type: "formToggled"; open: boolean }
  | { type: "datePresetChanged"; datePreset: DatePreset }
  | { type: "dateRangeChanged"; dateRange: DateRange };

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
    case "datePresetChanged":
      return { ...state, datePreset: action.datePreset };
    case "dateRangeChanged":
      return { ...state, dateRange: action.dateRange };
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
    datePreset: "all",
    dateRange: { startDate: null, endDate: null },
  });

  const healthQuery = useQuery({
    queryKey: ["health"],
    queryFn: fetchHealth,
  });
  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const holdingsQuery = useHoldings();
  const gainsQuery = useGains({
    includeClosedPositions: uiState.includeClosedPositions,
    startDate: uiState.dateRange.startDate,
    endDate: uiState.dateRange.endDate ?? undefined,
  });
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
                className={uiState.boardView === "gains" ? "active" : undefined}
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
                dateRange={uiState.dateRange}
                selectedDatePreset={uiState.datePreset}
                onDatePresetChange={(datePreset) =>
                  dispatch({ type: "datePresetChanged", datePreset })
                }
                onDateRangeChange={(dateRange) =>
                  dispatch({ type: "dateRangeChanged", dateRange })
                }
                displayPercentKind={gainsQuery.data?.display_percent_kind}
                reportPeriod={gainsQuery.data?.report_period}
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
