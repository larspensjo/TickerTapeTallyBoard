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
        {prefix}
        {value.value}
        {suffix}
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
}: {
  value: AvailabilityValue<string> | undefined;
  prefix?: string;
  suffix?: string;
  tone?: ValueTone;
}) {
  if (!value) {
    return <span className="status-chip warning">Unavailable</span>;
  }

  if (!isAvailable(value)) {
    return (
      <span
        className="status-chip warning"
        title={reasonSummary(value.reasons)}
      >
        Unavailable
      </span>
    );
  }

  return (
    <strong className={numberClass(value.value, tone)}>
      {prefix}
      {value.value}
      {suffix}
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

export function freshnessTone(freshness: string): "warning" | "flat" {
  return freshness === "fresh" ? "flat" : "warning";
}
