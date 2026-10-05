import { Outlet } from "react-router-dom";
import { useGains } from "../api/queries";
import { PortfolioSummary } from "./PortfolioSummary";
import type { PriceRefreshStatus } from "./priceRefreshViewModel";

export function PortfolioLayout({
  refreshStatus,
}: {
  refreshStatus: PriceRefreshStatus;
}) {
  const gainsQuery = useGains();

  return (
    <div className="portfolio-layout">
      <PortfolioSummary
        summary={gainsQuery.data?.summary}
        rows={gainsQuery.data?.rows}
        isCheckingPrices={gainsQuery.isFetching}
        refreshStatus={refreshStatus}
      />

      <Outlet />
    </div>
  );
}
