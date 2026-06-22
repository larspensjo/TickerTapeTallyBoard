import type { PriceHistoryResponse } from "../api/types";
import type { TimeSeriesPoint } from "./TimeSeriesChart";

export interface InstrumentPriceSeries {
  points: TimeSeriesPoint[];
  droppedForMissingFx: number;
  allUnavailable: boolean;
}

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
