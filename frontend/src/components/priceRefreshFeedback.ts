import type { RefreshPricesResult } from "../api/types";

export interface PriceRefreshFeedbackState {
  requestFailure: null | { message: string };
}

export type PriceRefreshFeedbackAction =
  | { type: "requestFailed"; message: string }
  | { type: "requestSucceeded"; result: RefreshPricesResult };

export const initialPriceRefreshFeedback: PriceRefreshFeedbackState = {
  requestFailure: null,
};

export function priceRefreshFeedbackReducer(
  state: PriceRefreshFeedbackState,
  action: PriceRefreshFeedbackAction,
): PriceRefreshFeedbackState {
  switch (action.type) {
    case "requestFailed":
      return { ...state, requestFailure: { message: action.message } };
    case "requestSucceeded":
      return action.result.status === "running"
        ? state
        : { ...state, requestFailure: null };
  }
}
