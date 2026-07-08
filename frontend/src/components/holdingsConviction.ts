import type { ConvictionChange } from "../api/queries";
import type {
  AvailabilityValue,
  Conviction,
  ConvictionTarget,
  Holding,
  TargetStatus,
} from "../api/types";
import {
  formatGroupedNumber,
  parseFiniteNumber,
  unavailableValue,
} from "./valuationDisplay";

/** Selectable conviction levels, ordered from lowest to highest weight. */
export const CONVICTION_OPTIONS: Conviction[] = [
  "Other",
  "Low",
  "Medium",
  "High",
];

/** Display label for a conviction level (already human-readable, kept as a
 * single source of truth for the UI and search text). */
export function convictionLabel(conviction: Conviction): string {
  return conviction;
}

/** Sort rank so Other < Low < Medium < High. */
export function convictionRank(conviction: Conviction): number {
  return CONVICTION_OPTIONS.indexOf(conviction);
}

const TARGET_STATUS_LABELS: Record<TargetStatus, string> = {
  below: "Below",
  on_target: "On target",
  above: "Above",
  no_target: "No target",
  excluded_unavailable: "Excluded",
  unavailable: "Unavailable",
};

export function targetStatusLabel(status: TargetStatus): string {
  return TARGET_STATUS_LABELS[status];
}

const TARGET_STATUS_ORDER: TargetStatus[] = [
  "below",
  "on_target",
  "above",
  "no_target",
  "excluded_unavailable",
  "unavailable",
];

export function targetStatusRank(status: TargetStatus): number {
  return TARGET_STATUS_ORDER.indexOf(status);
}

/**
 * A target status is a data-quality alert only when the value could not be
 * computed. Below/on-target/above are normal informative states and use the
 * neutral chip; excluded/unavailable use the warning chip.
 */
export function isTargetAlert(status: TargetStatus): boolean {
  return status === "excluded_unavailable" || status === "unavailable";
}

/**
 * Target gaps follow plain sign colouring (`signedTone`): a positive gap means
 * the holding is above target because it appreciated more than its peers and
 * reads as `up`/green; a negative gap reads as `down`/red. This matches P&L
 * colouring (decision log 2026-07-07, reversing the earlier inverted scheme).
 */

/** Full half-bar (and clamp point) of the target gap bar, in gap percent. */
export const TARGET_GAP_BAR_CLAMP_PERCENT = 50;

/**
 * View model for the diverging target gap bar. `widthPercent` is the fill
 * width as a percentage of the full track (0..50): the gap percent clamped to
 * `TARGET_GAP_BAR_CLAMP_PERCENT`, mapped onto one half of the track.
 */
export interface TargetGapBar {
  side: "above" | "below" | "on_target";
  widthPercent: number;
  tooltip: string;
}

export interface GapBarGeometry {
  side: "above" | "below" | "on_target";
  widthPercent: number;
}

/**
 * Geometry of the diverging gap bar for a signed gap percent: side by sign,
 * width clamped to TARGET_GAP_BAR_CLAMP_PERCENT and mapped onto one half of
 * the track (0..50). Shared by Holdings targets and the rebalance balance.
 */
export function gapBarGeometry(gapPercent: number): GapBarGeometry {
  const magnitude = Math.min(
    Math.abs(gapPercent),
    TARGET_GAP_BAR_CLAMP_PERCENT,
  );
  const widthPercent = (magnitude / TARGET_GAP_BAR_CLAMP_PERCENT) * 50;
  const side =
    gapPercent === 0 ? "on_target" : gapPercent > 0 ? "above" : "below";

  return { side, widthPercent };
}

/**
 * Bar for a holding's target gap, or null when no gap can be computed
 * (no target, excluded, or valuation unavailable) — those rows render an
 * empty cell by design.
 */
