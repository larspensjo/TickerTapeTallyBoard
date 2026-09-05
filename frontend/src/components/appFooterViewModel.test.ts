import { describe, expect, it } from "vitest";
import { appFooterViewModel } from "./appFooterViewModel";

describe("appFooterViewModel", () => {
  it("shows a file name and full-path tooltip", () => {
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "file", path: "C:/data/portfolio.sqlite" },
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
      }).ledger?.label,
    ).toBe("In-memory demo");
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "memory", path: null },
      }).ledger?.label,
    ).toBe("In-memory (unsaved)");
  });
  it("uses neutral DEV and warning DEMO chips and omits production", () => {
    expect(
      appFooterViewModel({
        mode: "development",
        ledger: { mode: "file", path: "C:/dev.sqlite" },
      }).modeChip,
    ).toEqual({ label: "DEV", className: "mode-chip" });
    expect(
      appFooterViewModel({
        mode: "demo",
        ledger: { mode: "memory", path: null },
      }).modeChip,
    ).toEqual({ label: "DEMO", className: "demo-badge" });
    expect(
      appFooterViewModel({
        mode: "production",
        ledger: { mode: "file", path: "C:/prod.sqlite" },
      }).modeChip,
    ).toBeUndefined();
    expect(appFooterViewModel(undefined)).toEqual({
      modeChip: undefined,
      ledger: undefined,
    });
  });
});
