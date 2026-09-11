// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  completedRefreshRunKey,
  isNewlyCompletedRefreshRun,
} from "./priceRefreshCompletion";
import { useGains, usePriceStatus } from "./queries";
import type { PriceStatusResponse, RefreshRunSummary } from "./types";

function run(overrides: Partial<RefreshRunSummary> = {}): RefreshRunSummary {
  return {
    run_id: 360,
    trigger: "launch",
    mode: "latest",
    status: "succeeded",
    started_at: "2026-09-11T14:45:58Z",
    finished_at: "2026-09-11T14:46:08Z",
    message: null,
    prices_written: 220,
    fx_rates_written: 22,
    unmapped_instruments: 0,
    failed_items: 0,
    ...overrides,
  };
}

function status(latest_run: RefreshRunSummary | null): PriceStatusResponse {
  return {
    refreshing: latest_run?.finished_at === null,
    latest_run,
    instruments: [],
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("completedRefreshRunKey", () => {
  it("is null while the latest run is still running or none exists", () => {
    expect(completedRefreshRunKey(status(null))).toBeNull();
    expect(
      completedRefreshRunKey(
        status(run({ status: "running", finished_at: null })),
      ),
    ).toBeNull();
  });

  it("identifies a finished run", () => {
    expect(completedRefreshRunKey(status(run()))).toBe(
      "360:2026-09-11T14:46:08Z",
    );
  });
});

describe("isNewlyCompletedRefreshRun", () => {
  it("ignores the first observation", () => {
    expect(isNewlyCompletedRefreshRun(undefined, "360:x")).toBe(false);
  });

  it("fires when a running refresh finishes or a later run completes", () => {
    expect(isNewlyCompletedRefreshRun(null, "360:x")).toBe(true);
    expect(isNewlyCompletedRefreshRun("359:y", "360:x")).toBe(true);
  });

  it("does not fire for the same run or while a run is in progress", () => {
    expect(isNewlyCompletedRefreshRun("360:x", "360:x")).toBe(false);
    expect(isNewlyCompletedRefreshRun("360:x", null)).toBe(false);
  });
});

describe("usePriceStatus", () => {
  it("refetches gains when a refresh started elsewhere finishes", async () => {
    let currentStatus = status(run({ status: "running", finished_at: null }));
    const gainsRequests: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        const isStatus = url.startsWith("/api/prices/status");
        if (!isStatus) gainsRequests.push(url);
        const body = isStatus ? currentStatus : { rows: [] };
        return Promise.resolve({
          status: 200,
          ok: true,
          text: () => Promise.resolve(JSON.stringify(body)),
        } as unknown as Response);
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(
      () => ({ status: usePriceStatus(), gains: useGains() }),
      { wrapper },
    );
    await waitFor(() => expect(result.current.gains.isSuccess).toBe(true));
    await waitFor(() => expect(result.current.status.isSuccess).toBe(true));
    expect(gainsRequests).toHaveLength(1);

    currentStatus = status(run());
    await queryClient.refetchQueries({ queryKey: ["price-status"] });

    await waitFor(() => expect(gainsRequests).toHaveLength(2));
  });
});
