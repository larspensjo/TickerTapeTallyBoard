import { describe, expect, it } from "vitest";
import { type GainsPageState, gainsPageReducer } from "./GainsPage";

const initialState: GainsPageState = {
  includeClosedPositions: false,
  returnMethod: "xirr",
};

describe("gainsPageReducer", () => {
  it("toggles closed positions without changing other controls", () => {
    expect(
      gainsPageReducer(initialState, {
        type: "closedPositionsToggled",
        includeClosedPositions: true,
      }),
    ).toEqual({ ...initialState, includeClosedPositions: true });
  });

  it("updates the return method", () => {
    expect(
      gainsPageReducer(initialState, {
        type: "returnMethodChanged",
        returnMethod: "modified_dietz",
      }),
    ).toEqual({ ...initialState, returnMethod: "modified_dietz" });
  });
});
