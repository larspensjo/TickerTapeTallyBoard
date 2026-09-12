// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Instrument, Transaction } from "../api/types";
import { TransactionsTable } from "./TransactionsTable";

function instrument(id: number, name: string): Instrument {
  return {
    id,
    symbol: `TEST${id}`,
    exchange: "NYSE",
    name,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
  };
}

function transaction(id: number, price: string): Transaction {
  return {
    id,
    instrument_id: id,
    type: "Buy",
    trade_date: "2026-09-12",
    quantity: 1,
    price,
    dividend_per_share: null,
    currency: "USD",
    fx_rate_to_base: "10.00",
    brokerage: null,
    brokerage_currency: null,
    source_value: null,
    source_currency: null,
    note: null,
    import_batch_id: null,
  };
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("transactions sorting", () => {
  it("sorts exact price strings by numeric value", () => {
    const prices = ["1000.00", "123.45", "77.00", "9.50"];
    const instruments = prices.map((price, index) =>
      instrument(index + 1, `Price ${price}`),
    );
    localStorage.setItem(
      "transactions.sorting",
      JSON.stringify([{ id: "price", desc: false }]),
    );

    render(
      <MemoryRouter>
        <TransactionsTable
          transactions={prices.map((price, index) =>
            transaction(index + 1, price),
          )}
          instruments={instruments}
        />
      </MemoryRouter>,
    );

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Price 9.50", "Price 77.00", "Price 123.45", "Price 1000.00"],
    );
  });
});
