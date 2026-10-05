// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GainsPage } from "./GainsPage";

const useGains = vi.fn();
const useAppMode = vi.fn();

vi.mock("../api/queries", () => ({
  useGains: (...args: unknown[]) => useGains(...args),
}));

vi.mock("./useAppMode", () => ({
  useAppMode: (...args: unknown[]) => useAppMode(...args),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("GainsPage empty state", () => {
  it.each([true, false])(
    "offers ledger entry and price refresh only when mutable (%s)",
    (canMutate) => {
      useAppMode.mockReturnValue({ canMutate });
      useGains.mockReturnValue({
        data: { rows: [] },
        isPending: false,
        isError: false,
        refetch: vi.fn(),
      });

      render(
        <MemoryRouter>
          <GainsPage
            selectedDatePreset="all"
            customRange={{ startDate: null, endDate: null }}
            valuationDate="2026-09-12"
            onDatePresetChange={vi.fn()}
            onDateRangeChange={vi.fn()}
          />
        </MemoryRouter>,
      );

      expect(screen.getByText(/No valued holdings yet\./)).toBeTruthy();
      if (canMutate) {
        expect(
          screen.getByRole("link", { name: "Transactions page" }),
        ).toHaveAttribute("href", "/transactions");
        expect(
          screen.getByRole("link", {
            name: "import from Avanza or Sharesight",
          }),
        ).toHaveAttribute("href", "/import");
        expect(screen.getByText(/Then refresh prices\./)).toBeTruthy();
      } else {
        expect(screen.getByText("No valued holdings yet.")).toBeTruthy();
        expect(screen.queryByRole("link")).toBeNull();
        expect(screen.queryByText(/Then refresh prices/)).toBeNull();
      }
    },
  );
});
