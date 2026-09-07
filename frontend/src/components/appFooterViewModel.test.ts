import { describe, expect, it } from "vitest";
import { appFooterViewModel } from "./appFooterViewModel";

describe("appFooterViewModel", () => {
  const backup = {
    directory: "C:/backups",
    last_snapshot_at: "2026-08-29T14:30:00.000Z",
    snapshot_count: 1,
    launch_status: "succeeded" as const,
    launch_error: null,
    listing_error: null,
  };
  it("shows a file name and full-path tooltip", () => {
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "file", path: "C:/data/portfolio.sqlite" },
        backup,
      }).ledger,
    ).toEqual({
      label: "portfolio.sqlite",
      tooltip: "C:/data/portfolio.sqlite",
    });
  });
  it("labels memory honestly", () => {
    expect(
      appFooterViewModel({
        mode: "demo",
        ledger: { mode: "memory", path: null },
        backup: { ...backup, launch_status: "skipped" },
      }).ledger?.label,
    ).toBe("In-memory demo");
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "memory", path: null },
        backup,
      }).ledger?.label,
    ).toBe("In-memory (unsaved)");
  });
  it("uses neutral DEV and warning DEMO chips and omits production", () => {
    expect(
      appFooterViewModel({
        mode: "development",
        ledger: { mode: "file", path: "C:/dev.sqlite" },
        backup,
      }).modeChip,
    ).toEqual({ label: "DEV", className: "mode-chip" });
    expect(
      appFooterViewModel({
        mode: "demo",
        ledger: { mode: "memory", path: null },
        backup: { ...backup, launch_status: "skipped" },
      }).modeChip,
    ).toEqual({ label: "DEMO", className: "demo-badge" });
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "file", path: "C:/prod.sqlite" },
        backup,
      }).modeChip,
    ).toBeUndefined();
    expect(appFooterViewModel(undefined)).toEqual({
      modeChip: undefined,
      ledger: undefined,
      backup: undefined,
    });
  });
  it("renders fresh, old, failed, and unavailable backup labels honestly", () => {
    const health = {
      mode: "production" as const,
      ledger: { mode: "file" as const, path: "C:/prod.sqlite" },
    };
    expect(
      appFooterViewModel(
        { ...health, backup },
        new Date("2026-08-29T16:00:00Z"),
      ).backup?.label,
    ).toMatch(/^Backup today/);
    expect(
      appFooterViewModel(
        {
          ...health,
          backup: { ...backup, last_snapshot_at: "2026-08-20T14:30:00.000Z" },
        },
        new Date("2026-08-29T16:00:00Z"),
      ).backup?.label,
    ).toBe("Backup 9d ago");
    expect(
      appFooterViewModel({
        ...health,
        backup: {
          ...backup,
          launch_status: "failed",
          launch_error: "disk full",
        },
      }).backup,
    ).toEqual({
      label: "Backup failed",
      tooltip: "disk full",
      className: "backup-warning-chip",
    });
    expect(
      appFooterViewModel({
        ...health,
        backup: {
          ...backup,
          last_snapshot_at: null,
          listing_error: "missing directory",
        },
      }).backup?.label,
    ).toBe("Backup unavailable");
  });
});
