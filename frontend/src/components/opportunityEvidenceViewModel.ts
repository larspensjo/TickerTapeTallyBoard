import type {
  OpportunityCaution,
  OpportunityDecidingCheck,
  OpportunityEvidence,
  OpportunityEvidenceReason,
  OpportunityVerdictCode,
} from "../api/types";
import { reportDateView } from "./companyFiguresViewModel";

export interface OpportunityVerdictViewModel {
  label: string;
  tone: "candidate" | "reason" | "neutral";
  because: string;
  cautions: string[];
}

const VERDICTS: Record<
  OpportunityVerdictCode,
  {
    label: string;
    tone: OpportunityVerdictViewModel["tone"];
    tier: number;
  }
> = {
  sentiment_dip_candidate: {
    label: "Sentiment-dip candidate",
    tone: "candidate",
    tier: 0,
  },
  possible_candidate_report_timing_unknown: {
    label: "Possible candidate, report timing unknown",
    tone: "neutral",
    tier: 1,
  },
  not_enough_data: { label: "Not enough data", tone: "neutral", tier: 2 },
  fell_for_a_reason: { label: "Fell for a reason?", tone: "reason", tier: 3 },
  normal_movement: { label: "Normal movement", tone: "neutral", tier: 4 },
};

const UNKNOWN_VERDICT = {
  label: "Unknown verdict",
  tone: "neutral",
  tier: 5,
} as const;

function verdictDefinition(code: OpportunityVerdictCode) {
  return Object.hasOwn(VERDICTS, code) ? VERDICTS[code] : UNKNOWN_VERDICT;
}

export function opportunityVerdictTier(
  evidence: OpportunityEvidence | null | undefined,
): number {
  return evidence == null ? 6 : verdictDefinition(evidence.verdict.code).tier;
}

const CHECK_UNKNOWN: Record<OpportunityDecidingCheck, string> = {
  unusual_drop: "unusual drop unknown",
  off_high: "distance from the high unknown",
  report_in_period: "report timing unknown",
  business: "business unknown",
};

// Only short reasons needed by the verdict summary. Full check sentences can
// consume the same typed evidence independently when the evidence line is added.
const MISSING_REASONS: Record<string, string> = {
  price_history_gap: "gaps in price history",
  split_in_lookback: "price history crosses a share split",
  insufficient_historical_moves: "too few historical moves",
  no_price_variation: "no variation in historical prices",
  non_positive_close: "invalid historical price",
  missing_latest_close: "latest price missing",
  calculation_out_of_range: "price comparison unavailable",
  no_fundamentals_source: "no company figures source",
  source_ambiguous: "company figures source unclear",
  source_unreachable: "company figures source unavailable",
  security_type_unknown: "security type unknown",
  awaiting_figures: "awaiting company figures",
  too_few_quarters: "too few quarters of figures",
  year_ago_quarter_missing: "year-ago quarter missing",
  quarter_missing: "quarterly figures missing",
  latest_quarter_too_old: "company figures too old",
  report_currency_changed: "reporting currency changed",
  expected_report_passed: "newer report expected",
  newer_report_expected: "newer report expected",
  eps_inconsistent: "earnings figures disagree",
  business_figure_unavailable: "company figures missing",
};

function missingReason(
  check: OpportunityDecidingCheck,
  reason: OpportunityEvidenceReason,
): string {
  if (reason.code === "price_history_too_short") {
    return check === "off_high"
      ? "less than a year of prices"
      : "too little price history";
  }
  // Report intervals and upper bounds remain uncertainty, never inferred dates.
  return MISSING_REASONS[reason.code] ?? CHECK_UNKNOWN[check];
}

function decidingPhrase(
  check: OpportunityDecidingCheck,
  evidence: OpportunityEvidence,
): string {
  const checks = evidence.checks;
  switch (check) {
    case "unusual_drop":
      return {
        yes: "unusual drop",
        no: "fall within usual moves",
        unknown: CHECK_UNKNOWN.unusual_drop,
      }[checks.unusual_drop.state];
    case "off_high":
      return {
        deep: "far below its high",
        not_deep: "not far below its high",
        unknown: CHECK_UNKNOWN.off_high,
      }[checks.off_high.state];
    case "report_in_period":
      return {
        yes: "report in the period",
        no: checks.report_in_period.reasons.some(
          ({ code }) => code === "report_history_horizon",
        )
          ? "no report expected in the period"
          : "no report in the period",
        unknown: CHECK_UNKNOWN.report_in_period,
        not_applicable: "report check n/a",
      }[checks.report_in_period.state];
    case "business":
      return {
        intact: "business intact",
        weakening: "business weakening",
        loss_making: "sales and earnings holding up",
        unknown: CHECK_UNKNOWN.business,
        not_applicable: "business check n/a",
      }[checks.business.state];
  }
}

export function opportunityVerdictViewModel(
  evidence: OpportunityEvidence,
): OpportunityVerdictViewModel {
  const { verdict, checks } = evidence;
  const definition = verdictDefinition(verdict.code);
  const phrases =
    definition === UNKNOWN_VERDICT
      ? []
      : verdict.code === "not_enough_data"
        ? verdict.missing.flatMap((check) => {
            const reasons = checks[check].reasons;
            return reasons.length > 0
              ? reasons.map((reason) => missingReason(check, reason))
              : [CHECK_UNKNOWN[check]];
          })
        : verdict.deciding.map((check) => decidingPhrase(check, evidence));
  const sentence = [...new Set(phrases)].join(", ");
  return {
    label: definition.label,
    tone: definition.tone,
    because: sentence.charAt(0).toUpperCase() + sentence.slice(1),
    cautions: verdict.cautions.map((caution) =>
      cautionLabel(caution, evidence),
    ),
  };
}

function cautionLabel(
  caution: OpportunityCaution,
  evidence: OpportunityEvidence,
): string {
  switch (caution) {
    case "loss_making":
      return "Loss-making";
    case "already_at_target":
      return "Already at target";
    case "no_target":
      return "No target";
    case "report_coming_soon": {
      const tag = reportDateView(evidence.checks.report_coming_soon.report).tag;
      return tag ? `Report coming soon (${tag})` : "Report coming soon";
    }
  }
}
