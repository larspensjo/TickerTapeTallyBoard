import type { RefreshPricesResult } from "../api/types";
import type { PriceRefreshFeedbackState } from "./priceRefreshFeedback";
import { formatGroupedNumber } from "./valuationDisplay";

export interface PriceRefreshInputs {
  serverRefreshing: boolean;
  requestInFlight: boolean;
  feedback: PriceRefreshFeedbackState;
}

export interface PriceRefreshStatus {
  running: boolean;
  warning: null | {
    kind: "request_failed";
    label: string;
    detail: string;
  };
}

export function priceRefreshStatus({
  serverRefreshing,
  requestInFlight,
  feedback,
}: PriceRefreshInputs): PriceRefreshStatus {
  return {
    running: serverRefreshing || requestInFlight,
    warning: feedback.requestFailure
      ? {
          kind: "request_failed",
          label: "Refresh failed",
          detail: feedback.requestFailure.message,
        }
      : null,
  };
}

export function backfillOutcome(result: RefreshPricesResult): {
  tone: "info" | "warning";
  message: string;
} {
  if (result.status === "running") {
    return {
      tone: "warning",
      message:
        "Another price refresh was already running, so the backfill did not start. Try again when it finishes.",
    };
  }

  return {
    tone: result.status === "succeeded" ? "info" : "warning",
    message: `Backfill ${result.status}: wrote ${formatGroupedNumber(result.prices_written)} price rows and ${formatGroupedNumber(result.fx_rates_written)} FX rates.`,
  };
}
