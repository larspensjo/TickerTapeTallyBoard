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
import { useMemo } from "react";
import type {
  AvailabilityValue,
  DateRange,
  GainsRow,
  GainsTotals,
  ReturnMethod,
} from "../api/types";
import { type DatePreset, DateRangeSelector } from "./DateRangeSelector";
import { InstrumentCell } from "./InstrumentCell";
import { usePersistentSorting } from "./persistence";
import {
  AvailabilityValueCell,
  availabilitySortRows,
  formatGroupedNumber,
  freshnessLabel,
  parseFiniteNumber,
  reasonLabel,
  reasonSummary,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

const RETURN_METHOD_KEY = "gains.returnMethod";

const GAINS_SORTING_KEY = "gains.sorting";
const GAINS_DEFAULT_SORTING: SortingState = [
  { id: "total_return_base", desc: true },
];
const GAINS_SORTABLE_IDS = new Set([
  "instrument",
  "cost_basis_base",
  "market_value_base",
  "total_return_base",
  "capital_gain_base",
  "income_base",
  "currency_gain_base",
  "day_change_base",
]);

export function loadReturnMethod(): ReturnMethod {
  const v = localStorage.getItem(RETURN_METHOD_KEY);
  return v === "simple" || v === "modified_dietz" ? v : "xirr";
}

function saveReturnMethod(m: ReturnMethod) {
  localStorage.setItem(RETURN_METHOD_KEY, m);
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
  totalReturnBase: SummaryValue;
  capitalGainBase: SummaryValue;
  incomeBase: SummaryValue;
  currencyGainBase: SummaryValue;
  dayChangeBase: SummaryValue;
  dayChangePercent: SummaryValue;
  incompleteRows: number;
}

const columnHelper = createColumnHelper<RowView>();

const numericColumns = new Set([
  "cost_basis_base",
  "market_value_base",
  "total_return_base",
  "capital_gain_base",
  "income_base",
  "currency_gain_base",
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
  // performance_denominator_base is now cost_basis_base (not an additive Modified Dietz denominator),
  // so row-level denominators cannot be summed to derive a meaningful subtotal percentage.
  let dayChangeBase = 0;
  let dayChangePreviousMarketValue = 0;
  let dayChangePercentRows = 0;
  let incompleteRows = 0;

  for (const { gain } of rows) {
    const values = [
      gain.cost_basis_base,
      gain.market_value_base,
      gain.total_return_base,
      gain.capital_gain_base,
      gain.income_base,
      gain.currency_gain_base,
      gain.day_change_base,
    ];

    if (values.some((value) => value.status === "unavailable")) {
      incompleteRows += 1;
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
    rows.map(({ gain }) => gain.total_return_base),
  );
  const dayChangeBaseSummary = summarizeMoneyValues(
    rows.map(({ gain }) => gain.day_change_base),
  );

  return {
    costBasisBase,
    marketValueBase,
    totalReturnBase: unrealizedGainBaseSummary,
    capitalGainBase: summarizeMoneyValues(
      rows.map(({ gain }) => gain.capital_gain_base),
    ),
    incomeBase: summarizeMoneyValues(rows.map(({ gain }) => gain.income_base)),
    currencyGainBase: summarizeMoneyValues(
      rows.map(({ gain }) => gain.currency_gain_base),
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
  columnHelper.accessor((row) => row.gain.total_return_base, {
    id: "total_return_base",
    header: () => stackedHeader("Total gain", "SEK + %"),
    sortingFn: availabilitySortRows,
    cell: (info) =>
      stackedMetricCell(
        info.getValue(),
        info.row.original.gain.total_return_percent,
      ),
  }),
  columnHelper.accessor((row) => row.gain.capital_gain_base, {
    id: "capital_gain_base",
    header: () => stackedHeader("Capital gain", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} tone="signed" />
    ),
  }),
  columnHelper.accessor((row) => row.gain.income_base, {
    id: "income_base",
    header: () => stackedHeader("Income", "SEK"),
    sortingFn: availabilitySortRows,
    cell: (info) => (
      <AvailabilityValueCell value={info.getValue()} tone="signed" />
    ),
  }),
  columnHelper.accessor((row) => row.gain.currency_gain_base, {
    id: "currency_gain_base",
    header: () => stackedHeader("Currency gain", "SEK"),
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
  percentageMethod,
  filter,
  onFilterChange,
  includeClosedPositions,
  onIncludeClosedPositionsChange,
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
  displayPercentKind = "absolute",
  returnMethod,
  onReturnMethodChange,
}: {
  rows: GainsRow[];
  totals?: GainsTotals;
  percentageMethod?: "money_weighted" | "simple" | "modified_dietz";
  filter: string;
  onFilterChange: (filter: string) => void;
  includeClosedPositions: boolean;
  onIncludeClosedPositionsChange: (includeClosedPositions: boolean) => void;
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (preset: DatePreset) => void;
  onDateRangeChange: (range: DateRange) => void;
  displayPercentKind?: string;
  returnMethod: ReturnMethod;
  onReturnMethodChange: (method: ReturnMethod) => void;
}) {
  const [sorting, handleSortingChange] = usePersistentSorting(
    GAINS_SORTING_KEY,
    GAINS_SORTABLE_IDS,
    GAINS_DEFAULT_SORTING,
  );

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
            gain.total_return_base.status === "available"
              ? gain.total_return_base.value
              : "",
            gain.total_return_percent.status === "available"
              ? gain.total_return_percent.value
              : "",
            gain.capital_gain_base.status === "available"
              ? gain.capital_gain_base.value
              : "",
            gain.income_base.status === "available"
              ? gain.income_base.value
              : "",
            gain.currency_gain_base.status === "available"
              ? gain.currency_gain_base.value
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
    onSortingChange: handleSortingChange,
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
          percentageMethod={percentageMethod}
          displayPercentKind={displayPercentKind}
        />
      ) : null}
      <div className="table-toolbar">
        <DateRangeSelector
          dateRange={dateRange}
          selectedDatePreset={selectedDatePreset}
          onDatePresetChange={onDatePresetChange}
          onDateRangeChange={onDateRangeChange}
          ariaLabel="Gains date range"
        />
        <select
          className="method-select"
          value={returnMethod}
          onChange={(e) => {
            const method = e.target.value as ReturnMethod;
            saveReturnMethod(method);
            onReturnMethodChange(method);
          }}
          aria-label="Return method"
        >
          <option value="xirr">Money-weighted (XIRR)</option>
          <option value="simple">Simple</option>
          <option value="modified_dietz">Modified Dietz (legacy)</option>
        </select>
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
              <td className="number">
                {/* SEK sum only — a method-specific return % is not additive across rows;
                    the whole-portfolio percentage lives in the totals band above. */}
                {plainSummaryCell(summary.totalReturnBase, true)}
              </td>
              <td className="number">
                {plainSummaryCell(summary.capitalGainBase, true)}
              </td>
              <td className="number">
                {plainSummaryCell(summary.incomeBase, true)}
              </td>
              <td className="number">
                {plainSummaryCell(summary.currencyGainBase, true)}
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

export function totalReturnLabel(
  percentageMethod: "money_weighted" | "simple" | "modified_dietz" | undefined,
  displayPercentKind: string,
): { label: string; title?: string; note?: string } {
  if (percentageMethod === "money_weighted") {
    return {
      label: "Total return",
      title: "Total return including closed positions (money-weighted)",
    };
  }
  if (percentageMethod === "simple") {
    return {
      label: "Total return",
      title: "Total return (simple)",
    };
  }
  if (percentageMethod === "modified_dietz") {
    return {
      label: "Total return",
      title:
        displayPercentKind === "annualised"
          ? "Annualised return (Modified Dietz legacy)"
          : "Performance return (Modified Dietz legacy)",
      note: "legacy / comparison only",
    };
  }
  return { label: "Total return", title: "Performance return" };
}

function GainsTotalsBand({
  totals,
  percentageMethod,
  displayPercentKind,
}: {
  totals: GainsTotals;
  percentageMethod?: "money_weighted" | "simple" | "modified_dietz";
  displayPercentKind: string;
}) {
  const componentTitle =
    percentageMethod === "modified_dietz" && displayPercentKind === "annualised"
      ? "Holding-period percentage; total return is annualised."
      : undefined;
  const { title } = totalReturnLabel(percentageMethod, displayPercentKind);
  return (
    <section className="gains-totals" aria-label="Gains totals">
      <div className="gains-totals-row">
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
            title={title}
          />
        </div>
        {totals.excluded_rows > 0 ? (
          <span className="status-chip warning compact gains-totals-warning">
            {formatGroupedNumber(totals.excluded_rows)} incomplete
          </span>
        ) : null}
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
