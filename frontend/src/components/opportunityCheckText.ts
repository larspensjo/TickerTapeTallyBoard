import type { OpportunityEvidenceReason } from "../api/types";
import { COMPANY_FIGURE_REASON_TEXT } from "./companyFiguresViewModel";
import { formatShortDate } from "./opportunityDateDisplay";
import {
  formatGroupedNumber,
  formatPercent,
  formatPercentagePoints,
  formatUnitPrice,
} from "./valuationDisplay";

export interface CheckText {
  text: string;
  figure?: boolean;
  approximate?: boolean;
  notApplicable?: boolean;
  title?: string;
  accessibleLabel?: string;
}
export const words = (text: string): CheckText => ({ text });
export const count = (value: number | null | undefined): CheckText => ({
  text: value == null ? "—" : formatGroupedNumber(value),
  figure: true,
});
export const date = (value: string | null | undefined): CheckText => ({
  text: value ? formatShortDate(value) : "—",
  figure: true,
});
export const percent = (value: string | null): CheckText => ({
  text: value == null ? "—" : formatPercent(value),
  figure: true,
});
export const points = (value: string | null): CheckText => ({
  text: value == null ? "—" : formatPercentagePoints(value),
  figure: true,
});
export const price = (value: string | null): CheckText => ({
  text: value == null ? "—" : formatUnitPrice(value),
  figure: true,
});
export function approximate(part: CheckText, explanation: string): CheckText {
  return {
    ...part,
    text: `~${part.text}`,
    approximate: true,
    title: explanation,
    accessibleLabel: `approximately ${part.text}; ${explanation}`,
  };
}
export const plainText = (parts: CheckText[]): string =>
  parts.map((p) => p.text).join("");

// Reasons reaching checks through price evidence, company figures, report dates,
// dividend capture and conviction targets. Keep provider codes out of visible text.
type EvidenceReasonCode =
  | "missing_latest_close"
  | "no_price_variation"
  | "missing_window_closes"
  | "calculation_out_of_range"
  | "expected_report_passed"
  | "business_figure_unavailable"
  | "eps_inconsistent"
  | "eps_not_cross_checked"
  | "confirmed_report"
  | "observed_report_inside_window"
  | "observed_interval_overlaps_window"
  | "first_fetch_without_baseline"
  | "figures_without_absence_baseline"
  | "observation_after_as_of"
  | "scheduled_report_figures_not_seen"
  | "app_estimate_passed_in_window"
  | "last_report_upper_bound_unavailable"
  | "strict_absence"
  | "report_history_horizon"
  | "next_quarter_ended_no_recent_check"
  | "next_report_unavailable"
  | "dividends_not_reported"
  | "invalid_dividend_interval"
  | "dividend_capture_invalid"
  | "dividend_coverage_missing"
  | "dividend_currency_mismatch"
  | "no_target"
  | "target_pool_empty"
  | "target_pool_zero"
  | "valuation_unavailable"
  | "price_mapping_disabled"
  | "current_value_collapsed_to_zero"
  | "no_usable_price";
