import type { HealthResponse } from "../api/types";

export interface AppFooterViewModel {
  modeChip:
    | { label: "DEV"; className: "mode-chip" }
    | { label: "DEMO"; className: "demo-badge" }
    | undefined;
  ledger: { label: string; tooltip?: string } | undefined;
  backup:
    | { label: string; tooltip: string; className?: "backup-warning-chip" }
    | undefined;
}

export function appFooterViewModel(
  health: Pick<HealthResponse, "mode" | "ledger" | "backup"> | undefined,
  now = new Date(),
): AppFooterViewModel {
  if (!health)
    return { modeChip: undefined, ledger: undefined, backup: undefined };
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
      backup: backupViewModel(health.backup, now),
    };
  }
  return {
    modeChip,
    ledger: {
      label: health.mode === "demo" ? "In-memory demo" : "In-memory (unsaved)",
    },
    backup:
      health.mode === "demo" ? undefined : backupViewModel(health.backup, now),
  };
}

const honestyTooltip =
  "Snapshot written locally to the synced folder; OneDrive upload not verified.";

function backupViewModel(backup: HealthResponse["backup"], now: Date) {
  if (backup.launch_status === "failed") {
    return {
      label: "Backup failed",
      tooltip: backup.launch_error ?? honestyTooltip,
      className: "backup-warning-chip" as const,
    };
  }
  if (backup.listing_error) {
    return {
      label: "Backup unavailable",
      tooltip: backup.listing_error,
      className: "backup-warning-chip" as const,
    };
  }
  if (backup.launch_status === "disabled") {
    return { label: "Backup disabled", tooltip: honestyTooltip };
  }
  if (!backup.last_snapshot_at) return undefined;
  const snapshot = new Date(backup.last_snapshot_at);
  if (Number.isNaN(snapshot.valueOf())) return undefined;
  return {
    label: `Backup ${relativeAge(snapshot, now)}`,
    tooltip: honestyTooltip,
  };
}

function relativeAge(snapshot: Date, now: Date) {
  const day = 86_400_000;
  const localMidnight = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).valueOf();
  const snapshotMidnight = new Date(
    snapshot.getFullYear(),
    snapshot.getMonth(),
    snapshot.getDate(),
  ).valueOf();
  const days = Math.round((localMidnight - snapshotMidnight) / day);
  const time = snapshot.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
  if (days === 0) return `today ${time}`;
  if (days === 1) return `yesterday ${time}`;
  return `${days}d ago`;
}
