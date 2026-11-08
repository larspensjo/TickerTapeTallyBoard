import { describe, expect, it } from "vitest";
import type { FigureReasonCode } from "../api/types";
import { reportDate } from "../test/companyFiguresFixtures";
import {
  companyFigureReason,
  companyFigureView,
  reportDateView,
} from "./companyFiguresViewModel";
import { formatFigureNumber } from "./valuationDisplay";

describe("company figure display", () => {
  it.each([
    ["complete", "24.3", "normal", ""],
    ["approximate", "24.3", "approximate", "~"],
    ["not_applicable", "n/a", "muted", ""],
    ["unavailable", "—", "muted", ""],
  ] as const)("renders %s", (certainty, text, tone, prefix) => {
    const result = companyFigureView({
      certainty,
      value:
        certainty === "complete" || certainty === "approximate"
          ? "24.34"
          : null,
      reasons:
        certainty === "approximate"
          ? [{ code: "ttm_from_three_quarters" }]
          : [],
    });
    expect(result).toMatchObject({ text, tone, prefix });
    expect(result.notApplicable).toBe(certainty === "not_applicable");
    if (certainty === "approximate") {
      expect(result.title).toBe("Built from three of four quarters");
      expect(result.accessibleLabel).toBe(
        "approximately 24.3; Built from three of four quarters",
      );
    }
  });

  it.each([
    ["1", "ratio", "1.0"],
    ["24.35", "ratio", "24.4"],
    ["999.95", "ratio", "1,000.0"],
    ["-1.25", "ratio", "-1.3"],
    ["0.12345", "percentage", "12.35%"],
    ["0.1", "percentage", "10.00%"],
    ["-0.01005", "percentage", "-1.01%"],
  ] as const)("formats %s as %s", (value, precision, expected) => {
    expect(formatFigureNumber(value, precision)).toBe(expected);
    expect(
      companyFigureView(
        { certainty: "complete", value, reasons: [] },
        precision,
      ).text,
    ).toBe(expected);
  });

  it.each([
    ["unavailable", "unavailable"],
    ["not_applicable", "not applicable"],
  ] as const)("speaks the %s state in plain words", (certainty, label) => {
    const reasons = [{ code: "no_fundamentals_source" }] as const;
    expect(
      companyFigureView({ certainty, value: null, reasons: [...reasons] })
        .accessibleLabel,
    ).toBe(`${label}; No company figures source found`);
    expect(
      reportDateView(reportDate({ certainty, reasons: [...reasons] }))
        .accessibleLabel,
    ).toBe(`${label}; No company figures source found`);
  });
});

describe("next report labels and backend day counts", () => {
  it.each([
    ["confirmed", "company_announced", "confirmed"],
    ["estimated", "nasdaq_expected", "Nasdaq expected"],
    ["estimated", "last_year_date", "est."],
    ["estimated", "typical_gap", "est."],
  ] as const)("labels %s / %s", (certainty, basis, tag) => {
    // Deliberately unrelated date and day count: the client must trust the count.
    const result = reportDateView(
      reportDate({
        certainty,
        basis,
        date: "2020-01-01",
        days_until: 8,
        reasons: [],
      }),
    );
    expect(result).toMatchObject({
      text: "2020-01-01",
      tag,
      hint: "in 8 days",
      prefix: "",
      tone: "normal",
    });
    expect(result.accessibleLabel).toContain(tag);
  });

  it.each([
    [0, "today"],
    [1, "in 1 day"],
    [-1, "1 day ago"],
    [-12, "12 days ago"],
    [null, null],
  ] as const)("displays backend days_until %s", (days_until, hint) => {
    const result = reportDateView(
      reportDate({
        certainty: "estimated",
        basis: "typical_gap",
        date: "2026-10-01",
        days_until,
        reasons: [],
      }),
    );
    expect(result.hint).toBe(hint);
  });

  it.each([
    ["confirmed", "company_announced", "report_date_passed", "confirmed"],
    ["estimated", "nasdaq_expected", "estimate_passed", "Nasdaq expected"],
    ["estimated", "typical_gap", "estimate_passed", "est."],
  ] as const)(
    "shows an unresolved %s / %s date visibly",
    (certainty, basis, code, tag) => {
      const result = reportDateView(
        reportDate({
          certainty,
          basis,
          date: "2026-10-01",
          days_until: -12,
          reasons: [{ code }],
        }),
      );
      expect(result).toMatchObject({
        text: "expected 2026-10-01",
        tag,
        hint: "figures not seen yet",
      });
      expect(result.accessibleLabel).toContain("figures not seen yet");
    },
  );

  it.each([0, 8, null])(
    "does not mark a date unresolved for backend days_until %s",
    (days_until) => {
      expect(
        reportDateView(
          reportDate({
            certainty: "estimated",
            date: "2020-01-01",
            days_until,
            reasons: [{ code: "estimate_passed" }],
          }),
        ).text,
      ).toBe("2020-01-01");
    },
  );

  it("covers observed, period-only, not-applicable and unavailable report states", () => {
    expect(
      reportDateView(
        reportDate({
          certainty: "observed",
          date: "2026-09-02",
          observed_between: ["2026-09-01", "2026-09-03"],
          reasons: [],
        }),
      ),
    ).toMatchObject({
      text: "around 2026-09-02",
      tag: null,
      title: "Observed between 2026-09-01 and 2026-09-03",
    });
    expect(
      reportDateView(
        reportDate({ certainty: "period_only", period_end: "2026-06-30" }),
      ).text,
    ).toBe("quarter ending 2026-06-30");
    expect(
      reportDateView(reportDate({ certainty: "not_applicable" })),
    ).toMatchObject({ text: "n/a", tone: "muted", notApplicable: true });
    expect(reportDateView(reportDate())).toMatchObject({
      text: "—",
      tone: "muted",
      notApplicable: false,
    });
  });
});

describe("plain-language reasons", () => {
  it("preserves an unknown reason code and its detail", () => {
    expect(
      companyFigureReason({
        code: "new_provider_reason" as FigureReasonCode,
        detail: "EUR",
      }),
    ).toBe("new_provider_reason: EUR");
  });
  const reasons: Record<FigureReasonCode, string> = {
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
    estimate_passed:
      "The estimated report date has passed; figures not seen yet",
    report_date_passed:
      "The announced report date has passed; figures not seen yet",
    quarter_missing: "No figures reported for this quarter",
  };
  for (const [code, expected] of Object.entries(reasons)) {
    it(`explains ${code} with optional detail`, () => {
      expect(companyFigureReason({ code: code as FigureReasonCode })).toBe(
        expected,
      );
      expect(
        companyFigureReason({ code: code as FigureReasonCode, detail: "EUR" }),
      ).toBe(
        code === "source_unreachable"
          ? `${expected}; last successful update: EUR`
          : `${expected}: EUR`,
      );
    });
  }
  it("combines all reasons into the hover and accessible explanation", () => {
    const result = companyFigureView({
      certainty: "approximate",
      value: "8.25",
      reasons: [
        { code: "price_stale" },
        { code: "fx_unavailable", detail: "TWD" },
      ],
    });
    expect(result.title).toBe(
      "The price is out of date; The exchange rate is unavailable: TWD",
    );
    expect(result.accessibleLabel).toContain(result.title);
  });
});
