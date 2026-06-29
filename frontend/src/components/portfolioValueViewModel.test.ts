import { describe, expect, it } from "vitest";
import type { ValueHistoryPoint } from "../api/types";
import {
  filterValueHistoryPoints,
  portfolioValueSeries,
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
