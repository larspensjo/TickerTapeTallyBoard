import type { RefreshPricesResult } from "../api/types";

export function refreshResult(
  status: RefreshPricesResult["status"] = "succeeded",
): RefreshPricesResult {
  return {
    run_id: 1,
    trigger: "manual",
    mode: "latest",
    status,
    started_at: "2026-10-05T10:00:00Z",
    finished_at: status === "running" ? null : "2026-10-05T10:01:00Z",
    message: null,
    prices_written: status === "running" ? 0 : 12,
    fx_rates_written: status === "running" ? 0 : 2,
    unmapped_instruments: 0,
    failed_items: 0,
    items: [],
  };
}
