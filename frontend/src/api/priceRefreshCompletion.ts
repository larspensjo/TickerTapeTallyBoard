import type { PriceStatusResponse } from "./types";

/**
 * Identifies the most recent finished refresh run, or `null` when no run has
 * finished (none recorded, or the latest one is still running).
 */
export function completedRefreshRunKey(
  status: PriceStatusResponse,
): string | null {
  const run = status.latest_run;
  if (!run || run.finished_at === null) return null;
  return `${run.run_id}:${run.finished_at}`;
}

/**
 * True when price status now shows a finished run that differs from the one
 * previously observed. The first observation (`previous === undefined`) never
 * counts: data fetched alongside it already reflects that run.
 */
export function isNewlyCompletedRefreshRun(
  previous: string | null | undefined,
  next: string | null,
): boolean {
  return previous !== undefined && next !== null && next !== previous;
}
