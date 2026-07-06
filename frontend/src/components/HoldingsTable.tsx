import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getFilteredRowModel,
  getSortedRowModel,
  type OnChangeFn,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp } from "lucide-react";
import { type Dispatch, useMemo, useReducer, useState } from "react";
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
  isTargetAlert,
  pendingConvictionChanges,
  targetGapField,
  targetGapTone,
  targetStatusLabel,
  targetStatusRank,
  targetValueField,
} from "./holdingsConviction";
import { InstrumentCell } from "./InstrumentCell";
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

interface RowView {
  holding: Holding;
  search: string;
}

const columnHelper = createColumnHelper<RowView>();
type PortfolioPercentage = AvailabilityValue<string>;
const HOLDINGS_SORTING_KEY = "holdings.sorting";
const DEFAULT_SORTING: SortingState = [{ id: "market_value_base", desc: true }];
const SORTABLE_COLUMN_IDS = new Set([
  "instrument",
  "quantity",
  "average_cost_native",
  "cost_basis_native",
  "market_value_base",
  "portfolio_percentage",
  "conviction",
  "target_value_base",
  "target_gap_base",
  "target_status",
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

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function defaultSorting(): SortingState {
  return DEFAULT_SORTING.map((sort) => ({ ...sort }));
}

function isSortingState(value: unknown): value is SortingState {
  if (!Array.isArray(value)) return false;

  const seen = new Set<string>();
  return value.every((sort) => {
    if (typeof sort !== "object" || sort === null) return false;

    const candidate = sort as Partial<SortingState[number]>;
    if (
      typeof candidate.id !== "string" ||
      typeof candidate.desc !== "boolean" ||
      !SORTABLE_COLUMN_IDS.has(candidate.id) ||
      seen.has(candidate.id)
    ) {
      return false;
    }

    seen.add(candidate.id);
    return true;
  });
}

export function loadHoldingsSorting(): SortingState {
  const saved = storage()?.getItem(HOLDINGS_SORTING_KEY);
  if (!saved) return defaultSorting();

  try {
    const parsed = JSON.parse(saved);
    return isSortingState(parsed) ? parsed : defaultSorting();
  } catch {
    return defaultSorting();
  }
}

export function saveHoldingsSorting(sorting: SortingState): void {
  if (!isSortingState(sorting)) return;

  try {
    storage()?.setItem(HOLDINGS_SORTING_KEY, JSON.stringify(sorting));
  } catch {
    // Ignore storage failures; sorting should still work for the current session.
  }
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

function currentValueCell(holding: Holding) {
  const valuation = holding.valuation;
  if (!valuation || !isAvailable(valuation.market_value_base)) {
    return valuationMissingChip(holding);
  }

  return <AvailabilityValueCell value={valuation.market_value_base} />;
}

function pnlHintCell(holding: Holding) {
  const valuation = holding.valuation;
  if (!valuation || !isAvailable(valuation.market_value_base)) {
    return valuationMissingChip(holding);
  }

  return (
    <div className="metric-stack">
      <span className="metric-subtle">
        P&amp;L{" "}
        <AvailabilityValueCell
          value={valuation.unrealized_gain_base}
          prefix="SEK "
          tone="signed"
          unavailableLabel="Missing"
        />{" "}
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

const numericColumns = new Set([
  "market_value_base",
  "quantity",
  "average_cost_native",
  "cost_basis_native",
  "portfolio_percentage",
  "target_value_base",
  "target_gap_base",
]);

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
  if (holding.base.status !== "available") {
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
  return (
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
  );
}

function targetValueCell(target: ConvictionTarget) {
  if (target.target_value_base.status === "available") {
    return (
      <AvailabilityValueCell value={target.target_value_base} prefix="SEK " />
    );
  }

  // No target / excluded / unavailable: the Target status column carries the
  // reason, so keep this cell quiet rather than a warning fill.
  return <span className="metric-subtle">--</span>;
}

function targetGapCell(target: ConvictionTarget) {
  const gap = target.target_gap_base;
  if (gap.status !== "available") {
    return <span className="metric-subtle">--</span>;
  }

  const tone = targetGapTone(gap.value);
  const percent = target.target_gap_percent;
  return (
    <div className="metric-stack">
      <span className={`number ${tone}`}>
        <FormattedNumber value={gap.value} prefix="SEK " />
      </span>
      {percent.status === "available" ? (
        <span className="metric-subtle">
          <span className={`number ${tone}`}>
            <FormattedNumber value={percent.value} suffix="%" />
          </span>
        </span>
      ) : null}
    </div>
  );
}

function targetStatusCell(target: ConvictionTarget) {
  const alert = isTargetAlert(target.status);
  const reasons =
    target.target_value_base.status === "unavailable"
      ? target.target_value_base.reasons
      : [];
  return (
    <span
      className={alert ? "status-chip warning" : "status-chip"}
      title={reasons.length > 0 ? reasonSummary(reasons) : undefined}
    >
      {targetStatusLabel(target.status)}
    </span>
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
      id: "average_cost_native",
      header: "Avg cost/share",
      cell: (info) => (
        <FormattedNumber
          value={info.getValue()}
          prefix={info.row.original.holding.instrument.currency}
        />
      ),
    }),
    columnHelper.accessor((row) => row.holding.cost_basis_native, {
      id: "cost_basis_native",
      header: "Cost basis",
      cell: (info) => (
        <FormattedNumber
          value={info.getValue()}
          prefix={info.row.original.holding.instrument.currency}
        />
      ),
    }),
    columnHelper.accessor(
      (row) =>
        row.holding.valuation?.market_value_base ??
        unavailableValue("valuation_unavailable"),
      {
        id: "market_value_base",
        header: "Current value (SEK)",
        sortingFn: availabilitySortRows,
        cell: (info) => currentValueCell(info.row.original.holding),
      },
    ),
    columnHelper.accessor(
      (row) =>
        portfolioPercentages.get(row.holding.instrument.id) ??
        unavailableValue("valuation_unavailable"),
      {
        id: "portfolio_percentage",
        header: "Portfolio %",
        sortingFn: availabilitySortRows,
        cell: (info) => portfolioPercentageCell(info.getValue()),
      },
    ),
    columnHelper.display({
      id: "pnl_hint",
      header: "P&L hint",
      cell: (info) => pnlHintCell(info.row.original.holding),
    }),
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
      (row) => targetValueField(row.holding.conviction_target),
      {
        id: "target_value_base",
        header: "Target (SEK)",
        sortingFn: availabilitySortRows,
        cell: (info) =>
          targetValueCell(info.row.original.holding.conviction_target),
      },
    ),
    columnHelper.accessor(
      (row) => targetGapField(row.holding.conviction_target),
      {
        id: "target_gap_base",
        header: "Target gap",
        sortingFn: availabilitySortRows,
        cell: (info) =>
          targetGapCell(info.row.original.holding.conviction_target),
      },
    ),
    columnHelper.accessor(
      (row) => targetStatusRank(row.holding.conviction_target.status),
      {
        id: "target_status",
        header: "Target status",
        cell: (info) =>
          targetStatusCell(info.row.original.holding.conviction_target),
      },
    ),
  ];
}

function holdingSearchText(holding: Holding): string {
  return [
    holding.instrument.symbol,
    holding.instrument.name,
    holding.instrument.exchange,
    holding.instrument.currency,
    holding.instrument.type,
    holding.quantity.toString(),
    holding.average_cost_native,
    holding.cost_basis_native,
    holding.base.status,
    ...(holding.base.status === "unavailable"
      ? holding.base.reasons.map((reason) => reason.code)
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
  canEditConviction = false,
  onApplyConvictions,
  isApplyingConvictions = false,
  applyError = null,
}: {
  holdings: Holding[];
  filter: string;
  onFilterChange: (filter: string) => void;
  canEditConviction?: boolean;
  onApplyConvictions?: (changes: ConvictionChange[]) => Promise<void>;
  isApplyingConvictions?: boolean;
  applyError?: string | null;
}) {
  const [sorting, setSorting] = useState<SortingState>(loadHoldingsSorting);
  const [edits, dispatchEdits] = useReducer(convictionEditsReducer, {});
  // Drop edits a background refetch has made match the saved value, so Apply
  // never fires a no-op write.
  const pendingChanges = pendingConvictionChanges(edits, holdings);
  const dirty = pendingChanges.length > 0;
  const handleSortingChange: OnChangeFn<SortingState> = (updater) => {
    setSorting((current) => {
      const next = typeof updater === "function" ? updater(current) : updater;
      saveHoldingsSorting(next);
      return next;
    });
  };
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
              <tr key={row.id}>
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
              <td />
              <td
                className="number"
                title={summaryTitle(summary.excludedMarketValueRows)}
              >
                <AvailabilityValueCell
                  value={summary.marketValueBase}
                  prefix="SEK "
                />
              </td>
              <td
                className="number"
                title={summaryTitle(summary.excludedMarketValueRows)}
              >
                {portfolioPercentageCell(summary.portfolioPercentage)}
              </td>
              <td>{holdingsSummaryPnlCell(summary)}</td>
              {/* conviction, target, target gap, target status: no totals */}
              <td />
              <td />
              <td />
              <td />
            </tr>
          </tfoot>
        </table>
      </div>
    </>
  );
}
