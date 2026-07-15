// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Holding, Instrument } from "../api/types";
import { HoldingsTable } from "./HoldingsTable";

const HOLDINGS_SORTING_KEY = "holdings.sorting";

function seedSorting(sorting: unknown): void {
  localStorage.setItem(HOLDINGS_SORTING_KEY, JSON.stringify(sorting));
}

function instrument(id: number, name: string, symbol: string): Instrument {
  return {
    id,
    symbol,
    exchange: "NYSE",
    name,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
  };
}

function holding(
  id: number,
  name: string,
  symbol: string,
  marketValueBase: string,
): Holding {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: instrument(id, name, symbol),
    quantity: 1,
    cost_basis_native: "100.00",
    average_cost_native: "100.00",
    base: {
      status: "available",
      cost_basis_base: "100.00",
      average_cost_base: "100.00",
      fee_component_base: "0.00",
    },
    valuation: {
      market_value_base: { status: "available", value: marketValueBase },
      unrealized_gain_base: money,
      unrealized_gain_percent: money,
      day_change_base: money,
    },
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

function renderHoldingsTable(holdings: Holding[]) {
  return render(
    <MemoryRouter>
      <HoldingsTable
        holdings={holdings}
        filter=""
        onFilterChange={() => undefined}
      />
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("holdings sorting persistence", () => {
  it("applies saved sorting when the table mounts", () => {
    seedSorting([{ id: "instrument", desc: false }]);

    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
      holding(3, "Metro Ltd", "METRO", "200.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Alpha Corp", "Metro Ltd", "Zulu Inc"],
    );
  });

  it("ignores a stored retired column id and falls back to value descending", () => {
    seedSorting([{ id: "target_gap_base", desc: true }]);

    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
      holding(3, "Metro Ltd", "METRO", "200.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Zulu Inc", "Metro Ltd", "Alpha Corp"],
    );
  });

  it("saves sorting when the user changes the table sort", () => {
    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Instrument" }));

    expect(
      JSON.parse(localStorage.getItem(HOLDINGS_SORTING_KEY) ?? "null"),
    ).toEqual([{ id: "instrument", desc: false }]);
  });

  it("sorts Cost by total purchase cost instead of average unit cost", () => {
    const largeTotal: Holding = {
      ...holding(1, "Large Total", "LARGE", "1000.00"),
      average_cost_native: "10.00",
      cost_basis_native: "1000.00",
    };
    const smallTotal: Holding = {
      ...holding(2, "Small Total", "SMALL", "200.00"),
      average_cost_native: "200.00",
      cost_basis_native: "200.00",
    };

    renderHoldingsTable([largeTotal, smallTotal]);

    fireEvent.click(screen.getByRole("button", { name: "Cost" }));

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Large Total", "Small Total"],
    );
  });
});

describe("holdings consolidated columns", () => {
  it("renders the seven consolidated column headers", () => {
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "100.00")]);

    for (const name of [
      "Instrument",
      "Qty",
      "Cost",
      "Value (SEK)",
      "P&L (SEK)",
      "Conviction (SEK)",
      "Target gap",
    ]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }

    for (const gone of [
      "Avg cost/share",
      "Cost basis",
      "Portfolio %",
      "P&L hint",
      "Target (SEK)",
      "Target status",
    ]) {
      expect(screen.queryByRole("button", { name: gone })).toBeNull();
    }
  });

  it("explains every consolidated column with an accessible tooltip", () => {
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "100.00")]);

    const descriptions = new Map([
      ["Instrument", "The instrument name, identifier, and trading venue."],
      ["Qty", "The number of shares or units currently held."],
      [
        "Cost",
        "Average purchase price per unit (top) and total purchase cost (bottom), both in the instrument's trading currency.",
      ],
      [
        "Value (SEK)",
        "Current market value in SEK (top) and the holding's share of the portfolio (bottom).",
      ],
      [
        "P&L (SEK)",
        "Unrealized profit or loss in SEK: current market value minus cost basis, including brokerage fees and currency effects (top), with the return on cost basis below.",
      ],
      [
        "Conviction (SEK)",
        "Your relative target preference. Low, Medium, and High use 1x, 2x, and 4x target weights; Other has no target. The derived target value in SEK appears below.",
      ],
      [
        "Target gap",
        "Current value minus the conviction target, as a percentage of the target. Green/right is above target; red/left is below target.",
      ],
    ]);

    for (const [name, description] of descriptions) {
      const header = screen.getByRole("button", { name });
      expect(header).toHaveAttribute("title", description);
      expect(header).toHaveAccessibleDescription(description);
    }
  });

  it("still renders regrouped sub-content (portfolio % and target details)", () => {
    const baseHolding = holding(1, "Alpha Corp", "ALPHA", "1000.00");
    const held: Holding = {
      ...baseHolding,
      valuation: {
        market_value_base: { status: "available", value: "1000.00" },
        unrealized_gain_base: { status: "available", value: "123.45" },
        unrealized_gain_percent: { status: "available", value: "14.08" },
        day_change_base: { status: "available", value: "0.00" },
      },
      conviction_target: {
        conviction: "High",
        status: "below",
        target_value_base: { status: "available", value: "2000.00" },
        target_gap_base: { status: "available", value: "-1000.00" },
        target_gap_percent: { status: "available", value: "-50.0" },
      },
    };

    renderHoldingsTable([held]);

    // Portfolio-% sub-line: a single valued holding is 100% of the portfolio.
    expect(screen.getAllByText("100.0%").length).toBeGreaterThan(0);
    // Target value moved under the conviction selector.
    expect(screen.getByText("2,000.00")).toBeTruthy();
    const holdingRow = screen
      .getByRole("link", { name: "Alpha Corp" })
      .closest("tr");
    expect(holdingRow).toHaveTextContent("123.45");
    // SEK belongs to the two headers, not to each row's P&L/target values.
    expect(holdingRow).not.toHaveTextContent("SEK");
    // The gap bar carries the full detail in its accessible label / tooltip.
    expect(
      screen.getByRole("img", {
        name: "Target SEK 2,000.00\nGap SEK -1,000.00 (-50.0%)\nBelow",
      }),
    ).toBeTruthy();
  });

  it("keeps the average-cost currency and amount on one line", () => {
    const held: Holding = {
      ...holding(1, "Alpha Corp", "ALPHA", "1000.00"),
      average_cost_native: "12.34",
      cost_basis_native: "1234.00",
    };

    renderHoldingsTable([held]);

    const holdingRow = screen
      .getByRole("link", { name: "Alpha Corp" })
      .closest("tr");
    const averageCostCurrency = holdingRow?.querySelector(
      "td:nth-child(3) .metric-stack > .number .number-prefix",
    );
    expect(averageCostCurrency?.parentElement).toHaveTextContent("USD12.34");
  });

  it("leaves the target gap cell empty when no target exists", () => {
    // The holding factory has a no_target conviction target.
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "1000.00")]);

    expect(screen.queryByRole("img")).toBeNull();
  });
});
