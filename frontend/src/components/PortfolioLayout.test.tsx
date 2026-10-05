// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PortfolioLayout } from "./PortfolioLayout";

const useGains = vi.fn();

vi.mock("../api/queries", () => ({
  useGains: (...args: unknown[]) => useGains(...args),
}));

function renderPortfolioLayout(isFetching = false) {
  useGains.mockReturnValue({ data: undefined, isFetching });

  render(
    <MemoryRouter initialEntries={["/"]}>
      <Routes>
        <Route
          element={
            <PortfolioLayout
              refreshStatus={{ running: false, warning: null }}
            />
          }
          path="/"
        >
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
  it.each([
    [true, "Checking", "No data"],
    [false, "No data", "Checking"],
  ] as const)(
    "shows %s/%s based only on gains fetching",
    (isFetching, label, absentLabel) => {
      renderPortfolioLayout(isFetching);

      expect(screen.getByText(label)).toBeTruthy();
      expect(screen.queryByText(absentLabel)).toBeNull();
    },
  );

  it("does not render the add transaction action", () => {
    renderPortfolioLayout();

    expect(
      screen.queryByRole("button", { name: "Add transaction" }),
    ).toBeNull();
  });
});
