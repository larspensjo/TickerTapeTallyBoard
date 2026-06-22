import {
  type AreaData,
  type AutoscaleInfoProvider,
  createChart,
  type IChartApi,
  type ISeriesApi,
  type SeriesMarker,
  TickMarkType,
  type Time,
  type WhitespaceData,
} from "lightweight-charts";
import { useEffect, useRef, useState } from "react";

export interface TimeSeriesPoint {
  time: string;
  value: number;
}

export interface ChartTradeMarker {
  time: string;
  side: "buy" | "sell";
  title: string;
  rows: { label: string; value: string }[];
}

interface TradeTooltipState {
  x: number;
  y: number;
  marker: ChartTradeMarker;
}

const markerColors = {
  buy: "#16c784",
  sell: "#ff4d4f",
} as const;

function isoFromTime(time: Time): string | null {
  const date = chartDate(time);
  return date ? date.toISOString().slice(0, 10) : null;
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
  markers = [],
  height = 240,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  visibleStart?: string;
  markers?: ChartTradeMarker[];
  height?: number;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);
  const markersRef = useRef<Map<string, ChartTradeMarker>>(new Map());
  const [tooltip, setTooltip] = useState<TradeTooltipState | null>(null);

  markersRef.current = new Map(markers.map((marker) => [marker.time, marker]));

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

    chart.subscribeCrosshairMove((param) => {
      const iso = param.time ? isoFromTime(param.time) : null;
      const marker = iso ? markersRef.current.get(iso) : undefined;
      if (!marker || !param.point) {
        setTooltip(null);
        return;
      }

      setTooltip({ x: param.point.x, y: param.point.y, marker });
    });

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
      setTooltip(null);
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

  useEffect(() => {
    const series = seriesRef.current;
    if (!series) return;

    const seriesMarkers: SeriesMarker<Time>[] = markers
      .slice()
      .sort((a, b) => a.time.localeCompare(b.time))
      .map((marker) => ({
        time: marker.time,
        position: marker.side === "buy" ? "belowBar" : "aboveBar",
        shape: marker.side === "buy" ? "arrowUp" : "arrowDown",
        color: markerColors[marker.side],
      }));

    series.setMarkers(seriesMarkers);
    setTooltip(null);
  }, [markers]);

  return (
    <div className="time-series-chart-wrap">
      <div
        ref={containerRef}
        className="time-series-chart"
        role="img"
        aria-label={ariaLabel}
      />
      {tooltip ? (
        <div
          className={`chart-trade-tooltip ${tooltip.marker.side}`}
          style={{ left: `${tooltip.x}px`, top: `${tooltip.y}px` }}
          role="tooltip"
        >
          <span className="chart-trade-tooltip-title">
            {tooltip.marker.title}
          </span>
          <dl>
            {tooltip.marker.rows.map((entry) => (
              <div key={entry.label}>
                <dt>{entry.label}</dt>
                <dd>{entry.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      ) : null}
    </div>
  );
}
