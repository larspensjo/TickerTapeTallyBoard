import type { Holding } from "../api/types";
import { companyFigureView, reportDateView } from "./companyFiguresViewModel";

type FundamentalHolding = Pick<Holding, "fundamentals">;

/** Undefined keeps n/a and unavailable rows last with TanStack's sortUndefined. */
export function holdingValuationSortField(
  holding: FundamentalHolding,
): number | undefined {
  const figure = holding.fundamentals.pe;
  if (
    (figure.certainty !== "complete" && figure.certainty !== "approximate") ||
    figure.value === null
  )
    return undefined;
  const value = Number(figure.value);
  return Number.isFinite(value) ? value : undefined;
}

export function holdingReportSortField(
  holding: FundamentalHolding,
): number | undefined {
  const report = holding.fundamentals.next_report;
  return report.date && report.days_until !== null
    ? report.days_until
    : undefined;
}

export function compareFundamentals(
  left: number | undefined,
  right: number | undefined,
): number {
  if (left === undefined) return right === undefined ? 0 : 1;
  if (right === undefined) return -1;
  return left - right;
}

export function holdingFundamentalsView(holding: FundamentalHolding) {
  return {
    pe: companyFigureView(holding.fundamentals.pe),
    ps: companyFigureView(holding.fundamentals.ps),
    report: reportDateView(holding.fundamentals.next_report),
  };
}
