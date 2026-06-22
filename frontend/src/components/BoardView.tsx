import { Plus, RefreshCw } from "lucide-react";
import { useReducer, useState } from "react";
import { useLocation } from "react-router-dom";
import {
  type ReturnMethod,
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
  GainsRow,
  MoneyValue,
  PercentValue,
} from "../api/types";
import { AddTransactionForm } from "./AddTransactionForm";
import { AsyncBoundary } from "./AsyncBoundary";
import { type DatePreset, GainsTable, loadReturnMethod } from "./GainsTable";
import { HoldingsTable } from "./HoldingsTable";
import { TransactionsTable } from "./TransactionsTable";
import {
  formatGroupedNumber,
  freshnessLabel,
  freshnessTone,
  isAvailable,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

type BoardView = "holdings" | "gains" | "transactions";

interface UiState {
  boardView: BoardView;
  boardFilter: string;
  includeClosedPositions: boolean;
  formOpen: boolean;
  datePreset: DatePreset;
  dateRange: DateRange;
  returnMethod: ReturnMethod;
}

type UiAction =
  | { type: "boardViewSelected"; boardView: BoardView }
  | { type: "boardFilterChanged"; filter: string }
  | { type: "closedPositionsToggled"; includeClosedPositions: boolean }
  | { type: "formToggled"; open: boolean }
  | { type: "datePresetChanged"; datePreset: DatePreset }
  | { type: "dateRangeChanged"; dateRange: DateRange }
  | { type: "returnMethodChanged"; returnMethod: ReturnMethod };

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
    case "returnMethodChanged":
      return { ...state, returnMethod: action.returnMethod };
  }

  return state;
}

function summaryMoney(value: MoneyValue | undefined) {
  return <SummaryAvailabilityValue value={value} prefix="SEK " />;
}

function summaryPercent(value: PercentValue | undefined) {
  return <SummaryAvailabilityValue value={value} suffix="%" />;
}

function freshnessRank(freshness: string): number {
  const dayMatch = freshness.match(/_(\d+)_days$/);
  const days = dayMatch ? Number(dayMatch[1]) : 0;

  if (freshness.startsWith("warning_stale_")) {
    return 200 + days;
  }

  if (freshness.startsWith("minor_stale_")) {
    return 100 + days;
  }

  return freshness === "fresh" ? 0 : 50;
}

function portfolioPriceFreshness(rows: GainsRow[] | undefined): string | null {
  const freshnessValues =
    rows?.flatMap((row) =>
      row.latest_price?.freshness ? [row.latest_price.freshness] : [],
    ) ?? [];

  return freshnessValues.reduce<string | null>((worst, freshness) => {
    if (!worst || freshnessRank(freshness) > freshnessRank(worst)) {
      return freshness;
    }

    return worst;
  }, null);
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
    returnMethod: loadReturnMethod(),
  });

  const instrumentsQuery = useInstruments();
  const transactionsQuery = useTransactions();
  const holdingsQuery = useHoldings();
  const gainsQuery = useGains({
    includeClosedPositions: uiState.includeClosedPositions,
    startDate: uiState.dateRange.startDate,
    endDate: uiState.dateRange.endDate ?? undefined,
    method: uiState.returnMethod,
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
  const priceFreshness = portfolioPriceFreshness(gainsQuery.data?.rows);
  const pricesRefreshing =
    refreshPrices.isPending || priceStatusQuery.data?.refreshing === true;

  const boardIsFetching =
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
          className="button primary"
          type="button"
          onClick={() => void refreshPrices.mutateAsync({ mode: "latest" })}
          disabled={pricesRefreshing}
        >
          <RefreshCw
            aria-hidden="true"
            className={pricesRefreshing ? "spin" : undefined}
            size={16}
          />
          <span>Refresh prices</span>
        </button>
        <button
          className="button secondary"
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
          {pricesRefreshing ? (
            <span className="status-chip warning">
              <RefreshCw aria-hidden="true" className="spin" size={12} />
              Prices: refreshing
            </span>
          ) : refreshPrices.isError ? (
            <span
              className="status-chip warning"
              title={refreshPrices.error.message}
            >
              Prices: refresh failed
            </span>
          ) : priceFreshness ? (
            <span
              className={
                freshnessTone(priceFreshness) === "warning"
                  ? "status-chip warning"
                  : "status-chip"
              }
            >
              Prices: {freshnessLabel(priceFreshness)}
            </span>
          ) : priceStatusQuery.isPending || boardIsFetching ? (
            <span className="status-chip">Prices: checking</span>
          ) : (
            <span className="status-chip">Prices: no data</span>
          )}
        </div>
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
            <AsyncBoundary
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
            </AsyncBoundary>
          ) : uiState.boardView === "gains" ? (
            <AsyncBoundary
              isPending={gainsQuery.isPending}
              isError={gainsQuery.isError}
              isEmpty={(gainsQuery.data?.rows.length ?? 0) === 0}
              onRetry={() => void gainsQuery.refetch()}
              emptyMessage="No valued holdings yet. Add a Buy and refresh prices."
            >
              <GainsTable
                rows={gainsQuery.data?.rows ?? []}
                totals={gainsQuery.data?.totals}
                percentageMethod={gainsQuery.data?.percentage_method}
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
                returnMethod={uiState.returnMethod}
                onReturnMethodChange={(returnMethod) =>
                  dispatch({ type: "returnMethodChanged", returnMethod })
                }
              />
            </AsyncBoundary>
          ) : (
            <AsyncBoundary
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
            </AsyncBoundary>
          )}
        </article>
      </section>
    </>
  );
}
