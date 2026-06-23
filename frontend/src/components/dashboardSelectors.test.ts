import { describe, expect, it } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { allocationBreakdown, topMovers } from "./dashboardSelectors";

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
    realized_gain_base: money,
    realized_cost_basis_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: dayChangePercent,
    reasons: [],
    position_status: status,
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
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

describe("allocationBreakdown", () => {
  function mvRow(
    instrument: Instrument,
    marketValue: GainsRow["market_value_base"],
    status: "open" | "closed" = "open",
  ): GainsRow {
    const gainsRow = row(instrument, status, {
      status: "available",
      value: "0.00",
    });
    return { ...gainsRow, market_value_base: marketValue };
  }

  it("aggregates by instrument and computes weights summing to 100", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "750.00" }),
      mvRow(inst(2, "BBB"), { status: "available", value: "250.00" }),
    ];
    const { slices, excludedCount } = allocationBreakdown(rows, "instrument");
    expect(excludedCount).toBe(0);
    expect(slices.map((slice) => slice.label)).toEqual(["AAA", "BBB"]);
    expect(slices.map((slice) => slice.weightPercent)).toEqual([75, 25]);
    expect(slices.reduce((sum, slice) => sum + slice.weightPercent, 0)).toBe(
      100,
    );
  });

  it("labels instrument slices with the name and keeps the ISIN as secondary", () => {
    const rows = [
      mvRow(inst(1, "US5951121038", "Microsoft Corp"), {
        status: "available",
        value: "100.00",
      }),
    ];
    const { slices } = allocationBreakdown(rows, "instrument");
    expect(slices[0].label).toBe("Microsoft Corp");
    expect(slices[0].secondary).toBe("US5951121038");
  });

  it("omits the secondary ISIN when no distinct name is known", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "100.00" }),
    ];
    const [slice] = allocationBreakdown(rows, "instrument").slices;
    expect(slice.label).toBe("AAA");
    expect(slice.secondary).toBeUndefined();
  });

  it("never sets a secondary label for currency or type slices", () => {
    const rows = [
      mvRow(inst(1, "US5951121038", "Microsoft Corp"), {
        status: "available",
        value: "100.00",
      }),
    ];
    expect(
      allocationBreakdown(rows, "currency").slices.every(
        (slice) => slice.secondary === undefined,
      ),
    ).toBe(true);
  });

  it("groups by currency and type", () => {
    const usd: Instrument = { ...inst(1, "AAA"), currency: "USD" };
    const sek: Instrument = { ...inst(2, "BBB"), currency: "SEK" };
    const etf: Instrument = { ...inst(3, "CCC"), type: "Etf" };
    const rows = [
      mvRow(usd, { status: "available", value: "100.00" }),
      mvRow(sek, { status: "available", value: "100.00" }),
      mvRow(etf, { status: "available", value: "200.00" }),
    ];
    expect(
      allocationBreakdown(rows, "currency")
        .slices.map((slice) => slice.label)
        .sort(),
    ).toEqual(["SEK", "USD"]);
    expect(
      allocationBreakdown(rows, "type").slices.some(
        (slice) => slice.label === "Etf",
      ),
    ).toBe(true);
  });

  it("excludes unavailable market values, never counting them as zero", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "100.00" }),
      mvRow(inst(2, "BBB"), {
        status: "unavailable",
        reasons: ["missing_fx"],
      }),
    ];
    const { slices, excludedCount } = allocationBreakdown(rows, "instrument");
    expect(excludedCount).toBe(1);
    expect(slices).toHaveLength(1);
    expect(slices[0].weightPercent).toBe(100);
  });

  it("ignores closed positions before counting unavailable market values", () => {
    const rows = [
      mvRow(inst(1, "AAA"), { status: "available", value: "100.00" }),
      mvRow(
        inst(2, "BBB"),
        { status: "unavailable", reasons: ["missing_price"] },
        "closed",
      ),
    ];
    const { slices, excludedCount } = allocationBreakdown(rows, "instrument");
    expect(excludedCount).toBe(0);
    expect(slices).toHaveLength(1);
    expect(slices[0].label).toBe("AAA");
  });

  it("returns empty allocation for no available rows", () => {
    expect(allocationBreakdown([], "instrument")).toEqual({
      slices: [],
      excludedCount: 0,
    });
  });
});
