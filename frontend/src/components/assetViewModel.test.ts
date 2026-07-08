import { describe, expect, it } from "vitest";
import type {
  GainsRow,
  Holding,
  Instrument,
  MoneyValue,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import {
  convictionPanelView,
  convictionResetVisible,
  findGainsRow,
  findHolding,
  findInstrument,
  findPriceStatus,
  instrumentTransactions,
  parseInstrumentId,
  sharesSold,
  splitEvents,
} from "./assetViewModel";

function makeInstrument(id: number): Instrument {
  return {
    id,
    symbol: `SYM${id}`,
    exchange: "NYSE",
    name: `Name ${id}`,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
  };
}

function makeTransaction(
  id: number,
  instrumentId: number,
  type: "Buy" | "Sell" | "Split" | "Dividend",
  quantity: number,
): Transaction {
  return {
    id,
    instrument_id: instrumentId,
    type,
    trade_date: "2024-01-01",
    quantity,
    price: null,
    dividend_per_share: null,
    currency: null,
    fx_rate_to_base: null,
    brokerage: null,
    brokerage_currency: null,
    source_value: null,
    source_currency: null,
    note: null,
    import_batch_id: null,
  };
}

const unavailableMoney: MoneyValue = {
  status: "unavailable",
  reasons: ["missing"],
};

function makeGainsRow(
  instrumentId: number,
  positionStatus: "open" | "closed",
): GainsRow {
  const instrument = makeInstrument(instrumentId);
  return {
    instrument,
    quantity: 10,
    cost_basis_native: "1000",
    cost_basis_base: unavailableMoney,
    performance_start_date: null,
    performance_denominator_base: unavailableMoney,
    capital_gain_base: unavailableMoney,
    capital_gain_percent: unavailableMoney,
    income_base: unavailableMoney,
    currency_gain_base: unavailableMoney,
    currency_gain_percent: unavailableMoney,
    total_return_base: unavailableMoney,
    total_return_percent: unavailableMoney,
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: unavailableMoney,
    market_value_base: unavailableMoney,
    proceeds_native: unavailableMoney,
    proceeds_base: unavailableMoney,
    unrealized_gain_base: unavailableMoney,
    unrealized_gain_percent: unavailableMoney,
    realized_gain_base: unavailableMoney,
    realized_cost_basis_base: unavailableMoney,
    price_effect_base: unavailableMoney,
    fx_effect_base: unavailableMoney,
    day_change_base: unavailableMoney,
    day_change_percent: unavailableMoney,
    reasons: [],
    position_status: positionStatus,
  };
}

function makeHolding(instrumentId: number): Holding {
  return {
    instrument: makeInstrument(instrumentId),
    quantity: 10,
    cost_basis_native: "1000",
    average_cost_native: "100",
    base: { status: "unavailable", reasons: [] },
    valuation: null,
    conviction_target: {
      conviction: "Other",
      status: "no_target",
      target_value_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_percent: { status: "unavailable", reasons: ["no_target"] },
    },
    row_kind: "open",
  };
}

function makePriceStatus(instrumentId: number): PriceStatusInstrument {
  return {
    instrument_id: instrumentId,
    exchange: "NYSE",
    symbol: `SYM${instrumentId}`,
    currency: "USD",
    mapping_enabled: true,
    provider_symbol: null,
    open_quantity: 10,
    latest_price: {
      status: "available",
      date: "2024-01-01",
      value: "100",
      provider: "alpha",
      provider_symbol: "SYM",
      reason: null,
    },
    latest_fx: {
      status: "available",
      date: "2024-01-01",
      value: "1",
      provider: "alpha",
      provider_symbol: "USD/SEK",
      reason: null,
    },
  };
}

describe("parseInstrumentId", () => {
  it("parses a valid positive integer string", () => {
    expect(parseInstrumentId("1")).toBe(1);
    expect(parseInstrumentId("42")).toBe(42);
  });

  it("returns null for undefined", () => {
    expect(parseInstrumentId(undefined)).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(parseInstrumentId("")).toBeNull();
  });

  it("returns null for non-numeric strings", () => {
    expect(parseInstrumentId("abc")).toBeNull();
  });

  it("returns null for zero and negative integers", () => {
    expect(parseInstrumentId("0")).toBeNull();
    expect(parseInstrumentId("-1")).toBeNull();
  });

  it("returns null for non-integer numbers", () => {
    expect(parseInstrumentId("1.5")).toBeNull();
  });
});

describe("findInstrument", () => {
  const instruments = [makeInstrument(1), makeInstrument(2)];

  it("returns matching instrument by id", () => {
    expect(findInstrument(instruments, 1)).toEqual(instruments[0]);
    expect(findInstrument(instruments, 2)).toEqual(instruments[1]);
  });

  it("returns null when not found", () => {
    expect(findInstrument(instruments, 99)).toBeNull();
  });
});

describe("findGainsRow", () => {
  it("returns null when no rows match", () => {
    expect(findGainsRow([makeGainsRow(1, "open")], 99)).toBeNull();
  });

  it("returns the matching open row", () => {
    const row = makeGainsRow(1, "open");
    expect(findGainsRow([row], 1)).toBe(row);
  });

  it("returns the matching closed row when there is no open row", () => {
    const row = makeGainsRow(1, "closed");
    expect(findGainsRow([row], 1)).toBe(row);
  });

  it("prefers the open row over the closed row for the same instrument", () => {
    const closed = makeGainsRow(1, "closed");
    const open = makeGainsRow(1, "open");
    expect(findGainsRow([closed, open], 1)).toBe(open);
  });
});

describe("findHolding", () => {
  const holdings = [makeHolding(1), makeHolding(2)];

  it("returns matching holding by instrument id", () => {
    expect(findHolding(holdings, 1)).toEqual(holdings[0]);
  });

  it("returns null when not found", () => {
    expect(findHolding(holdings, 99)).toBeNull();
  });
});

describe("findPriceStatus", () => {
  const entries = [makePriceStatus(1), makePriceStatus(2)];

  it("returns matching entry by instrument_id", () => {
    expect(findPriceStatus(entries, 1)).toEqual(entries[0]);
  });

  it("returns null when not found", () => {
    expect(findPriceStatus(entries, 99)).toBeNull();
  });
});

describe("instrumentTransactions", () => {
  const txns = [
    makeTransaction(1, 1, "Buy", 10),
    makeTransaction(2, 2, "Buy", 5),
    makeTransaction(3, 1, "Sell", 3),
  ];

  it("filters to the given instrument id", () => {
    const result = instrumentTransactions(txns, 1);
    expect(result).toHaveLength(2);
    expect(result[0].id).toBe(1);
    expect(result[1].id).toBe(3);
  });

  it("returns empty array when no transactions match", () => {
    expect(instrumentTransactions(txns, 99)).toHaveLength(0);
  });
});

describe("sharesSold", () => {
  it("sums absolute quantities of Sell transactions", () => {
    const txns = [
      makeTransaction(1, 1, "Buy", 10),
      makeTransaction(2, 1, "Sell", 3),
      makeTransaction(3, 1, "Sell", 2),
    ];
    expect(sharesSold(txns)).toBe(5);
  });

  it("returns zero when there are no Sell transactions", () => {
    const txns = [makeTransaction(1, 1, "Buy", 10)];
    expect(sharesSold(txns)).toBe(0);
  });

  it("uses absolute value of quantity", () => {
    const txns = [makeTransaction(1, 1, "Sell", -5)];
    expect(sharesSold(txns)).toBe(5);
  });
});

describe("splitEvents", () => {
  it("derives split ratios from running ledger quantity", () => {
    const events = splitEvents([
      makeTransaction(1, 1, "Buy", 10),
      makeTransaction(2, 1, "Split", 40),
      makeTransaction(3, 1, "Sell", -5),
      makeTransaction(4, 1, "Split", -36),
    ]);

    expect(events).toEqual([
      {
        id: 2,
        tradeDate: "2024-01-01",
        quantityDelta: 40,
        beforeQuantity: 10,
        afterQuantity: 50,
        ratioLabel: "5:1",
        factor: 5,
      },
      {
        id: 4,
        tradeDate: "2024-01-01",
        quantityDelta: -36,
        beforeQuantity: 45,
        afterQuantity: 9,
        ratioLabel: "1:5",
        factor: 0.2,
      },
    ]);
  });

  it("sorts transactions by date and id before deriving split state", () => {
    const events = splitEvents([
      { ...makeTransaction(4, 1, "Split", 10), trade_date: "2024-01-03" },
      { ...makeTransaction(2, 1, "Buy", 5), trade_date: "2024-01-01" },
      { ...makeTransaction(3, 1, "Buy", 5), trade_date: "2024-01-03" },
    ]);

    expect(events[0]).toMatchObject({
      beforeQuantity: 10,
      afterQuantity: 20,
      ratioLabel: "2:1",
    });
  });
});

describe("conviction panel view", () => {
  it("exposes the instrument conviction and the open holding's target", () => {
    const holding = makeHolding(1);
    holding.instrument.conviction = "High";
    holding.conviction_target = {
      conviction: "High",
      status: "below",
      target_value_base: { status: "available", value: "1000.00" },
      target_gap_base: { status: "available", value: "-200.00" },
      target_gap_percent: { status: "available", value: "-20.00" },
    };
    const instrument = { ...makeInstrument(1), conviction: "High" as const };

    const view = convictionPanelView(instrument, holding);
    expect(view.conviction).toBe("High");
    expect(view.target?.status).toBe("below");
  });

  it("has no target for a closed / no-position instrument", () => {
    const view = convictionPanelView(makeInstrument(2), null);
    expect(view.conviction).toBe("Other");
    expect(view.target).toBeNull();
  });
});

describe("conviction reset visibility", () => {
  it("offers reset only when the saved value differs from the baseline", () => {
    // Baseline captured at navigation time; a later save to "High" differs.
    expect(convictionResetVisible("Low", "High")).toBe(true);
    // After resetting (or saving back to the baseline) there is nothing to undo.
    expect(convictionResetVisible("Low", "Low")).toBe(false);
  });
});
