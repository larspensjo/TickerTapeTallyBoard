import type {
  Instrument,
  OpportunitiesResponse,
  OpportunityOrder,
  OpportunityRow,
  OpportunityWindow,
} from "../api/types";
import {
  type FreshnessKind,
  formatGroupedNumber,
  formatUnitPrice,
  freshnessKind,
  freshnessLabel,
  parseFiniteNumber,
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

export interface OpportunityRowViewModel {
  instrument: Instrument;
  changeAvailable: boolean;
  changeLabel: string;
  changeTone: "up" | "down" | "flat";
  changeTooltip: string;
  latestPrice: OpportunityLatestPriceViewModel | null;
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
