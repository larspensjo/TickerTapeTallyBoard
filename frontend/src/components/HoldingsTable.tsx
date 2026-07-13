import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp } from "lucide-react";
import {
  type Dispatch,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import type { ConvictionChange } from "../api/queries";
import type {
  AvailabilityValue,
  Conviction,
  ConvictionTarget,
  Holding,
} from "../api/types";
import {
  CONVICTION_OPTIONS,
  type ConvictionEditAction,
  type ConvictionEdits,
  convictionEditsReducer,
  convictionLabel,
  convictionRank,
  effectiveConviction,
  holdingConvictionSearchText,
  holdingValueSortField,
  pendingConvictionChanges,
  targetGapBar,
  targetGapPercentField,
  watchlistTargetsHint,
} from "./holdingsConviction";
import { InstrumentCell } from "./InstrumentCell";
import { usePersistentSorting } from "./persistence";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  FormattedNumber,
  formatGroupedNumber,
  isAvailable,
  parseFiniteNumber,
  reasonSummary,
  unavailableValue,
} from "./valuationDisplay";

interface HoldingsTableMeta {
  edits: ConvictionEdits;
  canEditConviction: boolean;
  dispatchEdits: Dispatch<ConvictionEditAction>;
}

export interface HoldingsTableHighlightRequest {
  instrumentId: number;
  token: number;
}

interface RowView {
  holding: Holding;
  search: string;
}

const columnHelper = createColumnHelper<RowView>();
type PortfolioPercentage = AvailabilityValue<string>;
const HOLDINGS_SORTING_KEY = "holdings.sorting";
const DEFAULT_SORTING: SortingState = [{ id: "value", desc: true }];
const SORTABLE_COLUMN_IDS = new Set([
  "instrument",
  "quantity",
  "cost",
  "value",
  "pnl",
  "conviction",
  "target",
]);

interface HoldingsSummary {
  marketValueBase: AvailabilityValue<string>;
  portfolioPercentage: PortfolioPercentage;
  unrealizedGainBase: AvailabilityValue<string>;
  unrealizedGainPercent: AvailabilityValue<string>;
  excludedMarketValueRows: number;
  excludedPnlRows: number;
}

function valuationUnavailableReasons(holding: Holding): string[] {
  return [
    ...(holding.valuation?.market_value_base.status === "unavailable"
      ? holding.valuation.market_value_base.reasons
      : []),
    ...(holding.valuation?.unrealized_gain_base.status === "unavailable"
      ? holding.valuation.unrealized_gain_base.reasons
      : []),
    ...(holding.valuation?.unrealized_gain_percent.status === "unavailable"
      ? holding.valuation.unrealized_gain_percent.reasons
      : []),
  ];
}

function valuationMissingChip(holding: Holding) {
  const reasons = holding.valuation ? valuationUnavailableReasons(holding) : [];

  return (
    <span
      className="status-chip warning"
      title={reasons.length > 0 ? reasonSummary(reasons) : undefined}
    >
      Valuation missing
    </span>
  );
}

function blankMetricCell() {
  return <span className="status-empty" aria-hidden="true" />;
}

function costCell(holding: Holding) {
  if (holding.row_kind === "watchlist") {
    return blankMetricCell();
  }

  if (holding.base === null) {
    return valuationMissingChip(holding);
  }

  const averageCostNative = holding.average_cost_native;
  const costBasisNative = holding.cost_basis_native;
  if (averageCostNative === null || costBasisNative === null) {
    return valuationMissingChip(holding);
  }

  const currency = holding.instrument.currency;
  return (
    <div className="metric-stack">
      <FormattedNumber value={averageCostNative} prefix={currency} />
      <span className="metric-subtle">
        <FormattedNumber value={costBasisNative} prefix={currency} />
      </span>
    </div>
  );
}

function valueCell(holding: Holding, percentage: PortfolioPercentage) {
  if (holding.row_kind === "watchlist") {
    return blankMetricCell();
  }

  const valuation = holding.valuation;
  if (!valuation || !isAvailable(valuation.market_value_base)) {
    return valuationMissingChip(holding);
  }
  return (
    <div className="metric-stack">
      <AvailabilityValueCell value={valuation.market_value_base} />
      <span className="metric-subtle">
        {portfolioPercentageCell(percentage)}
      </span>
    </div>
  );
}

