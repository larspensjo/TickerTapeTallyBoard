import { describe, expect, it } from "vitest";
import { type GainsPageState, gainsPageReducer } from "./GainsPage";

const initialState: GainsPageState = {
  includeClosedPositions: false,
  datePreset: "all",
  dateRange: { startDate: null, endDate: null },
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

  it("updates the selected date preset", () => {
    expect(
      gainsPageReducer(initialState, {
        type: "datePresetChanged",
        datePreset: "ytd",
      }),
    ).toEqual({ ...initialState, datePreset: "ytd" });
  });

  it("updates the active date range", () => {
    const dateRange = { startDate: "2026-01-01", endDate: "2026-06-22" };

    expect(
      gainsPageReducer(initialState, {
        type: "dateRangeChanged",
        dateRange,
      }),
    ).toEqual({ ...initialState, dateRange });
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
