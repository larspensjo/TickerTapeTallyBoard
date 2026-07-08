// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { apiGet } from "../api/client";
import type { RebalanceResponse } from "../api/types";
import { RebalancePage } from "./RebalancePage";

vi.mock("../api/client", async () => {
  const actual =
    await vi.importActual<typeof import("../api/client")>("../api/client");
  return {
    ...actual,
    apiGet: vi.fn(),
  };
});

function rebalanceResponse(): RebalanceResponse {
  return {
    amount_base: "1234.50",
    base_currency: "SEK",
    plan: {
      status: "available",
      pool_value_base: "700000.00",
      candidate_count: 1,
      rungs: [
        {
          selected_count: 1,
          effective_trade_count: 1,
          trades: [
            {
              instrument: {
                id: 1,
                symbol: "AAA",
                name: "Alpha",
                exchange: "STO",
                type: "Stock",
                currency: "SEK",
                conviction: "Low",
              },
              side: "buy",
              shares: 12,
              price_base: "512.30",
              amount_base: "6147.60",
              freshness: "warning_stale_4_days",
              is_new: true,
            },
          ],
          untraded: [],
          balance: [
            {
              instrument: {
                id: 1,
                symbol: "AAA",
                name: "Alpha",
                exchange: "STO",
                type: "Stock",
                currency: "SEK",
                conviction: "Low",
              },
              gap_before_base: "40.00",
              gap_after_base: "7.50",
              gap_before_percent: "40.00",
              gap_after_percent: "7.50",
              status_before: "above",
              status_after: "above",
              is_new: true,
            },
            {
              instrument: {
                id: 2,
                symbol: "BBB",
                name: "Beta",
                exchange: "STO",
                type: "Stock",
                currency: "SEK",
                conviction: "Low",
              },
              gap_before_base: "-25.00",
              gap_after_base: "7.50",
              gap_before_percent: "-25.00",
              gap_after_percent: "7.50",
              status_before: "below",
              status_after: "above",
              is_new: false,
            },
          ],
          achieved_net_base: "6147.60",
          residual_base: "-4913.10",
          coverage_percent: "88.00",
          total_gap_before_base: "65.00",
          total_gap_after_base: "15.00",
        },
      ],
    },
  };
}

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <RebalancePage />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  localStorage.clear();
  vi.mocked(apiGet).mockResolvedValue(rebalanceResponse());
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("RebalancePage", () => {
  it("shows a warning banner when the selected rung includes warning-stale trades", async () => {
    renderPage();

    fireEvent.change(screen.getByRole("textbox", { name: /amount/i }), {
      target: { value: "1234.50" },
    });
    fireEvent.blur(screen.getByRole("textbox", { name: /amount/i }));

    expect(await screen.findByRole("alert")).toBeDefined();
    expect(
      screen.getByText(/selected rung includes warning-stale trades/i),
    ).toBeDefined();
    expect(
      within(await screen.findByRole("alert")).getByText("Stale 4 days"),
    ).toBeDefined();
  });

  it("renders the balance table with a flip warning", async () => {
    renderPage();

    fireEvent.change(screen.getByRole("textbox", { name: /amount/i }), {
      target: { value: "1234.50" },
    });
    fireEvent.blur(screen.getByRole("textbox", { name: /amount/i }));

    const table = await screen.findByRole("table", {
      name: /post-trade balance/i,
    });
    expect(within(table).getByText(/buy SEK 6,147.60/i)).toBeDefined();
    expect(within(table).getAllByText(/new/i).length).toBeGreaterThan(0);
    expect(within(table).getByText(/flips target band/i)).toBeDefined();
  });

  it("switches the ranking mode to Relative and requests rank_by=percent", async () => {
    renderPage();

    await screen.findByRole("button", { name: "Relative" });

    fireEvent.click(screen.getByRole("button", { name: "Relative" }));

    await waitFor(() => {
      expect(apiGet).toHaveBeenCalledWith(
        "/api/rebalance?amount=0&rank_by=percent",
      );
    });

    expect(
      screen.getByRole("button", { name: "Relative", pressed: true }),
    ).toBeDefined();
  });
});
