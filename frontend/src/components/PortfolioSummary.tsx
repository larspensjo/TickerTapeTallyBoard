import { RefreshCw } from "lucide-react";
import type { GainsRow, GainsSummary } from "../api/types";
import {
  freshnessLabel,
  freshnessTone,
  SummaryAvailabilityValue,
} from "./valuationDisplay";

function freshnessRank(freshness: string): number {
  const dayMatch = freshness.match(/_(\d+)_days$/);
  const days = dayMatch ? Number(dayMatch[1]) : 0;

  if (freshness.startsWith("warning_stale_")) {
    return 200 + days;
  }

  if (freshness.startsWith("minor_stale_")) {
    return 100 + days;
  }

  return freshness === "fresh" ? 0 : 50;
}

function portfolioPriceFreshness(rows: GainsRow[] | undefined): string | null {
  const freshnessValues =
    rows?.flatMap((row) =>
      row.latest_price?.freshness ? [row.latest_price.freshness] : [],
    ) ?? [];

  return freshnessValues.reduce<string | null>((worst, freshness) => {
    if (!worst || freshnessRank(freshness) > freshnessRank(worst)) {
      return freshness;
    }

    return worst;
  }, null);
}

export function PortfolioSummary({
  summary,
  rows,
  isCheckingPrices,
  isRefreshingPrices,
  refreshError,
}: {
  summary: GainsSummary | undefined;
  rows: GainsRow[] | undefined;
  isCheckingPrices: boolean;
  isRefreshingPrices: boolean;
  refreshError?: Error | null;
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
          {isRefreshingPrices ? (
            <span className="status-chip warning">
              <RefreshCw aria-hidden="true" className="spin" size={12} />
              Refreshing
            </span>
          ) : refreshError ? (
            <span className="status-chip warning" title={refreshError.message}>
              Refresh failed
            </span>
          ) : priceFreshness ? (
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
        </span>
      </div>
    </section>
  );
}