const REASONS: Record<EvidenceReasonCode, string> = {
  missing_latest_close: "latest price missing",
  no_price_variation: "no variation in historical prices",
  missing_window_closes: "prices missing at the period boundaries",
  calculation_out_of_range: "calculation outside the supported range",
  expected_report_passed:
    "expected report date has passed; newer figures not seen yet",
  business_figure_unavailable: "company figure unavailable",
  eps_inconsistent: "EPS disagrees too much with net income and diluted shares",
  eps_not_cross_checked: "EPS not cross-checked",
  confirmed_report: "confirmed report in the period",
  observed_report_inside_window:
    "the observed report interval is inside the period",
  observed_interval_overlaps_window:
    "the observed report interval only partly overlaps the period",
  first_fetch_without_baseline:
    "figures first seen without an earlier check for their absence",
  figures_without_absence_baseline:
    "figures seen without an earlier check for their absence",
  observation_after_as_of: "figures were first seen after the period end",
  scheduled_report_figures_not_seen: "scheduled report, figures not seen yet",
  app_estimate_passed_in_window:
    "estimated report date passed in the period; figures not seen yet",
  last_report_upper_bound_unavailable:
    "no upper bound known for the last report",
  strict_absence: "no newer report in the period",
  report_history_horizon: "no report expected yet, based on report history",
  next_quarter_ended_no_recent_check:
    "the next quarter has ended without a recent figures check",
  next_report_unavailable: "next report date unavailable",
  dividends_not_reported: "the price source does not report dividends",
  invalid_dividend_interval: "the dividend period or start price is invalid",
  dividend_capture_invalid: "dividend capture failed validation",
  dividend_coverage_missing: "the dividend range is not fully covered",
  dividend_currency_mismatch: "dividend and start-price currencies differ",
  no_target: "no target set",
  target_pool_empty: "the target pool has no eligible holdings",
  target_pool_zero: "the target pool has zero value",
  valuation_unavailable: "holding valuation unavailable",
  price_mapping_disabled: "price source disabled",
  current_value_collapsed_to_zero: "the holding's value is zero or negative",
  no_usable_price: "no usable positive price",
};

export function checkReason(
  reason: OpportunityEvidenceReason,
  _name: string,
  diagnostics = false,
): CheckText[] {
  switch (reason.code) {
    case "market_index_unavailable":
      return [
        words(
          `${reason.detail ?? "The market index"} has no usable closes for these dates`,
        ),
      ];
    case "price_history_gap":
      return [words("gap in price history starting "), date(reason.start)];
    case "split_in_lookback":
      return [
        words("share split on "),
        date(reason.date),
        words(" inside the look-back; earlier prices may not be adjusted"),
      ];
    case "non_positive_close":
      return [words("zero or negative close on "), date(reason.date)];
    case "price_history_too_short":
      if (!diagnostics) return [words("too few closes")];
      return [
        words("too few closes: "),
        count(reason.count),
        words("; need "),
        count(reason.required),
      ];
    case "insufficient_historical_moves":
      return diagnostics
        ? [words("too few historical moves: "), count(reason.count)]
        : [words("too few historical moves")];
    case "insufficient_market_rows":
      if (!diagnostics)
        return [words("too few other holdings for the market comparison")];
      return [
        words("too few other holdings for the market comparison: "),
        count(reason.count),
        words("; need "),
        count(reason.required),
      ];
    case "source_unreachable":
      return [
        words(
          COMPANY_FIGURE_REASON_TEXT.source_unreachable.replace(
            /^./,
            (letter) => letter.toLowerCase(),
          ),
        ),
        ...(reason.detail
          ? [words("; last successful update "), date(reason.detail)]
          : []),
      ];
    case "eps_disagree":
      return [words("EPS disagreement "), percent(reason.detail ?? null)];
    default:
      return [
        words(
          Object.hasOwn(REASONS, reason.code)
            ? REASONS[reason.code as EvidenceReasonCode]
            : Object.hasOwn(COMPANY_FIGURE_REASON_TEXT, reason.code)
              ? COMPANY_FIGURE_REASON_TEXT[
                  reason.code as keyof typeof COMPANY_FIGURE_REASON_TEXT
                ].replace(/^./, (letter) => letter.toLowerCase())
              : "reason not described",
        ),
      ];
  }
}

export function reasonParts(
  reasons: OpportunityEvidenceReason[],
  name: string,
  diagnostics = false,
): CheckText[] {
  return reasons.flatMap((reason, index) => [
    ...(index ? [words("; ")] : []),
    ...checkReason(reason, name, diagnostics),
  ]);
}
export function unknownParts(
  name: string,
  reasons: OpportunityEvidenceReason[],
): CheckText[] {
  return reasons.length
    ? reasons.length === 1 &&
      plainText(checkReason(reasons[0], name)) === "reason not described"
      ? [words("unknown (reason not described)")]
      : [words("unknown; "), ...reasonParts(reasons, name)]
    : [words("unknown (reason not described)")];
}
