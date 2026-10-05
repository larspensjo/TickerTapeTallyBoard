import { useIsMutating } from "@tanstack/react-query";
import { useReducer } from "react";
import {
  PRICE_REFRESH_MUTATION_KEY,
  useDataVersion,
  useRefreshPrices,
} from "../api/queries";
import {
  initialPriceRefreshFeedback,
  priceRefreshFeedbackReducer,
} from "./priceRefreshFeedback";
import {
  type PriceRefreshStatus,
  priceRefreshStatus,
} from "./priceRefreshViewModel";

export interface PriceRefreshController {
  status: PriceRefreshStatus;
  refresh: () => void;
}

export function usePriceRefresh(): PriceRefreshController {
  const [feedback, dispatch] = useReducer(
    priceRefreshFeedbackReducer,
    initialPriceRefreshFeedback,
  );
  const heartbeat = useDataVersion();
  const requestCount = useIsMutating({
    mutationKey: PRICE_REFRESH_MUTATION_KEY,
  });
  const refreshPrices = useRefreshPrices();
  const status = priceRefreshStatus({
    serverRefreshing: heartbeat.data?.prices_refreshing === true,
    requestInFlight: requestCount > 0,
    feedback,
  });

  function refresh() {
    if (status.running) return;

    refreshPrices.mutate(
      { mode: "latest" },
      {
        onSuccess: (result) => dispatch({ type: "requestSucceeded", result }),
        onError: (error) =>
          dispatch({ type: "requestFailed", message: error.message }),
      },
    );
  }

  return { status, refresh };
}
