import { describe, expect, it } from "vitest";
import type {
  PriceHistoryResponse,
  RunningAverageCostSegment,
  Transaction,
} from "../api/types";
import type { ChartWindow } from "./chartTimeAxis";
import {
  averageAfterByDate,
  instrumentPriceSeries,
  priceChartMarkers,
  runningAverageCostLine,
  tradeMarkers,
} from "./instrumentChartViewModel";

function resp(points: PriceHistoryResponse["points"]): PriceHistoryResponse {
  return {
    instrument_id: 1,
    currency: "USD",
    base_currency: "SEK",
    points,
    running_average_cost: { segments: [], current_average: null },
  };
}

function withRunningAverage(
  segments: RunningAverageCostSegment[],
  currentAverage: string | null,
): PriceHistoryResponse {
  return {
    ...resp([]),
    running_average_cost: { segments, current_average: currentAverage },
  };
}

function segment(
  steps: [string, string][],
  closedOn: string | null,
): RunningAverageCostSegment {
  return {
    steps: steps.map(([date, average]) => ({ date, average })),
    closed_on: closedOn,
  };
}

function plotted(start: string, end: string): ChartWindow {
  return { start, end, startsAtVisibleStart: true };
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
          source: "YAHOO",
          close_base: { status: "available", value: "1000.00" },
        },
        {
          date: "2026-01-03",
          close: "110",
          source: "YAHOO",
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
          source: "YAHOO",
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
          source: "YAHOO",
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
          source: "YAHOO",
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
          source: "YAHOO",
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
          source: "YAHOO",
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

describe("runningAverageCostLine", () => {
  const window = plotted("2025-01-01", "2025-12-31");

  it("extends an open segment to the window end and tags it with the backend's text", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-02-01", "112"],
              ["2025-03-01", "112.125"],
            ],
            null,
          ),
        ],
        "112.12",
      ),
      window,
    );

    expect(lines).toHaveLength(1);
    expect(lines[0].points).toEqual([
      { time: "2025-02-01", value: 112 },
      { time: "2025-03-01", value: 112.125 },
      { time: "2025-12-31", value: 112.125 },
    ]);
    expect(lines[0].showLastValue).toBe(true);
    expect(lines[0].lastValueLabel).toBe("112.12");
  });

  it("ends a closed segment on its closing date without a tag", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [segment([["2025-02-01", "100"]], "2025-06-20")],
        null,
      ),
      window,
    );

    expect(lines).toHaveLength(1);
    expect(lines[0].points).toEqual([
      { time: "2025-02-01", value: 100 },
      { time: "2025-06-20", value: 100 },
    ]);
    expect(lines[0].showLastValue).toBe(false);
    expect(lines[0].lastValueLabel).toBeUndefined();
  });

  it("draws a reopened position as two lines with the tag only on the open one", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment([["2025-02-01", "100"]], "2025-06-20"),
          segment([["2025-09-01", "95.1"]], null),
        ],
        "95.10",
      ),
      window,
    );

    expect(lines).toHaveLength(2);
    expect(lines[0].key).not.toBe(lines[1].key);
    expect(lines[0].points.at(-1)).toEqual({ time: "2025-06-20", value: 100 });
    expect(lines[1].points).toEqual([
      { time: "2025-09-01", value: 95.1 },
      { time: "2025-12-31", value: 95.1 },
    ]);
    expect(lines.map((line) => line.showLastValue)).toEqual([false, true]);
    expect(lines.map((line) => line.lastValueLabel)).toEqual([
      undefined,
      "95.10",
    ]);
  });

  it("flags no line for a last value on a sold-out instrument", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment([["2025-02-01", "100"]], "2025-03-01"),
          segment([["2025-05-01", "90"]], "2025-07-01"),
        ],
        null,
      ),
      window,
    );

    expect(lines).toHaveLength(2);
    expect(lines.some((line) => line.showLastValue)).toBe(false);
    expect(lines.some((line) => line.lastValueLabel !== undefined)).toBe(false);
  });

  it("carries the prevailing average to the window start when clipped on the left", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2024-11-01", "100"],
              ["2024-12-01", "110"],
              ["2025-02-01", "120"],
            ],
            null,
          ),
        ],
        "120.00",
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-01-01", value: 110 },
      { time: "2025-02-01", value: 120 },
      { time: "2025-12-31", value: 120 },
    ]);
  });

  it("does not add a carried point when a step falls on the window start", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2024-12-01", "110"],
              ["2025-01-01", "120"],
            ],
            "2025-03-01",
          ),
        ],
        null,
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-01-01", value: 120 },
      { time: "2025-03-01", value: 120 },
    ]);
  });

  it("stops a segment at the window end when clipped on the right", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-11-01", "100"],
              ["2026-01-15", "105"],
            ],
            "2026-02-01",
          ),
        ],
        null,
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-11-01", value: 100 },
      { time: "2025-12-31", value: 100 },
    ]);
  });

  it("drops segments wholly outside the window", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment([["2024-02-01", "80"]], "2024-06-01"),
          segment([["2025-03-01", "90"]], "2025-04-01"),
          segment([["2026-03-01", "100"]], null),
        ],
        "100.00",
      ),
      window,
    );

    expect(lines).toHaveLength(1);
    expect(lines[0].points[0]).toEqual({ time: "2025-03-01", value: 90 });
  });

  it("does not duplicate the terminal point on the last step date", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-06-01", "100"],
              ["2025-12-31", "104"],
            ],
            null,
          ),
        ],
        "104.00",
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-06-01", value: 100 },
      { time: "2025-12-31", value: 104 },
    ]);
  });

  it("leaves a right-clipped open segment untagged", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-11-01", "100"],
              ["2026-01-15", "105"],
            ],
            null,
          ),
        ],
        "105.00",
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-11-01", value: 100 },
      { time: "2025-12-31", value: 100 },
    ]);
    expect(lines[0].showLastValue).toBe(false);
    expect(lines[0].lastValueLabel).toBeUndefined();
  });

  it("leaves an open segment untagged when its last step is unparsable", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-02-01", "100"],
              ["2025-03-01", "not-a-number"],
            ],
            null,
          ),
        ],
        "104.00",
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-02-01", value: 100 },
      { time: "2025-12-31", value: 100 },
    ]);
    expect(lines[0].showLastValue).toBe(false);
    expect(lines[0].lastValueLabel).toBeUndefined();
  });

  it("skips unparsable steps", () => {
    const lines = runningAverageCostLine(
      withRunningAverage(
        [
          segment(
            [
              ["2025-02-01", "100"],
              ["2025-03-01", "not-a-number"],
            ],
            "2025-04-01",
          ),
        ],
        null,
      ),
      window,
    );

    expect(lines[0].points).toEqual([
      { time: "2025-02-01", value: 100 },
      { time: "2025-04-01", value: 100 },
    ]);
  });

  it("returns no lines without segments, a response, or a window", () => {
    expect(
      runningAverageCostLine(withRunningAverage([], null), window),
    ).toEqual([]);
    expect(runningAverageCostLine(undefined, window)).toEqual([]);
    expect(
      runningAverageCostLine(
        withRunningAverage([segment([["2025-02-01", "100"]], null)], "100.00"),
        null,
      ),
    ).toEqual([]);
  });
});

