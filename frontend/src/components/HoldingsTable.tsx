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

const columnHelper = createColumnHelper<Holding>();

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
    header: "Avg cost",
    cell: (info) =>
      `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.accessor("cost_basis_native", {
    header: "Cost basis",
    cell: (info) =>
      `${info.row.original.instrument.currency} ${info.getValue()}`,
  }),
  columnHelper.display({
    id: "base",
    header: "Avg cost (SEK)",
    cell: (info) => {
      const holding = info.row.original;
      if (holding.base.status === "available") {
        return (
          <span className="number">SEK {holding.base.average_cost_base}</span>
        );
      }

      const title = holding.base.reasons
        .map((reason) => `${reason.code} @ ${reason.transaction_id}`)
        .join(", ");
      return (
        <span className="status-chip warning" title={title}>
          FX missing
        </span>
      );
    },
  }),
];

const numericColumns = new Set([
  "quantity",
  "average_cost_native",
  "cost_basis_native",
  "base",
]);

export function HoldingsTable({ holdings }: { holdings: Holding[] }) {
  const [sorting, setSorting] = useState<SortingState>([]);
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
