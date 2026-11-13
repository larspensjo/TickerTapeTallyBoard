import { expect, it } from "vitest";
import type { OpportunityEvidenceReason } from "../api/types";
import { checkReason, plainText } from "./opportunityCheckText";
import { formatPercent, formatPercentagePoints } from "./valuationDisplay";

// Public reason payloads from evidence/response.rs, including reasons passed
// through from company figures, report dates and conviction targets.
const reasons: [OpportunityEvidenceReason, string][] = [
  [
    { code: "price_history_gap", start: "2026-10-01" },
    "gap in price history starting 1 Oct",
  ],
  [
    { code: "split_in_lookback", date: "2026-10-02" },
    "share split on 2 Oct inside the look-back; earlier prices may not be adjusted",
  ],
  [
    { code: "price_history_too_short", count: 80, required: 120 },
    "too few closes: 80; need 120",
  ],
  [
    { code: "insufficient_historical_moves", count: 1 },
    "too few historical moves: 1",
  ],
  [
    { code: "non_positive_close", date: "2026-10-03" },
    "zero or negative close on 3 Oct",
  ],
  [
    { code: "insufficient_market_rows", count: 3, required: 5 },
    "too few other holdings for the market comparison: 3; need 5",
  ],
  [{ code: "missing_latest_close" }, "latest price missing"],
  [{ code: "no_price_variation" }, "no variation in historical prices"],
  [
    { code: "missing_window_closes" },
    "prices missing at the period boundaries",
  ],
  [
    { code: "calculation_out_of_range" },
    "calculation outside the supported range",
  ],
  [{ code: "fund_or_etf" }, "not applicable to a fund or ETF"],
  [{ code: "certificate" }, "not applicable to a certificate"],
  [{ code: "not_a_company" }, "marked as not a company"],
  [{ code: "no_fundamentals_source" }, "no company figures source found"],
  [
    { code: "source_ambiguous" },
    "more than one company figures source matches",
  ],
  [
    { code: "source_unreachable" },
    "company figures source could not be reached",
  ],
  [
    { code: "source_unreachable", detail: "2026-10-01" },
    "company figures source could not be reached; last successful update 1 Oct",
  ],
  [
    { code: "security_type_unknown" },
    "the type of security could not be established",
  ],
  [{ code: "awaiting_figures" }, "company figures load on the next refresh"],
  [{ code: "too_few_quarters" }, "too few quarters of company figures"],
  [
    { code: "year_ago_quarter_missing" },
    "the same quarter a year ago is missing",
  ],
  [{ code: "quarter_missing" }, "no figures reported for this quarter"],
  [{ code: "latest_quarter_too_old" }, "the latest quarter is too old"],
  [
    { code: "report_currency_changed" },
    "the reporting currency changed between quarters",
  ],
  [
    { code: "expected_report_passed" },
    "expected report date has passed; newer figures not seen yet",
  ],
  [
    { code: "newer_report_expected" },
    "a newer report was expected; figures have not appeared yet",
  ],
  [{ code: "non_positive_revenue" }, "reported revenue is zero or negative"],
  [{ code: "business_figure_unavailable" }, "company figure unavailable"],
  [
    { code: "split_since_report" },
    "a share split occurred after the latest report",
  ],
  [
    { code: "eps_inconsistent" },
    "EPS disagrees too much with net income and diluted shares",
  ],
  [{ code: "eps_disagree", detail: "12.34" }, "EPS disagreement 12.34%"],
  [{ code: "eps_not_cross_checked" }, "EPS not cross-checked"],
  [
    { code: "eps_derived_from_net_income" },
    "earnings per share derived from net income and share count",
  ],
  [{ code: "ttm_from_three_quarters" }, "built from three of four quarters"],
  [{ code: "confirmed_report" }, "confirmed report in the period"],
  [
    { code: "observed_report_inside_window" },
    "the observed report interval is inside the period",
  ],
  [
    { code: "observed_interval_overlaps_window" },
    "the observed report interval only partly overlaps the period",
  ],
  [
    { code: "first_fetch_without_baseline" },
    "figures first seen without an earlier check for their absence",
  ],
  [
    { code: "figures_without_absence_baseline" },
    "figures seen without an earlier check for their absence",
  ],
  [
    { code: "observation_after_as_of" },
    "figures were first seen after the period end",
  ],
  [
    { code: "scheduled_report_figures_not_seen" },
    "scheduled report, figures not seen yet",
  ],
  [
    { code: "app_estimate_passed_in_window" },
    "estimated report date passed in the period; figures not seen yet",
  ],
  [
    { code: "last_report_upper_bound_unavailable" },
    "no upper bound known for the last report",
  ],
  [{ code: "strict_absence" }, "no newer report in the period"],
  [
    { code: "report_history_horizon" },
    "no report expected yet, based on report history",
  ],
  [
    { code: "next_quarter_ended_no_recent_check" },
    "the next quarter has ended without a recent figures check",
  ],
  [{ code: "next_report_unavailable" }, "next report date unavailable"],
  [
    { code: "estimate_passed" },
    "the estimated report date has passed; figures not seen yet",
  ],
  [
    { code: "report_date_passed" },
    "the announced report date has passed; figures not seen yet",
  ],
  [
    { code: "dividends_not_reported" },
    "the price source does not report dividends",
  ],
  [
    { code: "invalid_dividend_interval" },
    "the dividend period or start price is invalid",
  ],
  [{ code: "dividend_capture_invalid" }, "dividend capture failed validation"],
  [
    { code: "dividend_coverage_missing" },
    "the dividend range is not fully covered",
  ],
  [
    { code: "dividend_currency_mismatch" },
    "dividend and start-price currencies differ",
  ],
  [{ code: "no_target" }, "no target set"],
  [{ code: "target_pool_empty" }, "the target pool has no eligible holdings"],
  [{ code: "target_pool_zero" }, "the target pool has zero value"],
  [{ code: "valuation_unavailable" }, "holding valuation unavailable"],
  [{ code: "price_mapping_disabled" }, "price source disabled"],
  [
    { code: "current_value_collapsed_to_zero" },
    "the holding's value is zero or negative",
  ],
  [{ code: "no_usable_price" }, "no usable positive price"],
];

it.each(reasons)(
  "translates reason %j without exposing provider codes",
  (reason, expected) => {
    expect(plainText(checkReason(reason, "Business", true))).toBe(expected);
  },
);

it.each(["future_reason", "toString", "constructor", "__proto__"])(
  "uses the honest unknown fallback for %s",
  (code) => {
    expect(plainText(checkReason({ code }, "Market"))).toBe(
      "reason not described",
    );
  },
);

it("formats percentage points and already-scaled percentages with the shared precision rule", () => {
  expect(formatPercentagePoints("12.345")).toBe("12.35");
  expect(formatPercent("12.345")).toBe("12.35%");
  expect(formatPercent("-0.005")).toBe("-0.01%");
});