describe("averageAfterByDate", () => {
  it("maps every parsable step date to its average across segments", () => {
    const averages = averageAfterByDate(
      withRunningAverage(
        [
          segment(
            [
              ["2025-02-01", "100"],
              ["2025-03-01", "bad"],
            ],
            "2025-04-01",
          ),
          segment([["2025-09-01", "95.1"]], null),
        ],
        "95.10",
      ),
    );

    expect([...averages]).toEqual([
      ["2025-02-01", 100],
      ["2025-09-01", 95.1],
    ]);
  });

  it("is empty without a response", () => {
    expect(averageAfterByDate(undefined).size).toBe(0);
  });
});

describe("priceChartMarkers", () => {
  function rows(marker: { rows: { label: string; value: string }[] }) {
    return marker.rows.map((row) => [row.label, row.value]);
  }

  it("keeps the Buy and Sell tooltip rows, with the fee last", () => {
    const markers = priceChartMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          trade_date: "2025-05-11",
          quantity: 1200,
          price: "140.5",
          brokerage: "9.6",
          brokerage_currency: "SEK",
        }),
        tx({
          id: 2,
          type: "Sell",
          trade_date: "2025-06-20",
          quantity: -200,
          price: "150",
        }),
      ],
      "USD",
      new Map(),
    );

    expect(markers.map((marker) => [marker.title, marker.side])).toEqual([
      ["Buy", "buy"],
      ["Sell", "sell"],
    ]);
    expect(rows(markers[0])).toEqual([
      ["Date", "2025-05-11"],
      ["Quantity", "1,200"],
      ["Avg price", "140.5 USD"],
      ["Fee", "9.60 SEK"],
    ]);
    expect(rows(markers[1])).toEqual([
      ["Date", "2025-06-20"],
      ["Quantity", "200"],
      ["Avg price", "150 USD"],
    ]);
  });

  it("keeps the Split tooltip rows", () => {
    const markers = priceChartMarkers(
      [
        tx({ id: 1, type: "Buy", trade_date: "2025-05-11", quantity: 10 }),
        tx({
          id: 2,
          type: "Split",
          trade_date: "2025-07-01",
          quantity: 30,
          price: null,
        }),
      ],
      "USD",
      new Map([["2025-07-01", 25]]),
    );

    const split = markers.find((marker) => marker.side === "split");
    expect(split?.title).toBe("Split");
    expect(split ? rows(split) : []).toEqual([
      ["Date", "2025-07-01"],
      ["Ratio", "4:1"],
      ["Before", "10"],
      ["After", "40"],
      ["Delta", "+30"],
    ]);
  });

  it("adds Avg after to a buy day with a running-average step, after Avg price", () => {
    const markers = priceChartMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          trade_date: "2025-05-11",
          quantity: 1,
          price: "112.25",
          brokerage: "9.6",
          brokerage_currency: "SEK",
        }),
      ],
      "USD",
      new Map([["2025-05-11", 112.125]]),
    );

    expect(rows(markers[0])).toEqual([
      ["Date", "2025-05-11"],
      ["Quantity", "1"],
      ["Avg price", "112.25 USD"],
      ["Avg after", "112.13 USD"],
      ["Fee", "9.60 SEK"],
    ]);
  });

  it("shows the split-adjusted average after a pre-split buy", () => {
    const response = withRunningAverage(
      [segment([["2025-05-11", "75"]], null)],
      "75.00",
    );
    const markers = priceChartMarkers(
      [
        tx({
          id: 1,
          type: "Buy",
          trade_date: "2025-05-11",
          quantity: 10,
          price: "300",
        }),
        tx({
          id: 2,
          type: "Split",
          trade_date: "2025-07-01",
          quantity: 30,
          price: null,
        }),
      ],
      "USD",
      averageAfterByDate(response),
    );

    const buy = markers.find((marker) => marker.side === "buy");
    expect(buy ? rows(buy) : []).toContainEqual(["Avg price", "300 USD"]);
    expect(buy ? rows(buy) : []).toContainEqual(["Avg after", "75 USD"]);
  });

  it("does not add Avg after to sells or to a buy day that ends flat", () => {
    const markers = priceChartMarkers(
      [
        tx({ id: 1, type: "Buy", trade_date: "2025-05-11", quantity: 10 }),
        tx({ id: 2, type: "Sell", trade_date: "2025-05-11", quantity: -2 }),
        tx({ id: 3, type: "Buy", trade_date: "2025-06-01", quantity: 4 }),
        tx({ id: 4, type: "Sell", trade_date: "2025-06-01", quantity: -12 }),
      ],
      "USD",
      // The flat-ending 2025-06-01 has no step.
      new Map([["2025-05-11", 140.68]]),
    );

    const labelled = markers.filter((marker) =>
      marker.rows.some((row) => row.label === "Avg after"),
    );
    expect(labelled.map((marker) => [marker.time, marker.side])).toEqual([
      ["2025-05-11", "buy"],
    ]);
  });
});
