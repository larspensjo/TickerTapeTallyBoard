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
import type { AvailabilityValue, GainsRow, GainsTotals } from "../api/types";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  formatGroupedNumber,
  freshnessLabel,
  reasonLabel,
  reasonSummary,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

interface RowView {
  gain: GainsRow;
  search: string;
}

const columnHelper = createColumnHelper<RowView>();

const numericColumns = new Set([
  "cost_basis_base",
  "market_value_base",
  "unrealized_gain_base",
  "price_effect_base",
  "fx_effect_base",
  "day_change_base",
]);

function stackedMetricCell(
  value: AvailabilityValue<string>,
  percent: AvailabilityValue<string>,
) {
  return (
    <div className="metric-stack">
      <AvailabilityValueCell value={value} tone="signed" />
      {value.status === "available" ? (
        <span className="metric-subtle">
          <AvailabilityValueCell value={percent} suffix="%" tone="signed" />
        </span>
      ) : null}
    </div>
  );
}

function stackedHeader(label: string, detail: string) {
  return (
    <span className="column-header-stack">
      <span>{label}</span>
      <span className="column-header-detail">{detail}</span>
    </span>
  );
}

interface LatestStatus {
  label: string;
  title?: string;
  visible: boolean;
}

function staleStatusLabel(kind: "price" | "fx", freshness: string): string {
  const prefix = kind === "price" ? "Stale price" : "Stale FX";

  if (
    freshness.startsWith("minor_stale_") ||
    freshness.startsWith("warning_stale_")
  ) {
    return prefix;
  }

  return freshnessLabel(freshness);
}

function latestStatus(row: GainsRow): LatestStatus {
  const reasonCodes = row.reasons.map(reasonLabel);

  if (reasonCodes.length > 0) {
    return {
      label: reasonCodes[0],
      title: reasonSummary(row.reasons),
      visible: true,
    };
  }

  if (row.position_status === "closed") {
    return {
      label: "Closed",
      title: "Realized gain from a fully closed position",
      visible: true,
    };
  }

  const priceFreshness = row.latest_price?.freshness;
  if (priceFreshness && priceFreshness !== "fresh") {
    return {
      label: staleStatusLabel("price", priceFreshness),
      title: freshnessLabel(priceFreshness),
      visible: true,
    };
  }

  const fxFreshness = row.latest_fx?.freshness;
  if (fxFreshness && fxFreshness !== "fresh") {
    return {
      label: staleStatusLabel("fx", fxFreshness),
      title: freshnessLabel(fxFreshness),
      visible: true,
    };
  }

  return { label: "Fresh", visible: false };
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
  columnHelper.accessor((row) => row.gain.cost_basis_base, {
    id: "cost_basis_base",
    header: () => stackedHeader("Cost basis", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => <AvailabilityValueCell value={info.getValue()} />,
  }),
  columnHelper.accessor((row) => row.gain.market_value_base, {
    id: "market_value_base",
    header: () => stackedHeader("Market value", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => <AvailabilityValueCell value={info.getValue()} />,
  }),
  columnHelper.accessor((row) => row.gain.unrealized_gain_base, {
    id: "unrealized_gain_base",
    header: () => stackedHeader("Total gain", "SEK + %"),
    sortingFn: availabilitySortRows,
    cell: (info) =>
      stackedMetricCell(
        info.getValue(),
        info.row.original.gain.unrealized_gain_percent,
      ),
  }),
  columnHelper.accessor((row) => row.gain.price_effect_base, {
    id: "price_effect_base",
    header: () => stackedHeader("Price effect", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} tone="signed" />
    ),
  }),
  columnHelper.accessor((row) => row.gain.fx_effect_base, {
    id: "fx_effect_base",
    header: () => stackedHeader("FX effect", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} tone="signed" />
    ),
  }),
  columnHelper.accessor((row) => row.gain.day_change_base, {
    id: "day_change_base",
    header: () => stackedHeader("Today", "SEK + %"),
    sortingFn: availabilitySortRows,
    cell: (info) =>
      stackedMetricCell(
        info.getValue(),
        info.row.original.gain.day_change_percent,
      ),
  }),
  columnHelper.display({
    id: "status",
    header: "Status",
    cell: (info) => {
      const { gain } = info.row.original;
      const status = latestStatus(gain);

      if (!status.visible) {
        return <span className="status-empty" title={status.label} />;
      }

      return (
        <span
          className={
            gain.position_status === "closed" && gain.reasons.length === 0
              ? "status-chip compact"
              : "status-chip compact warning"
          }
          title={status.title}
        >
          {status.label}
        </span>
      );
    },
  }),
];

