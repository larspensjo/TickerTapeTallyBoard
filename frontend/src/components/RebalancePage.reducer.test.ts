import { describe, expect, it } from "vitest";
import { type RebalancePageState, rebalancePageReducer } from "./RebalancePage";

const initialState: RebalancePageState = {
  amountInput: "",
  committedAmount: null,
  sliderPosition: 1,
  lastAvailableRungCount: null,
};

describe("rebalancePageReducer", () => {
  it("keeps the typed amount separate from the committed amount", () => {
    const edited = rebalancePageReducer(initialState, {
      type: "amountInputChanged",
      amountInput: "1,5",
    });

    expect(edited.amountInput).toBe("1,5");
    expect(edited.committedAmount).toBeNull();

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
