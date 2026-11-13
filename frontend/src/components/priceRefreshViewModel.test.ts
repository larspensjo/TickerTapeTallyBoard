import { describe, expect, it } from "vitest";
import { refreshResult } from "../test/priceRefresh";
import {
  initialPriceRefreshFeedback,
  priceRefreshFeedbackReducer,
} from "./priceRefreshFeedback";
import { backfillOutcome, priceRefreshStatus } from "./priceRefreshViewModel";

describe("priceRefreshStatus", () => {
  it.each(["failed", "missing", "unavailable"] as const)(
    "does not show a notice for a %s history item",
    (status) => {
      const baseline = refreshResult("succeeded");
      const result = {
        ...baseline,
        items: [
          {
            kind: "history" as const,
            instrument_id: 1,
            provider: "YAHOO",
            symbol_or_pair: "TEST.ST",
            status,
            reason: "provider_error",
            rows_written: 0,
          },
        ],
      };
      const feedback = priceRefreshFeedbackReducer(
        initialPriceRefreshFeedback,
        { type: "requestSucceeded", result },
      );
      expect(
        priceRefreshStatus({
          serverRefreshing: false,
          requestInFlight: false,
          feedback,
        }),
      ).toEqual({ running: false, warning: null });
      expect(backfillOutcome(result)).toEqual(backfillOutcome(baseline));
    },
  );
  it("does not show a notice or change price/FX status text for a failed market index item", () => {
    const baseline = refreshResult("succeeded");
    const result = {
      ...baseline,
      items: [
        {
          kind: "market_index" as const,
          instrument_id: null,
          provider: "YAHOO",
          symbol_or_pair: "SP500",
          status: "failed" as const,
          reason: "provider_error",
          rows_written: 0,
        },
      ],
    };
    const feedback = priceRefreshFeedbackReducer(initialPriceRefreshFeedback, {
      type: "requestSucceeded",
      result,
    });
    expect(
      priceRefreshStatus({
        serverRefreshing: false,
        requestInFlight: false,
        feedback,
      }),
    ).toEqual({ running: false, warning: null });
    expect(backfillOutcome(result)).toEqual(backfillOutcome(baseline));
  });
  it.each([
    [true, false, true],
    [false, true, true],
    [true, true, true],
    [false, false, false],
  ])(
    "combines server %s and request %s into running %s",
    (serverRefreshing, requestInFlight, running) => {
      expect(
        priceRefreshStatus({
          serverRefreshing,
          requestInFlight,
          feedback: initialPriceRefreshFeedback,
        }),
      ).toEqual({ running, warning: null });
    },
  );

  it.each([false, true])(
    "keeps unresolved request feedback while running=%s",
    (running) => {
      expect(
        priceRefreshStatus({
          serverRefreshing: running,
          requestInFlight: false,
          feedback: {
            ...initialPriceRefreshFeedback,
            requestFailure: { message: "Could not connect" },
          },
        }),
      ).toEqual({
        running,
        warning: {
          kind: "request_failed",
          label: "Refresh failed",
          detail: "Could not connect",
        },
      });
    },
  );
});

describe("backfillOutcome", () => {
  it("explains that a merged request started no backfill", () => {
    expect(backfillOutcome(refreshResult("running"))).toEqual({
      tone: "warning",
      message:
        "Another price refresh was already running, so the backfill did not start. Try again when it finishes.",
    });
  });

  it.each(["succeeded", "partial", "failed"] as const)(
    "reports a finished %s backfill",
    (status) => {
      expect(backfillOutcome(refreshResult(status))).toEqual({
        tone: status === "succeeded" ? "info" : "warning",
        message: `Backfill ${status}: wrote 12 price rows and 2 FX rates.`,
      });
    },
  );
});
