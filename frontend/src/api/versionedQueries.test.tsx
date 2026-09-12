// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import * as queries from "./queries";

const VERSION = {
  data_revision: "r:1",
  valuation_date: "2026-09-12",
  prices_refreshing: false,
};

// Hooks that deliberately do not name a snapshot: the heartbeat itself, and
// health, which describes the process rather than the data.
const UNVERSIONED = new Set(["useDataVersion", "useHealth"]);

// Arguments for hooks that fetch nothing until they are given one.
const HOOK_ARGS: Record<string, unknown[]> = {
  useInstrumentPrices: [7],
  useRebalancePlan: ["1000", "sek"],
};

function isMutationResult(value: unknown): value is { mutate: unknown } {
  return typeof value === "object" && value !== null && "mutate" in value;
}

function stubFetch(
  requests: string[] = [],
  dataVersion: () => typeof VERSION = () => VERSION,
) {
  vi.stubGlobal(
    "fetch",
    vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (!url.startsWith("/api/data-version")) requests.push(url);
      const body = url.startsWith("/api/data-version")
        ? dataVersion()
        : { rows: [] };
      return Promise.resolve({
        status: 200,
        ok: true,
        text: () => Promise.resolve(JSON.stringify(body)),
      } as unknown as Response);
    }),
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("every data hook names the snapshot", () => {
  const hookNames = Object.keys(queries).filter(
    (name) => name.startsWith("use") && !UNVERSIONED.has(name),
  );

  it("covers at least the known data hooks", () => {
    expect(hookNames).toEqual(
      expect.arrayContaining([
        "useGains",
        "useHoldings",
        "useInstruments",
        "useTransactions",
        "usePriceStatus",
        "usePortfolioValueHistory",
      ]),
    );
  });

  it.each(hookNames)(
    "%s puts the version token in its cache key",
    async (name) => {
      stubFetch();
      const queryClient = new QueryClient({
        defaultOptions: { queries: { retry: false } },
      });
      const wrapper = ({ children }: { children: ReactNode }) => (
        <QueryClientProvider client={queryClient}>
          {children}
        </QueryClientProvider>
      );
      const hook = (queries as Record<string, (...args: unknown[]) => unknown>)[
        name
      ];

      const { result } = renderHook(() => hook(...(HOOK_ARGS[name] ?? [])), {
        wrapper,
      });

      // Mutations are identified by the contract returned by useMutation,
      // rather than by an export-name convention that a future data hook could
      // accidentally match.
      if (isMutationResult(result.current)) return;

      // Active queries only: a versioned hook renders once with a null token
      // before the heartbeat resolves, and that disabled entry stays in the
      // cache until garbage collection.
      await waitFor(() => {
        const keys = queryClient
          .getQueryCache()
          .findAll({ type: "active" })
          .map((query) => query.queryKey)
          .filter((key) => key[0] !== "data-version");
        expect(keys.length).toBeGreaterThan(0);
        for (const key of keys) {
          expect(key[1]).toBe("r:1@2026-09-12");
        }
      });
    },
  );
});

describe("gains period request parameters", () => {
  it("sends a non-custom preset without explicit dates", async () => {
    const requests: string[] = [];
    stubFetch(requests);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    renderHook(
      () =>
        queries.useGains({
          period: "ytd",
          startDate: "2026-01-01",
          endDate: "2026-09-12",
        }),
      { wrapper },
    );

    await waitFor(() => expect(requests).toHaveLength(1));
    const request = new URL(requests[0], "http://localhost");
    expect(request.searchParams.get("period")).toBe("ytd");
    expect(request.searchParams.has("start_date")).toBe(false);
    expect(request.searchParams.has("end_date")).toBe(false);
  });

  it("sends explicit dates with the custom preset", async () => {
    const requests: string[] = [];
    stubFetch(requests);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    renderHook(
      () =>
        queries.useGains({
          period: "custom",
          startDate: "2026-01-01",
          endDate: "2026-09-12",
        }),
      { wrapper },
    );

    await waitFor(() => expect(requests).toHaveLength(1));
    const request = new URL(requests[0], "http://localhost");
    expect(request.searchParams.get("period")).toBe("custom");
    expect(request.searchParams.get("start_date")).toBe("2026-01-01");
    expect(request.searchParams.get("end_date")).toBe("2026-09-12");
  });
});

describe("a snapshot change refetches data", () => {
  it("surfaces a heartbeat failure and retries the heartbeat", async () => {
    let versionAvailable = false;
    let versionRequests = 0;
    const gainsRequests: string[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        const isVersion = url.startsWith("/api/data-version");
        if (isVersion) versionRequests += 1;
        else gainsRequests.push(url);

        const status = isVersion && !versionAvailable ? 503 : 200;
        const body = isVersion ? VERSION : { rows: [] };
        return Promise.resolve({
          status,
          ok: status === 200,
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

    const { result } = renderHook(() => queries.useGains(), { wrapper });

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.isPending).toBe(false);
    expect(gainsRequests).toHaveLength(0);

    versionAvailable = true;
    await result.current.refetch();

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(versionRequests).toBe(2);
    expect(gainsRequests).toHaveLength(1);
  });

  it("rechecks the heartbeat when a response has another revision", async () => {
    let versionRequests = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn((input: RequestInfo | URL) => {
        const url = String(input);
        const isVersion = url.startsWith("/api/data-version");
        if (isVersion) versionRequests += 1;
        return Promise.resolve({
          status: 200,
          ok: true,
          text: () =>
            Promise.resolve(
              JSON.stringify(
                isVersion ? VERSION : { data_revision: "r:2", rows: [] },
              ),
            ),
        } as unknown as Response);
      }),
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => queries.useGains(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await waitFor(() => expect(versionRequests).toBe(2));
  });

  it("refetches gains when the backend's revision moves", async () => {
    let version = { ...VERSION };
    const gainsRequests: string[] = [];
    stubFetch(gainsRequests, () => version);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => queries.useGains(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(gainsRequests).toHaveLength(1);

    version = { ...VERSION, data_revision: "r:2" };
    await queryClient.refetchQueries({ queryKey: ["data-version"] });

    await waitFor(() => expect(gainsRequests).toHaveLength(2));
  });

  it("refetches gains when the server's day changes", async () => {
    let version = { ...VERSION };
    const gainsRequests: string[] = [];
    stubFetch(gainsRequests, () => version);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );

    const { result } = renderHook(() => queries.useGains(), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    version = { ...VERSION, valuation_date: "2026-09-13" };
    await queryClient.refetchQueries({ queryKey: ["data-version"] });

    await waitFor(() => expect(gainsRequests).toHaveLength(2));
  });
});
