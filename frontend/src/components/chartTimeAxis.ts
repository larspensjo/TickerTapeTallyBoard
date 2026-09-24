import {
  type AreaData,
  TickMarkType,
  type Time,
  type WhitespaceData,
} from "lightweight-charts";

/**
 * The calendar-linear X axis: chart time is a calendar day, and days with no
 * observation are drawn as whitespace so spacing reflects elapsed time rather
 * than the number of stored points.
 */

export interface TimeSeriesPoint {
  time: string;
  value: number;
}

export type ChartSeriesPoint = AreaData<Time> | WhitespaceData<Time>;

const dayMs = 24 * 60 * 60 * 1000;

export function chartDate(time: Time): Date | null {
  if (typeof time === "string") {
    const parsed = new Date(`${time}T00:00:00Z`);
    return Number.isNaN(parsed.getTime()) ? null : parsed;
  }

  if (typeof time === "number") {
    return new Date(time * 1000);
  }

  return new Date(Date.UTC(time.year, time.month - 1, time.day));
}

export function isoFromTime(time: Time): string | null {
  const date = chartDate(time);
  return date ? date.toISOString().slice(0, 10) : null;
}

export function tickMarkFormatter(
  time: Time,
  tickMarkType: TickMarkType,
): string | null {
  if (tickMarkType !== TickMarkType.DayOfMonth) return null;

  const date = chartDate(time);
  if (!date) return null;

  return date.toLocaleDateString("en-US", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
  });
}

export function parseIsoDate(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;

  const [, year, month, day] = match;
  const timestamp = Date.UTC(Number(year), Number(month) - 1, Number(day));
  return Number.isNaN(timestamp) ? null : timestamp;
}

export function formatIsoDate(timestamp: number): string {
  return new Date(timestamp).toISOString().slice(0, 10);
}

export function calendarSpineData(
  data: TimeSeriesPoint[],
  rangeStart: string | undefined,
): ChartSeriesPoint[] {
  const end = data.at(-1)?.time;
  const start = rangeStart ?? data[0]?.time;
  if (!start || !end) return data;

  const startTimestamp = parseIsoDate(start);
  const endTimestamp = parseIsoDate(end);
  if (
    startTimestamp === null ||
    endTimestamp === null ||
    startTimestamp > endTimestamp
  ) {
    return data;
  }

  const pointsByDate = new Map(data.map((point) => [point.time, point]));
  const points: ChartSeriesPoint[] = [];
  for (
    let timestamp = startTimestamp;
    timestamp <= endTimestamp;
    timestamp += dayMs
  ) {
    const time = formatIsoDate(timestamp);
    points.push(pointsByDate.get(time) ?? { time });
  }

  return points;
}
