// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Holding, Instrument } from "../api/types";
import {
  HoldingsTable,
  loadHoldingsSorting,
  saveHoldingsSorting,
} from "./HoldingsTable";

const HOLDINGS_SORTING_KEY = "holdings.sorting";

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
  it("defaults to market value descending when nothing valid is stored", () => {
    expect(loadHoldingsSorting()).toEqual([{ id: "value", desc: true }]);

    localStorage.setItem(
      HOLDINGS_SORTING_KEY,
      JSON.stringify([{ id: "removed_column", desc: true }]),
    );

    expect(loadHoldingsSorting()).toEqual([{ id: "value", desc: true }]);
  });

  it("round-trips a valid sorting selection through localStorage", () => {
    const sorting = [{ id: "instrument", desc: false }];

    saveHoldingsSorting(sorting);

    expect(loadHoldingsSorting()).toEqual(sorting);
  });

  it("accepts the consolidated conviction and target sortable columns", () => {
    for (const id of ["conviction", "target"]) {
      const sorting = [{ id, desc: true }];
      saveHoldingsSorting(sorting);
      expect(loadHoldingsSorting()).toEqual(sorting);
    }
  });

  it("rejects a retired sortable column id", () => {
    localStorage.setItem(
      HOLDINGS_SORTING_KEY,
      JSON.stringify([{ id: "target_gap_base", desc: true }]),
    );
    expect(loadHoldingsSorting()).toEqual([{ id: "value", desc: true }]);
  });

  it("applies saved sorting when the table mounts", () => {
    saveHoldingsSorting([{ id: "instrument", desc: false }]);

    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
      holding(3, "Metro Ltd", "METRO", "200.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Alpha Corp", "Metro Ltd", "Zulu Inc"],
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
});

describe("holdings consolidated columns", () => {
  it("renders the seven consolidated column headers", () => {
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "100.00")]);

    for (const name of [
      "Instrument",
      "Qty",
      "Cost",
      "Value (SEK)",
      "P&L",
      "Conviction",
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

  it("still renders regrouped sub-content (portfolio % and target details)", () => {
    const held: Holding = {
      ...holding(1, "Alpha Corp", "ALPHA", "1000.00"),
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
    // The gap bar carries the full detail in its accessible label / tooltip.
    expect(
      screen.getByRole("img", {
        name: "Target SEK 2,000.00\nGap SEK -1,000.00 (-50.0%)\nBelow",
      }),
    ).toBeTruthy();
  });

  it("leaves the target gap cell empty when no target exists", () => {
    // The holding factory has a no_target conviction target.
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "1000.00")]);

    expect(screen.queryByRole("img")).toBeNull();
  });
});
