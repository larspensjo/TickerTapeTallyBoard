import { describe, expect, it } from "vitest";
import { appModeViewModel } from "./appModeViewModel";

describe("appModeViewModel", () => {
  it("shows mutation controls and import navigation outside demo mode", () => {
    const model = appModeViewModel(false);

    expect(model.showDemoBadge).toBe(false);
    expect(model.canMutate).toBe(true);
    expect(model.navItems.map((item) => item.label)).toContain("Import");
  });

  it("shows the demo badge and hides mutation entry points in demo mode", () => {
    const model = appModeViewModel(true);

    expect(model.showDemoBadge).toBe(true);
    expect(model.canMutate).toBe(false);
    expect(model.navItems.map((item) => item.label)).not.toContain("Import");
  });
});
