import type { PriceHistoryResponse, Transaction } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

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

function tradeSide(type: Transaction["type"]): TradeSide | null {
  if (type === "Buy") return "buy";
  if (type === "Sell") return "sell";
  return null;
}
