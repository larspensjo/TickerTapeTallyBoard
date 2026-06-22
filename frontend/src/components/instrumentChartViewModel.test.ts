import { describe, expect, it } from "vitest";
import type { PriceHistoryResponse, Transaction } from "../api/types";
import { instrumentPriceSeries } from "./instrumentChartViewModel";

function resp(points: PriceHistoryResponse["points"]): PriceHistoryResponse {
  return {
    instrument_id: 1,
    currency: "USD",
    base_currency: "SEK",
    points,
  };
}

function tx(overrides: Partial<Transaction>): Transaction {
  return {
    id: 1,
    instrument_id: 1,
    type: "Buy",
    trade_date: "2025-05-11",
    quantity: 1,
    price: "140.681299",
    currency: "USD",
    fx_rate_to_base: "9.5701",
    brokerage: null,
    brokerage_currency: null,
    source_value: null,
    source_currency: null,
    note: null,
    import_batch_id: null,
    ...overrides,
  };
}

describe("instrumentPriceSeries", () => {
  it("plots native close prices, not SEK-converted close_base values", () => {
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
    expect(result.points).toEqual([
      { time: "2026-01-02", value: 100 },
      { time: "2026-01-03", value: 110 },
    ]);
    expect(result.allUnavailable).toBe(false);
  });

  it("reports allUnavailable when every native close is invalid", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2026-01-02",
          close: "not-a-number",
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
    expect(result.allUnavailable).toBe(false);
  });

  it("returns empty for undefined input", () => {
    const result = instrumentPriceSeries(undefined);
    expect(result.points).toEqual([]);
    expect(result.allUnavailable).toBe(false);
  });

  it("prepends the first transaction price when provider history starts later", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2025-06-04",
          close: "157.11",
          close_base: { status: "available", value: "1503.55" },
        },
      ]),
      [tx({})],
    );

    expect(result.points[0].time).toBe("2025-05-11");
    expect(result.points[0].value).toBeCloseTo(140.681299);
    expect(result.points[1]).toEqual({ time: "2025-06-04", value: 157.11 });
  });

  it("includes native-currency transaction prices throughout gaps in provider history", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2026-06-04",
          close: "1000",
          close_base: { status: "available", value: "9570.10" },
        },
      ]),
      [
        tx({ id: 1, trade_date: "2025-09-16", price: "157.99" }),
        tx({ id: 2, trade_date: "2025-10-20", price: "208.43" }),
        tx({ id: 3, trade_date: "2025-11-20", price: "204.87" }),
      ],
    );

    expect(result.points).toEqual([
      { time: "2025-09-16", value: 157.99 },
      { time: "2025-10-20", value: 208.43 },
      { time: "2025-11-20", value: 204.87 },
      { time: "2026-06-04", value: 1000 },
    ]);
  });

  it("does not prepend a transaction point when its currency is not the native currency", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2025-06-04",
          close: "157.11",
          close_base: { status: "available", value: "1503.55" },
        },
      ]),
      [tx({ currency: "SEK" })],
    );

    expect(result.points).toEqual([{ time: "2025-06-04", value: 157.11 }]);
  });
});