function pnlCell(holding: Holding) {
  if (holding.row_kind === "watchlist") {
    return blankMetricCell();
  }

  const valuation = holding.valuation;
  // Gating on market_value_base availability is equivalent to gating on gain
  // availability: the backend derives unrealized_gain_base from market value
  // (backend/src/domain/valuation.rs), so an available gain with an unavailable
  // market value cannot occur. This keeps the guard aligned with the `pnl`
  // column sorting on unrealized_gain_base.
  if (!valuation || !isAvailable(valuation.market_value_base)) {
    return valuationMissingChip(holding);
  }
  return (
    <div className="metric-stack">
      <AvailabilityValueCell
        value={valuation.unrealized_gain_base}
        prefix="SEK "
        tone="signed"
        unavailableLabel="Missing"
      />
      <span className="metric-subtle">
        <AvailabilityValueCell
          value={valuation.unrealized_gain_percent}
          suffix="%"
          tone="signed"
          unavailableLabel="Missing"
        />
      </span>
    </div>
  );
}

const numericColumns = new Set(["quantity", "cost", "value", "pnl", "target"]);

function marketValueNumber(holding: Holding): number | null {
  const marketValue = holding.valuation?.market_value_base;
  if (marketValue?.status !== "available") {
    return null;
  }

  const parsed = Number(marketValue.value);
  return Number.isFinite(parsed) ? parsed : null;
}

function computePortfolioPercentages(
  holdings: Holding[],
): Map<number, PortfolioPercentage> {
  const marketValues = holdings.map((holding) => marketValueNumber(holding));
  // This display-only weight intentionally uses frontend float math.
  const totalMarketValue = marketValues.reduce<number>(
    (sum, value) => sum + (value ?? 0),
    0,
  );

  const percentages = new Map<number, PortfolioPercentage>();
  const canComputePercentages = totalMarketValue > 0;

  holdings.forEach((holding, index) => {
    const marketValue = marketValues[index];
    if (marketValue === null || !canComputePercentages) {
      percentages.set(
        holding.instrument.id,
        unavailableValue("valuation_unavailable"),
      );
      return;
    }

    percentages.set(holding.instrument.id, {
      status: "available",
      value: ((marketValue / totalMarketValue) * 100).toFixed(1),
    });
  });

  return percentages;
}

function portfolioPercentageCell(value: PortfolioPercentage) {
  if (value.status === "available") {
    return <span className="number">{formatGroupedNumber(value.value)}%</span>;
  }

  return (
    <span title="Excluded from portfolio weight (valuation unavailable)">
      --
    </span>
  );
}

function availableNumber(
  value: AvailabilityValue<string> | undefined,
): number | null {
  if (!value || value.status === "unavailable") {
    return null;
  }

  return parseFiniteNumber(value.value);
}

function holdingCostBasisBaseNumber(holding: Holding): number | null {
  if (holding.base === null || holding.base.status !== "available") {
    return null;
  }

  return parseFiniteNumber(holding.base.cost_basis_base);
}

function moneyValue(value: number): AvailabilityValue<string> {
  return { status: "available", value: value.toFixed(2) };
}

function percentValue(
  value: number,
  fractionDigits = 1,
): AvailabilityValue<string> {
  return { status: "available", value: value.toFixed(fractionDigits) };
}

function computeHoldingsSummary(
  rows: RowView[],
  portfolioPercentages: Map<number, PortfolioPercentage>,
): HoldingsSummary {
  let marketValueBase = 0;
  let marketValueCount = 0;
  let portfolioPercentage = 0;
  let portfolioPercentageCount = 0;
  let unrealizedGainBase = 0;
  let pnlCostBasisBase = 0;
  let pnlCount = 0;

  for (const { holding } of rows) {
    const currentMarketValue = availableNumber(
      holding.valuation?.market_value_base,
    );
    if (currentMarketValue !== null) {
      marketValueBase += currentMarketValue;
      marketValueCount += 1;
    }

    const currentPortfolioPercentage = availableNumber(
      portfolioPercentages.get(holding.instrument.id),
    );
    if (currentPortfolioPercentage !== null) {
      portfolioPercentage += currentPortfolioPercentage;
      portfolioPercentageCount += 1;
    }

    const currentUnrealizedGain = availableNumber(
      holding.valuation?.unrealized_gain_base,
    );
    const currentCostBasisBase = holdingCostBasisBaseNumber(holding);
    if (currentUnrealizedGain !== null && currentCostBasisBase !== null) {
      unrealizedGainBase += currentUnrealizedGain;
      pnlCostBasisBase += currentCostBasisBase;
      pnlCount += 1;
    }
  }

  const excludedMarketValueRows = rows.length - marketValueCount;
  const excludedPnlRows = rows.length - pnlCount;

  return {
    marketValueBase:
      marketValueCount > 0
        ? moneyValue(marketValueBase)
        : unavailableValue("valuation_unavailable"),
    portfolioPercentage:
      portfolioPercentageCount > 0
        ? percentValue(portfolioPercentage)
        : unavailableValue("valuation_unavailable"),
    unrealizedGainBase:
      pnlCount > 0
        ? moneyValue(unrealizedGainBase)
        : unavailableValue("valuation_unavailable"),
    unrealizedGainPercent:
      pnlCount > 0 && pnlCostBasisBase !== 0
        ? percentValue((unrealizedGainBase / pnlCostBasisBase) * 100, 2)
        : unavailableValue("base_cost_basis_unavailable"),
    excludedMarketValueRows,
    excludedPnlRows,
  };
}

