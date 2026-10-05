import { describe, expect, it } from "vitest";
import { refreshResult } from "../test/priceRefresh";
import {
  initialPriceRefreshFeedback,
  priceRefreshFeedbackReducer,
} from "./priceRefreshFeedback";

describe("priceRefreshFeedbackReducer", () => {
  it("records a request failure and replaces it with the next failure", () => {
    const failed = priceRefreshFeedbackReducer(initialPriceRefreshFeedback, {
      type: "requestFailed",
      message: "Server unavailable",
    });
    expect(failed.requestFailure).toEqual({ message: "Server unavailable" });
    expect(
      priceRefreshFeedbackReducer(failed, {
        type: "requestFailed",
        message: "Network disconnected",
      }).requestFailure,
    ).toEqual({ message: "Network disconnected" });
    expect(initialPriceRefreshFeedback.requestFailure).toBeNull();
  });

  it.each(["succeeded", "partial", "failed"] as const)(
    "clears a request failure after a finished %s response",
    (status) => {
      const state = {
        ...initialPriceRefreshFeedback,
        requestFailure: { message: "Server unavailable" },
      };
      expect(
        priceRefreshFeedbackReducer(state, {
          type: "requestSucceeded",
          result: refreshResult(status),
        }).requestFailure,
      ).toBeNull();
      expect(state.requestFailure.message).toBe("Server unavailable");
    },
  );

  it("retains a request failure after a merged response", () => {
    const state = {
      ...initialPriceRefreshFeedback,
      requestFailure: { message: "Server unavailable" },
    };
    expect(
      priceRefreshFeedbackReducer(state, {
        type: "requestSucceeded",
        result: refreshResult("running"),
      }),
    ).toEqual(state);
  });
});
