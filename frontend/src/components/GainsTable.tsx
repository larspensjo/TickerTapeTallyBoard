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
import type { GainsRow } from "../api/types";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  freshnessLabel,
  freshnessTone,
  reasonLabel,
  reasonSummary,
} from "./valuationDisplay";

interface RowView {
  gain: GainsRow;
  search: string;
}

const columnHelper = createColumnHelper<RowView>();

const numericColumns = new Set([
  "quantity",
  "cost_basis_native",
  "cost_basis_base",
  "market_value_native",
  "market_value_base",
  "unrealized_gain_base",
  "unrealized_gain_percent",
  "day_change_base",
  "day_change_percent",
]);

function snapshotStatus(freshness: string): string {
  if (freshness === "fresh") {
    return "Fresh";
  }

  return freshnessLabel(freshness);
}

function snapshotIsWarning(freshness: string): boolean {
  return freshnessTone(freshness) === "warning";
}

function latestStatusLabel(row: GainsRow): string {
  const reasonCodes = row.reasons.map(reasonLabel);

  if (reasonCodes.length > 0) {
    return reasonCodes[0];
  }

  const priceFreshness = row.latest_price?.freshness;
  if (priceFreshness && priceFreshness !== "fresh") {
    return snapshotStatus(priceFreshness);
  }

  const fxFreshness = row.latest_fx?.freshness;
  if (fxFreshness && fxFreshness !== "fresh") {
    return snapshotStatus(fxFreshness);
  }

  return "Fresh";
}

const columns = [
  columnHelper.accessor((row) => row.gain.instrument.symbol, {
    id: "instrument",
    header: "Instrument",
    cell: (info) => {
      const { symbol, name, exchange } = info.row.original.gain.instrument;
      return (
        <div className="instrument-cell">
          <strong>{symbol}</strong>
          <span>{name}</span>
          <em>{exchange}</em>
        </div>
      );
    },
  }),
  columnHelper.accessor((row) => row.gain.quantity, {
    id: "quantity",
    header: "Qty",
    cell: (info) => info.getValue(),
  }),
  columnHelper.accessor((row) => row.gain.latest_price, {
    id: "latest_price",
    header: "Latest close",
    cell: (info) => {
      const snapshot = info.getValue();
      if (!snapshot) {
        return (
          <span className="status-chip warning" title="Missing price">
            Missing price
          </span>
        );
      }

      return (
        <div className="metric-stack">
          <span className="number">
            {snapshot.currency} {snapshot.close}
          </span>
          <span className="metric-subtle">{snapshot.date}</span>
          {snapshotIsWarning(snapshot.freshness) ? (
            <span className="status-chip warning">
              {snapshotStatus(snapshot.freshness)}
            </span>
          ) : null}
        </div>
      );
    },
  }),
  columnHelper.accessor((row) => row.gain.market_value_native, {
    id: "market_value_native",
    header: "Market value",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell
        value={info.getValue()}
        prefix={`${info.row.original.gain.instrument.currency} `}
      />
    ),
  }),
  columnHelper.accessor((row) => row.gain.cost_basis_base, {
    id: "cost_basis_base",
    header: "Cost basis (SEK)",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} prefix="SEK " />
    ),
  }),
  columnHelper.accessor((row) => row.gain.market_value_base, {
    id: "market_value_base",
    header: "Market value (SEK)",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} prefix="SEK " />
    ),
  }),
  columnHelper.accessor((row) => row.gain.unrealized_gain_base, {
    id: "unrealized_gain_base",
    header: "Unrealized gain",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell
        value={info.getValue()}
        prefix="SEK "
        tone="signed"
      />
    ),
  }),
  columnHelper.accessor((row) => row.gain.unrealized_gain_percent, {
    id: "unrealized_gain_percent",
    header: "Gain %",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} suffix="%" tone="signed" />
    ),
  }),
  columnHelper.accessor((row) => row.gain.day_change_base, {
    id: "day_change_base",
    header: "Day change",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell
        value={info.getValue()}
        prefix="SEK "
        tone="signed"
      />
    ),
  }),
  columnHelper.accessor((row) => row.gain.day_change_percent, {
    id: "day_change_percent",
    header: "Day %",
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} suffix="%" tone="signed" />
    ),
  }),
  columnHelper.display({
    id: "status",
    header: "Status",
    cell: (info) => {
      const { gain } = info.row.original;
      const status = latestStatusLabel(gain);
      const title =
        gain.reasons.length > 0 ? reasonSummary(gain.reasons) : undefined;

      return (
        <span
          className={status === "Fresh" ? "status-chip" : "status-chip warning"}
          title={title}
        >
          {status}
        </span>
      );
    },
  }),
];

export function GainsTable({ rows }: { rows: GainsRow[] }) {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "unrealized_gain_base", desc: true },
  ]);
  const [filter, setFilter] = useState("");

  const tableRows = useMemo<RowView[]>(
    () =>
      rows.map((gain) => {
        const freshnessParts = [
          gain.latest_price?.freshness,
          gain.latest_fx?.freshness,
        ]
          .filter((part): part is string => Boolean(part))
          .map(freshnessLabel);

        return {
          gain,
          search: [
            gain.instrument.symbol,
            gain.instrument.name,
            gain.instrument.exchange,
            gain.instrument.currency,
            gain.quantity.toString(),
            gain.cost_basis_native,
            gain.cost_basis_base.status === "available"
              ? gain.cost_basis_base.value
              : "",
            gain.latest_price?.date ?? "",
            gain.latest_price?.currency ?? "",
            gain.latest_price?.close ?? "",
            gain.latest_fx?.date ?? "",
            gain.latest_fx?.base ?? "",
            gain.latest_fx?.quote ?? "",
            gain.latest_fx?.rate ?? "",
            gain.market_value_native.status === "available"
              ? gain.market_value_native.value
              : "",
            gain.market_value_base.status === "available"
              ? gain.market_value_base.value
              : "",
            gain.unrealized_gain_base.status === "available"
              ? gain.unrealized_gain_base.value
              : "",
            gain.unrealized_gain_percent.status === "available"
              ? gain.unrealized_gain_percent.value
              : "",
            gain.day_change_base.status === "available"
              ? gain.day_change_base.value
              : "",
            gain.day_change_percent.status === "available"
              ? gain.day_change_percent.value
              : "",
            ...gain.reasons.map(reasonLabel),
            ...freshnessParts,
          ]
            .join(" ")
            .toLowerCase(),
        };
      }),
    [rows],
  );

  const table = useReactTable({
    data: tableRows,
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
          placeholder="Filter gains"
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
        />
      </div>
      <div className="table-wrap gains-table">
        <table>
          <thead>
            {table.getHeaderGroups().map((headerGroup) => (
              <tr key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
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
