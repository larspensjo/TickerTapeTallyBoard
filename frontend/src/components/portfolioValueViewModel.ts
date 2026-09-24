import type { DateRange, ValueHistoryPoint } from "../api/types";
import type { TimeSeriesPoint } from "./chartTimeAxis";

export interface ReferenceEdgeTag {
  side: "above" | "below";
  value: number;
}

/**
 * Decide whether the invested-capital reference line needs an edge tag because
 * it falls outside the drawn value band.
 *
 * The test is whether the two ranges are *disjoint*, not whether any reference
 * point lies inside the value range: a line that crosses the band is partly on
 * screen, needs no tag, and has no single direction to name. The reported value
 * is the reference level at the latest drawn date, which is what the tag reads.
 */
export function referenceEdgeTag(
  value: TimeSeriesPoint[],
  reference: TimeSeriesPoint[],
): ReferenceEdgeTag | null {
  const latest = reference.at(-1)?.value;
  if (value.length === 0 || reference.length === 0 || latest === undefined) {
    return null;
  }

  const valueAmounts = value.map((point) => point.value);
  const referenceAmounts = reference.map((point) => point.value);
  const valueMin = Math.min(...valueAmounts);
  const valueMax = Math.max(...valueAmounts);
  const referenceMin = Math.min(...referenceAmounts);
  const referenceMax = Math.max(...referenceAmounts);

  if (referenceMax < valueMin) return { side: "below", value: latest };
  if (referenceMin > valueMax) return { side: "above", value: latest };
  return null;
}

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
