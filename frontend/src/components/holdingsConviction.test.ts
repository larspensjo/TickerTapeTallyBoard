import { describe, expect, it } from "vitest";
import type { Conviction, Holding } from "../api/types";
import {
  type ConvictionEdits,
  convictionEditsReducer,
  convictionEditsToChanges,
  convictionRank,
  effectiveConviction,
  hasConvictionEdits,
  holdingConvictionSearchText,
  pendingConvictionChanges,
  targetGapTone,
  targetStatusRank,
} from "./holdingsConviction";

function holding(id: number, conviction: Conviction): Holding {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: {
      id,
      symbol: `S${id}`,
      exchange: "STO",
      name: `Name ${id}`,
      type: "Stock",
      currency: "SEK",
      conviction,
    },
    quantity: 1,
    cost_basis_native: "100.00",
    average_cost_native: "100.00",
    base: {
      status: "available",
      cost_basis_base: "100.00",
      average_cost_base: "100.00",
      fee_component_base: "0.00",
    },
    valuation: {
      market_value_base: money,
      unrealized_gain_base: money,
      unrealized_gain_percent: money,
      day_change_base: money,
    },
    conviction_target: {
      conviction,
      status: "below",
      target_value_base: { status: "available", value: "100.00" },
      target_gap_base: { status: "available", value: "-10.00" },
      target_gap_percent: { status: "available", value: "-10.00" },
    },
  };
}

describe("conviction edits reducer", () => {
  it("stages a change that differs from the saved value", () => {
    const state = convictionEditsReducer(
      {},
      { type: "stage", instrumentId: 1, saved: "Other", conviction: "High" },
    );
    expect(state).toEqual({ 1: "High" });
    expect(hasConvictionEdits(state)).toBe(true);
  });

  it("clears a pending change when the saved value is re-selected", () => {
    const staged: ConvictionEdits = { 1: "High" };
    const state = convictionEditsReducer(staged, {
      type: "stage",
      instrumentId: 1,
      saved: "Other",
      conviction: "Other",
    });
    expect(state).toEqual({});
    expect(hasConvictionEdits(state)).toBe(false);
  });

  it("discards all staged changes", () => {
    const staged: ConvictionEdits = { 1: "High", 2: "Low" };
    expect(convictionEditsReducer(staged, { type: "discard" })).toEqual({});
  });

  it("does not mutate the previous state", () => {
    const staged: ConvictionEdits = { 1: "High" };
    convictionEditsReducer(staged, {
      type: "stage",
      instrumentId: 2,
      saved: "Other",
      conviction: "Low",
    });
    expect(staged).toEqual({ 1: "High" });
  });
});

describe("conviction edits serialization", () => {
  it("converts staged edits into API changes", () => {
    const changes = convictionEditsToChanges({ 3: "Medium", 7: "Low" });
    expect(changes).toEqual([
      { instrument_id: 3, conviction: "Medium" },
      { instrument_id: 7, conviction: "Low" },
    ]);
  });

  it("drops staged edits that already match the saved conviction", () => {
    // Instrument 1's saved value is "Other" (from the holding factory), so a
    // staged "Other" is a no-op; a staged "High" is a real change.
    const holdings = [holding(1, "Other"), holding(2, "Other")];
    const changes = pendingConvictionChanges(
      { 1: "Other", 2: "High" },
      holdings,
    );
    expect(changes).toEqual([{ instrument_id: 2, conviction: "High" }]);
  });
});

describe("effective conviction", () => {
  it("prefers a staged edit over the saved value", () => {
    const h = holding(1, "Other");
    expect(effectiveConviction(h, {})).toBe("Other");
    expect(effectiveConviction(h, { 1: "High" })).toBe("High");
  });
});

describe("sort ranks and tones", () => {
  it("orders convictions from Other to High", () => {
    expect(convictionRank("Other")).toBeLessThan(convictionRank("Low"));
    expect(convictionRank("Low")).toBeLessThan(convictionRank("Medium"));
    expect(convictionRank("Medium")).toBeLessThan(convictionRank("High"));
  });

  it("gives a stable rank to each target status", () => {
    expect(targetStatusRank("below")).toBeLessThan(targetStatusRank("above"));
    expect(targetStatusRank("above")).toBeLessThan(
      targetStatusRank("unavailable"),
    );
  });

  it("colours a below-target (negative) gap as up and above-target as down", () => {
    expect(targetGapTone("-10.00")).toBe("up");
    expect(targetGapTone("10.00")).toBe("down");
    expect(targetGapTone("0.00")).toBe("flat");
  });
});

describe("search text", () => {
  it("includes conviction and target status", () => {
    const text = holdingConvictionSearchText(holding(1, "Medium"));
    expect(text).toContain("Medium");
    expect(text).toContain("Below");
  });
});