function summaryTitle(excludedRows: number): string | undefined {
  return excludedRows > 0
    ? `${formatGroupedNumber(excludedRows)} rows excluded from this summary`
    : undefined;
}

function holdingsSummaryPnlCell(summary: HoldingsSummary) {
  return (
    <div className="metric-stack" title={summaryTitle(summary.excludedPnlRows)}>
      <AvailabilityValueCell value={summary.unrealizedGainBase} tone="signed" />
      {summary.unrealizedGainBase.status === "available" ? (
        <span className="metric-subtle">
          <AvailabilityValueCell
            value={summary.unrealizedGainPercent}
            suffix="%"
            tone="signed"
          />
        </span>
      ) : null}
    </div>
  );
}

function convictionCell(holding: Holding, meta: HoldingsTableMeta) {
  const saved = holding.instrument.conviction;
  const value = effectiveConviction(holding, meta.edits);
  const dirty = value !== saved;
  const targetValue = holding.conviction_target.target_value_base;
  return (
    <div className="conviction-stack">
      <select
        className={dirty ? "conviction-select dirty" : "conviction-select"}
        value={value}
        disabled={!meta.canEditConviction}
        aria-label={`Conviction for ${holding.instrument.symbol}`}
        title={dirty ? `Unsaved change from ${saved}` : undefined}
        onChange={(event) =>
          meta.dispatchEdits({
            type: "stage",
            instrumentId: holding.instrument.id,
            saved,
            conviction: event.target.value as Conviction,
          })
        }
      >
        {CONVICTION_OPTIONS.map((option) => (
          <option key={option} value={option}>
            {convictionLabel(option)}
          </option>
        ))}
      </select>
      {targetValue.status === "available" ? (
        <span className="metric-subtle">
          <FormattedNumber value={targetValue.value} prefix="SEK " />
        </span>
      ) : null}
    </div>
  );
}

function targetCell(target: ConvictionTarget) {
  const bar = targetGapBar(target);
  // No computable gap (no target / excluded / unavailable): empty cell.
  if (!bar) {
    return null;
  }

  return (
    <div
      className="target-gap-track"
      role="img"
      aria-label={bar.tooltip}
      title={bar.tooltip}
    >
      <span className="target-gap-axis" />
      {bar.side !== "on_target" ? (
        <span
          className={`target-gap-fill ${bar.side}`}
          style={{ width: `${bar.widthPercent}%` }}
        />
      ) : null}
    </div>
  );
}

function buildColumns(portfolioPercentages: Map<number, PortfolioPercentage>) {
  return [
    columnHelper.accessor((row) => row.holding.instrument.name, {
      id: "instrument",
      header: "Instrument",
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
    }),
    columnHelper.accessor((row) => row.holding.quantity, {
      id: "quantity",
      header: "Qty",
      cell: (info) => formatGroupedNumber(info.getValue()),
    }),
    columnHelper.accessor((row) => row.holding.average_cost_native, {
      id: "cost",
      header: "Cost",
      cell: (info) => costCell(info.row.original.holding),
    }),
    columnHelper.accessor((row) => holdingValueSortField(row.holding), {
      id: "value",
      header: "Value (SEK)",
      sortingFn: availabilitySortRows,
      cell: (info) =>
        valueCell(
          info.row.original.holding,
          portfolioPercentages.get(info.row.original.holding.instrument.id) ??
            unavailableValue("valuation_unavailable"),
        ),
    }),
    columnHelper.accessor(
      (row) =>
        row.holding.valuation?.unrealized_gain_base ??
        unavailableValue("valuation_unavailable"),
      {
        id: "pnl",
        header: "P&L",
        sortingFn: availabilitySortRows,
        cell: (info) => pnlCell(info.row.original.holding),
      },
    ),
    columnHelper.accessor(
      (row) => convictionRank(row.holding.instrument.conviction),
      {
        id: "conviction",
        header: "Conviction",
        cell: (info) =>
          convictionCell(
            info.row.original.holding,
            info.table.options.meta as HoldingsTableMeta,
          ),
      },
    ),
    columnHelper.accessor(
      (row) => targetGapPercentField(row.holding.conviction_target),
      {
        id: "target",
        header: "Target gap",
        sortingFn: availabilitySortRows,
        cell: (info) => targetCell(info.row.original.holding.conviction_target),
      },
    ),
  ];
}

