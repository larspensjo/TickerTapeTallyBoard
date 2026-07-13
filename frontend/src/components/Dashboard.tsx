import { useMemo } from "react";
import { Link } from "react-router-dom";
import { useGains, usePortfolioValueHistory } from "../api/queries";
import type { DateRange, GainsRow } from "../api/types";
import { type DatePreset, DateRangeSelector } from "./DateRangeSelector";
import {
  type AllocationDimension,
  allocationBreakdown,
  type MoverRow,
  topMovers,
} from "./dashboardSelectors";
import { PortfolioTreemap } from "./PortfolioTreemap";
import { isOneOf, usePersistentSetting } from "./persistence";
import {
  filterValueHistoryPoints,
  portfolioValueSeries,
} from "./portfolioValueViewModel";
import { TimeSeriesChart } from "./TimeSeriesChart";
import { formatGroupedNumber } from "./valuationDisplay";

export interface DashboardProps {
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
}

export function Dashboard({
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
}: DashboardProps) {
  const gainsQuery = useGains({
    startDate: dateRange.startDate,
    endDate: dateRange.endDate ?? undefined,
  });
  const valueHistory = usePortfolioValueHistory();

  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <DashboardChartPanel
        query={valueHistory}
        gainsQuery={gainsQuery}
        dateRange={dateRange}
        selectedDatePreset={selectedDatePreset}
        onDatePresetChange={onDatePresetChange}
        onDateRangeChange={onDateRangeChange}
      />
      <TopMoversPanel rows={gainsQuery.data?.rows ?? []} />
      <AllocationPanel rows={gainsQuery.data?.rows ?? []} />
    </section>
  );
}

type ChartView = "value" | "gain" | "treemap";

const CHART_VIEW_KEY = "dashboard.chartView";
const CHART_VIEWS: ChartView[] = ["value", "gain", "treemap"];
const isChartView = isOneOf(CHART_VIEWS);

const ALLOCATION_DIMENSION_KEY = "dashboard.allocationDimension";
const ALLOCATION_DIMENSIONS: AllocationDimension[] = [
  "instrument",
  "currency",
  "type",
];
const isAllocationDimension = isOneOf(ALLOCATION_DIMENSIONS);

function DashboardChartPanel({
  query,
  gainsQuery,
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
  gainsQuery: ReturnType<typeof useGains>;
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
}) {
  const history = query.data?.points;
  const filteredHistory = useMemo(
    () => filterValueHistoryPoints(history ?? [], dateRange),
    [history, dateRange],
  );
  const series = useMemo(
    () => portfolioValueSeries(filteredHistory),
    [filteredHistory],
  );
  const incompleteDays = useMemo(
    () => filteredHistory.filter((point) => point.incomplete).length,
    [filteredHistory],
  );
  const [view, setView] = usePersistentSetting<ChartView>(
    CHART_VIEW_KEY,
    isChartView,
    "value",
  );

  const isGain = view === "gain";
  const chartControls = (
    <div className="chart-controls">
      <DateRangeSelector
        dateRange={dateRange}
        selectedDatePreset={selectedDatePreset}
        onDatePresetChange={onDatePresetChange}
        onDateRangeChange={onDateRangeChange}
        ariaLabel="Dashboard date range"
      />
      <fieldset className="segmented-control">
        <legend className="sr-only">Chart view</legend>
        {CHART_VIEWS.map((v) => (
          <button
            key={v}
            type="button"
            className={view === v ? "active" : undefined}
            aria-pressed={view === v}
            onClick={() => setView(v)}
          >
            {v[0].toUpperCase() + v.slice(1)}
          </button>
        ))}
      </fieldset>
    </div>
  );

  if (view === "treemap") {
    return (
      <section className="panel chart-panel" aria-label="Portfolio map">
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>Portfolio map</h2>
          </div>
          {chartControls}
        </div>
        {gainsQuery.isPending ? (
          <div className="chart-band">
            <div className="skeleton-bar" />
          </div>
        ) : gainsQuery.isError ? (
          <div className="chart-band error">
            <p className="down">Could not load holdings data.</p>
            <button
              type="button"
              className="button outline"
              onClick={() => void gainsQuery.refetch()}
            >
              Retry
            </button>
          </div>
        ) : (
          <PortfolioTreemap rows={gainsQuery.data?.rows ?? []} />
        )}
      </section>
    );
  }

  if (query.isPending) {
    return (
      <section className="panel chart-panel" aria-label="Portfolio value">
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>{isGain ? "Portfolio gain (SEK)" : "Portfolio value (SEK)"}</h2>
          </div>
          {chartControls}
        </div>
        <div className="chart-band">
          <div className="skeleton-bar" />
        </div>
      </section>
    );
  }

  if (query.isError) {
    return (
      <section className="panel chart-panel" aria-label="Portfolio value">
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>{isGain ? "Portfolio gain (SEK)" : "Portfolio value (SEK)"}</h2>
          </div>
          {chartControls}
        </div>
        <div className="chart-band error">
          <p className="down">Could not load portfolio value.</p>
          <button
            type="button"
            className="button outline"
            onClick={() => void query.refetch()}
          >
            Retry
          </button>
        </div>
      </section>
    );
  }

  if (series.value.length === 0) {
    return (
      <section className="panel chart-panel" aria-label="Portfolio value">
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>{isGain ? "Portfolio gain (SEK)" : "Portfolio value (SEK)"}</h2>
          </div>
          {chartControls}
        </div>
        <div className="chart-band muted">
          <span className="chart-band-label">
            No portfolio history in this interval
          </span>
        </div>
      </section>
    );
  }

  return (
    <section className="panel chart-panel" aria-label="Portfolio value">
      <div className="chart-meta">
        <div className="chart-meta-title">
          <h2>{isGain ? "Portfolio gain (SEK)" : "Portfolio value (SEK)"}</h2>
          {incompleteDays > 0 ? (
            <span className="status-chip warning compact">
              {incompleteDays} days had missing inputs
            </span>
          ) : null}
        </div>
        {chartControls}
      </div>
      <div className="chart-legend" aria-hidden="true">
        {isGain ? (
          <span className="chart-legend-item gain">Gain</span>
        ) : (
          <>
            <span className="chart-legend-item value">Value</span>
            <span className="chart-legend-item invested">Invested capital</span>
          </>
        )}
      </div>
      <TimeSeriesChart
        data={isGain ? series.gain : series.value}
        referenceData={isGain ? undefined : series.invested}
        ariaLabel={
          isGain
            ? "Portfolio gain over time in SEK"
            : "Portfolio value over time in SEK, with net invested capital reference line"
        }
        visibleStart={
          dateRange.startDate ?? query.data?.start_date ?? undefined
        }
        height={280}
        lineColor={isGain ? "#16c784" : undefined}
        topColor={isGain ? "rgba(22, 199, 132, 0.30)" : undefined}
        bottomColor={isGain ? "rgba(22, 199, 132, 0.02)" : undefined}
      />
    </section>
  );
}

