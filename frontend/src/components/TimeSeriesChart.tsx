import {
  type AreaData,
  type AutoscaleInfoProvider,
  createChart,
  type IChartApi,
  type ISeriesApi,
  TickMarkType,
  type Time,
  type WhitespaceData,
} from "lightweight-charts";
import { useEffect, useRef } from "react";

export interface TimeSeriesPoint {
  time: string;
  value: number;
}

type AreaSeriesPoint = AreaData<Time> | WhitespaceData<Time>;
const dayMs = 24 * 60 * 60 * 1000;

function chartDate(time: Time): Date | null {
  if (typeof time === "string") {
    const parsed = new Date(`${time}T00:00:00Z`);
    return Number.isNaN(parsed.getTime()) ? null : parsed;
  }

  if (typeof time === "number") {
    return new Date(time * 1000);
  }

  return new Date(Date.UTC(time.year, time.month - 1, time.day));
}

function tickMarkFormatter(
  time: Time,
  tickMarkType: TickMarkType,
): string | null {
  if (tickMarkType !== TickMarkType.DayOfMonth) return null;

  const date = chartDate(time);
  if (!date) return null;

  return date.toLocaleDateString("en-US", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
}

function parseIsoDate(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;

  const [, year, month, day] = match;
  const timestamp = Date.UTC(Number(year), Number(month) - 1, Number(day));
  return Number.isNaN(timestamp) ? null : timestamp;
}

function formatIsoDate(timestamp: number): string {
  return new Date(timestamp).toISOString().slice(0, 10);
}

function calendarSpineData(
  data: TimeSeriesPoint[],
  rangeStart: string | undefined,
): AreaSeriesPoint[] {
  const end = data.at(-1)?.time;
  const start = rangeStart ?? data[0]?.time;
  if (!start || !end) return data;

  const startTimestamp = parseIsoDate(start);
  const endTimestamp = parseIsoDate(end);
  if (
    startTimestamp === null ||
    endTimestamp === null ||
    startTimestamp > endTimestamp
  ) {
    return data;
  }

  const pointsByDate = new Map(data.map((point) => [point.time, point]));
  const points: AreaSeriesPoint[] = [];
  for (
    let timestamp = startTimestamp;
    timestamp <= endTimestamp;
    timestamp += dayMs
  ) {
    const time = formatIsoDate(timestamp);
    points.push(pointsByDate.get(time) ?? { time });
  }

  return points;
}

const zeroBaselineAutoscale: AutoscaleInfoProvider = (baseImplementation) => {
  const autoscale = baseImplementation();
  if (autoscale === null) return null;

  return {
    ...autoscale,
    priceRange: {
      ...autoscale.priceRange,
      minValue: 0,
      maxValue:
        autoscale.priceRange.maxValue > 0 ? autoscale.priceRange.maxValue : 1,
    },
  };
};

export function TimeSeriesChart({
  data,
  ariaLabel,
  visibleStart,
  height = 240,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  visibleStart?: string;
  height?: number;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const chart = createChart(container, {
      height,
      layout: {
        background: { color: "transparent" },
        textColor: "#9aa4b2",
        attributionLogo: false,
      },
      grid: {
        vertLines: { color: "rgba(148, 163, 184, 0.08)" },
        horzLines: { color: "rgba(148, 163, 184, 0.08)" },
      },
      rightPriceScale: { borderColor: "rgba(148, 163, 184, 0.2)" },
      timeScale: {
        borderColor: "rgba(148, 163, 184, 0.2)",
        tickMarkFormatter,
      },
      handleScale: false,
      handleScroll: false,
    });
    const series = chart.addAreaSeries({
      lineColor: "#4f9cff",
      topColor: "rgba(79, 156, 255, 0.30)",
      bottomColor: "rgba(79, 156, 255, 0.02)",
      lineWidth: 2,
      priceLineVisible: false,
      autoscaleInfoProvider: zeroBaselineAutoscale,
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
  }, [height]);

  useEffect(() => {
    const end = data.at(-1)?.time;
    const rangeStart =
      visibleStart && end && visibleStart <= end ? visibleStart : undefined;
    const chartData = calendarSpineData(data, rangeStart);

    seriesRef.current?.setData(chartData);

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
    <div
      ref={containerRef}
      className="time-series-chart"
      role="img"
      aria-label={ariaLabel}
    />
  );
}
