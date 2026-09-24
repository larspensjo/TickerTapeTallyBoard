import {
  BaselineSeries,
  createChart,
  type IChartApi,
  type ISeriesApi,
} from "lightweight-charts";
import { useEffect, useRef } from "react";
import {
  baseChartOptions,
  chartColors,
  compactPriceFormat,
  percentPriceFormat,
  zeroInViewAutoscale,
} from "./chartTheme";
import {
  calendarSpineData,
  type TimeSeriesPoint,
  tickMarkFormatter,
} from "./chartTimeAxis";

export type GainUnit = "sek" | "percent";

/**
 * Gain over the selected period, drawn against a zero baseline: green above,
 * red below. Kept separate from `TimeSeriesChart` because a baseline series and
 * a percent axis are a different contract, not a prop — splitting leaves the
 * asset price chart's markers, tooltip and break-even line untouched.
 *
 * The incoming series already carries its zero origin point, and `visibleStart`
 * is that origin's date, so the left edge of the window is the zero origin.
 */
export function PortfolioGainChart({
  data,
  ariaLabel,
  visibleStart,
  unit,
  height = 280,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  visibleStart?: string;
  unit: GainUnit;
  height?: number;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Baseline"> | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const options = baseChartOptions(height);
    const chart = createChart(container, {
      ...options,
      timeScale: { ...options.timeScale, tickMarkFormatter },
    });

    const series = chart.addSeries(BaselineSeries, {
      baseValue: { type: "price", price: 0 },
      topLineColor: chartColors.gainUp,
      topFillColor1: chartColors.gainUpTop,
      topFillColor2: chartColors.gainUpBottom,
      bottomLineColor: chartColors.gainDown,
      bottomFillColor1: chartColors.gainDownTop,
      bottomFillColor2: chartColors.gainDownBottom,
      lineWidth: 2,
      priceLineVisible: false,
      autoscaleInfoProvider: zeroInViewAutoscale,
      priceFormat: unit === "percent" ? percentPriceFormat : compactPriceFormat,
    });

    chartRef.current = chart;
    seriesRef.current = series;

    const resize = () => chart.applyOptions({ width: container.clientWidth });
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
    };
  }, [height, unit]);

  useEffect(() => {
    const end = data.at(-1)?.time;
    const rangeStart =
      visibleStart && end && visibleStart <= end ? visibleStart : undefined;

    // A single measured point still draws as a line from the zero origin, so
    // point markers are only needed when even that is missing.
    seriesRef.current?.applyOptions({ pointMarkersVisible: data.length < 2 });
    seriesRef.current?.setData(calendarSpineData(data, rangeStart));

    if (rangeStart && end) {
      chartRef.current?.timeScale().setVisibleRange({
        from: rangeStart,
        to: end,
      });
    } else {
      chartRef.current?.timeScale().fitContent();
    }
  }, [data, visibleStart]);

  return (
    <div className="time-series-chart-wrap">
      <div
        ref={containerRef}
        className="time-series-chart"
        role="img"
        aria-label={ariaLabel}
      />
    </div>
  );
}