export function targetGapBar(target: ConvictionTarget): TargetGapBar | null {
  if (
    target.target_gap_percent.status !== "available" ||
    target.target_value_base.status !== "available" ||
    target.target_gap_base.status !== "available"
  ) {
    return null;
  }

  const gapPercent = parseFiniteNumber(target.target_gap_percent.value);
  if (gapPercent === null) {
    return null;
  }

  const geometry = gapBarGeometry(gapPercent);

  const tooltip = [
    `Target SEK ${formatGroupedNumber(target.target_value_base.value)}`,
    `Gap SEK ${formatGroupedNumber(target.target_gap_base.value)} (${formatGroupedNumber(target.target_gap_percent.value)}%)`,
    targetStatusLabel(target.status),
  ].join("\n");

  return { ...geometry, tooltip };
}

/** The conviction a holding currently shows, honouring any staged (unsaved)
 * edit over the saved value. */
export function effectiveConviction(
  holding: Holding,
  edits: ConvictionEdits,
): Conviction {
  return edits[holding.instrument.id] ?? holding.instrument.conviction;
}

/** Extra searchable text for a holding: its saved conviction and target
 * status, so the Holdings filter matches on them. */
export function holdingConvictionSearchText(holding: Holding): string {
  return [
    convictionLabel(holding.instrument.conviction),
    targetStatusLabel(holding.conviction_target.status),
  ].join(" ");
}

/**
 * Value sort key for the Holdings table. Watchlist rows display a blank value
 * but should cluster with zero when sorted by Value.
 */
export function holdingValueSortField(
  holding: Holding,
): AvailabilityValue<string> {
  if (holding.row_kind === "watchlist") {
    return { status: "available", value: "0.00" };
  }

  return (
    holding.valuation?.market_value_base ??
    unavailableValue("valuation_unavailable")
  );
}

/**
 * Hint text shown when watchlist rows are hidden but still participate in the
 * shared target pool.
 */
export function watchlistTargetsHint(
  hiddenWatchlistPoolCount: number,
): string | null {
  if (hiddenWatchlistPoolCount <= 0) {
    return null;
  }

  return `Targets include ${formatGroupedNumber(hiddenWatchlistPoolCount)} watchlist instruments`;
}

/** Signed gap percent used for the "Target gap" sort column, so the order
 * runs most-below -> most-above (rows without a gap sort as unavailable). */
export function targetGapPercentField(target: ConvictionTarget) {
  return target.target_gap_percent;
}

/**
 * Staged, not-yet-saved conviction edits keyed by instrument id. A key is
 * present only when the staged value differs from the saved value, so the map
 * being non-empty is exactly "there are unsaved changes".
 */
export type ConvictionEdits = Record<number, Conviction>;

export type ConvictionEditAction =
  | {
      type: "stage";
      instrumentId: number;
      saved: Conviction;
      conviction: Conviction;
    }
  | { type: "discard" };

export function convictionEditsReducer(
  state: ConvictionEdits,
  action: ConvictionEditAction,
): ConvictionEdits {
  switch (action.type) {
    case "stage": {
      const next = { ...state };
      if (action.conviction === action.saved) {
        // Selecting the saved value again clears the pending change.
        delete next[action.instrumentId];
      } else {
        next[action.instrumentId] = action.conviction;
      }
      return next;
    }
    case "discard":
      return {};
  }
}

export function hasConvictionEdits(edits: ConvictionEdits): boolean {
  return Object.keys(edits).length > 0;
}

export function convictionEditsToChanges(
  edits: ConvictionEdits,
): ConvictionChange[] {
  return Object.entries(edits).map(([instrumentId, conviction]) => ({
    instrument_id: Number(instrumentId),
    conviction,
  }));
}

/**
 * Staged changes that still differ from the currently saved conviction. A
 * background refetch can move a holding's saved value to match a staged edit;
 * such edits are no-ops and are dropped so Apply is not enabled for a write
 * that would change nothing.
 */
export function pendingConvictionChanges(
  edits: ConvictionEdits,
  holdings: Holding[],
): ConvictionChange[] {
  const saved = new Map(
    holdings.map((holding) => [
      holding.instrument.id,
      holding.instrument.conviction,
    ]),
  );
  return convictionEditsToChanges(edits).filter(
    (change) => saved.get(change.instrument_id) !== change.conviction,
  );
}
