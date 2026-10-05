// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { GainsRow } from "../api/types";
import { PortfolioSummary } from "./PortfolioSummary";
import type { PriceRefreshStatus } from "./priceRefreshViewModel";

const warning: PriceRefreshStatus["warning"] = {
  kind: "request_failed",
  label: "Refresh failed",
  detail: "Server unavailable",
};
const freshRows = [{ latest_price: { freshness: "fresh" } }] as GainsRow[];

afterEach(cleanup);

describe("PortfolioSummary price feedback", () => {
  it("shows warning and freshness together", () => {
    render(
      <PortfolioSummary
        summary={undefined}
        rows={freshRows}
        isCheckingPrices={false}
        refreshStatus={{ running: false, warning }}
      />,
    );
    expect(screen.getByText("Refresh failed")).toHaveAttribute(
      "title",
      "Server unavailable",
    );
    expect(screen.getByText("Fresh")).toBeTruthy();
    expect(screen.queryByText("Refreshing")).toBeNull();
  });

  it("shows only Refreshing while running, then restores warning and freshness", () => {
    const { rerender } = render(
      <PortfolioSummary
        summary={undefined}
        rows={freshRows}
        isCheckingPrices={true}
        refreshStatus={{ running: true, warning }}
      />,
    );
    expect(screen.getByText("Refreshing")).toBeTruthy();
    expect(screen.queryByText("Refresh failed")).toBeNull();
    expect(screen.queryByText("Fresh")).toBeNull();
    expect(screen.queryByText("Checking")).toBeNull();
    rerender(
      <PortfolioSummary
        summary={undefined}
        rows={freshRows}
        isCheckingPrices={false}
        refreshStatus={{ running: false, warning }}
      />,
    );
    expect(screen.getByText("Refresh failed")).toBeTruthy();
    expect(screen.getByText("Fresh")).toBeTruthy();
  });

  it.each([
    [true, "Checking"],
    [false, "No data"],
  ] as const)(
    "keeps warning alongside %s/%s when no freshness exists",
    (isCheckingPrices, label) => {
      render(
        <PortfolioSummary
          summary={undefined}
          rows={undefined}
          isCheckingPrices={isCheckingPrices}
          refreshStatus={{ running: false, warning }}
        />,
      );
      expect(screen.getByText("Refresh failed")).toBeTruthy();
      expect(screen.getByText(label)).toBeTruthy();
    },
  );
});
