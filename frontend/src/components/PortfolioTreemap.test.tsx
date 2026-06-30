// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GainsRow, Instrument } from "../api/types";
import { PortfolioTreemap } from "./PortfolioTreemap";

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

function inst(symbol: string, exchange = "NYSE"): Instrument {
  return {
    id: 1,
    symbol,
    exchange,
    name: symbol,
    type: "Stock",
    currency: "USD",
  };
}

function openRow(symbol: string, value: string): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: inst(symbol),
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
    total_return_percent: { status: "available", value: "5.00" },
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: { status: "available", value },
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
    position_status: "open",
  };
}

describe("PortfolioTreemap", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("shows empty state when rows is empty", () => {
    render(<PortfolioTreemap rows={[]} />);
    expect(
      screen.getByText("No valued open holdings to display."),
    ).toBeTruthy();
  });

  it("shows empty state when all holdings have unavailable market value", () => {
    const money = { status: "available", value: "0.00" } as const;
    const row: GainsRow = {
      instrument: inst("AAPL"),
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
      total_return_percent: money,
      latest_price: null,
      previous_price: null,
      latest_fx: null,
      previous_fx: null,
      market_value_native: money,
      market_value_base: { status: "unavailable", reasons: ["missing_price"] },
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
      position_status: "open",
    };
    render(<PortfolioTreemap rows={[row]} />);
    expect(
      screen.getByText("No valued open holdings to display."),
    ).toBeTruthy();
  });

  it("renders tiles with aria-label when rows have valued holdings", () => {
    // jsdom reports zero clientWidth/clientHeight by default, which would make
    // computeLeaves() return no leaves even with non-empty tiles. Stub the
    // container's layout box (scoped to this test only) so the squarified
    // treemap layout actually runs and produces real, positioned tiles.
    const widthDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "clientWidth",
    );
    const heightDescriptor = Object.getOwnPropertyDescriptor(
      HTMLElement.prototype,
      "clientHeight",
    );
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      value: 400,
    });
    Object.defineProperty(HTMLElement.prototype, "clientHeight", {
      configurable: true,
      value: 280,
    });

    try {
      const { container } = render(
        <PortfolioTreemap
          rows={[openRow("MSFT", "5000.00"), openRow("GOOG", "3000.00")]}
        />,
      );

      expect(
        screen.queryByText("No valued open holdings to display."),
      ).toBeNull();

      const msftTile = container.querySelector('[aria-label*="MSFT"]');
      const googTile = container.querySelector('[aria-label*="GOOG"]');
      expect(msftTile).not.toBeNull();
      expect(googTile).not.toBeNull();
      expect(msftTile?.getAttribute("aria-label")).toBe("MSFT.NYSE: +5.00%");
      expect(googTile?.getAttribute("aria-label")).toBe("GOOG.NYSE: +5.00%");
      // Tiles are native <li> elements (implicit role="listitem"), not
      // role="img", since a nested role="img" would prune these aria-labels
      // from the accessibility tree under the container's own list role.
      const listItems = screen.getAllByRole("listitem");
      expect(listItems).toContain(msftTile);
      expect(listItems).toContain(googTile);
      expect(msftTile?.classList.contains("treemap-tile")).toBe(true);
      expect(msftTile?.classList.contains("treemap-tile--up")).toBe(true);
      expect(msftTile?.textContent).toContain("MSFT.NYSE");
      expect(msftTile?.textContent).toContain("+5.00%");
    } finally {
      if (widthDescriptor) {
        Object.defineProperty(
          HTMLElement.prototype,
          "clientWidth",
          widthDescriptor,
        );
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, "clientWidth");
      }
      if (heightDescriptor) {
        Object.defineProperty(
          HTMLElement.prototype,
          "clientHeight",
          heightDescriptor,
        );
      } else {
        Reflect.deleteProperty(HTMLElement.prototype, "clientHeight");
      }
    }
  });
});