function TopMoversPanel({ rows }: { rows: GainsRow[] }) {
  const { gainers, losers } = topMovers(rows);
  if (gainers.length === 0 && losers.length === 0) {
    return null;
  }

  return (
    <section className="panel asset-panel" aria-label="Top movers">
      <h2>Top movers</h2>
      <div className="movers-grid">
        <MoverList title="Gainers" movers={gainers} />
        <MoverList title="Losers" movers={losers} />
      </div>
    </section>
  );
}

function MoverList({ title, movers }: { title: string; movers: MoverRow[] }) {
  return (
    <div className="mover-list">
      <h3>{title}</h3>
      {movers.length === 0 ? (
        <p className="asset-subtle">-</p>
      ) : (
        <ul>
          {movers.map((mover) => {
            const name = mover.instrument.name.trim();
            const symbol = mover.instrument.symbol.trim();
            const primary = name || symbol;
            const showSymbol = symbol.length > 0 && symbol !== primary;
            return (
              <li key={mover.instrument.id}>
                <span className="mover-identity">
                  <Link
                    className="instrument-link"
                    to={`/asset/${mover.instrument.id}`}
                  >
                    {primary}
                  </Link>
                  {showSymbol ? (
                    <span className="mover-isin">{symbol}</span>
                  ) : null}
                </span>
                <span
                  className={mover.percent >= 0 ? "up number" : "down number"}
                >
                  {mover.percent >= 0 ? "+" : ""}
                  {formatGroupedNumber(mover.percent.toFixed(2))}%
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function AllocationPanel({ rows }: { rows: GainsRow[] }) {
  const [dimension, setDimension] = usePersistentSetting<AllocationDimension>(
    ALLOCATION_DIMENSION_KEY,
    isAllocationDimension,
    "instrument",
  );
  const { slices, excludedCount } = useMemo(
    () => allocationBreakdown(rows, dimension),
    [rows, dimension],
  );
  const palette = [
    "var(--chart-1)",
    "var(--chart-2)",
    "var(--chart-3)",
    "var(--chart-4)",
    "var(--chart-5)",
    "var(--chart-6)",
  ];

  return (
    <section className="panel allocation-panel" aria-label="Allocation">
      <div className="panel-header">
        <h2>Allocation</h2>
        <fieldset className="segmented-control">
          <legend className="sr-only">Allocation dimension</legend>
          {ALLOCATION_DIMENSIONS.map((dim) => (
            <button
              key={dim}
              type="button"
              className={dimension === dim ? "active" : undefined}
              aria-pressed={dimension === dim}
              onClick={() => setDimension(dim)}
            >
              {dim[0].toUpperCase() + dim.slice(1)}
            </button>
          ))}
        </fieldset>
      </div>

      {slices.length === 0 ? (
        <p className="board-state muted">No valued holdings to allocate.</p>
      ) : (
        <div className="allocation-body">
          <div
            className="allocation-bar"
            role="img"
            aria-label="Allocation segments"
          >
            {slices.map((slice, index) => (
              <span
                key={slice.key}
                className="allocation-segment"
                style={{
                  width: `${slice.weightPercent}%`,
                  background: palette[index % palette.length],
                }}
                title={`${slice.label} ${slice.weightPercent.toFixed(1)}%`}
              />
            ))}
          </div>
          <table className="allocation-table">
            <tbody>
              {slices.map((slice, index) => (
                <tr key={slice.key}>
                  <td>
                    <span
                      className="allocation-swatch"
                      style={{ background: palette[index % palette.length] }}
                    />
                    <span className="allocation-label">
                      {slice.label}
                      {slice.secondary ? (
                        <span className="allocation-isin">
                          {slice.secondary}
                        </span>
                      ) : null}
                    </span>
                  </td>
                  <td className="number">
                    SEK {formatGroupedNumber(slice.valueBase.toFixed(2))}
                  </td>
                  <td className="number">{slice.weightPercent.toFixed(1)}%</td>
                </tr>
              ))}
            </tbody>
          </table>
          {excludedCount > 0 ? (
            <span className="status-chip warning compact">
              {excludedCount} excluded (no market value)
            </span>
          ) : null}
        </div>
      )}
    </section>
  );
}
