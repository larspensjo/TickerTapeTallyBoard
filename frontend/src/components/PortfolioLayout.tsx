import { RefreshCw } from "lucide-react";
import { Outlet } from "react-router-dom";
import { useGains, usePriceStatus, useRefreshPrices } from "../api/queries";
import { PortfolioSummary } from "./PortfolioSummary";
import { useAppMode } from "./useAppMode";

export function PortfolioLayout() {
  const gainsQuery = useGains();
  const appMode = useAppMode();
  const priceStatusQuery = usePriceStatus();
  const refreshPrices = useRefreshPrices();
  const pricesRefreshing =
    refreshPrices.isPending || priceStatusQuery.data?.refreshing === true;

  return (
    <div className="portfolio-layout">
      {appMode.canMutate ? (
        <div className="portfolio-actions">
          <button
            className="button primary"
            type="button"
            onClick={() => void refreshPrices.mutateAsync({ mode: "latest" })}
            disabled={pricesRefreshing}
          >
            <RefreshCw
              aria-hidden="true"
              className={pricesRefreshing ? "spin" : undefined}
              size={16}
            />
            <span>Refresh prices</span>
          </button>
        </div>
      ) : null}

      <PortfolioSummary
        summary={gainsQuery.data?.summary}
        rows={gainsQuery.data?.rows}
        isCheckingPrices={gainsQuery.isFetching || priceStatusQuery.isPending}
        isRefreshingPrices={pricesRefreshing}
        refreshError={refreshPrices.error}
      />

      <Outlet />
    </div>
  );
}
