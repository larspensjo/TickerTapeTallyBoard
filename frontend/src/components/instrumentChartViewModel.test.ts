import { describe, expect, it } from "vitest";
import type { PriceHistoryResponse, Transaction } from "../api/types";
import {
  instrumentPriceSeries,
  tradeMarkers,
} from "./instrumentChartViewModel";

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
    dividend_per_share: null,
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

  it("does not plot dividend per-share amounts as transaction price points", () => {
    const result = instrumentPriceSeries(
      resp([
        {
          date: "2026-06-04",
          close: "1000",
          close_base: { status: "available", value: "9570.10" },
        },
      ]),
      [
        tx({
          id: 1,
          type: "Dividend",
          trade_date: "2025-09-16",
          price: null,
          dividend_per_share: "0.227",
        }),
      ],
    );

    expect(result.points).toEqual([{ time: "2026-06-04", value: 1000 }]);
  });
});

describe("tradeMarkers", () => {
  it("maps Buy and Sell into sided markers, ignoring other types", () => {
    const markers = tradeMarkers(
      [
        tx({ id: 1, type: "Buy", trade_date: "2025-05-11", quantity: 2 }),
        tx({ id: 2, type: "Sell", trade_date: "2025-06-20", quantity: -1 }),
        tx({ id: 3, type: "Dividend", trade_date: "2025-07-01" }),
        tx({ id: 4, type: "Split", trade_date: "2025-08-01" }),
      ],
      "USD",
    );

    expect(markers.map((m) => [m.time, m.side])).toEqual([
      ["2025-05-11", "buy"],
      ["2025-06-20", "sell"],
    ]);
  });

  it("emits sell markers from negative quantities with a positive magnitude", () => {
    const markers = tradeMarkers(
      [tx({ id: 1, type: "Sell", trade_date: "2025-10-31", quantity: -11 })],
      "USD",
    );

    expect(markers).toHaveLength(1);
    expect(markers[0].side).toBe("sell");
    expect(markers[0].quantity).toBe(11);
  });

  it("merges same-day, same-side trades with a quantity-weighted price and summed fee", () => {
    const markers = tradeMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          trade_date: "2025-05-11",
          quantity: 1,
          price: "100",
          brokerage: "5",
        }),
        tx({
          id: 2,
          type: "Buy",
          trade_date: "2025-05-11",
          quantity: 3,
          price: "200",
          brokerage: "7",
        }),
      ],
      "USD",
    );

    expect(markers).toHaveLength(1);
    expect(markers[0].quantity).toBe(4);
    expect(markers[0].avgPrice).toBeCloseTo(175); // (100*1 + 200*3) / 4
    expect(markers[0].fee).toBeCloseTo(12);
  });

  it("keeps same-day opposite sides as separate markers", () => {
    const markers = tradeMarkers(
      [
        tx({ id: 1, type: "Buy", trade_date: "2025-05-11", quantity: 2 }),
        tx({ id: 2, type: "Sell", trade_date: "2025-05-11", quantity: 1 }),
      ],
      "USD",
    );

    expect(markers).toHaveLength(2);
    expect(markers.map((m) => m.side).sort()).toEqual(["buy", "sell"]);
  });

  it("reports a null fee when no brokerage was recorded", () => {
    const markers = tradeMarkers(
      [tx({ id: 1, type: "Buy", brokerage: null })],
      "USD",
    );

    expect(markers[0].fee).toBeNull();
    expect(markers[0].feeCurrency).toBeNull();
  });

  it("carries the brokerage currency, not the instrument currency, on the fee", () => {
    const markers = tradeMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          currency: "USD",
          brokerage: "9.60",
          brokerage_currency: "SEK",
        }),
      ],
      "USD",
    );

    expect(markers[0].fee).toBeCloseTo(9.6);
    expect(markers[0].feeCurrency).toBe("SEK");
  });

  it("drops the merged fee when same-day trades report differing fee currencies", () => {
    const markers = tradeMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          trade_date: "2025-05-11",
          brokerage: "9.60",
          brokerage_currency: "SEK",
        }),
        tx({
          id: 2,
          type: "Buy",
          trade_date: "2025-05-11",
          brokerage: "1.00",
          brokerage_currency: "USD",
        }),
      ],
      "USD",
    );

    expect(markers).toHaveLength(1);
    expect(markers[0].fee).toBeNull();
    expect(markers[0].feeCurrency).toBeNull();
  });

  it("skips trades whose currency is not the native currency", () => {
    const markers = tradeMarkers(
      [tx({ id: 1, type: "Buy", currency: "SEK" })],
      "USD",
    );

    expect(markers).toEqual([]);
  });

  it("sorts markers by date", () => {
    const markers = tradeMarkers(
      [
        tx({ id: 1, type: "Buy", trade_date: "2025-06-20" }),
        tx({ id: 2, type: "Sell", trade_date: "2025-05-11" }),
      ],
      "USD",
    );

    expect(markers.map((m) => m.time)).toEqual(["2025-05-11", "2025-06-20"]);
  });
});
