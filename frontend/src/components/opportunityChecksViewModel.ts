import type { OpportunityEvidence } from "../api/types";
import { reportDateView } from "./companyFiguresViewModel";
import { businessText } from "./opportunityBusinessText";
import {
  approximate,
  type CheckText,
  count,
  date,
  percent,
  plainText,
  points,
  price,
  reasonParts,
  unknownParts,
  words,
} from "./opportunityCheckText";

type Checks = OpportunityEvidence["checks"];
export interface OpportunityCheckEntry {
  key: keyof Checks;
  name: string;
  parts: (CheckText & { key: string })[];
  tooltip?: string;
}
const CHECK_NAMES = {
  unusual_drop: "Unusual drop",
  off_high: "Off its high",
  report_in_period: "Report in the period",
  report_coming_soon: "Report coming soon",
  business: "Business",
  market: "Market",
  stopped_falling: "Stopped falling",
  ex_dividend: "Ex-dividend",
  room_to_buy: "Room to buy",
} as const;
const MARKET_TOOLTIP =
  "The median mixes markets and currencies, includes funds, and can be self-referential in a concentrated portfolio.";

function unusualDrop(check: Checks["unusual_drop"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.unusual_drop, check.reasons);
  return [
    words(check.state),
    ...(check.adjusted_change_percent === null
      ? []
      : [
          words("; dividend-adjusted "),
          percent(check.adjusted_change_percent),
        ]),
    words("; usual move "),
    percent(check.usual_move_percent),
  ];
}
function offHigh(check: Checks["off_high"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.off_high, check.reasons);
  return [
    words(check.state === "deep" ? "deep: " : "not deep: "),
    percent(check.below_high_percent),
    words(" below the 1-year high"),
    ...(check.near_low ? [words(", near the 1-year low")] : []),
  ];
}

function lastReport(check: Checks["report_in_period"]): CheckText[] {
  const last = check.last_report;
  if (!last) return [];
  if (last.kind === "confirmed")
    return [words("confirmed report on "), date(last.date)];
  const prefix = [
    words("last report for the quarter ending "),
    date(last.period_end),
  ];
  // The no state establishes that the upper bound precedes the window. Never
  // substitute a period end or an observed midpoint for the actual report date.
  if (check.state === "no")
    return [...prefix, words(" was before "), date(check.window_start)];
  if (last.kind === "interval")
    return [
      ...prefix,
      words(" was between "),
      date(last.absent_on),
      words(" and "),
      date(last.first_seen),
    ];
  return [...prefix, words(" was on or before "), date(last.first_seen)];
}
function reportInPeriod(check: Checks["report_in_period"]): CheckText[] {
  const codes = check.reasons.map((r) => r.code);
  const pending =
    codes.includes("scheduled_report_figures_not_seen") ||
    codes.includes("app_estimate_passed_in_window");
  const remainingReasons = check.reasons.filter(
    ({ code }) =>
      !pending ||
      (code !== "scheduled_report_figures_not_seen" &&
        code !== "app_estimate_passed_in_window"),
  );
  const parts =
    check.state === "unknown" && remainingReasons.length
      ? unknownParts(CHECK_NAMES.report_in_period, remainingReasons)
      : [words(check.state)];
  if (pending) {
    const report = check.scheduled_report;
    const tag = report ? reportDateView(report).tag : null;
    const estimated =
      codes.includes("app_estimate_passed_in_window") || tag === "est.";
    parts.push(
      words(estimated ? "; estimated " : "; scheduled "),
      date(report?.date),
    );
    if (tag) parts.push(words(` (${tag})`));
    parts.push(words(", figures not seen yet"));
    return parts;
  }
  const last = lastReport(check);
  if (last.length) parts.push(words("; "), ...last);
  if (codes.includes("strict_absence"))
    parts.push(words("; no newer report in the period"));
  if (codes.includes("report_history_horizon"))
    parts.push(
      words("; no report expected before "),
      approximate(
        date(check.no_report_expected_before),
        "Estimated from report history",
      ),
      words(" (est.)"),
    );
  return parts;
}
function reportComingSoon(check: Checks["report_coming_soon"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.report_coming_soon, check.reasons);
  const view = reportDateView(check.report);
  const parts = [words(`${check.state}; `), date(check.report.date)];
  if (view.tag) parts.push(words(` (${view.tag})`));
  if (view.hint) parts.push(words(`, ${view.hint}`));
  const reasons = check.reasons.filter(
    ({ code }) =>
      view.hint !== "figures not seen yet" ||
      (code !== "estimate_passed" && code !== "report_date_passed"),
  );
  if (reasons.length)
    parts.push(
      words("; "),
      ...reasonParts(reasons, CHECK_NAMES.report_coming_soon),
    );
  return parts;
}
function market(check: Checks["market"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.market, check.reasons);
  const parts =
    check.state === "moved_with_market"
      ? [words("moved with")]
      : [
          words("fell "),
          points(check.difference_percent?.replace(/^-/, "") ?? null),
          words(
            check.state === "fell_more_than_market"
              ? " pts more than"
              : " pts less than",
          ),
        ];
  return [
    ...parts,
    words(
      ` ${check.source === "market_index" ? check.index_name : "the median of your other holdings"} (`,
    ),
    percent(check.reference_percent),
    words(")"),
    ...(check.reasons.length
      ? [words("; "), ...reasonParts(check.reasons, CHECK_NAMES.market)]
      : []),
  ];
}
function stoppedFalling(check: Checks["stopped_falling"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.stopped_falling, check.reasons);
  if (check.state === "no")
    return [words("no: low was "), date(check.low_date)];
  return [
    words("yes: up from the period low of "),
    price(check.low_close),
    words(" on "),
    date(check.low_date),
  ];
}
function exDividend(check: Checks["ex_dividend"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.ex_dividend, check.reasons);
  if (check.state === "no")
    return [words("no ex-dividend event in the covered period")];
  return [
    words("yes; dividends "),
    percent(check.percent_of_start_close),
    words(" of the start close"),
    ...(check.events ?? []).flatMap((event) => [
      words("; "),
      date(event.ex_date),
      words(`: ${event.currency} `),
      price(event.amount),
    ]),
  ];
}
function roomToBuy(check: Checks["room_to_buy"]): CheckText[] {
  if (check.state === "unknown")
    return unknownParts(CHECK_NAMES.room_to_buy, check.reasons);
  return [
    words(
      {
        room_to_buy: "below your target",
        already_at_target: "at or above your target",
        no_target: "no target set",
      }[check.state],
    ),
  ];
}

