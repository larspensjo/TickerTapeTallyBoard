// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "../api/client";
import { refreshResult } from "../test/priceRefresh";
import { ImportView } from "./ImportView";

const useRefreshPrices = vi.fn();
const mutate = vi.fn();

vi.mock("../api/queries", () => ({
  usePreviewImport: () => ({ isPending: false }),
  useCommitImport: () => ({ isPending: false }),
  useRollbackImport: () => ({ isPending: false }),
  useRefreshPrices: (...args: unknown[]) => useRefreshPrices(...args),
}));

beforeEach(() => {
  useRefreshPrices.mockReturnValue({
    mutate,
    isPending: false,
    isError: false,
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

function renderImport(priceRefreshRunning = false) {
  return render(
    <MemoryRouter>
      <ImportView priceRefreshRunning={priceRefreshRunning} />
    </MemoryRouter>,
  );
}

describe("ImportView Backfill", () => {
  it("disables Backfill and explains when another refresh is running", () => {
    renderImport(true);
    const button = screen.getByRole("button", {
      name: "Backfill full price history",
    });
    expect(button).toBeDisabled();
    expect(
      screen.getByText(
        "A price refresh is running. Backfill is available when it finishes.",
      ),
    ).toBeTruthy();
    fireEvent.click(button);
    expect(mutate).not.toHaveBeenCalled();
  });

  it("starts its own backfill request when available", () => {
    renderImport();
    fireEvent.click(
      screen.getByRole("button", { name: "Backfill full price history" }),
    );
    expect(mutate).toHaveBeenCalledWith({ mode: "backfill" });
    expect(screen.queryByText(/A price refresh is running/)).toBeNull();
  });

  it("keeps its own pending request disabled without the other-run note", () => {
    useRefreshPrices.mockReturnValue({
      mutate,
      isPending: true,
      isError: false,
    });
    renderImport(true);
    expect(
      screen.getByRole("button", { name: "Backfill full price history" }),
    ).toBeDisabled();
    expect(screen.queryByText(/A price refresh is running/)).toBeNull();
  });

  it("explains a merged request instead of claiming to have written zero rows", () => {
    useRefreshPrices.mockReturnValue({
      mutate,
      isPending: false,
      data: refreshResult("running"),
    });
    renderImport();
    expect(
      screen.getByText(
        "Another price refresh was already running, so the backfill did not start. Try again when it finishes.",
      ),
    ).toBeTruthy();
    expect(screen.queryByText(/wrote 0/)).toBeNull();
  });

  it.each(["succeeded", "partial", "failed"] as const)(
    "reports finished %s results",
    (status) => {
      useRefreshPrices.mockReturnValue({
        mutate,
        isPending: false,
        data: refreshResult(status),
      });
      renderImport();
      expect(
        screen.getByText(
          `Backfill ${status}: wrote 12 price rows and 2 FX rates.`,
        ),
      ).toBeTruthy();
    },
  );

  it("keeps its own request error visible", () => {
    useRefreshPrices.mockReturnValue({
      mutate,
      isPending: false,
      isError: true,
      error: new ApiError("unavailable", "Backfill server unavailable"),
    });
    renderImport();
    expect(screen.getByText("Backfill server unavailable")).toBeTruthy();
  });
});
