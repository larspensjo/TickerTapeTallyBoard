import { RefreshCw } from "lucide-react";
import type { GainsRow, GainsSummary } from "../api/types";
import type { PriceRefreshStatus } from "./priceRefreshViewModel";
import {
  freshnessLabel,
  freshnessTone,
  SummaryAvailabilityValue,
  worstFreshness,
} from "./valuationDisplay";

function portfolioPriceFreshness(rows: GainsRow[] | undefined): string | null {
  const freshnessValues =
    rows?.flatMap((row) =>
      row.latest_price?.freshness ? [row.latest_price.freshness] : [],
    ) ?? [];

  return worstFreshness(freshnessValues);
}

export function PortfolioSummary({
  summary,
  rows,
  isCheckingPrices,
  refreshStatus,
}: {
  summary: GainsSummary | undefined;
  rows: GainsRow[] | undefined;
  isCheckingPrices: boolean;
  refreshStatus: PriceRefreshStatus;
}) {
  const priceFreshness = portfolioPriceFreshness(rows);

  return (
    <section
      className="metric-tiles portfolio-summary"
      aria-label="Portfolio summary"
    >
      <div className="metric-tile">
        <span className="metric-tile-label">Total value</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue
            value={summary?.market_value_base}
            prefix="SEK "
            tone="plain"
          />
        </span>
      </div>
      <div className="metric-tile">
        <span className="metric-tile-label">Day change</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue
            value={summary?.day_change_base}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={summary?.day_change_percent}
            suffix="%"
            tone="signed"
          />
        </span>
      </div>
      <div className="metric-tile">
        <span className="metric-tile-label">Unrealized change</span>
        <span className="metric-tile-value">
          <SummaryAvailabilityValue
            value={summary?.unrealized_gain_base}
            prefix="SEK "
            tone="signed"
          />{" "}
          <SummaryAvailabilityValue
            value={summary?.unrealized_gain_percent}
            suffix="%"
            tone="signed"
          />
        </span>
      </div>
      <div className="metric-tile freshness-tile">
        <span className="metric-tile-label">Prices</span>
        <span className="metric-tile-value freshness-value">
          {refreshStatus.running ? (
            <span className="status-chip warning">
              <RefreshCw aria-hidden="true" className="spin" size={12} />
              Refreshing
            </span>
          ) : (
            <>
              {refreshStatus.warning ? (
                <span
                  className="status-chip warning"
                  title={refreshStatus.warning.detail}
                >
                  {refreshStatus.warning.label}
                </span>
              ) : null}
              {priceFreshness ? (
                <span
                  className={
                    freshnessTone(priceFreshness) === "warning"
                      ? "status-chip warning"
                      : "status-chip"
                  }
                >
                  {freshnessLabel(priceFreshness)}
                </span>
              ) : isCheckingPrices ? (
                <span className="status-chip">Checking</span>
              ) : (
                <span className="status-chip">No data</span>
              )}
            </>
          )}
        </span>
      </div>
    </section>
  );
}
