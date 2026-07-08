import { describe, expect, it } from "vitest";
import type { Conviction, ConvictionTarget, Holding } from "../api/types";
import {
  type ConvictionEdits,
  convictionEditsReducer,
  convictionEditsToChanges,
  convictionRank,
  effectiveConviction,
  gapBarGeometry,
  hasConvictionEdits,
  holdingConvictionSearchText,
  pendingConvictionChanges,
  TARGET_GAP_BAR_CLAMP_PERCENT,
  targetGapBar,
  targetGapPercentField,
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
});

function target(
  status: ConvictionTarget["status"],
  value: string | null,
  gap: string | null,
  gapPercent: string | null,
): ConvictionTarget {
  return {
    conviction: "High",
    status,
    target_value_base: value
      ? { status: "available", value }
      : { status: "unavailable", reasons: ["no_target"] },
    target_gap_base: gap
      ? { status: "available", value: gap }
      : { status: "unavailable", reasons: ["no_target"] },
    target_gap_percent: gapPercent
      ? { status: "available", value: gapPercent }
      : { status: "unavailable", reasons: ["no_target"] },
  };
}

describe("target gap bar", () => {
  it("puts an above-target gap on the right, proportional to the gap percent", () => {
    const bar = targetGapBar(target("above", "2000.00", "500.00", "25.00"));
    expect(bar).toEqual({
      side: "above",
      widthPercent: 25,
      tooltip: "Target SEK 2,000.00\nGap SEK 500.00 (25.00%)\nAbove",
    });
  });

  it("puts a below-target gap on the left", () => {
    const bar = targetGapBar(target("below", "2000.00", "-717.00", "-35.85"));
    expect(bar?.side).toBe("below");
    expect(bar?.widthPercent).toBeCloseTo(35.85);
  });

  it("clamps gaps beyond +/-50% to a full half-bar", () => {
    expect(
      targetGapBar(target("above", "1000.00", "800.00", "80.00"))?.widthPercent,
    ).toBe(50);
    expect(
      targetGapBar(target("below", "1000.00", "-900.00", "-90.00"))
        ?.widthPercent,
    ).toBe(50);
  });

  it("renders an on-target gap as the bare axis", () => {
    const bar = targetGapBar(target("on_target", "1000.00", "0.00", "0.00"));
    expect(bar?.side).toBe("on_target");
    expect(bar?.widthPercent).toBe(0);
  });

  it("returns null when no gap can be computed", () => {
    expect(targetGapBar(target("no_target", null, null, null))).toBeNull();
  });

  it("sorts the target column by the signed gap percent", () => {
    const field = targetGapPercentField(
      target("below", "2000.00", "-717.00", "-35.85"),
    );
    expect(field).toEqual({ status: "available", value: "-35.85" });
  });
});

describe("gapBarGeometry", () => {
  it("maps sign to side and clamps magnitude to the half-track", () => {
    expect(gapBarGeometry(0)).toEqual({ side: "on_target", widthPercent: 0 });
    expect(gapBarGeometry(25)).toEqual({ side: "above", widthPercent: 25 });
    expect(gapBarGeometry(-25)).toEqual({ side: "below", widthPercent: 25 });
    expect(gapBarGeometry(TARGET_GAP_BAR_CLAMP_PERCENT * 3)).toEqual({
      side: "above",
      widthPercent: 50,
    });
  });
});

describe("search text", () => {
  it("includes conviction and target status", () => {
    const text = holdingConvictionSearchText(holding(1, "Medium"));
    expect(text).toContain("Medium");
    expect(text).toContain("Below");
  });
});
