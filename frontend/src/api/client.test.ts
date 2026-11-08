import { afterEach, describe, expect, it, vi } from "vitest";
import { reportDate } from "../test/companyFiguresFixtures";
import { instrumentFundamentals } from "../test/instrumentFundamentalsFixture";
import { ApiError, apiGet, apiSend, apiSendWithStatus } from "./client";
import type {
  Figure,
  HoldingsResponse,
  Instrument,
  InstrumentFundamentals,
  ReportBasis,
} from "./types";

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
  vi.unstubAllGlobals();
});

describe("fundamentals source contracts", () => {
  it("preserves backend local dates separately from raw UTC instants", async () => {
    const body = instrumentFundamentals();
    body.source.last_success = "2026-11-01T23:30:00+00:00";
    body.source.last_attempt_at = "2026-11-02T23:30:00+00:00";
    body.source.last_success_on = "2026-11-02";
    body.source.last_attempt_on = "2026-11-03";
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse(200, body)));
    const parsed = await apiGet<InstrumentFundamentals>(
      "/api/instruments/1/fundamentals",
    );
    expect(parsed.source).toEqual(body.source);
  });
  it.each(["PUT", "DELETE"])(
    "preserves %s validation error codes and messages",
    async (method) => {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(
          mockResponse(400, {
            error: {
              code: "previous_link_required",
              message: "Choose the previous company",
            },
          }),
        ),
      );
      await expect(
        apiSend(method, "/api/instruments/1/fundamentals-source", {}),
      ).rejects.toMatchObject({
        code: "previous_link_required",
        message: "Choose the previous company",
      });
    },
  );
  it("preserves preview error bodies", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        mockResponse(400, {
          error: {
            code: "invalid_provider_symbol",
            message: "Symbol is required",
          },
        }),
      ),
    );
    await expect(
      apiGet("/api/instruments/1/fundamentals-source/preview?symbol="),
    ).rejects.toMatchObject({
      code: "invalid_provider_symbol",
      message: "Symbol is required",
    });
  });
  it.each(["PUT", "DELETE"])(
    "handles %s's empty successful body",
    async (method) => {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse(204)));
      await expect(
        apiSend(method, "/api/instruments/1/fundamentals-source", {
          previous_link: "wrong_company",
        }),
      ).resolves.toBeUndefined();
    },
  );
});

describe("apiGet", () => {
  it.each([
    "complete",
    "approximate",
    "not_applicable",
    "unavailable",
  ] as const)(
    "parses holdings %s figures without losing decimals, reasons or nulls",
    async (certainty) => {
      const figure: Figure = {
        certainty,
        value:
          certainty === "complete" || certainty === "approximate"
            ? "24.345678901234567890"
            : null,
        reasons:
          certainty === "complete"
            ? []
            : [
                {
                  code:
                    certainty === "not_applicable"
                      ? "fund_or_etf"
                      : "source_unreachable",
                  detail: "2026-11-01",
                },
              ],
      };
      const body = {
        data_revision: "test",
        hidden_watchlist_pool_count: 0,
        holdings: [
          {
            instrument: {
              id: 1,
              symbol: "SE0012673267",
              ticker: "EVO",
              exchange: "AVANZA",
              name: "Evolution",
              type: "Stock",
              currency: "SEK",
              conviction: "Other",
            },
            fundamentals: { pe: figure, ps: figure, next_report: reportDate() },
          },
        ],
      };
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(mockResponse(200, body)),
      );
      const result = await apiGet<HoldingsResponse>("/api/holdings");
      expect(result).toEqual(body);
      expect(result.holdings[0].fundamentals.pe).toEqual(figure);
      expect(result.holdings[0].instrument.ticker).toBe("EVO");
    },
  );
  it.each([
    "company_announced",
    "nasdaq_expected",
    "last_year_date",
    "typical_gap",
    null,
  ] satisfies (ReportBasis | null)[])(
    "preserves report basis %s and all serialized report fields",
    async (basis) => {
      const next_report = reportDate({
        certainty: basis === "company_announced" ? "confirmed" : "estimated",
        basis,
        date: "2026-11-01",
        days_until: -7,
        period_end: "2026-09-30",
        observed_between: ["2026-10-31", "2026-11-02"],
        reasons: [{ code: "estimate_passed" }],
      });
      const body = { holdings: [{ fundamentals: { next_report } }] };
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(mockResponse(200, body)),
      );
      const result = await apiGet<HoldingsResponse>("/api/holdings");
      expect(result.holdings[0].fundamentals.next_report).toEqual(next_report);
    },
  );
  it("preserves nullable ticker fields on the instruments response used by Transactions", async () => {
    const body = [
      { id: 1, ticker: null },
      { id: 2, ticker: "MSFT" },
    ];
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(mockResponse(200, body)));
    expect(await apiGet<Instrument[]>("/api/instruments")).toEqual(body);
  });
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

describe("apiSendWithStatus", () => {
  it("returns the response status alongside the parsed body", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(mockResponse(200, { ok: true })),
    );

    await expect(
      apiSendWithStatus("POST", "/api/x", { a: 1 }),
    ).resolves.toEqual({
      status: 200,
      body: { ok: true },
    });
  });
});
