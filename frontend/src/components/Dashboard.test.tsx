// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GainsRow, Instrument } from "../api/types";

const useGains = vi.fn();
const usePortfolioValueHistory = vi.fn();

vi.mock("../api/queries", () => ({
  useGains: (...args: unknown[]) => useGains(...args),
  usePortfolioValueHistory: (...args: unknown[]) =>
    usePortfolioValueHistory(...args),
}));

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

// Import after vi.mock so the mocked queries module is wired up.
import { Dashboard } from "./Dashboard";

function inst(symbol: string): Instrument {
  return {
    id: 1,
    symbol,
    exchange: "NYSE",
    name: symbol,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
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

function renderDashboard() {
  return render(
    <MemoryRouter>
      <Dashboard
        dateRange={{ startDate: null, endDate: null }}
        selectedDatePreset="all"
        onDatePresetChange={vi.fn()}
        onDateRangeChange={vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe("Dashboard chart panel", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("keeps the Treemap view reachable when value-history fails", () => {
    // Value-history query is in error (owns the default "value" view), while
    // the gains query has usable holdings for the treemap.
    usePortfolioValueHistory.mockReturnValue({
      data: undefined,
      isPending: false,
      isError: true,
      refetch: vi.fn(),
    });
    useGains.mockReturnValue({
      data: { rows: [openRow("MSFT", "5000.00")] },
      isPending: false,
      isError: false,
    });

    renderDashboard();

    // The error state still renders the view controls, so Treemap is reachable.
    expect(screen.getByText("Could not load portfolio value.")).toBeTruthy();
    const treemapButton = screen.getByRole("button", { name: "Treemap" });

    fireEvent.click(treemapButton);

    // Switched to the treemap, driven by the successful gains query rather than
    // the failed value-history query.
    expect(screen.getByRole("heading", { name: "Portfolio map" })).toBeTruthy();
    expect(screen.queryByText("Could not load portfolio value.")).toBeNull();
    expect(
      screen.queryByText("No valued open holdings to display."),
    ).toBeNull();
  });
});
