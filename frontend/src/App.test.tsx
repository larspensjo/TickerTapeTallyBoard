// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { appModeViewModel } from "./components/appModeViewModel";
import type { PriceRefreshStatus } from "./components/priceRefreshViewModel";

const useAppMode = vi.fn();
const usePriceRefresh = vi.fn();
const refresh = vi.fn();

vi.mock("./api/queries", () => ({
  useDataVersion: () => ({ data: { valuation_date: "2026-10-05" } }),
  useGains: () => ({ data: { rows: [] }, isFetching: false }),
  useInstruments: () => ({ data: [] }),
  useTransactions: () => ({ data: [], isPending: false, isError: false }),
  useDeleteTransaction: () => ({ isPending: false }),
  useCreateTransaction: () => ({ isPending: false }),
  useUpsertInstrument: () => ({ isPending: false }),
}));
vi.mock("./components/useAppMode", () => ({ useAppMode: () => useAppMode() }));
vi.mock("./components/usePriceRefresh", () => ({
  usePriceRefresh: () => usePriceRefresh(),
}));
vi.mock("./components/Dashboard", () => ({
  Dashboard: () => <h1>Dashboard content</h1>,
}));
vi.mock("./components/HoldingsPage", () => ({
  HoldingsPage: () => <h1>Holdings content</h1>,
}));
vi.mock("./components/RebalancePage", () => ({
  RebalancePage: () => <h1>Rebalance content</h1>,
}));
vi.mock("./components/GainsPage", () => ({
  GainsPage: () => <h1>Gains content</h1>,
}));
vi.mock("./components/ImportView", () => ({
  ImportView: ({ priceRefreshRunning }: { priceRefreshRunning: boolean }) => (
    <h1>
      {priceRefreshRunning ? "Import waiting for refresh" : "Import content"}
    </h1>
  ),
}));
vi.mock("./components/AssetView", () => ({
  AssetView: () => <h1>Asset content</h1>,
}));
vi.mock("./components/AppFooter", () => ({ AppFooter: () => null }));

beforeEach(() => {
  useAppMode.mockReturnValue(appModeViewModel("development"));
  usePriceRefresh.mockReturnValue({
    status: { running: false, warning: null },
    refresh,
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  localStorage.clear();
});

async function openRoute(route: string, heading: string) {
  render(
    <MemoryRouter initialEntries={[route]}>
      <App />
    </MemoryRouter>,
  );
  await screen.findByRole("heading", { name: heading }, { timeout: 5000 });
}

const routes = [
  ["/", "Dashboard content"],
  ["/holdings", "Holdings content"],
  ["/rebalance", "Rebalance content"],
  ["/gains", "Gains content"],
  ["/transactions", "Transactions"],
  ["/import", "Import content"],
  ["/asset/7", "Asset content"],
];

describe("App refresh actions", () => {
  it.each(routes)(
    "offers Refresh on %s and manual entry only on Transactions",
    async (route, heading) => {
      await openRoute(route, heading);
      expect(screen.getByRole("group", { name: "App actions" })).toBeTruthy();
      expect(
        screen.getByRole("button", { name: "Refresh prices" }),
      ).toHaveTextContent("Refresh");
      const addTransaction = screen.queryByRole("button", {
        name: "Add transaction",
      });
      if (route === "/transactions") expect(addTransaction).toBeTruthy();
      else expect(addTransaction).toBeNull();
    },
  );

  it.each(routes)("hides Refresh in demo on %s", async (route, heading) => {
    useAppMode.mockReturnValue(appModeViewModel("demo"));
    await openRoute(route, route === "/import" ? "Dashboard content" : heading);
    expect(screen.queryByRole("button", { name: "Refresh prices" })).toBeNull();
    expect(screen.queryByRole("group", { name: "App actions" })).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Add transaction" }),
    ).toBeNull();
    expect(screen.getByText("DEMO")).toBeTruthy();
  });

  it("calls the controller on Refresh", async () => {
    await openRoute("/", "Dashboard content");
    fireEvent.click(screen.getByRole("button", { name: "Refresh prices" }));
    expect(refresh).toHaveBeenCalledOnce();
  });

  it.each([
    ["/import", "Import content"],
    ["/asset/7", "Asset content"],
  ])("shows request failure feedback on %s", async (route, heading) => {
    const status: PriceRefreshStatus = {
      running: false,
      warning: {
        kind: "request_failed",
        label: "Refresh failed",
        detail: "Server unavailable",
      },
    };
    usePriceRefresh.mockReturnValue({ status, refresh });
    await openRoute(route, heading);
    expect(
      screen.getByRole("img", { name: "Refresh failed: Server unavailable" }),
    ).toHaveAttribute("title", "Refresh failed: Server unavailable");
  });

  it("disables Refresh while running and passes the status to the summary", async () => {
    usePriceRefresh.mockReturnValue({
      status: { running: true, warning: null },
      refresh,
    });
    await openRoute("/", "Dashboard content");
    const button = screen.getByRole("button", { name: "Refresh prices" });
    expect(button).toBeDisabled();
    expect(screen.getByText("Refreshing")).toBeTruthy();
    fireEvent.click(button);
    expect(refresh).not.toHaveBeenCalled();
  });

  it("passes the shared running signal to Import", async () => {
    usePriceRefresh.mockReturnValue({
      status: { running: true, warning: null },
      refresh,
    });
    await openRoute("/import", "Import waiting for refresh");
    expect(
      screen.getByRole("button", { name: "Refresh prices" }),
    ).toBeDisabled();
  });

  it("hides Refresh while app mode is loading", async () => {
    useAppMode.mockReturnValue(appModeViewModel(undefined));
    await openRoute("/", "Dashboard content");
    expect(screen.queryByRole("button", { name: "Refresh prices" })).toBeNull();
    expect(screen.queryByRole("group", { name: "App actions" })).toBeNull();
  });
});
