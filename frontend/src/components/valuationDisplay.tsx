import type { Row } from "@tanstack/react-table";
import type { AvailabilityValue } from "../api/types";

type ValueTone = "plain" | "signed";

export function unavailableValue(reason: string): AvailabilityValue<string> {
  return { status: "unavailable", reasons: [reason] };
}

export function isAvailable<T>(
  value: AvailabilityValue<T> | undefined,
): value is { status: "available"; value: T } {
  return value?.status === "available";
}

export function availabilityNumber(value: AvailabilityValue<string>): number {
  if (value.status === "unavailable") {
    return Number.NEGATIVE_INFINITY;
  }

  const parsed = Number(value.value);
  return Number.isFinite(parsed) ? parsed : Number.NEGATIVE_INFINITY;
}

export function availabilitySortValues(
  left: AvailabilityValue<string>,
  right: AvailabilityValue<string>,
): number {
  return availabilityNumber(left) - availabilityNumber(right);
}

export function availabilitySortRows<RowData>(
  rowA: Row<RowData>,
  rowB: Row<RowData>,
  columnId: string,
): number {
  return availabilitySortValues(
    rowA.getValue<AvailabilityValue<string>>(columnId),
    rowB.getValue<AvailabilityValue<string>>(columnId),
  );
}

export function signedTone(value: string): "up" | "down" | "flat" {
  const parsed = Number(value);

  if (!Number.isFinite(parsed) || parsed === 0) {
    return "flat";
  }

  return parsed > 0 ? "up" : "down";
}

export function parseFiniteNumber(value: string | number): number | null {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

export function formatGroupedNumber(value: string | number): string {
  const rawValue = String(value).trim();
  const match = rawValue.match(/^([+-]?)(\d+)(\.\d+)?$/);

  if (!match) {
    return rawValue;
  }

  const [, sign, integerPart, fractionalPart = ""] = match;
  const groupedInteger = integerPart.replace(/\B(?=(\d{3})+(?!\d))/g, ",");

  return `${sign}${groupedInteger}${fractionalPart}`;
}

export function FormattedNumber({
  value,
  prefix = "",
  suffix = "",
}: {
  value: string | number;
  prefix?: string;
  suffix?: string;
}) {
  return (
    <>
      {prefix ? <span className="number-prefix">{prefix.trim()}</span> : null}
      {formatGroupedNumber(value)}
      {suffix}
    </>
  );
}

function numberClass(value: string, tone: ValueTone): string {
  return tone === "signed" ? `number ${signedTone(value)}` : "number";
}

export function AvailabilityValueCell({
  value,
  prefix = "",
  suffix = "",
  tone = "plain",
  unavailableLabel = "Unavailable",
}: {
  value: AvailabilityValue<string>;
  prefix?: string;
  suffix?: string;
  tone?: ValueTone;
  unavailableLabel?: string;
}) {
  if (value.status === "available") {
    return (
      <span className={numberClass(value.value, tone)}>
        <FormattedNumber value={value.value} prefix={prefix} suffix={suffix} />
      </span>
    );
  }

  return (
    <span className="status-chip warning" title={reasonSummary(value.reasons)}>
      {unavailableLabel}
    </span>
  );
}

export function SummaryAvailabilityValue({
  value,
  prefix = "",
  suffix = "",
  tone = "signed",
  unavailableLabel = "Unavailable",
}: {
  value: AvailabilityValue<string> | undefined;
  prefix?: string;
  suffix?: string;
  tone?: ValueTone;
  unavailableLabel?: string;
}) {
  if (!value) {
    return <span className="status-chip warning">{unavailableLabel}</span>;
  }

  if (!isAvailable(value)) {
    return (
      <span
        className="status-chip warning"
        title={reasonSummary(value.reasons)}
      >
        {unavailableLabel}
      </span>
    );
  }

  return (
    <strong className={numberClass(value.value, tone)}>
      <FormattedNumber value={value.value} prefix={prefix} suffix={suffix} />
    </strong>
  );
}

export function reasonLabel(code: string): string {
  const normalized = code.toLowerCase();

  if (normalized.startsWith("stale_price_")) {
    return normalized
      .replace("stale_price_", "Stale price ")
      .replace("_days", " days");
  }

  if (normalized.startsWith("stale_fx_")) {
    return normalized
      .replace("stale_fx_", "Stale FX ")
      .replace("_days", " days");
  }

  switch (normalized) {
    case "missing_price":
      return "Missing price";
    case "missing_fx":
      return "Missing FX";
    case "missing_previous_close":
      return "Missing previous close";
    case "missing_previous_fx":
      return "Missing previous FX";
    case "symbol_unmapped":
      return "Symbol unmapped";
    case "base_cost_basis_unavailable":
      return "Cost basis unavailable";
    case "zero_cost_basis":
      return "Zero cost basis";
    case "zero_previous_market_value":
      return "Zero previous value";
    case "income_not_tracked":
      return "Income not tracked";
    default:
      return normalized.replaceAll("_", " ");
  }
}

export function reasonSummary(reasons: string[]): string {
  return reasons.map(reasonLabel).join(", ");
}

export function freshnessLabel(freshness: string): string {
  if (freshness === "fresh") {
    return "Fresh";
  }

  if (freshness.startsWith("minor_stale_")) {
    return freshness
      .replace("minor_stale_", "Minor stale ")
      .replace("_days", " days");
  }

  if (freshness.startsWith("warning_stale_")) {
    return freshness
      .replace("warning_stale_", "Stale ")
      .replace("_days", " days");
  }

  return freshness.replaceAll("_", " ");
}

export function freshnessRank(freshness: string): number {
  const dayMatch = freshness.match(/_(\d+)_days$/);
  const days = dayMatch ? Number(dayMatch[1]) : 0;

  if (freshness.startsWith("warning_stale_")) {
    return 200 + days;
  }

  if (freshness.startsWith("minor_stale_")) {
    return 100 + days;
  }

  return freshness === "fresh" ? 0 : 50;
}

export function worstFreshness(freshnessValues: string[]): string | null {
  return freshnessValues.reduce<string | null>((worst, freshness) => {
    if (!worst || freshnessRank(freshness) > freshnessRank(worst)) {
      return freshness;
    }

    return worst;
  }, null);
}

export function freshnessTone(freshness: string): "warning" | "flat" {
  return freshness === "fresh" ? "flat" : "warning";
}
