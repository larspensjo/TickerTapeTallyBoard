// @vitest-environment jsdom

import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useRefreshPrices } from "../api/queries";
import { refreshResult } from "../test/priceRefresh";
import { usePriceRefresh } from "./usePriceRefresh";

function response(body: unknown, status = 200): Response {
  return {
    status,
    ok: status < 400,
    text: async () => JSON.stringify(body),
  } as Response;
}

const clients: QueryClient[] = [];

function setup() {
  let serverRefreshing = false;
  let heartbeatRequests = 0;
  const posts: { body: unknown; resolve: (value: Response) => void }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: RequestInfo | URL, init?: RequestInit) => {
      if (String(url) === "/api/data-version") {
        heartbeatRequests++;
        return Promise.resolve(
          response({
            data_revision: "ledger-a:1",
            valuation_date: "2026-10-05",
            prices_refreshing: serverRefreshing,
          }),
        );
      }
      if (String(url) === "/api/prices/refresh" && init?.method === "POST") {
        return new Promise<Response>((resolve) => {
          posts.push({ body: JSON.parse(String(init.body)), resolve });
        });
      }
      throw new Error(`Unexpected request: ${url}`);
    }),
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  clients.push(client);
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  );
  const controller = renderHook(() => usePriceRefresh(), { wrapper });

  return {
    controller,
    posts,
    wrapper,
    heartbeatRequests: () => heartbeatRequests,
    async observeHeartbeat(refreshing: boolean) {
      await act(async () => {
        await client.refetchQueries({ queryKey: ["data-version"] });
      });
      serverRefreshing = refreshing;
      await act(async () => {
        await client.refetchQueries({ queryKey: ["data-version"] });
      });
    },
    async startRequest() {
      const count = posts.length;
      act(() => controller.result.current.refresh());
      await waitFor(() => expect(posts).toHaveLength(count + 1));
      await waitFor(() =>
        expect(controller.result.current.status.running).toBe(true),
      );
      return posts[count];
    },
    async failRequest() {
      const post = await this.startRequest();
      act(() =>
        post.resolve(
          response(
            {
              error: {
                code: "refresh_failed",
                message: "Provider unavailable",
              },
            },
            500,
          ),
        ),
      );
      await waitFor(() =>
        expect(controller.result.current.status.warning?.kind).toBe(
          "request_failed",
        ),
      );
      await waitFor(() =>
        expect(controller.result.current.status.running).toBe(false),
      );
    },
  };
}

afterEach(() => {
  cleanup();
  for (const client of clients) client.clear();
  clients.length = 0;
  vi.unstubAllGlobals();
});

describe("usePriceRefresh", () => {
  it("reports the header request as running until it finishes and ignores another click", async () => {
    const context = setup();
    await context.observeHeartbeat(false);
    const post = await context.startRequest();
    expect(post.body).toEqual({ mode: "latest" });
    act(() => context.controller.result.current.refresh());
    expect(context.posts).toHaveLength(1);
    act(() => post.resolve(response(refreshResult())));
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(false),
    );
  });

  it("reports a separately mounted Backfill mutation as running until it finishes", async () => {
    const context = setup();
    await context.observeHeartbeat(false);
    const backfill = renderHook(() => useRefreshPrices(), {
      wrapper: context.wrapper,
    });
    act(() => backfill.result.current.mutate({ mode: "backfill" }));
    await waitFor(() => expect(context.posts).toHaveLength(1));
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(true),
    );
    expect(context.posts[0].body).toEqual({ mode: "backfill" });
    act(() => context.controller.result.current.refresh());
    expect(context.posts).toHaveLength(1);
    act(() => context.posts[0].resolve(response(refreshResult())));
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(false),
    );
  });

  it("reports heartbeat-only running, ignores refresh, and clears when the heartbeat finishes", async () => {
    const context = setup();
    await context.observeHeartbeat(true);
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(true),
    );
    act(() => context.controller.result.current.refresh());
    expect(context.posts).toHaveLength(0);
    await context.observeHeartbeat(false);
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(false),
    );
  });

  it("shows a 500 request failure and refetches the heartbeat on error and success", async () => {
    const context = setup();
    await context.observeHeartbeat(false);
    const beforeFailure = context.heartbeatRequests();
    await context.failRequest();
    expect(context.controller.result.current.status.warning).toEqual({
      kind: "request_failed",
      label: "Refresh failed",
      detail: "Provider unavailable",
    });
    await waitFor(() =>
      expect(context.heartbeatRequests()).toBeGreaterThan(beforeFailure),
    );
    const beforeSuccess = context.heartbeatRequests();
    const retry = await context.startRequest();
    act(() => retry.resolve(response(refreshResult())));
    await waitFor(() =>
      expect(context.controller.result.current.status.warning).toBeNull(),
    );
    await waitFor(() =>
      expect(context.heartbeatRequests()).toBeGreaterThan(beforeSuccess),
    );
  });

  it("keeps a failure through a pending retry and a merged response, then clears on finished success", async () => {
    const context = setup();
    await context.observeHeartbeat(false);
    await context.failRequest();
    const retry = await context.startRequest();
    expect(context.controller.result.current.status.warning?.kind).toBe(
      "request_failed",
    );
    act(() => retry.resolve(response(refreshResult("running"))));
    await waitFor(() =>
      expect(context.controller.result.current.status.running).toBe(false),
    );
    expect(context.controller.result.current.status.warning?.kind).toBe(
      "request_failed",
    );
    const finished = await context.startRequest();
    expect(context.controller.result.current.status.warning?.kind).toBe(
      "request_failed",
    );
    act(() => finished.resolve(response(refreshResult("succeeded"))));
    await waitFor(() =>
      expect(context.controller.result.current.status.warning).toBeNull(),
    );
  });
});
