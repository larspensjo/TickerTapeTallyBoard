import type { Figure, HoldingsFundamentals, ReportDate } from "../api/types";

export function unavailableFigure(): Figure {
  return {
    certainty: "unavailable",
    value: null,
    reasons: [{ code: "no_fundamentals_source" }],
  };
}
export function reportDate(overrides: Partial<ReportDate> = {}): ReportDate {
  return {
    certainty: "unavailable",
    basis: null,
    date: null,
    days_until: null,
    period_end: null,
    observed_between: null,
    reasons: [{ code: "too_few_quarters" }],
    ...overrides,
  };
}
export function holdingsFundamentals(): HoldingsFundamentals {
  return {
    pe: unavailableFigure(),
    ps: unavailableFigure(),
    next_report: reportDate(),
  };
}
