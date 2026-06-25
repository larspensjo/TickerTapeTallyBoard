import { describe, expect, it } from "vitest";
import type { GainsRow, MoneyValue } from "../api/types";
import { waterfallView } from "./waterfallViewModel";

const money = (value: string): MoneyValue => ({ status: "available", value });
const missing = (...reasons: string[]): MoneyValue => ({
  status: "unavailable",
  reasons,
});

function openGain(overrides: Partial<GainsRow> = {}): GainsRow {
  return {
    instrument: {
      id: 1,
      symbol: "ANET",
      exchange: "NYSE",
      name: "Arista",
      type: "Stock",
      currency: "USD",
    },
    quantity: 100,
    cost_basis_native: "0",
    cost_basis_base: money("265582.94"),
    performance_start_date: null,
    performance_denominator_base: money("265582.94"),
    capital_gain_base: money("53546.54"),
    capital_gain_percent: money("20.16"),
    currency_gain_base: money("9418.19"),
    currency_gain_percent: money("3.55"),
    total_return_base: money("62964.73"),
    total_return_percent: money("23.71"),
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money("0"),
    market_value_base: money("328547.67"),
    proceeds_native: missing(),
    proceeds_base: missing(),
    unrealized_price_effect_base: money("53546.54"),
    unrealized_fx_effect_base: money("9418.19"),
    unrealized_gain_base: money("62964.73"),
    unrealized_gain_percent: money("23.71"),
    realized_gain_base: money("0.00"),
    realized_cost_basis_base: money("0.00"),
    price_effect_base: money("53546.54"),
    fx_effect_base: money("9418.19"),
    day_change_base: money("0"),
    day_change_percent: money("0"),
    reasons: [],
    position_status: "open",
    income_base: missing("income_not_tracked"),
    ...overrides,
  };
}

