import type { DateRange, ValueHistoryPoint } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface PortfolioValueSeries {
  value: TimeSeriesPoint[];
  invested: TimeSeriesPoint[];
  gain: TimeSeriesPoint[];
}

export function filterValueHistoryPoints(
  points: ValueHistoryPoint[],
  range: DateRange,
): ValueHistoryPoint[] {
  return points.filter((point) => {
    if (range.startDate && point.date < range.startDate) return false;
    if (range.endDate && point.date > range.endDate) return false;
    return true;
  });
}

/**
 * Split the value-history response into the total-value series and the net
 * invested-capital reference series. Invested points are dropped when the
 * backend could not derive them (`invested_base === null`) so the reference
 * line renders a gap instead of a fabricated value.
 */
export function portfolioValueSeries(
  points: ValueHistoryPoint[],
): PortfolioValueSeries {
  const value: TimeSeriesPoint[] = [];
  const invested: TimeSeriesPoint[] = [];
  const investedByDate = new Map<string, number>();

  for (const point of points) {
    const valueAmount = Number(point.value_base);
    if (Number.isFinite(valueAmount)) {
      value.push({ time: point.date, value: valueAmount });
    }

    if (point.invested_base !== null) {
      const investedValue = Number(point.invested_base);
      if (Number.isFinite(investedValue)) {
        invested.push({ time: point.date, value: investedValue });
        investedByDate.set(point.date, investedValue);
      }
    }
  }

  const gain: TimeSeriesPoint[] = [];
  for (const v of value) {
    const inv = investedByDate.get(v.time);
    if (inv !== undefined) {
      gain.push({ time: v.time, value: v.value - inv });
    }
  }

  return { value, invested, gain };
}
