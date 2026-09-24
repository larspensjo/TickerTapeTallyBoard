import { describe, expect, it } from "vitest";
import type { ValueHistoryPoint } from "../api/types";
import {
  filterValueHistoryPoints,
  portfolioValueSeries,
  referenceEdgeTag,
} from "./portfolioValueViewModel";

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

describe("portfolioValueSeries", () => {
  it("maps value_base and invested_base into parallel numeric series", () => {
    const { value, invested } = portfolioValueSeries([
      point("2026-01-02", "1000.00", "1000.00"),
      point("2026-01-05", "1100.00", "405.00"),
    ]);

    expect(value).toEqual([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-05", value: 1100 },
    ]);
    expect(invested).toEqual([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-05", value: 405 },
    ]);
  });

  it("omits invested points when invested_base is null so the line shows a gap", () => {
    const { value, invested } = portfolioValueSeries([
      point("2026-01-02", "1000.00", null),
      point("2026-01-05", "1100.00", "900.00"),
    ]);

    expect(value).toHaveLength(2);
    expect(invested).toEqual([{ time: "2026-01-05", value: 900 }]);
  });
});

describe("filterValueHistoryPoints", () => {
  it("keeps points inside the inclusive date range", () => {
    const points = [
      point("2026-01-01", "1000.00", "1000.00"),
      point("2026-02-01", "1100.00", "1000.00"),
      point("2026-03-01", "1200.00", "1000.00"),
    ];

    expect(
      filterValueHistoryPoints(points, {
        startDate: "2026-02-01",
        endDate: "2026-02-28",
      }).map((filteredPoint) => filteredPoint.date),
    ).toEqual(["2026-02-01"]);
  });

  it("treats null boundaries as open-ended", () => {
    const points = [
      point("2026-01-01", "1000.00", "1000.00"),
      point("2026-02-01", "1100.00", "1000.00"),
    ];

    expect(
      filterValueHistoryPoints(points, {
        startDate: null,
        endDate: "2026-01-31",
      }).map((filteredPoint) => filteredPoint.date),
    ).toEqual(["2026-01-01"]);
  });
});

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
