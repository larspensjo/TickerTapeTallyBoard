import {
  AreaSeries,
  createChart,
  createSeriesMarkers,
  type IChartApi,
  type ISeriesApi,
  type ISeriesMarkersPluginApi,
  LineSeries,
  LineStyle,
  LineType,
  type SeriesMarker,
  type Time,
} from "lightweight-charts";
import { useEffect, useRef, useState } from "react";
import {
  baseChartOptions,
  chartColors,
  compactPriceFormat,
  noAutoscale,
} from "./chartTheme";
import {
  calendarSpineData,
  chartWindow,
  isoFromTime,
  type TimeSeriesPoint,
  tickMarkFormatter,
} from "./chartTimeAxis";
import { fixedTextValueTag } from "./chartValueTag";

export type { TimeSeriesPoint };

/** A reference line outside the drawn band is announced here instead of vanishing. */
export interface ChartEdgeTag {
  side: "above" | "below";
  label: string;
  description: string;
}

interface ChartMarkerBase {
  time: string;
  title: string;
  rows: { label: string; value: string }[];
}

export type ChartTradeMarker =
  | (ChartMarkerBase & {
      side: "buy" | "sell";
      price: number;
    })
  | (ChartMarkerBase & {
      side: "split";
    });

/**
 * A step-shaped line drawn as its own series, so separate lines never join
 * across a gap. Only a line flagged `showLastValue` gets a right-axis tag;
 * `lastValueLabel` is that tag's text when it must not be re-formatted.
 */
export interface ChartStepLine {
  key: string;
  points: TimeSeriesPoint[];
  showLastValue: boolean;
  lastValueLabel?: string;
}

interface TradeTooltipState {
  x: number;
  y: number;
  markers: ChartTradeMarker[];
}

const markerColors = {
  buy: chartColors.gainUp,
  sell: chartColors.gainDown,
  split: "#a8acb3",
} as const;

