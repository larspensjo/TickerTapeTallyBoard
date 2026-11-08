import type {
  FundamentalsCorrection,
  FundamentalsPreview,
  PreviousFundamentalsLink,
} from "../api/types";

export interface CorrectionState {
  mode: "symbol" | "not_company" | "automatic";
  symbol: string;
  preview: FundamentalsPreview | null;
  previous: PreviousFundamentalsLink | null;
  phase: "editing" | "previewing" | "ready" | "saving" | "saved";
  error: string | null;
}
export const initialCorrectionState: CorrectionState = {
  mode: "symbol",
  symbol: "",
  preview: null,
  previous: null,
  phase: "editing",
  error: null,
};
export type CorrectionAction =
  | { type: "symbol_changed"; symbol: string }
  | { type: "mode_changed"; mode: CorrectionState["mode"] }
  | { type: "preview_requested" }
  | { type: "preview_received"; symbol: string; preview: FundamentalsPreview }
  | { type: "preview_failed"; symbol: string; error: string }
  | { type: "previous_chosen"; previous: PreviousFundamentalsLink }
  | { type: "save_requested"; hasFigures: boolean }
  | { type: "save_succeeded" }
  | { type: "save_failed"; error: string };

export function correctionRequest(
  state: CorrectionState,
  hasFigures: boolean,
): { automatic: boolean; input: FundamentalsCorrection } | null {
  if (state.phase !== "ready" || (hasFigures && !state.previous)) return null;
  if (
    state.mode === "symbol" &&
    (state.preview?.status !== "found" ||
      state.preview.symbol.toUpperCase() !== state.symbol.trim().toUpperCase())
  )
    return null;
  return {
    automatic: state.mode === "automatic",
    input: {
      ...(state.mode === "symbol"
        ? { provider: "YAHOO" as const, provider_symbol: state.preview?.symbol }
        : state.mode === "not_company"
          ? { provider: null, not_a_company: true }
          : {}),
      ...(hasFigures && state.previous
        ? { previous_link: state.previous }
        : {}),
    },
  };
}
export function fundamentalsCorrectionReducer(
  state: CorrectionState,
  action: CorrectionAction,
): CorrectionState {
  if (
    state.phase === "saving" &&
    action.type !== "save_succeeded" &&
    action.type !== "save_failed"
  )
    return state;
  switch (action.type) {
    case "symbol_changed":
      return { ...initialCorrectionState, symbol: action.symbol };
    case "mode_changed":
      return {
        ...initialCorrectionState,
        mode: action.mode,
        phase: action.mode === "symbol" ? "editing" : "ready",
      };
    case "preview_requested":
      return state.mode === "symbol" && state.symbol.trim()
        ? {
            ...state,
            phase: "previewing",
            preview: null,
            previous: null,
            error: null,
          }
        : state;
    case "preview_received":
      if (state.phase !== "previewing" || state.symbol !== action.symbol)
        return state;
      return {
        ...state,
        preview: action.preview,
        phase: action.preview.status === "found" ? "ready" : "editing",
        error:
          action.preview.status === "no_match"
            ? "No matching symbol found."
            : action.preview.status === "provider_unavailable"
              ? "Provider unavailable. Try preview again."
              : null,
      };
    case "preview_failed":
      return state.phase === "previewing" && state.symbol === action.symbol
        ? { ...state, phase: "editing", error: action.error }
        : state;
    case "previous_chosen":
      return state.phase === "ready"
        ? { ...state, previous: action.previous }
        : state;
    case "save_requested":
      return correctionRequest(state, action.hasFigures)
        ? { ...state, phase: "saving", error: null }
        : state;
    case "save_succeeded":
      return state.phase === "saving" ? { ...state, phase: "saved" } : state;
    case "save_failed":
      return state.phase === "saving"
        ? { ...state, phase: "ready", error: action.error }
        : state;
  }
}
