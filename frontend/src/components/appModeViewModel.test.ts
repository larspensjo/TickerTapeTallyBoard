import { describe, expect, it } from "vitest";
import { appModeViewModel } from "./appModeViewModel";

describe("appModeViewModel", () => {
  it("shows mutation controls and import navigation outside demo mode", () => {
    const model = appModeViewModel(false);

    expect(model.showDemoBadge).toBe(false);
    expect(model.canMutate).toBe(true);
    expect(model.navItems.map((item) => item.label)).toEqual([
      "Dashboard",
      "Holdings",
      "Rebalance",
      "Gains",
      "Transactions",
      "Import",
    ]);
  });

  it("shows the demo badge and hides mutation entry points in demo mode", () => {
    const model = appModeViewModel(true);

    expect(model.showDemoBadge).toBe(true);
    expect(model.canMutate).toBe(false);
    expect(model.navItems.map((item) => item.label)).toEqual([
      "Dashboard",
      "Holdings",
      "Rebalance",
      "Gains",
      "Transactions",
    ]);
  });

  it("hides mutation entry points until app mode is known", () => {
    const model = appModeViewModel(undefined);

    expect(model.showDemoBadge).toBe(false);
    expect(model.canMutate).toBe(false);
    expect(model.navItems.map((item) => item.label)).toEqual([
      "Dashboard",
      "Holdings",
      "Rebalance",
      "Gains",
      "Transactions",
    ]);
  });
});
