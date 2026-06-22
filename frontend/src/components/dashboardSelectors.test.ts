import { describe, expect, it } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { topMovers } from "./dashboardSelectors";

function inst(id: number, symbol: string, name = symbol): Instrument {
  return { id, symbol, exchange: "STO", name, type: "Stock", currency: "SEK" };
}

function row(
  instrument: Instrument,
  status: "open" | "closed",
  dayChangePercent: GainsRow["day_change_percent"],
): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument,
    quantity: status === "open" ? 1 : 0,
    cost_basis_native: "0.00",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: money,
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: money,
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: dayChangePercent,
    reasons: [],
    position_status: status,
  };
}

const avail = (value: string) => ({ status: "available", value }) as const;
const unavail = {
  status: "unavailable",
  reasons: ["zero_previous_market_value"],
} satisfies GainsRow["day_change_percent"];

describe("topMovers", () => {
  it("ranks open gainers desc and losers asc, excluding unavailable and closed rows", () => {
    const rows = [
      row(inst(1, "AAA"), "open", avail("5.0")),
      row(inst(2, "BBB"), "open", avail("-3.0")),
      row(inst(3, "CCC"), "open", avail("1.0")),
      row(inst(4, "DDD"), "open", avail("-8.0")),
      row(inst(5, "EEE"), "open", unavail),
      row(inst(6, "FFF"), "closed", avail("99.0")),
    ];
    const { gainers, losers } = topMovers(rows);
    expect(gainers.map((m) => m.instrument.symbol)).toEqual(["AAA", "CCC"]);
    expect(losers.map((m) => m.instrument.symbol)).toEqual(["DDD", "BBB"]);
  });

  it("breaks ties by symbol then name", () => {
    const rows = [
      row(inst(1, "ZZZ"), "open", avail("4.0")),
      row(inst(2, "AAA"), "open", avail("4.0")),
    ];
    const { gainers } = topMovers(rows);
    expect(gainers.map((m) => m.instrument.symbol)).toEqual(["AAA", "ZZZ"]);
  });

  it("caps each side at three and tolerates fewer", () => {
    const rows = [
      row(inst(1, "A"), "open", avail("1")),
      row(inst(2, "B"), "open", avail("2")),
      row(inst(3, "C"), "open", avail("3")),
      row(inst(4, "D"), "open", avail("4")),
    ];
    const { gainers, losers } = topMovers(rows);
    expect(gainers).toHaveLength(3);
    expect(losers).toHaveLength(0);
  });

  it("returns empty sides for all-flat / empty input", () => {
    expect(topMovers([])).toEqual({ gainers: [], losers: [] });
    const { gainers, losers } = topMovers([
      row(inst(1, "A"), "open", avail("0")),
    ]);
    expect(gainers).toEqual([]);
    expect(losers).toEqual([]);
  });
});
