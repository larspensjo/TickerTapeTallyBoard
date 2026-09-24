import { type ReactNode, useMemo } from "react";
import { Link } from "react-router-dom";
import { useGains, usePortfolioValueHistory } from "../api/queries";
import type { DateRange, GainsRow } from "../api/types";
import { compactPriceFormat } from "./chartTheme";
import { type DatePreset, DateRangeSelector } from "./DateRangeSelector";
import { type MoverRow, topMovers } from "./dashboardSelectors";
import { GainsWaterfall } from "./GainsWaterfall";
import { type GainUnit, PortfolioGainChart } from "./PortfolioGainChart";
import { PortfolioTreemap } from "./PortfolioTreemap";
import { isOneOf, usePersistentSetting } from "./persistence";
import {
  chartPanelHeading,
  dividendCaveatApplies,
  type PeriodGainSeries,
  type PeriodGainStatus,
  periodGainSeries,
  periodValueSeries,
  referenceEdgeTag,
  valueHistoryWindow,
} from "./portfolioValueViewModel";
import { TimeSeriesChart } from "./TimeSeriesChart";
import { formatGroupedNumber } from "./valuationDisplay";
import { portfolioWaterfallView } from "./waterfallViewModel";

export interface DashboardProps {
  selectedDatePreset: DatePreset;
  customRange: DateRange;
  valuationDate: string | null;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
}

