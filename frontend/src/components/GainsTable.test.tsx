// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { GainsRow } from "../api/types";
import { GainsTable, totalReturnLabel } from "./GainsTable";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

it("shows the shared ticker beneath the Gains instrument name without an exchange tag", () => {
  const money = { status: "available", value: "100.00" } as const;
  const row: GainsRow = {
    instrument: {
      id: 1,
      name: "Evolution",
      symbol: "SE0012673267",
      ticker: "EVO",
      exchange: "AVANZA",
      type: "Stock",
      currency: "SEK",
      conviction: "Other",
    },
    quantity: 1,
    cost_basis_native: "100.00",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    income_base: money,
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
    day_change_percent: money,
    reasons: [],
    position_status: "open",
  };
  render(
    <MemoryRouter>
      <GainsTable
        rows={[row]}
        filter=""
        onFilterChange={() => undefined}
        includeClosedPositions={false}
        onIncludeClosedPositionsChange={() => undefined}
        customRange={{ startDate: null, endDate: null }}
        selectedDatePreset="all"
        onDatePresetChange={() => undefined}
        onDateRangeChange={() => undefined}
        valuationDate="2026-11-08"
        returnMethod="xirr"
        onReturnMethodChange={() => undefined}
      />
    </MemoryRouter>,
  );
  expect(screen.getByRole("link", { name: "Evolution" })).toHaveAttribute(
    "href",
    "/asset/1",
  );
  expect(screen.getByText("EVO")).toBeTruthy();
  expect(screen.queryByText("AVANZA")).toBeNull();
  expect(screen.queryByText("SE0012673267")).toBeNull();
});

describe("totalReturnLabel", () => {
  it("keeps the visible label short and moves the method detail into the tooltip", () => {
    expect(totalReturnLabel("money_weighted", "absolute")).toEqual({
      label: "Total return",
      title: "Total return including closed positions (money-weighted)",
    });
    expect(totalReturnLabel("simple", "absolute")).toEqual({
      label: "Total return",
      title: "Total return (simple)",
    });
  });

  it("describes the modified Dietz legacy mode through the tooltip", () => {
    expect(totalReturnLabel("modified_dietz", "annualised")).toEqual({
      label: "Total return",
      title: "Annualised return (Modified Dietz legacy)",
      note: "legacy / comparison only",
    });
  });
});
