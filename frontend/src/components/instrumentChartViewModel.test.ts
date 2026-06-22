import { describe, expect, it } from "vitest";
import type { PriceHistoryResponse } from "../api/types";
import { instrumentPriceSeries } from "./instrumentChartViewModel";

function resp(points: PriceHistoryResponse["points"]): PriceHistoryResponse {
  return {
    instrument_id: 1,
    currency: "USD",
    base_currency: "SEK",
    points,
  };
}

describe("instrumentPriceSeries", () => {
  it("excludes unavailable close_base points from the plotted series", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2026-01-02",
          close: "100",
          close_base: { status: "available", value: "1000.00" },
        },
        {
          date: "2026-01-03",
          close: "110",
          close_base: { status: "unavailable", reasons: ["missing_fx"] },
        },
      ]),
    );
    expect(result.points).toEqual([{ time: "2026-01-02", value: 1000 }]);
    expect(result.droppedForMissingFx).toBe(1);
    expect(result.allUnavailable).toBe(false);
  });

  it("reports allUnavailable when every point is unavailable", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2026-01-02",
          close: "100",
          close_base: { status: "unavailable", reasons: ["missing_fx"] },
        },
      ]),
    );
    expect(result.points).toEqual([]);
    expect(result.allUnavailable).toBe(true);
  });

  it("treats an empty response as empty, not all-unavailable", () => {
    const result = instrumentPriceSeries(resp([]));
    expect(result.points).toEqual([]);
    expect(result.droppedForMissingFx).toBe(0);
    expect(result.allUnavailable).toBe(false);
  });

  it("returns empty for undefined input", () => {
    const result = instrumentPriceSeries(undefined);
    expect(result.points).toEqual([]);
    expect(result.allUnavailable).toBe(false);
  });
});
