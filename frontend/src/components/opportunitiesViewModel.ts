import type {
  Instrument,
  OpportunitiesResponse,
  OpportunityOrder,
  OpportunityRow,
  OpportunityWindow,
  PercentValue,
} from "../api/types";
import { type TargetGapBar, targetGapBar } from "./holdingsConviction";
import {
  type FreshnessKind,
  formatGroupedNumber,
  formatUnitPrice,
  freshnessKind,
  freshnessLabel,
  parseFiniteNumber,
  reasonLabel,
  signedTone,
} from "./valuationDisplay";

export const opportunityWindowOptions: ReadonlyArray<{
  value: OpportunityWindow;
  label: string;
}> = [
  { value: "7d", label: "1 week" },
  { value: "14d", label: "2 weeks" },
  { value: "30d", label: "30 days" },
];

export const opportunityOrderOptions: ReadonlyArray<{
  value: OpportunityOrder;
  label: string;
}> = [
  { value: "fallers_first", label: "Biggest fallers" },
  { value: "risers_first", label: "Biggest risers" },
];

const CHANGE_COLUMN_LABELS: Record<OpportunityWindow, string> = {
  "7d": "1-week price change",
  "14d": "2-week price change",
  "30d": "30-day price change",
};

/** Header used while the rows on screen belong to another window. */
export const UNNAMED_CHANGE_COLUMN_LABEL = "Price change";

const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** "2026-10-07" → "7 Oct". Calendar dates are formatted without a time zone. */
export function formatShortDate(isoDate: string): string {
  const match = isoDate.match(/^\d{4}-(\d{2})-(\d{2})$/);
  const month = match ? MONTHS[Number(match[1]) - 1] : undefined;
  if (!match || !month) {
    return isoDate;
  }

  return `${Number(match[2])} ${month}`;
}

export function priceChangeReasonLabel(
  code: string,
  startDate: string,
): string {
  const start = formatShortDate(startDate);
  switch (code) {
    case "no_price_source":
      return "No price source";
    case "missing_latest_close":
      return "No price yet";
    case "no_close_since_period_start":
      return `No price since ${start}`;
    case "missing_start_close":
      return `No price before ${start}`;
    case "start_close_too_old":
      return `No price near ${start}`;
    case "start_close_source_mismatch":
      return "Prices from different sources";
    case "start_close_currency_mismatch":
      return "Prices in different currencies";
    case "zero_start_close":
      return "Start price is zero";
    case "split_in_period":
      return "Share split in this period";
    default:
      return code.replaceAll("_", " ");
  }
}

function changeValue(row: OpportunityRow): number | null {
  return row.price_change.status === "available"
    ? parseFiniteNumber(row.price_change.change_percent)
    : null;
}

function compareText(left: string, right: string): number {
  if (left < right) return -1;
  if (left > right) return 1;
  return 0;
}

function compareInstruments(left: Instrument, right: Instrument): number {
  return (
    compareText(left.symbol, right.symbol) ||
    compareText(left.exchange, right.exchange)
  );
}

/**
 * Orders rows by the 2-decimal change (parsed for ordering only): ascending
 * for fallers first, descending for risers first, ties by symbol then
 * exchange. Rows without a change follow every row with one, in both orders.
 */
export function sortOpportunityRows(
  rows: readonly OpportunityRow[],
  order: OpportunityOrder,
): OpportunityRow[] {
  const direction = order === "fallers_first" ? 1 : -1;
  return [...rows].sort((left, right) => {
    const leftChange = changeValue(left);
    const rightChange = changeValue(right);
    if (leftChange === null || rightChange === null) {
      if (leftChange !== rightChange) {
        return leftChange === null ? 1 : -1;
      }
      return compareInstruments(left.instrument, right.instrument);
    }

    return (
      (leftChange - rightChange) * direction ||
      compareInstruments(left.instrument, right.instrument)
    );
  });
}

export interface OpportunityLatestPriceViewModel {
  label: string;
  freshnessKind: FreshnessKind;
  freshnessLabel: string;
}

/** A signed percent cell, or the plain-language reason it is unavailable. */
export interface OpportunityPercentViewModel {
  available: boolean;
  label: string;
  tone: "up" | "down" | "flat";
  tooltip: string | null;
}

export interface OpportunityStaleSignalViewModel {
  kind: FreshnessKind;
  label: string;
}

export interface OpportunityRowViewModel {
  instrument: Instrument;
  changeAvailable: boolean;
  changeLabel: string;
  changeTone: "up" | "down" | "flat";
  changeTooltip: string;
  latestPrice: OpportunityLatestPriceViewModel | null;
  vsAverage: OpportunityPercentViewModel;
  /** From the backend's exact comparison, never from the rounded percent. */
  belowAverage: boolean;
  gain: OpportunityPercentViewModel;
  /** Staler of the price and exchange-rate freshness behind the SEK gain. */
  gainFreshness: OpportunityStaleSignalViewModel | null;
  targetBar: TargetGapBar | null;
}

export type OpportunitiesPageStatus =
  | "loading"
  | "error"
  | "empty"
  | "available";

export interface OpportunitiesViewModel {
  status: OpportunitiesPageStatus;
  message: string | null;
  changeColumnLabel: string;
  periodSubtitle: string | null;
  isUpdating: boolean;
  rows: OpportunityRowViewModel[];
}

export interface BuildOpportunitiesViewModelInput {
  response?: OpportunitiesResponse;
  requestedWindow: OpportunityWindow;
  order: OpportunityOrder;
  isError: boolean;
  errorMessage: string | null;
}

