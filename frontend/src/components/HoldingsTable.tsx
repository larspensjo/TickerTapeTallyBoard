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
import { useMemo, useState } from "react";
import type { AvailabilityValue, Holding } from "../api/types";
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
  ]
    .join(" ")
    .toLowerCase();
}

export function HoldingsTable({
  holdings,
  filter,
  onFilterChange,
}: {
  holdings: Holding[];
  filter: string;
  onFilterChange: (filter: string) => void;
}) {
  const [sorting, setSorting] = useState<SortingState>(loadHoldingsSorting);
  const handleSortingChange: OnChangeFn<SortingState> = (updater) => {
    setSorting((current) => {
      const next = typeof updater === "function" ? updater(current) : updater;
      saveHoldingsSorting(next);
      return next;
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
  const table = useReactTable({
    data: tableRows,
    columns,
    state: { sorting, globalFilter: filter },
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
            </tr>
          </tfoot>
        </table>
      </div>
    </>
  );
}