export function Dashboard({
  selectedDatePreset,
  customRange,
  valuationDate,
  onDatePresetChange,
  onDateRangeChange,
}: DashboardProps) {
  const gainsQuery = useGains({
    period: selectedDatePreset,
    ...(selectedDatePreset === "custom"
      ? { startDate: customRange.startDate, endDate: customRange.endDate }
      : {}),
  });
  const valueHistory = usePortfolioValueHistory();

  return (
    <section className="dashboard" aria-label="Portfolio dashboard">
      <DashboardChartPanel
        query={valueHistory}
        gainsQuery={gainsQuery}
        selectedDatePreset={selectedDatePreset}
        customRange={customRange}
        valuationDate={valuationDate}
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

const GAIN_UNIT_KEY = "dashboard.gainUnit";
const GAIN_UNITS: GainUnit[] = ["sek", "percent"];
const isGainUnit = isOneOf(GAIN_UNITS);

function DashboardChartPanel({
  query,
  gainsQuery,
  selectedDatePreset,
  customRange,
  valuationDate,
  onDatePresetChange,
  onDateRangeChange,
}: {
  query: ReturnType<typeof usePortfolioValueHistory>;
  gainsQuery: ReturnType<typeof useGains>;
  selectedDatePreset: DatePreset;
  customRange: DateRange;
  valuationDate: string | null;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
}) {
  const [view, setView] = usePersistentSetting<ChartView>(
    CHART_VIEW_KEY,
    isChartView,
    "value",
  );
  const [unit, setUnit] = usePersistentSetting<GainUnit>(
    GAIN_UNIT_KEY,
    isGainUnit,
    "sek",
  );

  const isGain = view === "gain";
  const reportPeriod = gainsQuery.data?.report_period;
  // While the gains query serves the previous selection's response, the panel
  // must not claim the newly selected period.
  const periodIsStale = gainsQuery.isPlaceholderData;

  const range = useMemo<DateRange>(
    () => ({
      startDate: reportPeriod?.start_date ?? null,
      endDate: reportPeriod?.end_date ?? null,
    }),
    [reportPeriod],
  );
  const historyWindow = useMemo(
    () =>
      valueHistoryWindow(
        query.data?.points ?? [],
        range,
        query.data?.start_date ?? null,
      ),
    [query.data, range],
  );
  const valueSeries = useMemo(
    () => periodValueSeries(historyWindow),
    [historyWindow],
  );
  const gain = useMemo(() => periodGainSeries(historyWindow), [historyWindow]);

  const investedEdgeTag = useMemo(() => {
    if (isGain) return undefined;

    const tag = referenceEdgeTag(valueSeries.value, valueSeries.invested);
    if (!tag) return undefined;

    const amount = compactPriceFormat.formatter(tag.value);
    return {
      side: tag.side,
      label: `Invested ${amount}`,
      description: `Net invested capital is ${amount} SEK, ${tag.side} the visible range`,
    };
  }, [isGain, valueSeries]);

  const heading = chartPanelHeading({
    view: isGain ? "gain" : "value",
    unit,
    period:
      periodIsStale || !reportPeriod
        ? null
        : {
            preset: selectedDatePreset,
            startDate: reportPeriod.start_date,
            endDate: reportPeriod.end_date,
          },
  });

  const showsApproximate = isGain && unit === "percent" && gain.approximate;
  const showsDividendCaveat =
    isGain && dividendCaveatApplies(gainsQuery.data?.portfolio_waterfall);

  const chartControls = (
    <div className="chart-controls">
      <DateRangeSelector
        customRange={customRange}
        selectedDatePreset={selectedDatePreset}
        onDatePresetChange={onDatePresetChange}
        onDateRangeChange={onDateRangeChange}
        valuationDate={valuationDate}
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
      {isGain ? (
        <fieldset className="segmented-control">
          <legend className="sr-only">Gain unit</legend>
          {GAIN_UNITS.map((u) => (
            <button
              key={u}
              type="button"
              className={unit === u ? "active" : undefined}
              aria-pressed={unit === u}
              aria-label={u === "percent" ? "Percent" : undefined}
              onClick={() => setUnit(u)}
            >
              {u === "percent" ? "%" : "SEK"}
            </button>
          ))}
        </fieldset>
      ) : null}
    </div>
  );

  const metaChips = (
    <>
      {periodIsStale ? (
        <span className="status-chip compact">Updating</span>
      ) : null}
      {historyWindow.incompleteCount > 0 ? (
        <span className="status-chip warning compact">
          {historyWindow.incompleteCount} days had missing inputs
        </span>
      ) : null}
      {showsApproximate ? (
        <span className="status-chip warning compact">Approximate</span>
      ) : null}
      {isGain && gain.investedUnavailableAfter ? (
        <span className="status-chip warning compact">
          Gain unavailable after {gain.investedUnavailableAfter} — a trade is
          missing its exchange rate.
        </span>
      ) : null}
    </>
  );

  function panel(children: ReactNode, chips: ReactNode = null) {
    return (
      <section
        className="panel chart-panel"
        aria-label={isGain ? "Portfolio gain" : "Portfolio value"}
      >
        <div className="chart-meta">
          <div className="chart-meta-title">
            <h2>{heading}</h2>
            {chips}
          </div>
          {chartControls}
        </div>
        {children}
      </section>
    );
  }

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

  if (gainsQuery.isPending || query.isPending) {
    return panel(
      <div className="chart-band">
        <div className="skeleton-bar" />
      </div>,
    );
  }

  // Without a resolved period there is no range to plot. Falling back to
  // unbounded lifetime history under a period-named heading would be a lie.
  if (!reportPeriod) {
    return panel(
      <div className="chart-band error">
        <p className="down">Could not work out the selected period.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => void gainsQuery.refetch()}
        >
          Retry
        </button>
      </div>,
    );
  }

  if (query.isError) {
    return panel(
      <div className="chart-band error">
        <p className="down">Could not load portfolio value.</p>
        <button
          type="button"
          className="button outline"
          onClick={() => void query.refetch()}
        >
          Retry
        </button>
      </div>,
    );
  }

  const caption = (
    <p className="chart-caption">
      {isGain && unit === "percent"
        ? "How your holdings performed over the selected period — money you added or took out does not move this line. The percentages in the portfolio summary and on the Gains page answer a different question: how your money performed."
        : "Gain earned since the start of the selected period. Money you added or took out is not counted as gain."}
      {showsApproximate
        ? " Part of this period could not be measured exactly: money moved in or out around days that are missing prices, so this percentage is a close estimate."
        : null}
      {showsDividendCaveat
        ? " Dividends are not included here, so holdings that paid them read a little low."
        : null}
    </p>
  );

  if (isGain) {
    if (gain.status !== "available") {
      return panel(
        <div className="chart-band muted">
          <span className="chart-band-label">
            {GAIN_UNAVAILABLE_MESSAGES[gain.status](gain)}
          </span>
        </div>,
        metaChips,
      );
    }

    return panel(
      <>
        <div className="chart-legend" aria-hidden="true">
          <span className="chart-legend-item gain">
            {unit === "percent"
              ? "Performance this period (%)"
              : "Gain this period (SEK)"}
          </span>
        </div>
        <PortfolioGainChart
          data={unit === "percent" ? gain.percent : gain.sek}
          ariaLabel={
            unit === "percent"
              ? "Portfolio performance over the selected period, in percent"
              : "Portfolio gain over the selected period, in SEK"
          }
          visibleStart={gain.originDate ?? undefined}
          unit={unit}
          height={280}
        />
        {gain.endsEarlyAt ? (
          <p className="chart-caption">
            Chart ends {gain.endsEarlyAt} — no valued holdings after that date.
          </p>
        ) : null}
        {caption}
      </>,
      metaChips,
    );
  }

  if (valueSeries.value.length === 0) {
    return panel(
      <div className="chart-band muted">
        <span className="chart-band-label">
          No portfolio history in this interval
        </span>
      </div>,
      metaChips,
    );
  }

  return panel(
    <>
      <div className="chart-legend" aria-hidden="true">
        <span className="chart-legend-item value">Value</span>
        <span className="chart-legend-item invested">Invested capital</span>
      </div>
      <TimeSeriesChart
        data={valueSeries.value}
        referenceData={valueSeries.invested}
        ariaLabel="Portfolio value over time in SEK, with net invested capital reference line"
        visibleStart={range.startDate ?? query.data?.start_date ?? undefined}
        edgeTag={investedEdgeTag}
        height={280}
        compactValueAxis
      />
    </>,
    metaChips,
  );
}

const GAIN_UNAVAILABLE_MESSAGES: Record<
  Exclude<PeriodGainStatus, "available">,
  (gain: PeriodGainSeries) => string
> = {
  no_history: () => "No portfolio history in this interval",
  all_incomplete: () =>
    "Every day in this period is missing prices for at least one holding, so gain cannot be measured. Refreshing prices may fill this in.",
  unknown_opening: () =>
    "The portfolio's value at the start of this period is not known, so gain for this period cannot be measured. Choose a range that starts earlier.",
  missing_trade_fx: (gain) =>
    `Gain cannot be shown: a trade in a foreign currency has no exchange rate for its trade date, so the money you put in is unknown from ${gain.investedUnavailableFrom} onwards.`,
};

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
