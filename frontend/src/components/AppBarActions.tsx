import { RefreshCw, TriangleAlert } from "lucide-react";
import type { PriceRefreshController } from "./usePriceRefresh";

export function AppBarActions({
  canMutate,
  priceRefresh,
}: {
  canMutate: boolean;
  priceRefresh: PriceRefreshController;
}) {
  if (!canMutate) return null;

  const { status, refresh } = priceRefresh;
  const warningText = status.warning
    ? `${status.warning.label}: ${status.warning.detail}`
    : undefined;

  return (
    // biome-ignore lint/a11y/useSemanticElements: App actions are a labelled control group, not form fields.
    <div className="app-bar-actions" role="group" aria-label="App actions">
      {status.warning ? (
        <span
          className="price-refresh-warning"
          role="img"
          aria-label={warningText}
          title={warningText}
        >
          <TriangleAlert aria-hidden="true" size={16} />
        </span>
      ) : null}
      <button
        className="button outline compact"
        type="button"
        aria-label="Refresh prices"
        disabled={status.running}
        onClick={refresh}
      >
        <RefreshCw
          aria-hidden="true"
          className={status.running ? "spin" : undefined}
          size={16}
        />
        <span>Refresh</span>
      </button>
    </div>
  );
}
