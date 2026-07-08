// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HoldingsPage } from "./HoldingsPage";

const useHoldings = vi.fn();
const useUpdateInstrumentConvictions = vi.fn();
const useUpsertInstrument = vi.fn();
const useAppMode = vi.fn();

vi.mock("../api/queries", () => ({
  useHoldings: (...args: unknown[]) => useHoldings(...args),
  useUpdateInstrumentConvictions: (...args: unknown[]) =>
    useUpdateInstrumentConvictions(...args),
  useUpsertInstrument: (...args: unknown[]) => useUpsertInstrument(...args),
}));

vi.mock("./useAppMode", () => ({
  useAppMode: (...args: unknown[]) => useAppMode(...args),
}));

function openHolding() {
  return {
    instrument: {
      id: 1,
      symbol: "OPEN",
      exchange: "STO",
      name: "Open Corp",
      type: "Stock",
      currency: "SEK",
      conviction: "High",
    },
    quantity: 100,
    cost_basis_native: "1000.00",
    average_cost_native: "10.00",
    base: {
      status: "available",
      cost_basis_base: "1000.00",
      average_cost_base: "10.00",
      fee_component_base: "0.00",
    },
    valuation: {
      market_value_base: { status: "available", value: "1200.00" },
      unrealized_gain_base: { status: "available", value: "200.00" },
      unrealized_gain_percent: { status: "available", value: "20.00" },
      day_change_base: { status: "available", value: "5.00" },
    },
    conviction_target: {
      conviction: "High",
      status: "above",
      target_value_base: { status: "available", value: "900.00" },
      target_gap_base: { status: "available", value: "300.00" },
      target_gap_percent: { status: "available", value: "33.33" },
    },
    row_kind: "open",
  } as const;
}

function watchlistHolding() {
  return {
    instrument: {
      id: 2,
      symbol: "WATCH",
      exchange: "STO",
      name: "Watch Co",
      type: "Stock",
      currency: "SEK",
      conviction: "Low",
    },
    quantity: 0,
    cost_basis_native: null,
    average_cost_native: null,
    base: null,
    valuation: null,
    conviction_target: {
      conviction: "Low",
      status: "on_target",
      target_value_base: { status: "available", value: "300.00" },
      target_gap_base: { status: "available", value: "0.00" },
      target_gap_percent: { status: "available", value: "0.00" },
    },
    row_kind: "watchlist",
  } as const;
}

function setupQueryMock() {
  useHoldings.mockImplementation((includeWatchlist: boolean) => ({
    data: {
      holdings: includeWatchlist
        ? [openHolding(), watchlistHolding()]
        : [openHolding()],
      hidden_watchlist_pool_count: includeWatchlist ? 0 : 1,
    },
    isPending: false,
    isError: false,
    error: null,
    refetch: vi.fn(),
  }));

  useUpdateInstrumentConvictions.mockReturnValue({
    mutateAsync: vi.fn(),
    isPending: false,
    isError: false,
    error: null,
  });
  useUpsertInstrument.mockReturnValue({
    mutateAsync: vi.fn(),
    isPending: false,
    isError: false,
    error: null,
  });

  useAppMode.mockReturnValue({ canMutate: false });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("HoldingsPage", () => {
  it("shows and hides watchlist rows from the include-watchlist toggle", () => {
    setupQueryMock();

    render(
      <MemoryRouter>
        <HoldingsPage />
      </MemoryRouter>,
    );

    expect(
      screen.getByRole("checkbox", { name: /include watchlist/i }),
    ).not.toBeChecked();
    expect(screen.getByRole("link", { name: /open corp/i })).toBeTruthy();
    expect(screen.queryByRole("link", { name: /watch co/i })).toBeNull();
    expect(
      screen.getByText("Targets include 1 watchlist instruments"),
    ).toBeTruthy();

    fireEvent.click(
      screen.getByRole("checkbox", { name: /include watchlist/i }),
    );

    expect(
      screen.getByRole("checkbox", { name: /include watchlist/i }),
    ).toBeChecked();
    expect(screen.getByRole("link", { name: /watch co/i })).toBeTruthy();
    expect(
      screen.queryByText("Targets include 1 watchlist instruments"),
    ).toBeNull();
  });

  it("opens the add instrument dialog from the page button", () => {
    setupQueryMock();
    useAppMode.mockReturnValue({ canMutate: true });

    render(
      <MemoryRouter>
        <HoldingsPage />
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: /add instrument/i }));

    expect(
      screen.getByRole("dialog", { name: /add instrument/i }),
    ).toBeTruthy();
    expect(screen.getByLabelText(/^symbol$/i)).toBeTruthy();
  });
});
