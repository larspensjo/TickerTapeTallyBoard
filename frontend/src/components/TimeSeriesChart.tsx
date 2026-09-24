import {
  AreaSeries,
  createChart,
  createSeriesMarkers,
  type IChartApi,
  type IPriceLine,
  type ISeriesApi,
  type ISeriesMarkersPluginApi,
  LineSeries,
  LineStyle,
  type SeriesMarker,
  type Time,
} from "lightweight-charts";
import { useEffect, useRef, useState } from "react";
import {
  baseChartOptions,
  chartColors,
  compactPriceFormat,
  fitToDataAutoscale,
  noAutoscale,
} from "./chartTheme";
import {
  calendarSpineData,
  isoFromTime,
  type TimeSeriesPoint,
  tickMarkFormatter,
} from "./chartTimeAxis";

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
  costBasisLine,
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
  costBasisLine?: number;
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
  const costPriceLineRef = useRef<IPriceLine | null>(null);
  const costBasisValueRef = useRef<number | undefined>(costBasisLine);
  const markersRef = useRef<Map<string, ChartTradeMarker[]>>(new Map());
  const [tooltip, setTooltip] = useState<TradeTooltipState | null>(null);

  markersRef.current = groupedMarkers(markers);
  costBasisValueRef.current = costBasisLine;

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
      // `createPriceLine` primitives do not participate in scale aggregation,
      // so the break-even value is folded in here to stop it clipping.
      autoscaleInfoProvider: fitToDataAutoscale(() =>
        costBasisValueRef.current === undefined
          ? []
          : [costBasisValueRef.current],
      ),
      ...(compactValueAxis ? { priceFormat: compactPriceFormat } : {}),
    });

    chartRef.current = chart;
    seriesRef.current = series;
    seriesMarkersRef.current = createSeriesMarkers(series);
    referenceSeriesRef.current = referenceSeries;

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
      setTooltip(null);
    };
  }, [height, lineColor, topColor, bottomColor, compactValueAxis]);

  useEffect(() => {
    const end = data.at(-1)?.time;
    const rangeStart =
      visibleStart && end && visibleStart <= end ? visibleStart : undefined;
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

    if (rangeStart && end) {
      chartRef.current?.timeScale().setVisibleRange({
        from: rangeStart,
        to: end,
      });
    } else {
      chartRef.current?.timeScale().fitContent();
    }
  }, [data, visibleStart, referenceData]);

  useEffect(() => {
    const previousPriceLine = costPriceLineRef.current;
    const series = seriesRef.current;
    const price = costBasisLine;

    if (previousPriceLine) {
      if (series) {
        series.removePriceLine(previousPriceLine);
      }
      costPriceLineRef.current = null;
    }

    // Re-applying options re-runs the autoscale provider, so a changed
    // break-even value rescales the axis instead of clipping at the edge.
    series?.applyOptions({});

    if (!series || typeof price !== "number" || !Number.isFinite(price)) {
      return () => {
        costPriceLineRef.current = null;
      };
    }

    const priceLine = series.createPriceLine({
      price,
      color: chartColors.reference,
      lineStyle: LineStyle.Dotted,
      lineWidth: 1,
      axisLabelVisible: true,
      title: "",
      lineVisible: true,
    });

    costPriceLineRef.current = priceLine;

    return () => {
      costPriceLineRef.current = null;
      if (seriesRef.current) {
        seriesRef.current.removePriceLine(priceLine);
      }
    };
  }, [costBasisLine]);

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
