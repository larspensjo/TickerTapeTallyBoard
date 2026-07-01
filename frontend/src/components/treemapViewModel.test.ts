import { describe, expect, it } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { treemapViewModel } from "./treemapViewModel";

function inst(symbol: string, exchange = "NASDAQ"): Instrument {
  return {
    id: 1,
    symbol,
    exchange,
    name: symbol,
    type: "Stock",
    currency: "USD",
  };
}

function row(
  instrument: Instrument,
  marketValue: GainsRow["market_value_base"],
  opts: {
    position_status?: "open" | "closed";
    total_return_percent?: GainsRow["total_return_percent"];
  } = {},
): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument,
    quantity: 10,
    cost_basis_native: "1000",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: opts.total_return_percent ?? {
      status: "available",
      value: "10.00",
    },
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: marketValue,
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    realized_gain_base: money,
    realized_cost_basis_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: money,
    reasons: [],
    position_status: opts.position_status ?? "open",
  } as GainsRow;
}

const avail = (value: string) => ({ status: "available", value }) as const;
const unavailMv = {
  status: "unavailable",
  reasons: ["missing_price"],
} as const;
const unavailPct = {
  status: "unavailable",
  reasons: ["missing_price"],
} as const;

describe("treemapViewModel", () => {
  it("includes open positions with available market value, sorted by value desc", () => {
    const tiles = treemapViewModel([
      row(inst("SMALL"), avail("1000.00")),
      row(inst("BIG"), avail("5000.00")),
      row(inst("MID"), avail("3000.00")),
    ]);
    expect(tiles.map((t: { symbol: string }) => t.symbol)).toEqual([
      "BIG",
      "MID",
      "SMALL",
    ]);
    expect(tiles[0].marketValueBase).toBe(5000);
  });

  it("excludes closed positions", () => {
    const tiles = treemapViewModel([
      row(inst("OPEN"), avail("1000.00")),
      row(inst("CLOSED"), avail("2000.00"), { position_status: "closed" }),
    ]);
    expect(tiles.map((t: { symbol: string }) => t.symbol)).toEqual(["OPEN"]);
  });

  it("excludes positions with unavailable market value", () => {
    const tiles = treemapViewModel([
      row(inst("OK"), avail("1000.00")),
      row(inst("NO"), unavailMv as unknown as GainsRow["market_value_base"]),
    ]);
    expect(tiles.map((t: { symbol: string }) => t.symbol)).toEqual(["OK"]);
  });

  it("excludes positions with zero or negative market value", () => {
    const tiles = treemapViewModel([
      row(inst("OK"), avail("1000.00")),
      row(inst("ZERO"), avail("0.00")),
    ]);
    expect(tiles.map((t: { symbol: string }) => t.symbol)).toEqual(["OK"]);
  });

  it("maps available total_return_percent to a number", () => {
    const [tile] = treemapViewModel([
      row(inst("AAPL"), avail("5000.00"), {
        total_return_percent: avail("42.50"),
      }),
    ]);
    expect(tile.totalReturnPercent).toBe(42.5);
  });

  it("maps unavailable total_return_percent to null", () => {
    const [tile] = treemapViewModel([
      row(inst("AAPL"), avail("5000.00"), {
        total_return_percent:
          unavailPct as unknown as GainsRow["total_return_percent"],
      }),
    ]);
    expect(tile.totalReturnPercent).toBeNull();
  });

  it("uses symbol, exchange, and name from the instrument", () => {
    const [tile] = treemapViewModel([
      row(
        {
          id: 1,
          symbol: "MSFT",
          exchange: "NYSE",
          name: "Microsoft Corporation",
          type: "Stock",
          currency: "USD",
        },
        avail("1000.00"),
      ),
    ]);
    expect(tile.symbol).toBe("MSFT");
    expect(tile.exchange).toBe("NYSE");
    expect(tile.name).toBe("Microsoft Corporation");
  });

  it("returns empty array for empty input", () => {
    expect(treemapViewModel([])).toEqual([]);
  });
});
