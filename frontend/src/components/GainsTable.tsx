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
import type {
  AvailabilityValue,
  DateRange,
  GainsRow,
  GainsTotals,
} from "../api/types";
import { InstrumentCell } from "./InstrumentCell";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  formatGroupedNumber,
  freshnessLabel,
  reasonLabel,
  reasonSummary,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

export type DatePreset = "today" | "7d" | "12m" | "ytd" | "all" | "custom";

const PRESETS: DatePreset[] = ["today", "7d", "12m", "ytd", "all", "custom"];

const PRESET_LABELS: Record<DatePreset, string> = {
  today: "Today",
  "7d": "7D",
  "12m": "12M",
  ytd: "YTD",
  all: "All",
  custom: "Custom",
};

function localDateString(d: Date): string {
  return d.toLocaleDateString("sv-SE");
}

function presetToRange(
  preset: DatePreset,
  customStart: string,
  customEnd: string,
): DateRange {
  const today = new Date();
  const fmt = localDateString;

  switch (preset) {
    case "today":
      return { startDate: fmt(today), endDate: fmt(today) };
    case "7d": {
      const start = new Date(today);
      start.setDate(start.getDate() - 7);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "12m": {
      const start = new Date(today);
      start.setFullYear(start.getFullYear() - 1);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "ytd":
      return { startDate: `${today.getFullYear()}-01-01`, endDate: fmt(today) };
    case "all":
      return { startDate: null, endDate: fmt(today) };
    case "custom":
      return {
        startDate: customStart || null,
        endDate: customEnd || fmt(today),
      };
  }
}

interface RowView {
  gain: GainsRow;
  search: string;
}

interface SummaryValue {
  value: AvailabilityValue<string>;
  excludedRows: number;
}

interface GainsColumnSummary {
  costBasisBase: SummaryValue;
  marketValueBase: SummaryValue;
  unrealizedGainBase: SummaryValue;
  unrealizedGainPercent: SummaryValue;
  priceEffectBase: SummaryValue;
  fxEffectBase: SummaryValue;
  dayChangeBase: SummaryValue;
  dayChangePercent: SummaryValue;
  incompleteRows: number;
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

function parseFiniteNumber(value: string): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function availableNumber(value: AvailabilityValue<string>): number | null {
  if (value.status === "unavailable") {
    return null;
  }

  return parseFiniteNumber(value.value);
}

function moneyValue(value: number): AvailabilityValue<string> {
  return { status: "available", value: value.toFixed(2) };
}

function percentValue(value: number): AvailabilityValue<string> {
  return { status: "available", value: value.toFixed(2) };
}

function summarizeMoneyValues(values: Array<AvailabilityValue<string>>) {
  let total = 0;
  let availableRows = 0;

  for (const value of values) {
    const numberValue = availableNumber(value);
    if (numberValue !== null) {
      total += numberValue;
      availableRows += 1;
    }
  }

  return {
    value:
      availableRows > 0 ? moneyValue(total) : unavailableValue("unavailable"),
    excludedRows: values.length - availableRows,
  };
}

function unavailableValue(reason: string): AvailabilityValue<string> {
  return { status: "unavailable", reasons: [reason] };
}

function summaryTitle(excludedRows: number): string | undefined {
  return excludedRows > 0
    ? `${formatGroupedNumber(excludedRows)} rows excluded from this summary`
    : undefined;
}

function metricSummaryCell(
  amount: SummaryValue,
  percent: SummaryValue,
  unavailableLabel = "Unavailable",
) {
  return (
    <div className="metric-stack" title={summaryTitle(amount.excludedRows)}>
      <AvailabilityValueCell
        value={amount.value}
        tone="signed"
        unavailableLabel={unavailableLabel}
      />
      {amount.value.status === "available" ? (
        <span
          className="metric-subtle"
          title={summaryTitle(percent.excludedRows)}
        >
          <AvailabilityValueCell
            value={percent.value}
            suffix="%"
            tone="signed"
            unavailableLabel={unavailableLabel}
          />
        </span>
      ) : null}
    </div>
  );
}

function plainSummaryCell(summary: SummaryValue, signed = false) {
  return (
    <span title={summaryTitle(summary.excludedRows)}>
      <AvailabilityValueCell
        value={summary.value}
        tone={signed ? "signed" : "plain"}
      />
    </span>
  );
}

function computeGainsColumnSummary(rows: RowView[]): GainsColumnSummary {
  let unrealizedGainBase = 0;
  let unrealizedGainCostBasis = 0;
  let unrealizedPercentRows = 0;
  let dayChangeBase = 0;
  let dayChangePreviousMarketValue = 0;
  let dayChangePercentRows = 0;
  let incompleteRows = 0;

  for (const { gain } of rows) {
    const values = [
      gain.cost_basis_base,
      gain.market_value_base,
      gain.unrealized_gain_base,
      gain.price_effect_base,
      gain.fx_effect_base,
      gain.day_change_base,
    ];

    if (
      values.some((value) => value.status === "unavailable") ||
      gain.reasons.length > 0
    ) {
      incompleteRows += 1;
    }

    const currentUnrealizedGain = availableNumber(gain.unrealized_gain_base);
    const currentCostBasis = availableNumber(gain.cost_basis_base);
    if (currentUnrealizedGain !== null && currentCostBasis !== null) {
      unrealizedGainBase += currentUnrealizedGain;
      unrealizedGainCostBasis += currentCostBasis;
      unrealizedPercentRows += 1;
    }

    const currentDayChange = availableNumber(gain.day_change_base);
    const currentMarketValue = availableNumber(gain.market_value_base);
    if (currentDayChange !== null && currentMarketValue !== null) {
      const previousMarketValue = currentMarketValue - currentDayChange;
      dayChangeBase += currentDayChange;
      dayChangePreviousMarketValue += previousMarketValue;
      dayChangePercentRows += 1;
    }
  }

  const costBasisBase = summarizeMoneyValues(
    rows.map(({ gain }) => gain.cost_basis_base),
  );
  const marketValueBase = summarizeMoneyValues(
    rows.map(({ gain }) => gain.market_value_base),
  );
  const unrealizedGainBaseSummary = summarizeMoneyValues(
    rows.map(({ gain }) => gain.unrealized_gain_base),
  );
  const dayChangeBaseSummary = summarizeMoneyValues(
    rows.map(({ gain }) => gain.day_change_base),
  );

  return {
    costBasisBase,
    marketValueBase,
    unrealizedGainBase: unrealizedGainBaseSummary,
    unrealizedGainPercent: {
      value:
        unrealizedPercentRows > 0 && unrealizedGainCostBasis !== 0
          ? percentValue((unrealizedGainBase / unrealizedGainCostBasis) * 100)
          : unavailableValue("base_cost_basis_unavailable"),
      excludedRows: rows.length - unrealizedPercentRows,
    },
    priceEffectBase: summarizeMoneyValues(
      rows.map(({ gain }) => gain.price_effect_base),
    ),
    fxEffectBase: summarizeMoneyValues(
      rows.map(({ gain }) => gain.fx_effect_base),
    ),
    dayChangeBase: dayChangeBaseSummary,
    dayChangePercent: {
      value:
        dayChangePercentRows > 0 && dayChangePreviousMarketValue !== 0
          ? percentValue((dayChangeBase / dayChangePreviousMarketValue) * 100)
          : unavailableValue("zero_previous_market_value"),
      excludedRows: rows.length - dayChangePercentRows,
    },
    incompleteRows,
  };
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
  columnHelper.accessor((row) => row.gain.instrument.name, {
    id: "instrument",
    header: "Instrument",
    cell: (info) => {
      const { id, symbol, name, exchange } = info.row.original.gain.instrument;
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
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
  displayPercentKind = "absolute",
}: {
  rows: GainsRow[];
  totals?: GainsTotals;
  filter: string;
  onFilterChange: (filter: string) => void;
  includeClosedPositions: boolean;
  onIncludeClosedPositionsChange: (includeClosedPositions: boolean) => void;
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (preset: DatePreset) => void;
  onDateRangeChange: (range: DateRange) => void;
  displayPercentKind?: string;
}) {
  const [sorting, setSorting] = useState<SortingState>([
    { id: "unrealized_gain_base", desc: true },
  ]);
  const [customStart, setCustomStart] = useState(dateRange.startDate ?? "");
  const [customEnd, setCustomEnd] = useState(dateRange.endDate ?? "");

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
  const visibleRows = table.getRowModel().rows.map((row) => row.original);
  const summary = computeGainsColumnSummary(visibleRows);

  return (
    <>
      {totals ? (
        <GainsTotalsBand
          totals={totals}
          displayPercentKind={displayPercentKind}
        />
      ) : null}
      <div className="table-toolbar">
        <div className="date-range-presets">
          {PRESETS.map((p) => (
            <button
              key={p}
              type="button"
              className={`preset-btn${
                selectedDatePreset === p ? " active" : ""
              }`}
              aria-pressed={selectedDatePreset === p}
              onClick={() => {
                onDatePresetChange(p);
                if (p !== "custom") {
                  onDateRangeChange(presetToRange(p, customStart, customEnd));
                }
              }}
            >
              {PRESET_LABELS[p]}
            </button>
          ))}
          {selectedDatePreset === "custom" && (
            <>
              <input
                className="date-range-input"
                type="date"
                value={customStart}
                onChange={(e) => {
                  setCustomStart(e.target.value);
                  onDateRangeChange(
                    presetToRange("custom", e.target.value, customEnd),
                  );
                }}
              />
              <input
                className="date-range-input"
                type="date"
                value={customEnd}
                onChange={(e) => {
                  setCustomEnd(e.target.value);
                  onDateRangeChange(
                    presetToRange("custom", customStart, e.target.value),
                  );
                }}
              />
            </>
          )}
        </div>
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
          <tfoot>
            <tr>
              <th scope="row">Total</th>
              <td className="number">
                {plainSummaryCell(summary.costBasisBase)}
              </td>
              <td className="number">
                {plainSummaryCell(summary.marketValueBase)}
              </td>
              <td>
                {metricSummaryCell(
                  summary.unrealizedGainBase,
                  summary.unrealizedGainPercent,
                )}
              </td>
              <td className="number">
                {plainSummaryCell(summary.priceEffectBase, true)}
              </td>
              <td className="number">
                {plainSummaryCell(summary.fxEffectBase, true)}
              </td>
              <td>
                {metricSummaryCell(
                  summary.dayChangeBase,
                  summary.dayChangePercent,
                )}
              </td>
              <td>
                {summary.incompleteRows > 0 ? (
                  <span
                    className="status-chip compact warning"
                    title="Rows with unavailable values or valuation warnings"
                  >
                    {formatGroupedNumber(summary.incompleteRows)} incomplete
                  </span>
                ) : (
                  <span className="status-empty" title="All summarized" />
                )}
              </td>
            </tr>
          </tfoot>
        </table>
      </div>
    </>
  );
}

function GainsTotalsBand({
  totals,
  displayPercentKind,
}: {
  totals: GainsTotals;
  displayPercentKind: string;
}) {
  const label =
    displayPercentKind === "annualised"
      ? "Annualised return"
      : "Performance return";
  const componentTitle =
    displayPercentKind === "annualised"
      ? "Holding-period percentage; total return is annualised."
      : undefined;
  return (
    <section className="gains-totals" aria-label="Gains totals">
      <div className="gains-totals-header">
        <span className="gains-totals-method">{label}</span>
        {totals.excluded_rows > 0 ? (
          <span className="status-chip warning gains-totals-warning">
            {formatGroupedNumber(totals.excluded_rows)} incomplete
          </span>
        ) : null}
      </div>
      <div className="gains-totals-grid">
        <GainsTotalMetric
          label="Capital gain"
          percent={totals.capital_gain_percent}
          amount={totals.capital_gain_base}
          title={componentTitle}
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
          title={componentTitle}
        />
        <GainsTotalMetric
          label="Total return"
          percent={totals.total_return_percent}
          amount={totals.total_return_base}
        />
      </div>
    </section>
  );
}

function GainsTotalMetric({
  label,
  percent,
  amount,
  unavailableLabel = "Unavailable",
  title,
}: {
  label: string;
  percent: AvailabilityValue<string>;
  amount: AvailabilityValue<string>;
  unavailableLabel?: string;
  title?: string;
}) {
  return (
    <div className="gains-total-metric">
      <span className="gains-total-label" title={title}>
        {label}
      </span>
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
