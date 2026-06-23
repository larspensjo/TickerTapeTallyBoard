import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, apiGet, apiSend } from "./client";

// The production parse() function only reads response.status, response.ok,
// and response.text(), so a plain object satisfies its interface without
// triggering Node's Response constructor restrictions on null-body status codes.
function mockResponse(status: number, body: unknown = undefined): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    text: () => Promise.resolve(body === undefined ? "" : JSON.stringify(body)),
  } as unknown as Response;
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("apiGet", () => {
  it("returns the parsed JSON body on a 200 response", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse(200, { value: 42 })),
    );
    const result = await apiGet<{ value: number }>("/api/x");
    expect(result).toEqual({ value: 42 });
  });

  it("resolves to undefined for a 204 response", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse(204)));
    await expect(apiGet("/api/x")).resolves.toBeUndefined();
  });

  it("resolves to null when the 200 body contains malformed JSON", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({
        status: 200,
        ok: true,
        text: () => Promise.resolve("not-valid-json"),
      } as unknown as Response),
    );
    await expect(apiGet("/api/x")).resolves.toBeNull();
  });
});

describe("client error mapping", () => {
  it("maps a structured error body to ApiError", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        mockResponse(400, {
          error: { code: "invalid_date_range", message: "bad" },
        }),
      ),
    );
    await expect(apiGet("/api/x")).rejects.toMatchObject({
      name: "ApiError",
      code: "invalid_date_range",
      message: "bad",
    });
  });

  it("falls back to unknown for a bodyless error", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse(500)));
    await expect(apiGet("/api/x")).rejects.toMatchObject({
      code: "unknown",
      message: "Request failed: 500",
    });
  });

  it("throws ApiError instances", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        mockResponse(422, {
          error: { code: "bad_input", message: "invalid" },
        }),
      ),
    );
    await expect(apiGet("/api/x")).rejects.toBeInstanceOf(ApiError);
  });
});

describe("apiSend", () => {
  it("stringifies the body and sets the json content-type", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(mockResponse(200, { ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    await apiSend("POST", "/api/x", { a: 1 });
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBe(JSON.stringify({ a: 1 }));
    expect((init.headers as Record<string, string>)["content-type"]).toBe(
      "application/json",
    );
  });

  it("sends undefined body when no body is provided", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(mockResponse(200, { ok: true }));
    vi.stubGlobal("fetch", fetchMock);
    await apiSend("DELETE", "/api/x", undefined);
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBeUndefined();
  });
});