function formatChangePercent(value: string): string {
  const formatted = `${formatGroupedNumber(value)}%`;
  return signedTone(value) === "up" ? `+${formatted}` : formatted;
}

export function priceVersusAverageReasonLabel(code: string): string {
  switch (code) {
    case "missing_latest_close":
      return "No price yet";
    case "price_currency_mismatch":
      return "Price in another currency";
    case "zero_average_cost":
      return "Average price is zero";
    default:
      return code.replaceAll("_", " ");
  }
}

function percentViewModel(
  value: PercentValue,
  unavailableLabel: (reasons: string[]) => string,
): OpportunityPercentViewModel {
  if (value.status === "available") {
    return {
      available: true,
      label: formatChangePercent(value.value),
      tone: signedTone(value.value),
      tooltip: null,
    };
  }

  const label = unavailableLabel(value.reasons);
  return { available: false, label, tone: "flat", tooltip: label };
}

function joinedReasons(
  reasons: string[],
  label: (code: string) => string,
): string {
  return reasons.length > 0 ? reasons.map(label).join(", ") : "Unavailable";
}

function hasNoPriceSource(row: OpportunityRow): boolean {
  return (
    row.price_change.status === "unavailable" &&
    row.price_change.reasons.includes("no_price_source")
  );
}

function contextViewModel(row: OpportunityRow) {
  // A holding without a price source reports its other values as missing a
  // price; naming the cause once reads better than a generic reason.
  const noPriceSource = hasNoPriceSource(row);
  const reasonText = (label: (code: string) => string) => (reasons: string[]) =>
    noPriceSource ? "No price source" : joinedReasons(reasons, label);

  return {
    vsAverage: percentViewModel(
      row.price_vs_average_percent,
      reasonText(priceVersusAverageReasonLabel),
    ),
    belowAverage: row.below_average === true,
    gain: percentViewModel(
      row.unrealized_gain_percent,
      reasonText(reasonLabel),
    ),
    gainFreshness:
      row.unrealized_gain_freshness === null
        ? null
        : {
            kind: freshnessKind(row.unrealized_gain_freshness),
            label: freshnessLabel(row.unrealized_gain_freshness),
          },
    targetBar: targetGapBar(row.conviction_target),
  };
}

function rowViewModel(
  row: OpportunityRow,
  periodStart: string,
): OpportunityRowViewModel {
  const change = row.price_change;
  const latestPrice = row.latest_close
    ? {
        label: `${row.latest_close.currency} ${formatUnitPrice(row.latest_close.close)}`,
        freshnessKind: freshnessKind(row.latest_close.freshness),
        freshnessLabel: freshnessLabel(row.latest_close.freshness),
      }
    : null;

  if (change.status === "available") {
    return {
      instrument: row.instrument,
      changeAvailable: true,
      changeLabel: formatChangePercent(change.change_percent),
      changeTone: signedTone(change.change_percent),
      changeTooltip: `From ${formatShortDate(change.start_date)} close ${formatUnitPrice(
        change.start_close,
      )} to ${formatShortDate(change.end_date)} close ${formatUnitPrice(
        change.end_close,
      )}`,
      latestPrice,
      ...contextViewModel(row),
    };
  }

  const reasons = change.reasons.map((reason) =>
    priceChangeReasonLabel(reason, periodStart),
  );
  const label = reasons.length > 0 ? reasons.join(", ") : "Unavailable";
  return {
    instrument: row.instrument,
    changeAvailable: false,
    changeLabel: label,
    changeTone: "flat",
    changeTooltip: label,
    latestPrice,
    ...contextViewModel(row),
  };
}

export function buildOpportunitiesViewModel(
  input: BuildOpportunitiesViewModelInput,
): OpportunitiesViewModel {
  const { response } = input;
  if (!response) {
    return {
      status: input.isError ? "error" : "loading",
      message: input.isError
        ? (input.errorMessage ?? "Could not load opportunities.")
        : null,
      changeColumnLabel: UNNAMED_CHANGE_COLUMN_LABEL,
      periodSubtitle: null,
      isUpdating: false,
      rows: [],
    };
  }

  // Rows kept from another window stay on screen but stop naming a period.
  // A window mismatch means the requested window's fetch is still pending, so
  // a failure of that fetch must surface here, or the old rows would sit under
  // an Updating chip with no way to retry.
  const isUpdating = response.period.window !== input.requestedWindow;
  if (isUpdating && input.isError) {
    return {
      status: "error",
      message: input.errorMessage ?? "Could not load opportunities.",
      changeColumnLabel: UNNAMED_CHANGE_COLUMN_LABEL,
      periodSubtitle: null,
      isUpdating: false,
      rows: [],
    };
  }
  const changeColumnLabel = isUpdating
    ? UNNAMED_CHANGE_COLUMN_LABEL
    : (CHANGE_COLUMN_LABELS[response.period.window] ??
      UNNAMED_CHANGE_COLUMN_LABEL);
  const periodSubtitle = isUpdating
    ? null
    : `Closes from ${formatShortDate(response.period.start_date)} to ${formatShortDate(
        response.period.end_date,
      )}`;

  return {
    status: response.rows.length === 0 ? "empty" : "available",
    message: response.rows.length === 0 ? "No holdings yet." : null,
    changeColumnLabel,
    periodSubtitle,
    isUpdating,
    rows: sortOpportunityRows(response.rows, input.order).map((row) =>
      rowViewModel(row, response.period.start_date),
    ),
  };
}