export function TimeSeriesChart({
  data,
  ariaLabel,
  visibleStart,
  markers = [],
  referenceData,
  stepLines,
  edgeTag,
  height = 240,
  lineColor = chartColors.value,
  topColor = chartColors.valueTop,
  bottomColor = chartColors.valueBottom,
  compactValueAxis = false,
}: {
  data: TimeSeriesPoint[];
  ariaLabel: string;
  visibleStart?: string;
  markers?: ChartTradeMarker[];
  referenceData?: TimeSeriesPoint[];
  stepLines?: ChartStepLine[];
  edgeTag?: ChartEdgeTag;
  height?: number;
  lineColor?: string;
  topColor?: string;
  bottomColor?: string;
  compactValueAxis?: boolean;
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const chartRef = useRef<IChartApi | null>(null);
  const seriesRef = useRef<ISeriesApi<"Area"> | null>(null);
  const seriesMarkersRef = useRef<ISeriesMarkersPluginApi<Time> | null>(null);
  const referenceSeriesRef = useRef<ISeriesApi<"Line"> | null>(null);
  const markersRef = useRef<Map<string, ChartTradeMarker[]>>(new Map());
  // Held in state as well as the ref so effects that add their own series
  // re-run against a recreated chart.
  const [chart, setChart] = useState<IChartApi | null>(null);
  const [tooltip, setTooltip] = useState<TradeTooltipState | null>(null);

  markersRef.current = groupedMarkers(markers);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const options = baseChartOptions(height);
    const chart = createChart(container, {
      ...options,
      timeScale: { ...options.timeScale, tickMarkFormatter },
    });
    // The reference line is drawn but never scales the axis, so a value line
    // far above net invested capital still fills the band on short ranges.
    const referenceSeries = chart.addSeries(LineSeries, {
      color: chartColors.reference,
      lineWidth: 2,
      lineStyle: LineStyle.Dashed,
      priceLineVisible: false,
      lastValueVisible: false,
      crosshairMarkerVisible: false,
      autoscaleInfoProvider: noAutoscale,
      ...(compactValueAxis ? { priceFormat: compactPriceFormat } : {}),
    });

    const series = chart.addSeries(AreaSeries, {
      lineColor,
      topColor,
      bottomColor,
      lineWidth: 2,
      priceLineVisible: false,
      ...(compactValueAxis ? { priceFormat: compactPriceFormat } : {}),
    });

    chartRef.current = chart;
    seriesRef.current = series;
    seriesMarkersRef.current = createSeriesMarkers(series);
    referenceSeriesRef.current = referenceSeries;
    setChart(chart);

    const resize = () => chart.applyOptions({ width: container.clientWidth });
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);

    chart.subscribeCrosshairMove((param) => {
      const iso = param.time ? isoFromTime(param.time) : null;
      const tooltipMarkers = iso ? markersRef.current.get(iso) : undefined;
      if (!tooltipMarkers || !param.point) {
        setTooltip(null);
        return;
      }

      setTooltip({
        x: param.point.x,
        y: param.point.y,
        markers: tooltipMarkers,
      });
    });

    return () => {
      observer.disconnect();
      chart.remove();
      chartRef.current = null;
      seriesRef.current = null;
      seriesMarkersRef.current = null;
      referenceSeriesRef.current = null;
      setChart(null);
      setTooltip(null);
    };
  }, [height, lineColor, topColor, bottomColor, compactValueAxis]);

  useEffect(() => {
    const plotted = chartWindow(data, visibleStart);
    const rangeStart = plotted?.startsAtVisibleStart
      ? plotted.start
      : undefined;
    const chartData = calendarSpineData(data, rangeStart);

    // A single-observation range (the Today preset) has no line to draw, so the
    // point is shown as a marker rather than an empty chart.
    seriesRef.current?.applyOptions({ pointMarkersVisible: data.length < 2 });
    seriesRef.current?.setData(chartData);

    const referenceSeries = referenceSeriesRef.current;
    if (referenceSeries) {
      referenceSeries.setData(
        referenceData && referenceData.length > 0
          ? calendarSpineData(referenceData, rangeStart)
          : [],
      );
    }

    if (plotted?.startsAtVisibleStart) {
      chartRef.current?.timeScale().setVisibleRange({
        from: plotted.start,
        to: plotted.end,
      });
    } else {
      chartRef.current?.timeScale().fitContent();
    }
  }, [data, visibleStart, referenceData]);

  useEffect(() => {
    // Until the state catches up with a recreated chart, `chart` may be the
    // removed one; the live chart is always the ref.
    if (!chart || chartRef.current !== chart) return;
    if (!stepLines || stepLines.length === 0) return;

    // Added after the reference and area series, so the price scale keeps
    // taking its tick format from those and never from a step line.
    const added = stepLines.map((line) => {
      const series = chart.addSeries(LineSeries, {
        color: chartColors.reference,
        lineWidth: 1,
        lineStyle: LineStyle.Dotted,
        lineType: LineType.WithSteps,
        priceLineVisible: false,
        // The library's own tag takes its text from the shared price scale's
        // formatter, so a flagged line draws its tag as a primitive instead.
        lastValueVisible: false,
        crosshairMarkerVisible: false,
        pointMarkersVisible: line.points.length < 2,
        ...(compactValueAxis ? { priceFormat: compactPriceFormat } : {}),
      });
      series.setData(line.points);

      const terminal = line.points.at(-1);
      const tag =
        line.showLastValue && terminal
          ? fixedTextValueTag({
              value: terminal.value,
              text: line.lastValueLabel ?? terminal.value.toFixed(2),
              color: chartColors.reference,
              textColor: chartColors.referenceLabelText,
            })
          : null;
      if (tag) series.attachPrimitive(tag);
      return { series, tag };
    });

    return () => {
      // A chart that has already been torn down took its series with it.
      if (chartRef.current !== chart) return;
      for (const { series, tag } of added) {
        if (tag) series.detachPrimitive(tag);
        chart.removeSeries(series);
      }
    };
  }, [chart, stepLines, compactValueAxis]);

  useEffect(() => {
    const seriesMarkersApi = seriesMarkersRef.current;
    if (!seriesMarkersApi) return;

    const seriesMarkers: SeriesMarker<Time>[] = markers
      .slice()
      .sort((a, b) => a.time.localeCompare(b.time))
      .map((marker): SeriesMarker<Time> => {
        if (marker.side === "split") {
          return {
            time: marker.time,
            position: "inBar",
            shape: "circle",
            color: markerColors.split,
          };
        }

        return {
          time: marker.time,
          position: marker.side === "buy" ? "atPriceBottom" : "atPriceTop",
          shape: marker.side === "buy" ? "arrowUp" : "arrowDown",
          color: markerColors[marker.side],
          price: marker.price,
        };
      });

    seriesMarkersApi.setMarkers(seriesMarkers);
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
      {edgeTag ? (
        <p
          className={`chart-edge-tag ${edgeTag.side}`}
          role="note"
          aria-label={edgeTag.description}
        >
          {edgeTag.label}
        </p>
      ) : null}
      {tooltip ? (
        <div
          className={`chart-trade-tooltip ${tooltip.markers[0]?.side ?? "split"}`}
          style={{ left: `${tooltip.x}px`, top: `${tooltip.y}px` }}
          role="tooltip"
        >
          {tooltip.markers.map((marker) => (
            <div
              className={`chart-trade-tooltip-section ${marker.side}`}
              key={`${marker.time}-${marker.title}`}
            >
              <span className="chart-trade-tooltip-title">{marker.title}</span>
              <dl>
                {marker.rows.map((entry) => (
                  <div key={entry.label}>
                    <dt>{entry.label}</dt>
                    <dd>{entry.value}</dd>
                  </div>
                ))}
              </dl>
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function groupedMarkers(
  markers: ChartTradeMarker[],
): Map<string, ChartTradeMarker[]> {
  const grouped = new Map<string, ChartTradeMarker[]>();

  for (const marker of markers) {
    const entries = grouped.get(marker.time) ?? [];
    entries.push(marker);
    grouped.set(marker.time, entries);
  }

  for (const entries of grouped.values()) {
    entries.sort((a, b) => markerSortValue(a) - markerSortValue(b));
  }

  return grouped;
}

function markerSortValue(marker: ChartTradeMarker): number {
  if (marker.side === "buy") return 0;
  if (marker.side === "sell") return 1;
  return 2;
}
