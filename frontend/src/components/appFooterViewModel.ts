import type { HealthResponse } from "../api/types";

export interface AppFooterViewModel {
  modeChip:
    | { label: "DEV"; className: "mode-chip" }
    | { label: "DEMO"; className: "demo-badge" }
    | undefined;
  ledger: { label: string; tooltip?: string } | undefined;
}

export function appFooterViewModel(
  health: Pick<HealthResponse, "mode" | "ledger"> | undefined,
): AppFooterViewModel {
  if (!health) return { modeChip: undefined, ledger: undefined };
  const modeChip =
    health.mode === "development"
      ? { label: "DEV" as const, className: "mode-chip" as const }
      : health.mode === "demo"
        ? { label: "DEMO" as const, className: "demo-badge" as const }
        : undefined;
  if (health.ledger.mode === "file") {
    const path = health.ledger.path;
    return {
      modeChip,
      ledger: path
        ? {
            label: path.replace(/\\/g, "/").split("/").pop() ?? path,
            tooltip: path,
          }
        : undefined,
    };
  }
  return {
    modeChip,
    ledger: {
      label: health.mode === "demo" ? "In-memory demo" : "In-memory (unsaved)",
    },
  };
}
