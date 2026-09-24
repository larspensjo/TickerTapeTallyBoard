import { describe, expect, it } from "vitest";
import type { ValueHistoryPoint } from "../api/types";
import { referenceEdgeTag } from "./portfolioValueViewModel";

function point(
  date: string,
  value: string,
  invested: string | null,
): ValueHistoryPoint {
  return {
    date,
    value_base: value,
    invested_base: invested,
    incomplete: false,
    included_count: 1,
    excluded_count: 0,
  };
}

describe("referenceEdgeTag", () => {
  const value = [
    { time: "2026-09-21", value: 3_950_000 },
    { time: "2026-09-22", value: 4_100_000 },
  ];

  it("announces a reference line that sits entirely below the drawn value band", () => {
    const tag = referenceEdgeTag(value, [
      { time: "2026-09-21", value: 750_000 },
      { time: "2026-09-22", value: 760_000 },
    ]);

    expect(tag).toEqual({ side: "below", value: 760_000 });
  });

  it("announces a reference line that sits entirely above the drawn value band", () => {
    const tag = referenceEdgeTag(value, [
      { time: "2026-09-21", value: 5_000_000 },
      { time: "2026-09-22", value: 5_100_000 },
    ]);

    expect(tag).toEqual({ side: "above", value: 5_100_000 });
  });

  it("stays silent when the reference line crosses the visible band", () => {
    expect(
      referenceEdgeTag(value, [
        { time: "2026-09-21", value: 3_000_000 },
        { time: "2026-09-22", value: 4_500_000 },
      ]),
    ).toBeNull();
  });

  it("stays silent when the two ranges overlap", () => {
    expect(
      referenceEdgeTag(value, [
        { time: "2026-09-21", value: 4_000_000 },
        { time: "2026-09-22", value: 4_050_000 },
      ]),
    ).toBeNull();
  });

  it("stays silent when either series is empty", () => {
    expect(referenceEdgeTag(value, [])).toBeNull();
    expect(referenceEdgeTag([], [{ time: "2026-09-22", value: 1 }])).toBeNull();
  });

  it("reports the reference value at the latest drawn date, not its extreme", () => {
    const tag = referenceEdgeTag(value, [
      { time: "2026-09-21", value: 100_000 },
      { time: "2026-09-22", value: 700_000 },
    ]);

    expect(tag).toEqual({ side: "below", value: 700_000 });
  });
});
