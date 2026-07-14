import { describe, expect, it } from "vitest";
import { totalReturnLabel } from "./GainsTable";

describe("totalReturnLabel", () => {
  it("keeps the visible label short and moves the method detail into the tooltip", () => {
    expect(totalReturnLabel("money_weighted", "absolute")).toEqual({
      label: "Total return",
      title: "Total return including closed positions (money-weighted)",
    });
    expect(totalReturnLabel("simple", "absolute")).toEqual({
      label: "Total return",
      title: "Total return (simple)",
    });
  });

  it("describes the modified Dietz legacy mode through the tooltip", () => {
    expect(totalReturnLabel("modified_dietz", "annualised")).toEqual({
      label: "Total return",
      title: "Annualised return (Modified Dietz legacy)",
      note: "legacy / comparison only",
    });
  });
});