function checkParts(key: keyof Checks, checks: Checks): CheckText[] {
  if (checks[key].state === "not_applicable")
    return [
      { text: "n/a", notApplicable: true, accessibleLabel: "not applicable" },
    ];
  switch (key) {
    case "unusual_drop":
      return unusualDrop(checks.unusual_drop);
    case "off_high":
      return offHigh(checks.off_high);
    case "report_in_period":
      return reportInPeriod(checks.report_in_period);
    case "report_coming_soon":
      return reportComingSoon(checks.report_coming_soon);
    case "business":
      return businessText(checks.business);
    case "market":
      return market(checks.market);
    case "stopped_falling":
      return stoppedFalling(checks.stopped_falling);
    case "ex_dividend":
      return exDividend(checks.ex_dividend);
    case "room_to_buy":
      return roomToBuy(checks.room_to_buy);
  }
}

type HoldingDates = { start_date: string; end_date: string };
function checkTooltip(
  key: keyof Checks,
  checks: Checks,
  holdingDates?: HoldingDates,
): string | undefined {
  const check = checks[key];
  const details = [
    plainText(reasonParts(check.reasons, CHECK_NAMES[key], true)),
  ];
  switch (key) {
    case "unusual_drop":
      details.push(
        plainText([
          words("Threshold "),
          percent(checks.unusual_drop.threshold_percent),
          words("; "),
          count(checks.unusual_drop.closes),
          words(" closes; "),
          count(checks.unusual_drop.sample_moves),
          words(" historical moves"),
        ]),
      );
      break;
    case "off_high":
      details.push(
        plainText([
          words("1-year high "),
          price(checks.off_high.high_close),
          words("; 1-year low "),
          price(checks.off_high.low_close),
          words("; "),
          percent(checks.off_high.above_low_percent),
          words(" above the low; "),
          count(checks.off_high.closes),
          words(" closes"),
        ]),
      );
      break;
    case "market":
      if (checks.market.source === "market_index") {
        details.push(
          `${checks.market.index_name} is a price index without dividends.`,
        );
        if (
          !holdingDates ||
          checks.market.index_start_date !== holdingDates.start_date ||
          checks.market.index_end_date !== holdingDates.end_date
        ) {
          details.push(
            plainText([
              words("Index closes paired on "),
              date(checks.market.index_start_date),
              words(" and "),
              date(checks.market.index_end_date),
            ]),
          );
        }
        details.push(
          plainText([
            words("Comparison margin "),
            points(checks.market.margin_percent),
            words(" pts"),
          ]),
        );
      } else {
        details.push(
          MARKET_TOOLTIP,
          plainText([
            count(checks.market.count),
            words(" other holdings; comparison margin "),
            points(checks.market.margin_percent),
            words(" pts"),
          ]),
        );
      }
      break;
    case "stopped_falling":
      details.push(
        plainText([
          words("Low was "),
          count(checks.stopped_falling.low_age_trading_days),
          words(
            checks.stopped_falling.low_age_trading_days === 1
              ? " trading day ago"
              : " trading days ago",
          ),
        ]),
      );
      break;
    case "business":
      for (const [label, test] of [
        ["Margin", checks.business.profitable],
        ["Sales", checks.business.sales_not_shrinking],
        ["EPS", checks.business.earnings_not_sliding],
      ] as const) {
        const reasons = plainText(reasonParts(test.reasons, label, true));
        if (reasons) details.push(`${label}: ${reasons}`);
      }
      break;
    case "report_in_period":
    case "report_coming_soon":
    case "ex_dividend":
    case "room_to_buy":
      break;
  }
  return details.filter(Boolean).join("; ") || undefined;
}

/** Display only: state, order, dates and figures all come from the API. */
export function opportunityChecksViewModel(
  evidence: OpportunityEvidence,
  holdingDates?: HoldingDates,
): OpportunityCheckEntry[] {
  return (Object.keys(CHECK_NAMES) as (keyof Checks)[]).map((key) => ({
    key,
    name: CHECK_NAMES[key],
    parts: checkParts(key, evidence.checks).map((part, index) => ({
      ...part,
      key: `${key}-${index}`,
    })),
    tooltip: checkTooltip(key, evidence.checks, holdingDates),
  }));
}
