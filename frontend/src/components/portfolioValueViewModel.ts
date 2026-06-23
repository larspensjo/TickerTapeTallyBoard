import type { ValueHistoryPoint } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface PortfolioValueSeries {
  value: TimeSeriesPoint[];
  invested: TimeSeriesPoint[];
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

  for (const point of points) {
    value.push({ time: point.date, value: Number(point.value_base) });

    if (point.invested_base !== null) {
      const investedValue = Number(point.invested_base);
      if (Number.isFinite(investedValue)) {
        invested.push({ time: point.date, value: investedValue });
      }
    }
  }

  return { value, invested };
}
