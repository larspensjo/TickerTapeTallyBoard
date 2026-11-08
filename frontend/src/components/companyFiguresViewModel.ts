import type {
  Figure,
  FigureReason,
  FigureReasonCode,
  ReportDate,
} from "../api/types";
import { type FigurePrecision, formatFigureNumber } from "./valuationDisplay";

export interface CompanyFigureView {
  text: string;
  tone: "normal" | "approximate" | "muted";
  prefix: string;
  tag: string | null;
  hint: string | null;
  title: string;
  accessibleLabel: string;
  notApplicable: boolean;
}

const REASON_TEXT: Record<FigureReasonCode, string> = {
  fund_or_etf: "Not applicable to a fund or ETF",
  not_a_company: "Marked as not a company",
  certificate: "Not applicable to a certificate",
  no_fundamentals_source: "No company figures source found",
  source_ambiguous: "More than one company figures source matches",
  source_unreachable: "Company figures source could not be reached",
  security_type_unknown: "The type of security could not be established",
  awaiting_figures: "Company figures load on the next refresh",
  too_few_quarters: "Too few quarters of company figures",
  year_ago_quarter_missing: "The same quarter a year ago is missing",
  latest_quarter_too_old: "The latest quarter is too old",
  report_currency_changed: "The reporting currency changed between quarters",
  loss_making: "The company has no positive trailing earnings",
  non_positive_revenue: "Reported revenue is zero or negative",
  price_unavailable: "No usable price is available",
  fx_unavailable: "The exchange rate is unavailable",
  share_basis_unverified:
    "One listing unit has not been verified as one reported share",
  listing_unit_mismatch:
    "One listing unit represents a different number of reported shares; observed ratio",
  split_since_report: "A share split occurred after the latest report",
  figures_inconsistent: "Reported company figures are inconsistent",
  implausible_ratio: "The calculated ratio is implausible",
  ttm_from_three_quarters: "Built from three of four quarters",
  eps_derived_from_net_income:
    "Earnings per share derived from net income and share count",
  price_stale: "The price is out of date",
  newer_report_expected:
    "A newer report was expected; figures have not appeared yet",
  figures_disagree: "Reported earnings and net income disagree",
  estimate_passed: "The estimated report date has passed; figures not seen yet",
  report_date_passed:
    "The announced report date has passed; figures not seen yet",
  quarter_missing: "No figures reported for this quarter",
};

export function companyFigureReason(reason: FigureReason): string {
  const detail = reason.detail
    ? reason.code === "source_unreachable"
      ? `; last successful update: ${reason.detail}`
      : `: ${reason.detail}`
    : "";
  return `${REASON_TEXT[reason.code] ?? reason.code}${detail}`;
}

function view(
  text: string,
  tone: CompanyFigureView["tone"],
  reasons: FigureReason[],
  options: Partial<CompanyFigureView> = {},
): CompanyFigureView {
  const title = options.title ?? reasons.map(companyFigureReason).join("; ");
  const prefix = tone === "approximate" ? "~" : "";
  const tag = options.tag ?? null;
  const hint = options.hint ?? null;
  const accessibleText =
    text === "—" ? "unavailable" : text === "n/a" ? "not applicable" : text;
  return {
    text,
    tone,
    prefix,
    tag,
    hint,
    title,
    accessibleLabel: [
      prefix ? `approximately ${accessibleText}` : accessibleText,
      tag,
      hint,
      title,
    ]
      .filter(Boolean)
      .join("; "),
    notApplicable: false,
    ...options,
  };
}

export function companyFigureView(
  figure: Figure,
  precision: FigurePrecision = "ratio",
): CompanyFigureView {
  if (figure.certainty === "not_applicable")
    return view("n/a", "muted", figure.reasons, { notApplicable: true });
  if (figure.certainty === "unavailable" || figure.value === null)
    return view("—", "muted", figure.reasons);
  return view(
    formatFigureNumber(figure.value, precision),
    figure.certainty === "approximate" ? "approximate" : "normal",
    figure.reasons,
  );
}

export function reportDateView(report: ReportDate): CompanyFigureView {
  if (report.certainty === "not_applicable")
    return view("n/a", "muted", report.reasons, { notApplicable: true });
  if (report.certainty === "unavailable")
    return view("—", "muted", report.reasons);
  if (report.certainty === "period_only")
    return view(
      report.period_end ? `quarter ending ${report.period_end}` : "—",
      "muted",
      report.reasons,
    );
  if (!report.date) return view("—", "muted", report.reasons);
  const tag =
    report.certainty === "confirmed"
      ? "confirmed"
      : report.certainty === "estimated"
        ? report.basis === "nasdaq_expected"
          ? "Nasdaq expected"
          : "est."
        : null;
  const days = report.days_until;
  const unresolved =
    days !== null &&
    days < 0 &&
    report.reasons.some(
      ({ code }) => code === "estimate_passed" || code === "report_date_passed",
    );
  const hint = unresolved
    ? "figures not seen yet"
    : days === null
      ? null
      : days === 0
        ? "today"
        : days > 0
          ? `in ${days} ${days === 1 ? "day" : "days"}`
          : `${Math.abs(days)} ${days === -1 ? "day" : "days"} ago`;
  return view(
    unresolved
      ? `expected ${report.date}`
      : report.certainty === "observed"
        ? `around ${report.date}`
        : report.date,
    "normal",
    report.reasons,
    {
      tag,
      hint,
      title: [
        ...report.reasons.map(companyFigureReason),
        ...(report.observed_between
          ? [`Observed between ${report.observed_between.join(" and ")}`]
          : []),
      ].join("; "),
    },
  );
}
