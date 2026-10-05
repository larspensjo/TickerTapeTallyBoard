// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PortfolioLayout } from "./PortfolioLayout";

const useGains = vi.fn();
const usePriceStatus = vi.fn();
const useRefreshPrices = vi.fn();
const useAppMode = vi.fn();

vi.mock("../api/queries", () => ({
  useGains: (...args: unknown[]) => useGains(...args),
  usePriceStatus: (...args: unknown[]) => usePriceStatus(...args),
  useRefreshPrices: (...args: unknown[]) => useRefreshPrices(...args),
}));

vi.mock("./PortfolioSummary", () => ({
  PortfolioSummary: () => null,
}));

vi.mock("./useAppMode", () => ({
  useAppMode: (...args: unknown[]) => useAppMode(...args),
}));

function renderPortfolioLayout() {
  useGains.mockReturnValue({ data: undefined, isFetching: false });
  usePriceStatus.mockReturnValue({ data: undefined, isPending: false });
  useRefreshPrices.mockReturnValue({
    isPending: false,
    error: null,
  });
  useAppMode.mockReturnValue({ canMutate: true });

  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route element={<PortfolioLayout />} path="/">
          <Route element={<div>Dashboard content</div>} index />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("PortfolioLayout", () => {
  it("does not render the add transaction action", () => {
    renderPortfolioLayout();

    expect(
      screen.queryByRole("button", { name: "Add transaction" }),
    ).toBeNull();
  });
});
