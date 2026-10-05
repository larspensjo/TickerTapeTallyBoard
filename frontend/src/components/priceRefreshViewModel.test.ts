import { describe, expect, it } from "vitest";
import { refreshResult } from "../test/priceRefresh";
import { initialPriceRefreshFeedback } from "./priceRefreshFeedback";
import { backfillOutcome, priceRefreshStatus } from "./priceRefreshViewModel";

describe("priceRefreshStatus", () => {
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
