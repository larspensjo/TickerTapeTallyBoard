import { Plus, RefreshCw } from "lucide-react";
import { useState } from "react";
import { Outlet } from "react-router-dom";
import {
  useGains,
  useInstruments,
  usePriceStatus,
  useRefreshPrices,
} from "../api/queries";
import { AddTransactionForm } from "./AddTransactionForm";
import { PortfolioSummary } from "./PortfolioSummary";
import { useAppMode } from "./useAppMode";

export function PortfolioLayout() {
  const [formOpen, setFormOpen] = useState(false);
  const gainsQuery = useGains();
  const appMode = useAppMode();
  const instrumentsQuery = useInstruments();
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
          <button
            className="button secondary"
            type="button"
            onClick={() => setFormOpen((open) => !open)}
          >
            <Plus aria-hidden="true" size={16} />
            <span>Add transaction</span>
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

      {formOpen && appMode.canMutate ? (
        <section className="panel form-panel" aria-label="Add transaction">
          <div className="panel-header">
            <div>
              <p className="eyebrow">Manual entry</p>
              <h2>Add transaction</h2>
            </div>
          </div>
          <AddTransactionForm
            instruments={instrumentsQuery.data ?? []}
            onClose={() => setFormOpen(false)}
          />
        </section>
      ) : null}

      <Outlet />
    </div>
  );
}
