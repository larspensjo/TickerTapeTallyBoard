import type { PriceHistoryResponse, Transaction } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface InstrumentPriceSeries {
  points: TimeSeriesPoint[];
  allUnavailable: boolean;
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
    if (!transaction.price || existingDates.has(transaction.trade_date)) {
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
