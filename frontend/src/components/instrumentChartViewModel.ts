import type {
  PriceHistoryResponse,
  RunningAverageCostStep,
  Transaction,
} from "../api/types";
import { type SplitEvent, splitEvents } from "./assetViewModel";
import type { ChartWindow } from "./chartTimeAxis";
import type {
  ChartStepLine,
  ChartTradeMarker,
  TimeSeriesPoint,
} from "./TimeSeriesChart";
import {
  formatGroupedNumber,
  formatSignedQuantity,
  formatUnitPrice,
  parseFiniteNumber,
} from "./valuationDisplay";

export interface InstrumentPriceSeries {
  points: TimeSeriesPoint[];
  allUnavailable: boolean;
}

export type TradeSide = "buy" | "sell";

export interface TradeMarker {
  time: string;
  side: TradeSide;
  quantity: number;
  /** Quantity-weighted average trade price, in the instrument's native currency. */
  avgPrice: number;
  /** Summed brokerage fee, or null when no fee was recorded or fee currencies conflict. */
  fee: number | null;
  /** Currency of `fee` (the brokerage currency, often SEK), or null when unknown/omitted. */
  feeCurrency: string | null;
  currency: string;
}

/**
 * Convert native-currency price points to chart data.
 * The asset price chart should stay in the instrument's own currency; portfolio
 * valuation charts are responsible for SEK conversion.
 */
export function instrumentPriceSeries(
  response: PriceHistoryResponse | undefined,
  transactions: Transaction[] = [],
): InstrumentPriceSeries {
  const all = response?.points ?? [];
  const points: TimeSeriesPoint[] = [];

  for (const point of all) {
    const value = Number(point.close);
    if (Number.isFinite(value)) points.push({ time: point.date, value });
  }

  points.push(...transactionPricePoints(response, transactions, points));
  points.sort((a, b) => a.time.localeCompare(b.time));

  return {
    points,
    allUnavailable: all.length > 0 && points.length === 0,
  };
}

function transactionPricePoints(
  response: PriceHistoryResponse | undefined,
  transactions: Transaction[],
  existingPoints: TimeSeriesPoint[],
): TimeSeriesPoint[] {
  if (!response) return [];

  const nativeCurrency = response.currency.toUpperCase();
  const existingDates = new Set(existingPoints.map((point) => point.time));
  const transactionPoints: TimeSeriesPoint[] = [];

  for (const transaction of transactions) {
    if (
      !transaction.price ||
      tradeSide(transaction.type) === null ||
      existingDates.has(transaction.trade_date)
    ) {
      continue;
    }

    const price = Number(transaction.price);
    if (!Number.isFinite(price)) continue;

    const tradeCurrency = transaction.currency?.toUpperCase() ?? nativeCurrency;
    if (tradeCurrency !== nativeCurrency) continue;

    transactionPoints.push({ time: transaction.trade_date, value: price });
    existingDates.add(transaction.trade_date);
  }

  return transactionPoints;
}

interface MarkerAccumulator {
  quantity: number;
  priceWeightedQuantity: number;
  fee: number | null;
  feeCurrency: string | null;
  /** Set when merged trades report fees in differing currencies; the fee is then dropped. */
  feeConflict: boolean;
}

/**
 * Build Buy/Sell event markers for the asset price chart. Transactions on the
 * same day and same side are merged into one marker: quantities sum and the
 * price is quantity-weighted. The chart stays in the instrument's own currency,
 * so trades in another currency are skipped — mirroring `transactionPricePoints`.
 * Brokerage fees carry their own currency (`brokerage_currency`, typically SEK
 * even for a USD instrument); merged fees only sum when their currencies match,
 * otherwise the fee is omitted rather than mislabelled.
 */
export function tradeMarkers(
  transactions: Transaction[],
  nativeCurrency: string,
): TradeMarker[] {
  const native = nativeCurrency.toUpperCase();
  const groups = new Map<string, MarkerAccumulator>();

  for (const transaction of transactions) {
    const side = tradeSide(transaction.type);
    if (side === null) continue;

    const tradeCurrency = transaction.currency?.toUpperCase() ?? native;
    if (tradeCurrency !== native) continue;

    const price = Number(transaction.price);
    const quantity = Math.abs(Number(transaction.quantity));
    if (
      !Number.isFinite(price) ||
      !Number.isFinite(quantity) ||
      quantity <= 0
    ) {
      continue;
    }

    const key = `${transaction.trade_date}|${side}`;
    const accumulator = groups.get(key) ?? {
      quantity: 0,
      priceWeightedQuantity: 0,
      fee: null,
      feeCurrency: null,
      feeConflict: false,
    };
    accumulator.quantity += quantity;
    accumulator.priceWeightedQuantity += price * quantity;

    accumulateFee(accumulator, transaction);

    groups.set(key, accumulator);
  }

  const markers: TradeMarker[] = [];
  for (const [key, accumulator] of groups) {
    const [time, side] = key.split("|") as [string, TradeSide];
    markers.push({
      time,
      side,
      quantity: accumulator.quantity,
      avgPrice: accumulator.priceWeightedQuantity / accumulator.quantity,
      fee: accumulator.fee,
      feeCurrency: accumulator.feeCurrency,
      currency: native,
    });
  }

  markers.sort((a, b) => a.time.localeCompare(b.time));
  return markers;
}

function accumulateFee(
  accumulator: MarkerAccumulator,
  transaction: Transaction,
): void {
  if (accumulator.feeConflict) return;

  const fee = Number(transaction.brokerage);
  if (transaction.brokerage === null || !Number.isFinite(fee)) return;

  const feeCurrency = transaction.brokerage_currency?.toUpperCase() ?? null;

  if (accumulator.fee === null) {
    accumulator.fee = fee;
    accumulator.feeCurrency = feeCurrency;
    return;
  }

  if (accumulator.feeCurrency !== feeCurrency) {
    // Different fee currencies cannot be summed; drop the fee for this marker.
    accumulator.fee = null;
    accumulator.feeCurrency = null;
    accumulator.feeConflict = true;
    return;
  }

  accumulator.fee += fee;
}

