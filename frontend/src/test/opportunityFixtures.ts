import type {
  OpportunityEvidence,
  OpportunityRow,
  OpportunityVerdictCode,
} from "../api/types";
import { reportDate } from "./companyFiguresFixtures";

export function opportunityEvidence(
  code: OpportunityVerdictCode = "sentiment_dip_candidate",
): OpportunityEvidence {
  const subtest = () => ({
    passes: true,
    value: "1",
    year_ago_value: "1",
    approximate: false,
    reasons: [],
  });
  const evidence: OpportunityEvidence = {
    verdict: {
      code,
      deciding: ["unusual_drop", "report_in_period", "business"],
      missing: [],
      cautions: [],
    },
    checks: {
      unusual_drop: {
        state: "yes",
        reasons: [],
        change_percent: "-8.00",
        adjusted_change_percent: "-8.00",
        usual_move_percent: "2.00",
        threshold_percent: "4.00",
        closes: 250,
        sample_moves: 240,
      },
      off_high: {
        state: "not_deep",
        reasons: [],
        closes: 250,
        latest_close: "92",
        high_close: "100",
        low_close: "80",
        below_high_percent: "8.00",
        above_low_percent: "15.00",
        near_low: false,
      },
      report_in_period: {
        state: "no",
        reasons: [{ code: "strict_absence" }],
        window_start: "2026-10-01",
        window_end: "2026-10-15",
        last_report: { kind: "confirmed", date: "2026-09-01" },
        scheduled_report: null,
        no_report_expected_before: null,
      },
      report_coming_soon: { state: "no", reasons: [], report: reportDate() },
      business: {
        state: "intact",
        reasons: [],
        profitable: subtest(),
        sales_not_shrinking: subtest(),
        earnings_not_sliding: subtest(),
        eps_consistency: {
          state: "consistent",
          gap_percent: "0.00",
          compared_quarters: 2,
        },
      },
      market: {
        state: "fell_more_than_market",
        reasons: [],
        source: "other_holdings_median",
        change_percent: "-8.00",
        reference_percent: "-1.00",
        difference_percent: "-7.00",
        margin_percent: "2.00",
        count: 8,
      },
      stopped_falling: {
        state: "unknown",
        reasons: [{ code: "missing_window_closes" }],
        latest_close: null,
        low_close: null,
        low_date: null,
        low_age_trading_days: null,
      },
      ex_dividend: {
        state: "unknown",
        reasons: [{ code: "dividend_coverage_missing" }],
        events: null,
        amount: null,
        percent_of_start_close: null,
      },
      room_to_buy: {
        state: "room_to_buy",
        reasons: [],
        target_status: "below",
      },
    },
  };
  switch (code) {
    case "possible_candidate_report_timing_unknown":
      evidence.checks.report_in_period.state = "unknown";
      evidence.checks.report_in_period.reasons = [
        { code: "last_report_upper_bound_unavailable" },
      ];
      break;
    case "not_enough_data":
      evidence.checks.unusual_drop.state = "no";
      evidence.checks.off_high.state = "unknown";
      evidence.checks.off_high.reasons = [
        { code: "price_history_too_short", count: 140, required: 200 },
      ];
      evidence.checks.report_in_period.state = "unknown";
      evidence.checks.report_in_period.reasons = [
        { code: "first_fetch_without_baseline" },
      ];
      evidence.verdict.deciding = ["off_high", "report_in_period"];
      evidence.verdict.missing = ["off_high", "report_in_period"];
      break;
    case "fell_for_a_reason":
      evidence.verdict.deciding = ["report_in_period", "business"];
      evidence.checks.report_in_period.state = "yes";
      evidence.checks.report_in_period.reasons = [{ code: "confirmed_report" }];
      evidence.checks.business.state = "weakening";
      break;
    case "normal_movement":
      evidence.verdict.deciding = ["unusual_drop", "off_high"];
      evidence.checks.unusual_drop.state = "no";
      break;
  }
  return evidence;
}