describe("waterfallView (open)", () => {
  it("builds the open ladder ending at total return = unrealized + realized", () => {
    const view = waterfallView(openGain());
    expect(view.mode).toBe("open");
    expect(view.rows.map((r) => [r.key, r.kind, r.label])).toEqual([
      ["cost-basis", "base", "Cost basis (held)"],
      ["price", "effect", "Price effect"],
      ["fx", "effect", "FX effect"],
      ["market-value", "subtotal", "Market value"],
      ["realized", "effect", "Realized gain"],
      ["income", "placeholder", "Dividend income"],
      ["total-return", "total", "Total return"],
    ]);

    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "62964.73" });
    // delta bar floats from cost basis to cost basis + total return
    expect(total?.span).toEqual({ from: 265582.94, to: 328547.67 });
    expect(total?.direction).toBe("up");
  });

  it("colors steps by sign and computes percent against cost basis", () => {
    const view = waterfallView(
      openGain({ unrealized_price_effect_base: money("-1000.00") }),
    );
    const price = view.rows.find((r) => r.key === "price");
    expect(price?.direction).toBe("down");
    expect(price?.percent).toEqual({ status: "available", value: "-0.38" });
  });

  it("treats zero realized as flat with a 0.00 percent (de-emphasized, not unavailable)", () => {
    const realized = waterfallView(openGain()).rows.find(
      (r) => r.key === "realized",
    );
    expect(realized?.direction).toBe("flat");
    expect(realized?.value).toEqual({ status: "available", value: "0.00" });
    expect(realized?.percent).toEqual({ status: "available", value: "0.00" });
  });

  it("sums realized into total return when there were sells", () => {
    const view = waterfallView(
      openGain({ realized_gain_base: money("200.00") }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "63164.73" });
  });

  it("uses held price and FX effects so realized gain is added exactly once", () => {
    const view = waterfallView(
      openGain({
        cost_basis_base: money("203028.32"),
        market_value_base: money("221246.20"),
        price_effect_base: money("170372.62"),
        fx_effect_base: money("13645.25"),
        unrealized_price_effect_base: money("4572.63"),
        unrealized_fx_effect_base: money("13645.25"),
        unrealized_gain_base: money("18217.88"),
        realized_gain_base: money("165799.99"),
        realized_cost_basis_base: money("280491.70"),
        total_return_base: money("184017.87"),
      }),
    );

    const price = view.rows.find((r) => r.key === "price");
    const fx = view.rows.find((r) => r.key === "fx");
    const realized = view.rows.find((r) => r.key === "realized");
    const total = view.rows.find((r) => r.key === "total-return");

    expect(price?.value).toEqual({ status: "available", value: "4572.63" });
    expect(fx?.value).toEqual({ status: "available", value: "13645.25" });
    expect(realized?.value).toEqual({
      status: "available",
      value: "165799.99",
    });
    expect(total?.value).toEqual({ status: "available", value: "184017.87" });
    expect(total?.percent).toEqual({ status: "available", value: "38.06" });
  });

  it("uses population-matched denominators for a partial sell (review finding #3)", () => {
    // Sold 4 shares at cost 400; the realized row's % is gain / sold cost basis.
    // The total-return row's % is total return / (held cost basis + sold cost basis).
    const view = waterfallView(
      openGain({
        realized_gain_base: money("200.00"),
        realized_cost_basis_base: money("400.00"),
      }),
    );
    const realized = view.rows.find((r) => r.key === "realized");
    expect(realized?.percent).toEqual({ status: "available", value: "50.00" });

    const total = view.rows.find((r) => r.key === "total-return");
    // 63164.73 / (265582.94 + 400) * 100 = 23.75
    expect(total?.percent).toEqual({ status: "available", value: "23.75" });
    // Price/FX effect rows keep the held-cost denominator.
    const price = view.rows.find((r) => r.key === "price");
    expect(price?.percent).toEqual({ status: "available", value: "20.16" });
  });

  it("exposes a domain that drops below zero when total return wipes out cost basis", () => {
    // A partially-sold position with a realized loss larger than the held cost basis:
    // cost basis 1000, total return -1500 -> total span ends at -500.
    const view = waterfallView(
      openGain({
        cost_basis_base: money("1000.00"),
        unrealized_gain_base: money("-1500.00"),
        realized_gain_base: money("0.00"),
        realized_cost_basis_base: money("0.00"),
        market_value_base: money("-500.00"),
        price_effect_base: money("-1500.00"),
        fx_effect_base: money("0.00"),
      }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.span).toEqual({ from: 1000, to: -500 });
    expect(view.minValue).toBeLessThanOrEqual(-500);
  });

  it("renders the income placeholder as inert and not contributing", () => {
    const incomeRow = waterfallView(openGain()).rows.find(
      (r) => r.key === "income",
    );
    expect(incomeRow?.kind).toBe("placeholder");
    expect(incomeRow?.span).toBeNull();
    expect(incomeRow?.percent).toBeNull();
    expect(incomeRow?.value).toEqual({
      status: "unavailable",
      reasons: ["income_not_tracked"],
    });
  });

  it("renders income as an effect row when income_base is available", () => {
    const gain = openGain({ income_base: money("250.00") });
    const view = waterfallView(gain);
    const incomeRow = view.rows.find((r) => r.key === "income");
    expect(incomeRow?.kind).toBe("effect");
    expect(incomeRow?.value).toEqual({ status: "available", value: "250.00" });
    expect(incomeRow?.span).toEqual({ from: 328547.67, to: 328797.67 });
    // income contributes to total return
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "63214.73" });
  });

  it("income bar starts where realized bar ends (bars are sequential)", () => {
    const gain = openGain({
      realized_gain_base: money("50.00"),
      income_base: money("100.00"),
    });
    const view = waterfallView(gain);
    const realizedRow = view.rows.find((r) => r.key === "realized");
    const incomeRow = view.rows.find((r) => r.key === "income");
    expect(incomeRow?.span?.from).toBeCloseTo(realizedRow?.span?.to ?? 0, 5);
  });

  it("renders an unavailable effect with no bar and an unavailable percent", () => {
    const view = waterfallView(
      openGain({ unrealized_fx_effect_base: missing("missing_fx") }),
    );
    const fx = view.rows.find((r) => r.key === "fx");
    expect(fx?.span).toBeNull();
    expect(fx?.direction).toBeNull();
    expect(fx?.percent).toEqual({
      status: "unavailable",
      reasons: ["missing_fx"],
    });
  });

  it("merges reasons from both sides when total-return has two unavailable inputs", () => {
    const view = waterfallView(
      openGain({
        unrealized_gain_base: missing("missing_price"),
        realized_gain_base: missing("missing_fx"),
      }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({
      status: "unavailable",
      reasons: expect.arrayContaining(["missing_price", "missing_fx"]),
    });
  });

  it("stacked segments: profitable case anchors gray at cost basis then chains effect spans right", () => {
    // Default openGain: costBasis=265582.94, price=53546.54, fx=9418.19, realized=0, income=placeholder
    const view = waterfallView(openGain());
    const total = view.rows.find((r) => r.key === "total-return");
    const segs = total?.stackedSegments;
    expect(segs).toBeDefined();

    // Gray base = [0, costBasis]
    expect(segs?.[0]).toEqual({
      key: "stacked-base",
      direction: null,
      span: { from: 0, to: 265582.94 },
    });
    // Price and FX segments match their own row spans
    const priceRow = view.rows.find((r) => r.key === "price");
    const fxRow = view.rows.find((r) => r.key === "fx");
    expect(segs?.[1]).toEqual({
      key: "stacked-price",
      direction: priceRow?.direction,
      span: priceRow?.span,
    });
    expect(segs?.[2]).toEqual({
      key: "stacked-fx",
      direction: fxRow?.direction,
      span: fxRow?.span,
    });
    // Realized (zero) has a span so it is included; income placeholder has no span so it is skipped
    const realizedRow = view.rows.find((r) => r.key === "realized");
    expect(segs?.[3]).toEqual({
      key: "stacked-realized",
      direction: realizedRow?.direction,
      span: realizedRow?.span,
    });
    expect(segs).toHaveLength(4);
  });

  it("stacked segments: loss case anchors gray at surviving value, effect spans overlay loss zone", () => {
    const view = waterfallView(
      openGain({
        unrealized_price_effect_base: money("-50000.00"),
        unrealized_fx_effect_base: money("5000.00"),
        unrealized_gain_base: money("-45000.00"),
        market_value_base: money("220582.94"),
      }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    const segs = total?.stackedSegments;
    expect(segs).toBeDefined();

    // Gray base = [0, costBasis + totalReturn] = [0, 265582.94 - 45000] = [0, 220582.94]
    expect(segs?.[0]).toEqual({
      key: "stacked-base",
      direction: null,
      span: { from: 0, to: 220582.94 },
    });
    // Price is a loss — direction "down", span reaches below surviving value
    expect(segs?.[1]).toMatchObject({
      key: "stacked-price",
      direction: "down",
    });
    // FX is a gain — direction "up"
    expect(segs?.[2]).toMatchObject({ key: "stacked-fx", direction: "up" });
  });

  it("stacked segments: income effect row is included when income is tracked", () => {
    const view = waterfallView(openGain({ income_base: money("250.00") }));
    const total = view.rows.find((r) => r.key === "total-return");
    const segs = total?.stackedSegments;
    const incomeRow = view.rows.find((r) => r.key === "income");
    const incomeSeg = segs?.find((s) => s.key === "stacked-income");
    expect(incomeSeg).toEqual({
      key: "stacked-income",
      direction: incomeRow?.direction,
      span: incomeRow?.span,
    });
  });

  it("stacked segments: absent when total return value is unavailable", () => {
    const view = waterfallView(
      openGain({
        unrealized_gain_base: missing("missing_price"),
        realized_gain_base: missing("missing_fx"),
      }),
    );
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.stackedSegments).toBeUndefined();
  });
});

describe("waterfallView (closed)", () => {
  it("pivots to proceeds and terminates at realized total return", () => {
    const view = waterfallView(
      openGain({
        position_status: "closed",
        market_value_base: money("0.00"),
        proceeds_base: money("13195.00"),
        cost_basis_base: money("10020.00"),
        price_effect_base: money("2175.00"),
        fx_effect_base: money("1000.00"),
        unrealized_gain_base: money("3175.00"),
        realized_gain_base: money("3175.00"),
        realized_cost_basis_base: money("10020.00"),
        total_return_base: money("3175.00"),
      }),
    );
    expect(view.mode).toBe("closed");
    expect(view.rows.map((r) => [r.key, r.kind, r.label])).toEqual([
      ["cost-basis", "base", "Cost basis (sold)"],
      ["price", "effect", "Price effect"],
      ["fx", "effect", "FX effect"],
      ["proceeds", "subtotal", "Proceeds"],
      ["income", "placeholder", "Dividend income"],
      ["total-return", "total", "Total return"],
    ]);
    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "3175.00" });
    expect(total?.span).toEqual({ from: 10020, to: 13195 });
  });

  it("adds available dividend income to closed total return", () => {
    const view = waterfallView(
      openGain({
        position_status: "closed",
        market_value_base: money("0.00"),
        proceeds_base: money("13195.00"),
        cost_basis_base: money("10020.00"),
        price_effect_base: money("2175.00"),
        fx_effect_base: money("1000.00"),
        unrealized_gain_base: money("3175.00"),
        realized_gain_base: money("3175.00"),
        realized_cost_basis_base: money("10020.00"),
        income_base: money("25.00"),
        total_return_base: money("3175.00"),
      }),
    );

    const income = view.rows.find((r) => r.key === "income");
    expect(income?.kind).toBe("effect");
    expect(income?.value).toEqual({ status: "available", value: "25.00" });

    const total = view.rows.find((r) => r.key === "total-return");
    expect(total?.value).toEqual({ status: "available", value: "3200.00" });
    expect(total?.span).toEqual({ from: 10020, to: 13220 });
  });
});
