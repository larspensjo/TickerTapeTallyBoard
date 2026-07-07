// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RebalancePage } from "./RebalancePage";

vi.mock("../api/queries", () => ({
  useRebalancePlan: () => ({
    data: {
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
              },
            ],
            untraded: [],
            achieved_net_base: "6147.60",
            residual_base: "-4913.10",
            coverage_percent: "88.00",
          },
        ],
      },
    },
    isFetching: false,
    isError: false,
    error: null,
    refetch: vi.fn(),
  }),
}));

afterEach(cleanup);

describe("RebalancePage", () => {
  it("shows a warning banner when the selected rung includes warning-stale trades", async () => {
    render(
      <MemoryRouter>
        <RebalancePage />
      </MemoryRouter>,
    );

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
});
