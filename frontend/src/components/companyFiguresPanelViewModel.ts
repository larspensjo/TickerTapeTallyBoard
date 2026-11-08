import type {
  FundamentalsQuarter,
  FundamentalsSource,
  InstrumentFundamentals,
} from "../api/types";
import { companyFigureView, reportDateView } from "./companyFiguresViewModel";
import {
  formatCompactAmount,
  formatExchangeRate,
  formatGroupedNumber,
} from "./valuationDisplay";

export interface QuarterBar {
  periodEnd: string;
  dateText: string;
  label: string;
  title: string;
  missing: boolean;
  negative: boolean;
  height: number;
  bottom: number;
  text: string;
}
export function quarterChartView(
  quarters: FundamentalsQuarter[],
  metric: "sales" | "eps",
) {
  const latestCurrency = [...quarters]
    .reverse()
    .find((q) => !q.missing && q.currency)?.currency;
  const missingKey = metric === "sales" ? "sales_missing" : "eps_missing";
  const exclusions = quarters.map((q) => {
    if (
      !q.missing &&
      q.currency &&
      latestCurrency &&
      q.currency !== latestCurrency
    ) {
      return `Reporting currency ${q.currency} differs from latest report currency ${latestCurrency}`;
    }
    const figure = q[metric];
    return figure.value === null &&
      !q.missing &&
      (!q[missingKey] ||
        figure.reasons.some((r) => r.code !== "quarter_missing"))
      ? companyFigureView(figure).title || "Figures unavailable"
      : null;
  });
  const values = quarters.map((q, i) => {
    const figure = q[metric];
    const missing =
      exclusions[i] !== null ||
      q.missing ||
      q[missingKey] ||
      figure.value === null;
    return missing ? null : Number(figure.value);
  });
  const low = Math.min(0, ...values.map((v) => v ?? 0));
  const high =
    Math.max(0, ...values.map((v) => v ?? 0)) || Math.max(1, -low * 0.25);
  const range = high - low || 1;
  const axis = (-low / range) * 100;
  return {
    axis,
    slotCount: Math.max(1, quarters.length),
    bars: quarters.map((q, i): QuarterBar => {
      const value = values[i];
      const figure = companyFigureView(q[metric]);
      const text =
        value === null
          ? "—"
          : `${formatCompactAmount(q[metric].value ?? "")} ${q.currency ?? "currency unknown"}`;
      const fullText =
        value === null
          ? "—"
          : `${formatGroupedNumber((q[metric].value ?? "").replace(/\.0+$/, ""))} ${q.currency ?? "currency unknown"}`;
      return {
        periodEnd: q.period_end,
        dateText: q.period_end.slice(0, 7),
        label:
          value === null
            ? exclusions[i]
              ? `Figures excluded for the quarter ending ${q.period_end}: ${exclusions[i]}`
              : `No figures reported for the quarter ending ${q.period_end}`
            : `${metric === "sales" ? "Sales" : "EPS"} for the quarter ending ${q.period_end}: ${fullText}${figure.prefix ? "; approximate" : ""}`,
        title: exclusions[i] ?? figure.title,
        missing: value === null,
        negative: value !== null && value < 0,
        height: value === null ? 100 - axis : (Math.abs(value) / range) * 100,
        bottom:
          value !== null && value < 0
            ? axis - (Math.abs(value) / range) * 100
            : axis,
        text,
      };
    }),
  };
}
export function companyFiguresPanelView(data: InstrumentFundamentals) {
  const fx = data.fx_conversion;
  return {
    figures: [
      { label: "P/E", value: companyFigureView(data.pe) },
      { label: "P/S", value: companyFigureView(data.ps) },
      {
        label: "Profit margin",
        value: companyFigureView(data.profit_margin, "percentage"),
      },
      {
        label: "Sales growth",
        value: companyFigureView(data.sales_growth, "percentage"),
      },
    ],
    nextReport: reportDateView(data.next_report),
    lastReport: reportDateView(data.last_report),
    lastPeriod:
      data.last_report.certainty !== "period_only" &&
      data.last_report.period_end
        ? `quarter ending ${data.last_report.period_end}`
        : null,
    observedWindow: data.last_report.observed_between
      ? `Observed between ${data.last_report.observed_between.join(" and ")}`
      : null,
    fxNote:
      fx && fx.from !== fx.to
        ? `Converted ${fx.from}→${fx.to} at ${formatExchangeRate(fx.rate)} (${fx.date})${fx.from_rate_date !== fx.date || fx.to_rate_date !== fx.date ? `; rates from ${fx.from_rate_date} and ${fx.to_rate_date}` : ""}`
        : null,
    awaiting:
      data.source.awaiting_figures ||
      (data.source.pending_update && !data.source.has_active_figures)
        ? `Awaiting figures. Figures${data.source.symbol ? ` for ${data.source.symbol}` : ""} load on the next refresh.`
        : null,
    sales: quarterChartView(data.quarters, "sales"),
    eps: quarterChartView(data.quarters, "eps"),
  };
}
const SOURCE_REASONS: Record<string, string> = {
  no_match: "No matching company found",
  source_ambiguous: "More than one company matches",
  ambiguous: "More than one company matches",
  security_type_unknown: "Security type could not be established",
  no_quarterly_data: "No quarterly figures returned",
  provider_unavailable: "Provider unavailable",
  rate_limited: "Provider rate limit reached",
  not_listed: "Symbol is not listed",
  fund_or_etf: "Fund or ETF",
  not_a_company: "Marked as not a company",
  certificate: "Certificate",
};
export function fundamentalsSourceView(source: FundamentalsSource) {
  const reason = source.last_attempt_reason;
  return {
    symbol:
      source.symbol ??
      (source.classification === "NOT_COMPANY"
        ? "Not a company"
        : "No symbol selected"),
    origin: source.origin === "MANUAL" ? "Set by hand" : "Found automatically",
    securityType: source.security_type ?? "type unknown",
    outcome:
      source.last_attempt_outcome === "SUCCEEDED"
        ? "Figures updated"
        : source.last_attempt_outcome === "EMPTY"
          ? "No figures returned"
          : source.last_attempt_outcome === "FAILED"
            ? "Update failed"
            : "Not attempted yet",
    reason: reason
      ? (SOURCE_REASONS[reason] ?? reason.replaceAll("_", " "))
      : source.status === "AMBIGUOUS"
        ? "More than one company matches"
        : source.status === "NO_MATCH"
          ? "No matching company found"
          : null,
    candidates: source.candidates.map(
      (c) => `${c.symbol}${c.name ? ` — ${c.name}` : ""}`,
    ),
  };
}
