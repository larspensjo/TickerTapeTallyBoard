import type { PriceHistoryResponse } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface InstrumentPriceSeries {
  points: TimeSeriesPoint[];
  droppedForMissingFx: number;
  allUnavailable: boolean;
}

/**
 * Convert only available SEK-denominated price points to chart data.
 * Unavailable `close_base` values are counted, not coerced to zero, so missing
 * FX cannot draw misleading drops in the price chart.
 */
export function instrumentPriceSeries(
  response: PriceHistoryResponse | undefined,
): InstrumentPriceSeries {
  const all = response?.points ?? [];
  const points: TimeSeriesPoint[] = [];
  let dropped = 0;

  for (const point of all) {
    if (point.close_base.status === "available") {
      points.push({ time: point.date, value: Number(point.close_base.value) });
    } else {
      dropped += 1;
    }
  }

  return {
    points,
    droppedForMissingFx: dropped,
    allUnavailable: all.length > 0 && points.length === 0,
  };
}