export function opportunityFixture(
  id: number,
  evidence: OpportunityEvidence | null = opportunityEvidence(),
): OpportunityRow {
  const unavailable = {
    status: "unavailable",
    reasons: ["no_target"],
  } as const;
  return {
    evidence,
    instrument: {
      id,
      symbol: `STOCK${id}`,
      ticker: `STOCK${id}`,
      exchange: "STO",
      name: `Company ${id}`,
      type: "Stock",
      currency: "SEK",
      conviction: "High",
    },
    quantity: 10,
    latest_close: {
      date: "2026-10-15",
      close: "92",
      currency: "SEK",
      source: "YAHOO",
      freshness: "fresh",
    },
    price_change: {
      status: "available",
      change_percent: "-8.00",
      start_date: "2026-10-08",
      start_close: "100",
      end_date: "2026-10-15",
      end_close: "92",
    },
    average_cost_native: "100.00",
    price_vs_average_percent: { status: "available", value: "-8.00" },
    below_average: true,
    unrealized_gain_percent: { status: "available", value: "-8.00" },
    unrealized_gain_freshness: "fresh",
    conviction_target: {
      conviction: "High",
      status: "no_target",
      target_value_base: { ...unavailable, reasons: [...unavailable.reasons] },
      target_gap_base: { ...unavailable, reasons: [...unavailable.reasons] },
      target_gap_percent: { ...unavailable, reasons: [...unavailable.reasons] },
    },
  };
}

/** Long demo-style labels with populated numeric columns and target bars. */
export function opportunitiesWithLongVerdicts(): OpportunityRow[] {
  const cases = [
    ["Alba Renewables", "sentiment_dip_candidate"],
    ["Harbor Utilities", "possible_candidate_report_timing_unknown"],
    ["Nordic Automation", "not_enough_data"],
    ["Sprout Diagnostics", "fell_for_a_reason"],
    ["Varvsgatan Industri", "normal_movement"],
  ] as const;
  return cases.map(([name, code], index) => {
    const evidence = opportunityEvidence(code);
    const row = opportunityFixture(index + 1, evidence);
    row.instrument.name = name;
    row.conviction_target = {
      conviction: "High",
      status: "below",
      target_value_base: { status: "available", value: "1000.00" },
      target_gap_base: { status: "available", value: "-80.00" },
      target_gap_percent: { status: "available", value: "-8.00" },
    };
    if (code === "sentiment_dip_candidate") {
      evidence.verdict.deciding = [
        "unusual_drop",
        "off_high",
        "report_in_period",
        "business",
      ];
      evidence.checks.off_high.state = "deep";
      evidence.checks.off_high.high_close = "140";
      evidence.checks.off_high.below_high_percent = "34.29";
      evidence.checks.business.state = "loss_making";
      evidence.checks.business.profitable.passes = false;
      evidence.checks.business.profitable.value = "-1.00";
      evidence.checks.business.profitable.year_ago_value = null;
      evidence.checks.business.earnings_not_sliding.value = "-0.80";
      evidence.checks.business.earnings_not_sliding.year_ago_value = "-1.00";
      evidence.verdict.cautions = ["loss_making"];
    } else if (code === "possible_candidate_report_timing_unknown") {
      evidence.verdict.cautions = ["report_coming_soon", "already_at_target"];
      evidence.checks.report_coming_soon.state = "yes";
      evidence.checks.report_coming_soon.report = reportDate({
        certainty: "estimated",
        basis: "nasdaq_expected",
        date: "2026-10-20",
        days_until: 5,
        reasons: [],
      });
      evidence.checks.room_to_buy.state = "already_at_target";
      evidence.checks.room_to_buy.target_status = "above";
      row.conviction_target = {
        conviction: "High",
        status: "above",
        target_value_base: { status: "available", value: "900.00" },
        target_gap_base: { status: "available", value: "20.00" },
        target_gap_percent: { status: "available", value: "2.22" },
      };
    } else if (code === "fell_for_a_reason") {
      // A report alone can decide this verdict, giving a short explanation.
      evidence.verdict.deciding = ["report_in_period"];
      evidence.checks.report_in_period.last_report = {
        kind: "confirmed",
        date: "2026-10-12",
      };
      evidence.checks.business.state = "intact";
    }
    return row;
  });
}
