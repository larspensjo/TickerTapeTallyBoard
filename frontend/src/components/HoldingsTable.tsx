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
import { useMemo, useState } from "react";
import type { AvailabilityValue, Holding } from "../api/types";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  FormattedNumber,
  formatGroupedNumber,
  isAvailable,
  reasonSummary,
  unavailableValue,
} from "./valuationDisplay";

interface RowView {
  holding: Holding;
  search: string;
}

const columnHelper = createColumnHelper<RowView>();
type PortfolioPercentage = AvailabilityValue<string>;

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

function buildColumns(portfolioPercentages: Map<number, PortfolioPercentage>) {
  return [
    columnHelper.accessor((row) => row.holding.instrument.symbol, {
      id: "instrument",
      header: "Instrument",
      cell: (info) => {
        const { symbol, name, exchange } = info.row.original.holding.instrument;
        return (
          <div className="instrument-cell">
            <strong>{symbol}</strong>
            <span>{name}</span>
            <em>{exchange}</em>
          </div>
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
  const [sorting, setSorting] = useState<SortingState>([
    { id: "market_value_base", desc: true },
  ]);
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
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    globalFilterFn: (row, _columnId, filterValue) =>
      row.original.search.includes(String(filterValue).trim().toLowerCase()),
  });

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
        </table>
      </div>
    </>
  );
}
