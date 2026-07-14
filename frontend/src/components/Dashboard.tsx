import { useMemo } from "react";
import { Link } from "react-router-dom";
import { useGains, usePortfolioValueHistory } from "../api/queries";
import type { DateRange, GainsRow } from "../api/types";
import { type DatePreset, DateRangeSelector } from "./DateRangeSelector";
import { type MoverRow, topMovers } from "./dashboardSelectors";
import { GainsWaterfall } from "./GainsWaterfall";
import { PortfolioTreemap } from "./PortfolioTreemap";
import { isOneOf, usePersistentSetting } from "./persistence";
import {
  filterValueHistoryPoints,
  portfolioValueSeries,
} from "./portfolioValueViewModel";
import { TimeSeriesChart } from "./TimeSeriesChart";
import { formatGroupedNumber } from "./valuationDisplay";
import { portfolioWaterfallView } from "./waterfallViewModel";

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
      <div className="dashboard-row">
        <TopMoversPanel rows={gainsQuery.data?.rows ?? []} />
        <PortfolioWaterfallPanel gainsQuery={gainsQuery} />
      </div>
    </section>
  );
}

type ChartView = "value" | "gain" | "treemap";

const CHART_VIEW_KEY = "dashboard.chartView";
const CHART_VIEWS: ChartView[] = ["value", "gain", "treemap"];
const isChartView = isOneOf(CHART_VIEWS);

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

function PortfolioWaterfallPanel({
  gainsQuery,
}: {
  gainsQuery: ReturnType<typeof useGains>;
}) {
  if (gainsQuery.isPending) {
    return (
      <section
        className="panel dashboard-waterfall"
        aria-label="Portfolio gains breakdown"
      >
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>Portfolio gains breakdown</h2>
          </div>
        </div>
        <div className="chart-band">
          <div className="skeleton-bar" />
        </div>
      </section>
    );
  }

  if (gainsQuery.isError) {
    return (
      <section
        className="panel dashboard-waterfall"
        aria-label="Portfolio gains breakdown"
      >
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>Portfolio gains breakdown</h2>
          </div>
        </div>
        <div className="chart-band error">
          <p className="down">Could not load portfolio gains.</p>
          <button
            type="button"
            className="button outline"
            onClick={() => void gainsQuery.refetch()}
          >
            Retry
          </button>
        </div>
      </section>
    );
  }

  const data = gainsQuery.data;
  if (data?.portfolio_waterfall.total_return_base.status !== "available") {
    return (
      <section
        className="panel dashboard-waterfall"
        aria-label="Portfolio gains breakdown"
      >
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>Portfolio gains breakdown</h2>
          </div>
        </div>
        <p className="board-state muted">
          No valued holdings in this interval.
        </p>
      </section>
    );
  }

  const view = portfolioWaterfallView(data.portfolio_waterfall);
  const excludedRows = data.portfolio_waterfall.excluded_rows;
  const headerRight =
    excludedRows > 0 ? (
      <span className="status-chip warning compact">
        {formatGroupedNumber(excludedRows)} incomplete
      </span>
    ) : undefined;

  return (
    <GainsWaterfall
      view={view}
      title="Portfolio gains breakdown"
      className="panel dashboard-waterfall"
      headerRight={headerRight}
    />
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