/**
 * The running average cost line: one step line per held period, clipped to the
 * plotted window. A period that starts before the window carries its prevailing
 * average to the window start; periods wholly outside are dropped. A closed
 * period ends on its closing date and an open one on the window end. Only the
 * open period carries a right-axis tag, whose text is the backend's display
 * string for today's average, never re-formatted here.
 */
export function runningAverageCostLine(
  response: PriceHistoryResponse | undefined,
  window: ChartWindow | null,
): ChartStepLine[] {
  if (!response || !window) return [];

  const { segments, current_average } = response.running_average_cost;
  const lines: ChartStepLine[] = [];

  for (const segment of segments) {
    const steps = parsedSteps(segment.steps);
    const first = steps[0];
    if (!first) continue;

    const isOpen = segment.closed_on === null;
    const segmentEnd = segment.closed_on ?? window.end;
    if (first.time > window.end || segmentEnd < window.start) continue;

    const points: TimeSeriesPoint[] = [];
    const prevailing = steps.filter((step) => step.time < window.start).at(-1);
    if (prevailing && !steps.some((step) => step.time === window.start)) {
      points.push({ time: window.start, value: prevailing.value });
    }
    points.push(
      ...steps.filter(
        (step) => step.time >= window.start && step.time <= window.end,
      ),
    );

    const last = points.at(-1);
    if (!last) continue;
    const terminal = segmentEnd < window.end ? segmentEnd : window.end;
    if (terminal !== last.time) {
      points.push({ time: terminal, value: last.value });
    }

    // The tag shows today's average, so it belongs only on a line that ends
    // at that level: an open period whose final step parsed and is plotted.
    const lastParsed = steps.at(-1);
    const endsOnCurrentAverage =
      isOpen &&
      lastParsed !== undefined &&
      lastParsed.time === segment.steps.at(-1)?.date &&
      lastParsed.time <= window.end;
    const line: ChartStepLine = {
      key: `running-average-cost:${first.time}`,
      points,
      showLastValue: endsOnCurrentAverage,
    };
    if (endsOnCurrentAverage && current_average !== null) {
      line.lastValueLabel = current_average;
    }
    lines.push(line);
  }

  return lines;
}

/**
 * The running average after each buy day, keyed by date, in the same
 * split-adjusted frame the running average cost line draws.
 */
export function averageAfterByDate(
  response: PriceHistoryResponse | undefined,
): Map<string, number> {
  const averages = new Map<string, number>();
  for (const segment of response?.running_average_cost.segments ?? []) {
    for (const step of parsedSteps(segment.steps)) {
      averages.set(step.time, step.value);
    }
  }
  return averages;
}

function parsedSteps(steps: RunningAverageCostStep[]): TimeSeriesPoint[] {
  const points: TimeSeriesPoint[] = [];
  for (const step of steps) {
    const value = parseFiniteNumber(step.average);
    if (value !== null) points.push({ time: step.date, value });
  }
  return points;
}

/**
 * Buy, Sell, and Split markers with their tooltip rows for the asset price
 * chart. A Buy marker shows the running average after that day ("Avg after")
 * when the day has a running-average step; sells and splits never do.
 */
export function priceChartMarkers(
  transactions: Transaction[],
  currency: string,
  averageAfter: Map<string, number>,
): ChartTradeMarker[] {
  return [
    ...tradeChartMarkers(transactions, currency, averageAfter),
    ...splitChartMarkers(splitEvents(transactions)),
  ];
}

function tradeChartMarkers(
  transactions: Transaction[],
  currency: string,
  averageAfter: Map<string, number>,
): ChartTradeMarker[] {
  return tradeMarkers(transactions, currency).map((marker) => {
    const rows = [
      { label: "Date", value: marker.time },
      {
        label: "Quantity",
        value: formatGroupedNumber(String(marker.quantity)),
      },
      {
        label: "Avg price",
        value: `${formatUnitPrice(marker.avgPrice)} ${marker.currency}`,
      },
    ];
    const average = averageAfter.get(marker.time);
    if (marker.side === "buy" && average !== undefined) {
      rows.push({
        label: "Avg after",
        value: `${formatUnitPrice(average)} ${marker.currency}`,
      });
    }
    if (marker.fee !== null && marker.feeCurrency !== null) {
      rows.push({
        label: "Fee",
        value: `${formatGroupedNumber(marker.fee.toFixed(2))} ${marker.feeCurrency}`,
      });
    }

    return {
      time: marker.time,
      side: marker.side,
      price: marker.avgPrice,
      title: marker.side === "buy" ? "Buy" : "Sell",
      rows,
    };
  });
}

function splitChartMarkers(events: SplitEvent[]): ChartTradeMarker[] {
  return events.map((event) => ({
    time: event.tradeDate,
    side: "split",
    title: "Split",
    rows: [
      { label: "Date", value: event.tradeDate },
      { label: "Ratio", value: event.ratioLabel },
      {
        label: "Before",
        value: formatGroupedNumber(event.beforeQuantity),
      },
      {
        label: "After",
        value: formatGroupedNumber(event.afterQuantity),
      },
      {
        label: "Delta",
        value: formatSignedQuantity(event.quantityDelta),
      },
    ],
  }));
}

function tradeSide(type: Transaction["type"]): TradeSide | null {
  if (type === "Buy") return "buy";
  if (type === "Sell") return "sell";
  return null;
}