function holdingSearchText(holding: Holding): string {
  const base = holding.base;
  return [
    holding.instrument.symbol,
    holding.instrument.name,
    holding.instrument.exchange,
    holding.instrument.currency,
    holding.instrument.type,
    holding.quantity.toString(),
    holding.average_cost_native ?? "",
    holding.cost_basis_native ?? "",
    base?.status ?? "",
    ...(base?.status === "unavailable"
      ? base.reasons.map((reason) => reason.code)
      : []),
    holding.valuation?.market_value_base.status === "available"
      ? holding.valuation.market_value_base.value
      : "",
    holding.valuation?.unrealized_gain_base.status === "available"
      ? holding.valuation.unrealized_gain_base.value
      : "",
    holding.valuation?.unrealized_gain_percent.status === "available"
      ? holding.valuation.unrealized_gain_percent.value
      : "",
    holdingConvictionSearchText(holding),
  ]
    .join(" ")
    .toLowerCase();
}

export function HoldingsTable({
  holdings,
  filter,
  onFilterChange,
  includeWatchlist = false,
  onIncludeWatchlistChange,
  hiddenWatchlistPoolCount = 0,
  highlightRequest = null,
  canEditConviction = false,
  onApplyConvictions,
  isApplyingConvictions = false,
  applyError = null,
}: {
  holdings: Holding[];
  filter: string;
  onFilterChange: (filter: string) => void;
  includeWatchlist?: boolean;
  onIncludeWatchlistChange?: (includeWatchlist: boolean) => void;
  hiddenWatchlistPoolCount?: number;
  highlightRequest?: HoldingsTableHighlightRequest | null;
  canEditConviction?: boolean;
  onApplyConvictions?: (changes: ConvictionChange[]) => Promise<void>;
  isApplyingConvictions?: boolean;
  applyError?: string | null;
}) {
  const [sorting, handleSortingChange] = usePersistentSorting(
    HOLDINGS_SORTING_KEY,
    SORTABLE_COLUMN_IDS,
    DEFAULT_SORTING,
  );
  const [edits, dispatchEdits] = useReducer(convictionEditsReducer, {});
  const rowRefs = useRef(new Map<number, HTMLTableRowElement>());
  const [activeHighlight, setActiveHighlight] =
    useState<HoldingsTableHighlightRequest | null>(null);
  const lastHandledToken = useRef<number | null>(null);
  // Drop edits a background refetch has made match the saved value, so Apply
  // never fires a no-op write.
  const pendingChanges = pendingConvictionChanges(edits, holdings);
  const dirty = pendingChanges.length > 0;
  const handleApply = () => {
    if (!onApplyConvictions || !dirty || isApplyingConvictions) return;
    onApplyConvictions(pendingChanges)
      .then(() => {
        // Clear staged edits only after a successful save; the refetched
        // holdings then carry the new saved convictions and pool-wide targets.
        dispatchEdits({ type: "discard" });
      })
      .catch(() => {
        // Keep staged edits so the user can retry; the error is surfaced via
        // the applyError message below.
      });
  };
  const tableRows = useMemo<RowView[]>(
    () =>
      holdings.map((holding) => ({
        holding,
        search: holdingSearchText(holding),
      })),
    [holdings],
  );
  const portfolioPercentages = useMemo(
    () => computePortfolioPercentages(holdings),
    [holdings],
  );
  const columns = useMemo(
    () => buildColumns(portfolioPercentages),
    [portfolioPercentages],
  );
  const meta: HoldingsTableMeta = { edits, canEditConviction, dispatchEdits };
  const table = useReactTable({
    data: tableRows,
    columns,
    state: { sorting, globalFilter: filter },
    meta,
    onSortingChange: handleSortingChange,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    globalFilterFn: (row, _columnId, filterValue) =>
      row.original.search.includes(String(filterValue).trim().toLowerCase()),
  });
  const visibleRows = table.getRowModel().rows.map((row) => row.original);
  const summary = computeHoldingsSummary(visibleRows, portfolioPercentages);
  const watchlistHint = watchlistTargetsHint(hiddenWatchlistPoolCount);
  const highlightedRow = highlightRequest
    ? (table
        .getRowModel()
        .rows.find(
          (row) =>
            row.original.holding.instrument.id ===
            highlightRequest.instrumentId,
        ) ?? null)
    : null;

  useEffect(() => {
    if (!highlightRequest || !highlightedRow) {
      return;
    }

    if (lastHandledToken.current === highlightRequest.token) {
      return;
    }

    const row = rowRefs.current.get(highlightRequest.instrumentId);
    if (!row) {
      return;
    }

    lastHandledToken.current = highlightRequest.token;
    setActiveHighlight(highlightRequest);
    row.scrollIntoView({ block: "center", behavior: "smooth" });

    const timer = setTimeout(() => setActiveHighlight(null), 1800);
    return () => clearTimeout(timer);
  }, [highlightRequest, highlightedRow]);

  return (
    <>
      <div className="table-toolbar">
        <input
          className="filter-input"
          type="search"
          placeholder="Filter instrument"
          value={filter}
          onChange={(event) => onFilterChange(event.target.value)}
        />
        {onIncludeWatchlistChange ? (
          <div className="toolbar-watchlist">
            <label className="toolbar-check">
              <input
                type="checkbox"
                checked={includeWatchlist}
                onChange={(event) =>
                  onIncludeWatchlistChange(event.target.checked)
                }
              />
              <span>Include watchlist</span>
            </label>
            {watchlistHint && !includeWatchlist ? (
              <span className="toolbar-hint">{watchlistHint}</span>
            ) : null}
          </div>
        ) : null}
        {canEditConviction ? (
          <div className="toolbar-actions">
            {applyError ? (
              <span className="toolbar-error down" role="alert">
                {applyError}
              </span>
            ) : null}
            <button
              type="button"
              className="button secondary"
              onClick={() => dispatchEdits({ type: "discard" })}
              disabled={!dirty || isApplyingConvictions}
            >
              Discard
            </button>
            <button
              type="button"
              className="button primary"
              onClick={handleApply}
              disabled={!dirty || isApplyingConvictions}
            >
              {isApplyingConvictions ? "Applying…" : "Apply conviction changes"}
            </button>
          </div>
        ) : null}
      </div>
      <div className="table-wrap holdings-table">
        <table>
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  const sorted = header.column.getIsSorted();
                  return (
                    <th
                      key={header.id}
                      className={
                        numericColumns.has(header.column.id)
                          ? "sortable number-head"
                          : "sortable"
                      }
                    >
                      <button
                        type="button"
                        className="sort-button"
                        onClick={header.column.getToggleSortingHandler()}
                      >
                        {flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                        {sorted === "asc" ? (
                          <ChevronUp aria-hidden="true" size={12} />
                        ) : sorted === "desc" ? (
                          <ChevronDown aria-hidden="true" size={12} />
                        ) : null}
                      </button>
                    </th>
                  );
                })}
              </tr>
            ))}
          </thead>
          <tbody>
            {table.getRowModel().rows.map((row) => (
              <tr
                key={row.id}
                ref={(element) => {
                  if (element) {
                    rowRefs.current.set(
                      row.original.holding.instrument.id,
                      element,
                    );
                  } else {
                    rowRefs.current.delete(row.original.holding.instrument.id);
                  }
                }}
                className={
                  activeHighlight?.instrumentId ===
                  row.original.holding.instrument.id
                    ? "row-highlighted"
                    : undefined
                }
              >
                {row.getVisibleCells().map((cell) => (
                  <td
                    key={cell.id}
                    className={
                      numericColumns.has(cell.column.id) ? "number" : undefined
                    }
                  >
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <th scope="row">Total</th>
              <td />
              <td />
              <td
                className="number"
                title={summaryTitle(summary.excludedMarketValueRows)}
              >
                <div className="metric-stack">
                  <AvailabilityValueCell
                    value={summary.marketValueBase}
                    prefix="SEK "
                  />
                  <span className="metric-subtle">
                    {portfolioPercentageCell(summary.portfolioPercentage)}
                  </span>
                </div>
              </td>
              <td className="number">{holdingsSummaryPnlCell(summary)}</td>
              {/* conviction, target: no totals */}
              <td />
              <td />
            </tr>
          </tfoot>
        </table>
      </div>
    </>
  );
}
