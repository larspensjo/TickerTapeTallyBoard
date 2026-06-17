import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  getSortedRowModel,
  type SortingState,
  useReactTable,
} from "@tanstack/react-table";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useState } from "react";
import type { Holding } from "../api/types";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  isAvailable,
  reasonSummary,
  unavailableValue,
} from "./valuationDisplay";

const columnHelper = createColumnHelper<Holding>();

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
    ...(holding.valuation?.day_change_base.status === "unavailable"
      ? holding.valuation.day_change_base.reasons
      : []),
  ];
}

function valuationSummaryCell(holding: Holding) {
  const valuation = holding.valuation;
  if (!valuation || !isAvailable(valuation.market_value_base)) {
    const reasons = valuation ? valuationUnavailableReasons(holding) : [];
    return (
      <span
        className="status-chip warning"
        title={reasons.length > 0 ? reasonSummary(reasons) : undefined}
      >
        Valuation missing
      </span>
    );
  }

  return (
    <div className="metric-stack">
      <AvailabilityValueCell
        value={valuation.market_value_base}
        prefix="SEK "
      />
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
      <span className="metric-subtle">
        Day{" "}
        <AvailabilityValueCell
          value={valuation.day_change_base}
          prefix="SEK "
          tone="signed"
          unavailableLabel="Missing"
        />
      </span>
    </div>
  );
}

const columns = [
  columnHelper.accessor((row) => row.instrument.symbol, {
    id: "instrument",
    header: "Instrument",
    cell: (info) => {
      const { symbol, name, exchange } = info.row.original.instrument;
      return (
        <div className="instrument-cell">
          <strong>{symbol}</strong>
          <span>{name}</span>
          <em>{exchange}</em>
        </div>
      );
    },
  }),
  columnHelper.accessor("quantity", {
    header: "Qty",
    cell: (info) => info.getValue(),
  }),
  columnHelper.accessor("average_cost_native", {
    header: "Avg cost/share",
    cell: (info) =>
      `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.accessor("cost_basis_native", {
    header: "Cost basis (total)",
    cell: (info) =>
      `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.accessor(
    (row) =>
      row.valuation?.market_value_base ??
      unavailableValue("valuation_unavailable"),
    {
      id: "valuation_summary",
      header: "Value / P&L",
      sortingFn: availabilitySortRows,
      cell: (info) => valuationSummaryCell(info.row.original),
    },
  ),
];

const numericColumns = new Set([
  "valuation_summary",
  "quantity",
  "average_cost_native",
  "cost_basis_native",
]);

export function HoldingsTable({ holdings }: { holdings: Holding[] }) {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "instrument", desc: false },
  ]);
  const table = useReactTable({
    data: holdings,
    columns,
    state: { sorting },
    onSortingChange: setSorting,
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
  });

  return (
    <div className="table-wrap">
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
  );
}
