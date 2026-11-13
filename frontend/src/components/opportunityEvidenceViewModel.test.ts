import { describe, expect, it } from "vitest";
import { reportDate } from "../test/companyFiguresFixtures";
import { opportunityEvidence } from "../test/opportunityFixtures";
import { opportunityVerdictViewModel } from "./opportunityEvidenceViewModel";

const verdictCases = [
  [
    "sentiment_dip_candidate",
    "Sentiment-dip candidate",
    "candidate",
    "Unusual drop, no report in the period, business intact",
  ],
  [
    "possible_candidate_report_timing_unknown",
    "Possible candidate, report timing unknown",
    "neutral",
    "Unusual drop, report timing unknown, business intact",
  ],
  [
    "not_enough_data",
    "Not enough data",
    "neutral",
    "Less than a year of prices, report timing unknown",
  ],
  [
    "fell_for_a_reason",
    "Fell for a reason?",
    "reason",
    "Report in the period, business weakening",
  ],
  [
    "normal_movement",
    "Normal movement",
    "neutral",
    "Fall within usual moves, not far below its high",
  ],
] as const;

describe("opportunity verdict text", () => {
  it.each(verdictCases)(
    "labels and explains %s",
    (code, label, tone, because) => {
      expect(opportunityVerdictViewModel(opportunityEvidence(code))).toEqual({
        label,
        tone,
        because,
        cautions: [],
      });
    },
  );

  it("uses only the backend's deciding checks, including a deep fall from the high", () => {
    const evidence = opportunityEvidence();
    evidence.verdict.deciding = ["off_high", "report_in_period", "business"];
    evidence.checks.unusual_drop.state = "unknown";
    evidence.checks.off_high.state = "deep";
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      "Far below its high, no report in the period, business intact",
    );
  });

  it("keeps relaxed report absence qualified", () => {
    const evidence = opportunityEvidence();
    evidence.checks.report_in_period.reasons = [
      { code: "report_history_horizon" },
    ];
    evidence.checks.report_in_period.no_report_expected_before = "2026-11-30";
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      "Unusual drop, no report expected in the period, business intact",
    );
  });

  it("names non-company checks as not applicable", () => {
    const evidence = opportunityEvidence();
    evidence.checks.report_in_period.state = "not_applicable";
    evidence.checks.business.state = "not_applicable";
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      "Unusual drop, report check n/a, business check n/a",
    );
  });

  it.each([
    "sentiment_dip_candidate",
    "possible_candidate_report_timing_unknown",
  ] as const)("keeps every candidate caution for %s", (code) => {
    const evidence = opportunityEvidence(code);
    evidence.verdict.cautions = [
      "loss_making",
      "report_coming_soon",
      "already_at_target",
      "no_target",
    ];
    evidence.checks.business.state = "loss_making";
    evidence.checks.report_coming_soon.report = reportDate({
      certainty: "confirmed",
      basis: "company_announced",
      date: "2026-10-20",
      days_until: 5,
    });
    const view = opportunityVerdictViewModel(evidence);
    expect(view.cautions).toEqual([
      "Loss-making",
      "Report coming soon (confirmed)",
      "Already at target",
      "No target",
    ]);
    expect(view.because).toContain("sales and earnings holding up");
  });

  it.each([
    ["confirmed", "company_announced", "confirmed"],
    ["estimated", "nasdaq_expected", "Nasdaq expected"],
    ["estimated", "last_year_date", "est."],
    ["estimated", "typical_gap", "est."],
  ] as const)(
    "reuses the Holdings certainty label for %s/%s",
    (certainty, basis, tag) => {
      const evidence = opportunityEvidence();
      evidence.verdict.cautions = ["report_coming_soon"];
      evidence.checks.report_coming_soon.report = reportDate({
        certainty,
        basis,
        date: "2026-10-20",
        days_until: 5,
      });
      expect(opportunityVerdictViewModel(evidence).cautions).toEqual([
        `Report coming soon (${tag})`,
      ]);
    },
  );

  it("does not invent report certainty when the report has no label", () => {
    const evidence = opportunityEvidence();
    evidence.verdict.cautions = ["report_coming_soon"];
    evidence.checks.report_coming_soon.report = reportDate();
    expect(opportunityVerdictViewModel(evidence).cautions).toEqual([
      "Report coming soon",
    ]);
  });

  it("does not infer cautions from checks", () => {
    const evidence = opportunityEvidence();
    evidence.checks.business.state = "loss_making";
    evidence.checks.room_to_buy.state = "no_target";
    expect(opportunityVerdictViewModel(evidence).cautions).toEqual([]);
  });

  it("uses missing checks and reason codes, deduplicating shared reasons", () => {
    const evidence = opportunityEvidence("not_enough_data");
    evidence.verdict.missing = ["business", "report_in_period", "unusual_drop"];
    evidence.checks.business.reasons = [
      { code: "no_fundamentals_source" },
      { code: "eps_inconsistent" },
    ];
    evidence.checks.report_in_period.reasons = [
      { code: "no_fundamentals_source" },
    ];
    evidence.checks.unusual_drop.reasons = [
      { code: "price_history_too_short", count: 80, required: 120 },
    ];
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      "No company figures source, earnings figures disagree, too little price history",
    );
  });

  it.each([
    ["unusual_drop", "price_history_gap", "gaps in price history"],
    ["off_high", "split_in_lookback", "price history crosses a share split"],
    ["business", "latest_quarter_too_old", "company figures too old"],
    [
      "report_in_period",
      "observed_interval_overlaps_window",
      "report timing unknown",
    ],
    ["business", "future_reason", "business unknown"],
  ] as const)("summarizes missing %s with reason %s", (check, code, phrase) => {
    const evidence = opportunityEvidence("not_enough_data");
    evidence.verdict.missing = [check];
    evidence.checks[check].reasons = [{ code }];
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      phrase.charAt(0).toUpperCase() + phrase.slice(1),
    );
  });

  it("still names a missing check with no reasons supplied", () => {
    const evidence = opportunityEvidence("not_enough_data");
    evidence.verdict.missing = ["business"];
    evidence.checks.business.reasons = [];
    expect(opportunityVerdictViewModel(evidence).because).toBe(
      "Business unknown",
    );
  });
});
