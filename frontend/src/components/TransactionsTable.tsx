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
import type { Instrument, Transaction } from "../api/types";
import { FormattedNumber, formatGroupedNumber } from "./valuationDisplay";

interface Row {
  transaction: Transaction;
  symbol: string;
  exchange: string;
  search: string;
}

const columnHelper = createColumnHelper<Row>();

const numericColumns = new Set(["trade_date", "quantity", "price"]);

export function TransactionsTable({
  transactions,
  instruments,
  onDelete,
  deletingId,
  errorMessage,
}: {
  transactions: Transaction[];
  instruments: Instrument[];
  onDelete: (id: number) => void;
  deletingId: number | null;
  errorMessage: string | null;
}) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [filter, setFilter] = useState("");

  const byId = useMemo(() => {
    const map = new Map<number, Instrument>();
    for (const instrument of instruments) {
      map.set(instrument.id, instrument);
    }
    return map;
  }, [instruments]);

  const rows = useMemo<Row[]>(
    () =>
      transactions.map((transaction) => {
        const instrument = byId.get(transaction.instrument_id);
        const symbol = instrument?.symbol ?? `#${transaction.instrument_id}`;
        const exchange = instrument?.exchange ?? "";
        return {
          transaction,
          symbol,
          exchange,
          search: [
            transaction.trade_date,
            transaction.type,
            symbol,
            exchange,
            instrument?.name ?? "",
            transaction.quantity.toString(),
            transaction.price ?? "",
            transaction.currency ?? "",
            transaction.note ?? "",
          ]
            .join(" ")
            .toLowerCase(),
        };
      }),
    [transactions, byId],
  );

  const columns = useMemo(
    () => [
      columnHelper.accessor((row) => row.transaction.trade_date, {
        id: "trade_date",
        header: "Date",
        cell: (info) => info.getValue(),
      }),
      columnHelper.accessor((row) => row.transaction.type, {
        id: "type",
        header: "Type",
        cell: (info) => <span className="type-chip">{info.getValue()}</span>,
      }),
      columnHelper.accessor((row) => row.symbol, {
        id: "instrument",
        header: "Instrument",
        cell: (info) => (
          <div className="instrument-cell compact">
            <strong>{info.row.original.symbol}</strong>
            <em>{info.row.original.exchange}</em>
          </div>
        ),
      }),
      columnHelper.accessor((row) => row.transaction.quantity, {
        id: "quantity",
        header: "Qty",
        cell: (info) => formatGroupedNumber(info.getValue()),
      }),
      columnHelper.accessor((row) => row.transaction.price ?? "", {
        id: "price",
        header: "Price",
        cell: (info) => {
          const { price, currency } = info.row.original.transaction;
          return price ? (
            <FormattedNumber value={price} prefix={currency ?? ""} />
          ) : (
            "-"
          );
        },
      }),
      columnHelper.display({
        id: "actions",
        header: "",
        cell: (info) => {
          const id = info.row.original.transaction.id;
          return (
            <button
              type="button"
              className="button outline danger"
              onClick={() => onDelete(id)}
              disabled={deletingId === id}
            >
              Delete
            </button>
          );
        },
      }),
    ],
    [deletingId, onDelete],
  );

  const table = useReactTable({
    data: rows,
    columns,
    state: { sorting, globalFilter: filter },
    onSortingChange: setSorting,
    onGlobalFilterChange: setFilter,
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
          placeholder="Filter transactions"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
        {errorMessage ? (
          <p className="form-error table-error">{errorMessage}</p>
        ) : null}
      </div>
      <div className="table-wrap">
        <table>
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  if (header.column.id === "actions") {
                    return <th key={header.id} />;
                  }

                  if (!header.column.getCanSort()) {
                    return (
                      <th key={header.id}>
                        {flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                      </th>
                    );
                  }

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
                {row.getVisibleCells().map((cell) => {
                  if (cell.column.id === "actions") {
                    return (
                      <td key={cell.id}>
                        {flexRender(
                          cell.column.columnDef.cell,
                          cell.getContext(),
                        )}
                      </td>
                    );
                  }

                  return (
                    <td
                      key={cell.id}
                      className={
                        numericColumns.has(cell.column.id)
                          ? "number"
                          : undefined
                      }
                    >
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext(),
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
