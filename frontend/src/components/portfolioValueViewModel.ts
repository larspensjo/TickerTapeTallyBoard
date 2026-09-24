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

// ---------------------------------------------------------------------------
// Period-rebased gain
//
// An observation is a point the backend could value completely; the anchor is
// the last observation strictly before the range; and the drawn line carries an
// explicit zero origin at the period boundary, so it starts at zero while the
// measured content still starts at the range start.
// ---------------------------------------------------------------------------

const dayMs = 24 * 60 * 60 * 1000;

export interface ValueHistoryWindow {
  range: DateRange;
  /** Last complete observation strictly before the range start. */
  anchor: ValueHistoryPoint | null;
  /** The portfolio genuinely held nothing before the range. */
  openingKnownEmpty: boolean;
  /** In-range observations (complete points only). */
  measured: ValueHistoryPoint[];
  /** In-range points including the ones that could not be valued. */
  rangePoints: ValueHistoryPoint[];
  incompleteCount: number;
  /** First date in the WHOLE history where invested capital went unknown. */
  investedUnavailableFrom: string | null;
  /** Last observation in the whole history. */
  lastMeasuredDate: string | null;
  /** Full history, so a link can see the points it stepped over. */
  points: ValueHistoryPoint[];
}

function amount(value: string | null): number | null {
  if (value === null) return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function shiftDate(date: string, days: number): string {
  const parsed = Date.parse(`${date}T00:00:00Z`);
  if (Number.isNaN(parsed)) return date;
  return new Date(parsed + days * dayMs).toISOString().slice(0, 10);
}

export function valueHistoryWindow(
  points: ValueHistoryPoint[],
  range: DateRange,
  historyStartDate: string | null,
): ValueHistoryWindow {
  const inRange = (point: ValueHistoryPoint) =>
    (range.startDate === null || point.date >= range.startDate) &&
    (range.endDate === null || point.date <= range.endDate);

  const observations = points.filter((point) => !point.incomplete);
  const rangePoints = points.filter(inRange);
  const measured = observations.filter(inRange);

  // An incomplete point sums only the positions that could be valued, so using
  // one as the anchor would inflate the whole rebased series.
  const startDate = range.startDate;
  const anchor =
    startDate === null
      ? null
      : (observations.filter((point) => point.date < startDate).at(-1) ?? null);

  const openingKnownEmpty =
    startDate === null ||
    historyStartDate === null ||
    startDate <= historyStartDate;

  return {
    range,
    anchor,
    openingKnownEmpty,
    measured,
    rangePoints,
    incompleteCount: rangePoints.filter((point) => point.incomplete).length,
    investedUnavailableFrom:
      points.find((point) => point.invested_base === null)?.date ?? null,
    lastMeasuredDate: observations.at(-1)?.date ?? null,
    points,
  };
}

export interface PeriodValueSeries {
  value: TimeSeriesPoint[];
  invested: TimeSeriesPoint[];
}

/**
 * The Value tab plots measured points only, so a day the backend could not
 * value fully never renders as a plunge and recovery, and the two tabs can
 * never disagree about whether such a day happened.
 */
export function periodValueSeries(
  window: ValueHistoryWindow,
): PeriodValueSeries {
  const value: TimeSeriesPoint[] = [];
  const invested: TimeSeriesPoint[] = [];

  for (const point of window.measured) {
    const valueAmount = amount(point.value_base);
    if (valueAmount !== null) {
      value.push({ time: point.date, value: valueAmount });
    }

    const investedAmount = amount(point.invested_base);
    if (investedAmount !== null) {
      invested.push({ time: point.date, value: investedAmount });
    }
  }

  return { value, invested };
}

export type PeriodGainStatus =
  | "available"
  | "no_history"
  | "all_incomplete"
  | "unknown_opening"
  | "missing_trade_fx";

export interface PeriodGainSeries {
  status: PeriodGainStatus;
  sek: TimeSeriesPoint[];
  percent: TimeSeriesPoint[];
  /** The percentage could not be chained exactly over part of this range. */
  approximate: boolean;
  originDate: string | null;
  endsEarlyAt: string | null;
  investedUnavailableAfter: string | null;
  investedUnavailableFrom: string | null;
}

interface Observation {
  date: string;
  value: number;
  invested: number;
}

function gainUnavailable(
  status: PeriodGainStatus,
  window: ValueHistoryWindow,
): PeriodGainSeries {
  return {
    status,
    sek: [],
    percent: [],
    approximate: false,
    originDate: null,
    endsEarlyAt: null,
    investedUnavailableAfter: null,
    investedUnavailableFrom: window.investedUnavailableFrom,
  };
}

export function periodGainSeries(window: ValueHistoryWindow): PeriodGainSeries {
  if (window.rangePoints.length === 0) {
    return gainUnavailable("no_history", window);
  }
  if (window.measured.length === 0) {
    return gainUnavailable("all_incomplete", window);
  }
  if (!window.openingKnownEmpty && window.anchor === null) {
    return gainUnavailable("unknown_opening", window);
  }

  const usable: Observation[] = [];
  for (const point of window.measured) {
    const value = amount(point.value_base);
    const invested = amount(point.invested_base);
    if (value === null || invested === null) break;
    usable.push({ date: point.date, value, invested });
  }

  const firstMeasured = usable[0];
  if (!firstMeasured) return gainUnavailable("missing_trade_fx", window);

  const anchorValue = window.anchor ? amount(window.anchor.value_base) : 0;
  const anchorInvested = window.anchor
    ? amount(window.anchor.invested_base)
    : 0;
  if (anchorValue === null || anchorInvested === null) {
    return gainUnavailable("missing_trade_fx", window);
  }

  const opening: Observation = {
    date: window.anchor?.date ?? "",
    value: anchorValue,
    invested: anchorInvested,
  };

  // The origin is the period boundary, drawn separately from the first day's
  // measured result. It is always strictly before the range start, so it can
  // never collide with a measured point.
  const originDate = shiftDate(
    window.range.startDate ?? firstMeasured.date,
    -1,
  );

  const sek: TimeSeriesPoint[] = [{ time: originDate, value: 0 }];
  const percent: TimeSeriesPoint[] = [{ time: originDate, value: 0 }];

  let chained = 1;
  let approximate = false;
  let previous = opening;

  for (const observation of usable) {
    sek.push({
      time: observation.date,
      value:
        observation.value -
        opening.value -
        (observation.invested - opening.invested),
    });

    const flow = observation.invested - previous.invested;
    let factor: number;
    if (previous.value <= 0) {
      // No usable denominator. A known-empty opening lands here by
      // construction and is not an approximation: money arriving is not
      // performance, so the first measured point reads exactly 0.00%.
      factor = 1;
      if (window.anchor !== null || previous !== opening) {
        approximate = true;
      }
    } else if (observation.value - flow > 0) {
      factor = (observation.value - flow) / previous.value;
    } else if (previous.value + flow > 0) {
      // The end-of-period flow assumption broke down. The start-of-period form
      // is well defined here and cannot produce a loss worse than -100%, but
      // the answer depends on when in the step the money actually moved.
      factor = observation.value / (previous.value + flow);
      approximate = true;
    } else {
      factor = 1;
      approximate = true;
    }

    if (previous.date !== "" && skippedFlow(window, previous, observation)) {
      approximate = true;
    }

    chained *= factor;
    percent.push({ time: observation.date, value: (chained - 1) * 100 });
    previous = observation;
  }

  const lastUsable = usable.at(-1)?.date ?? null;
  const lastMeasured = window.measured.at(-1)?.date ?? null;
  const endDate = window.range.endDate;

  return {
    status: "available",
    sek,
    percent,
    approximate,
    originDate,
    endsEarlyAt:
      endDate !== null && lastMeasured !== null && lastMeasured < endDate
        ? lastMeasured
        : null,
    investedUnavailableAfter:
      lastUsable !== null && lastMeasured !== null && lastUsable < lastMeasured
        ? lastUsable
        : null,
    investedUnavailableFrom: window.investedUnavailableFrom,
  };
}

/**
 * Did money move across a point this link stepped over? Chaining only the
 * measured points is exact when invested capital is unchanged across every
 * skipped point; otherwise the trade cannot be separated from the market move.
 */
function skippedFlow(
  window: ValueHistoryWindow,
  previous: Observation,
  current: Observation,
): boolean {
  return window.points.some(
    (point) =>
      point.date > previous.date &&
      point.date < current.date &&
      amount(point.invested_base) !== previous.invested,
  );
}

const PERIOD_NAMES: Record<string, string> = {
  today: "today",
  "7d": "last 7 days",
  "12m": "last 12 months",
  ytd: "year to date",
  all: "all time",
};

export interface ChartHeadingPeriod {
  preset: string;
  startDate: string | null;
  endDate: string | null;
}

/**
 * The heading names a period only when the displayed response actually
 * resolved one; a stale or absent period drops the clause rather than
 * labelling one selection's numbers with another's name.
 */
export function chartPanelHeading({
  view,
  unit,
  period,
}: {
  view: "value" | "gain";
  unit: "sek" | "percent";
  period: ChartHeadingPeriod | null;
}): string {
  const subject = view === "gain" ? "Portfolio gain" : "Portfolio value";
  const suffix = view === "gain" && unit === "percent" ? "(%)" : "(SEK)";
  const name = period ? periodName(period) : null;

  return name ? `${subject}, ${name} ${suffix}` : `${subject} ${suffix}`;
}

function periodName(period: ChartHeadingPeriod): string {
  const named = PERIOD_NAMES[period.preset];
  if (named) return named;
  if (period.startDate && period.endDate) {
    return `${period.startDate} to ${period.endDate}`;
  }
  return "custom range";
}

/**
 * The chart series is ex-dividend while the Gains totals include income, so the
 * caveat is shown, but only when the period actually contains dividends.
 */
export function dividendCaveatApplies(
  waterfall:
    | {
        income_base: { status: string; value?: string };
        income_not_tracked: boolean;
      }
    | undefined,
): boolean {
  if (!waterfall || waterfall.income_not_tracked) return false;
  if (waterfall.income_base.status !== "available") return false;

  const income = Number(waterfall.income_base.value);
  return Number.isFinite(income) && income !== 0;
}
