import type {
  OpportunityBusinessSubtest,
  OpportunityEvidence,
  OpportunityEvidenceReason,
} from "../api/types";
import {
  approximate,
  type CheckText,
  checkReason,
  percent,
  plainText,
  price,
  reasonParts,
  unknownParts,
  words,
} from "./opportunityCheckText";

/** Reasons that tell the reader the same thing are stated once. */
function reasonsWithDistinctMeaning(
  reasons: OpportunityEvidenceReason[],
  visibleReasons: OpportunityEvidenceReason[],
): OpportunityEvidenceReason[] {
  const reportAlreadyPassed = visibleReasons.some(
    ({ code }) => code === "expected_report_passed",
  );
  return reasons.filter(
    ({ code }) => !reportAlreadyPassed || code !== "newer_report_expected",
  );
}

function speakVisibleExplanationsOnce(parts: CheckText[]): CheckText[] {
  const visibleText = plainText(parts);
  return parts.map((part) =>
    part.approximate && part.title && visibleText.includes(part.title)
      ? { ...part, accessibleLabel: `approximately ${part.text.slice(1)}` }
      : part,
  );
}

function subtest(
  test: OpportunityBusinessSubtest,
  label: "margin" | "sales" | "EPS",
  parentReasons: OpportunityEvidenceReason[],
  visibleReasonsInCheck: OpportunityEvidenceReason[],
): CheckText[] {
  const explanation =
    plainText(reasonParts(test.reasons, label)) || "Approximate company figure";
  const format = (value: string | null) => {
    let part = label === "EPS" ? price(value) : percent(value);
    if (
      label === "sales" &&
      !test.approximate &&
      value !== null &&
      !value.startsWith("-") &&
      !/^0(?:\.0*)?$/.test(value)
    )
      part = { ...part, text: `+${part.text}` };
    return test.approximate && value !== null
      ? approximate(part, explanation)
      : part;
  };
  const visibleReasons = reasonsWithDistinctMeaning(
    test.reasons,
    visibleReasonsInCheck,
  ).filter(
    (reason) =>
      !parentReasons.some(
        (parent) =>
          plainText(checkReason(parent, "Business")) ===
          plainText(checkReason(reason, "Business")),
      ),
  );
  const parts: CheckText[] = [words(label)];
  if (test.passes === null) {
    parts.push(words(" unknown"));
    if (visibleReasons.length)
      parts.push(words("; "), ...reasonParts(visibleReasons, label));
    if (test.value === null && test.year_ago_value === null) return parts;
  } else if (!test.passes) {
    if (label === "sales") parts.push(words(" shrinking"));
    if (label === "EPS") parts.push(words(" sliding"));
  }
  parts.push(words(" "), format(test.value));
  if (label === "EPS")
    parts.push(
      words(" vs "),
      format(test.year_ago_value),
      words(" a year ago"),
    );
  if (label === "sales") parts.push(words(" vs a year ago"));
  return parts;
}

export function businessText(
  check: OpportunityEvidence["checks"]["business"],
): CheckText[] {
  const visibleReasons = [
    ...(check.state === "unknown" ? check.reasons : []),
    ...[
      check.profitable,
      check.sales_not_shrinking,
      check.earnings_not_sliding,
    ].flatMap((test) => (test.passes === null ? test.reasons : [])),
  ];
  // Not-applicable checks are handled before this formatter is called.
  const parts =
    check.state === "unknown"
      ? unknownParts(
          "Business",
          reasonsWithDistinctMeaning(check.reasons, visibleReasons),
        )
      : [words(check.state === "loss_making" ? "loss-making" : check.state)];
  parts.push(
    words(check.state === "unknown" ? "; " : ": "),
    ...subtest(check.profitable, "margin", check.reasons, visibleReasons),
    words(", "),
    ...subtest(
      check.sales_not_shrinking,
      "sales",
      check.reasons,
      visibleReasons,
    ),
    words(", "),
    ...subtest(
      check.earnings_not_sliding,
      "EPS",
      check.reasons,
      visibleReasons,
    ),
  );
  const eps = check.eps_consistency;
  if (eps.state === "not_cross_checked")
    parts.push(words("; EPS not cross-checked"));
  if (eps.state === "approximate" || eps.state === "inconsistent") {
    parts.push(words("; EPS disagreement "), percent(eps.gap_percent));
    if (eps.state === "inconsistent")
      parts.push(words("; earnings comparison unavailable"));
  }
  return speakVisibleExplanationsOnce(parts);
}