export function GainsTable({
  rows,
  totals,
  filter,
  onFilterChange,
  includeClosedPositions,
  onIncludeClosedPositionsChange,
}: {
  rows: GainsRow[];
  totals?: GainsTotals;
  filter: string;
  onFilterChange: (filter: string) => void;
  includeClosedPositions: boolean;
  onIncludeClosedPositionsChange: (includeClosedPositions: boolean) => void;
}) {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "unrealized_gain_base", desc: true },
  ]);

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
            gain.position_status,
            gain.cost_basis_base.status === "available"
              ? gain.cost_basis_base.value
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
            gain.price_effect_base.status === "available"
              ? gain.price_effect_base.value
              : "",
            gain.fx_effect_base.status === "available"
              ? gain.fx_effect_base.value
              : "",
            gain.day_change_base.status === "available"
              ? gain.day_change_base.value
              : "",
            gain.day_change_percent.status === "available"
              ? gain.day_change_percent.value
              : "",
            gain.position_status === "closed" ? "closed realized" : "open",
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
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    getFilteredRowModel: getFilteredRowModel(),
    globalFilterFn: (row, _columnId, filterValue) =>
      row.original.search.includes(String(filterValue).trim().toLowerCase()),
  });

  return (
    <>
      {totals ? <GainsTotalsBand totals={totals} /> : null}
      <div className="table-toolbar">
        <input
          className="filter-input"
          type="search"
          placeholder="Filter instrument"
          value={filter}
          onChange={(event) => onFilterChange(event.target.value)}
        />
        <label className="toolbar-check">
          <input
            type="checkbox"
            checked={includeClosedPositions}
            onChange={(event) =>
              onIncludeClosedPositionsChange(event.target.checked)
            }
          />
          <span>Include closed positions</span>
        </label>
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

function GainsTotalsBand({ totals }: { totals: GainsTotals }) {
  return (
    <section className="gains-totals" aria-label="Gains totals">
      <GainsTotalMetric
        label="Capital gain"
        percent={totals.capital_gain_percent}
        amount={totals.capital_gain_base}
      />
      <GainsTotalMetric
        label="Income"
        percent={totals.income_percent}
        amount={totals.income_base}
        unavailableLabel="Not tracked"
      />
      <GainsTotalMetric
        label="Currency gain"
        percent={totals.currency_gain_percent}
        amount={totals.currency_gain_base}
      />
      <GainsTotalMetric
        label="Total return"
        percent={totals.total_return_percent}
        amount={totals.total_return_base}
      />
      {totals.excluded_rows > 0 ? (
        <span className="status-chip warning gains-totals-warning">
          {formatGroupedNumber(totals.excluded_rows)} incomplete
        </span>
      ) : null}
    </section>
  );
}

function GainsTotalMetric({
  label,
  percent,
  amount,
  unavailableLabel = "Unavailable",
}: {
  label: string;
  percent: AvailabilityValue<string>;
  amount: AvailabilityValue<string>;
  unavailableLabel?: string;
}) {
  return (
    <div className="gains-total-metric">
      <span className="gains-total-label">{label}</span>
      <span className="gains-total-percent">
        <SummaryAvailabilityValue
          value={percent}
          suffix="%"
          tone="plain"
          unavailableLabel={unavailableLabel}
        />
      </span>
      <span className="gains-total-amount">
        <SummaryAvailabilityValue
          value={amount}
          prefix="SEK "
          tone="plain"
          unavailableLabel={unavailableLabel}
        />
      </span>
    </div>
  );
}
