// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import {
  initialState,
  loadRebalancePageState,
  rebalancePageReducer,
  saveRebalancePageState,
} from "./RebalancePage";

describe("rebalancePageReducer", () => {
  it("starts committed at amount 0 so the default plan loads immediately", () => {
    expect(initialState.amountInput).toBe("0");
    expect(initialState.committedAmount).toBe("0");
  });

  it("keeps the typed amount separate from the committed amount", () => {
    const edited = rebalancePageReducer(initialState, {
      type: "amountInputChanged",
      amountInput: "1,5",
    });

    expect(edited.amountInput).toBe("1,5");
    expect(edited.committedAmount).toBe("0");

    const committed = rebalancePageReducer(edited, {
      type: "amountCommitted",
      amount: "1.5",
    });

    expect(committed.amountInput).toBe("1,5");
    expect(committed.committedAmount).toBe("1.5");
  });

  it("defaults the slider to the rung count, preserves it when the rung count is unchanged, and clamps new ladders", () => {
    const firstLadder = rebalancePageReducer(initialState, {
      type: "planChanged",
      rungCount: 4,
    });

    expect(firstLadder.lastAvailableRungCount).toBe(4);
    expect(firstLadder.sliderPosition).toBe(4);

    const moved = rebalancePageReducer(firstLadder, {
      type: "sliderChanged",
      sliderPosition: 3,
    });
    expect(moved.sliderPosition).toBe(3);

    const sameLadder = rebalancePageReducer(moved, {
      type: "planChanged",
      rungCount: 4,
    });
    expect(sameLadder.sliderPosition).toBe(3);

    const smallerLadder = rebalancePageReducer(moved, {
      type: "planChanged",
      rungCount: 2,
    });
    expect(smallerLadder.lastAvailableRungCount).toBe(2);
    expect(smallerLadder.sliderPosition).toBe(2);
  });

  it("ignores unavailable transitions without dropping the last available ladder", () => {
    const withLadder = rebalancePageReducer(initialState, {
      type: "planChanged",
      rungCount: 3,
    });
    const moved = rebalancePageReducer(withLadder, {
      type: "sliderChanged",
      sliderPosition: 2,
    });

    expect(
      rebalancePageReducer(moved, { type: "planChanged", rungCount: null }),
    ).toEqual(moved);
  });
});

describe("rebalance page persistence", () => {
  beforeEach(() => localStorage.clear());

  it("round trips committed amount and slider position", () => {
    saveRebalancePageState({
      amountInput: "25000",
      committedAmount: "25000",
      sliderPosition: 3,
      lastAvailableRungCount: 8,
      sliderRestored: false,
    });
    const restored = loadRebalancePageState();
    expect(restored.committedAmount).toBe("25000");
    expect(restored.amountInput).toBe("25000");
    expect(restored.sliderPosition).toBe(3);
    expect(restored.lastAvailableRungCount).toBeNull();
    expect(restored.sliderRestored).toBe(true);
  });

  it("does not persist an un-restored default before the first available plan", () => {
    saveRebalancePageState({
      amountInput: "0",
      committedAmount: "0",
      sliderPosition: 1,
      lastAvailableRungCount: null,
      sliderRestored: false,
    });
    expect(localStorage.getItem("rebalance-page-state")).toBeNull();
    const later = rebalancePageReducer(loadRebalancePageState(), {
      type: "planChanged",
      rungCount: 8,
    });
    expect(later.sliderPosition).toBe(8);
  });

  it("falls back to the initial state on missing or corrupt storage", () => {
    expect(loadRebalancePageState().committedAmount).toBe("0");
    localStorage.setItem("rebalance-page-state", "{not json");
    expect(loadRebalancePageState().committedAmount).toBe("0");
  });

  it("clamps a restored slider position instead of jumping to max", () => {
    const restored = {
      amountInput: "0",
      committedAmount: "0",
      sliderPosition: 3,
      lastAvailableRungCount: null,
      sliderRestored: true,
    };
    const afterPlan = rebalancePageReducer(restored, {
      type: "planChanged",
      rungCount: 8,
    });
    expect(afterPlan.sliderPosition).toBe(3);

    const shrunkPool = rebalancePageReducer(restored, {
      type: "planChanged",
      rungCount: 2,
    });
    expect(shrunkPool.sliderPosition).toBe(2);
  });

  it("still defaults a first visit to all assets", () => {
    const first = rebalancePageReducer(
      {
        amountInput: "0",
        committedAmount: "0",
        sliderPosition: 1,
        lastAvailableRungCount: null,
        sliderRestored: false,
      },
      { type: "planChanged", rungCount: 8 },
    );
    expect(first.sliderPosition).toBe(8);
  });
});
