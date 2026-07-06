import type { ConvictionChange } from "../api/queries";
import type {
  Conviction,
  ConvictionTarget,
  Holding,
  TargetStatus,
} from "../api/types";

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
 * Colour tone for the signed target gap. A negative gap means the holding is
 * below target (underweight, room to buy) and reads as `up`; a positive gap is
 * above target (overweight) and reads as `down`. This is intentionally the
 * opposite of P&L sign colouring.
 */
export function targetGapTone(value: string): "up" | "down" | "flat" {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed === 0) {
    return "flat";
  }
  return parsed < 0 ? "up" : "down";
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

/** Target field used for the "Target" sort column. */
export function targetValueField(target: ConvictionTarget) {
  return target.target_value_base;
}

/** Target field used for the "Target gap" sort column. */
export function targetGapField(target: ConvictionTarget) {
  return target.target_gap_base;
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
