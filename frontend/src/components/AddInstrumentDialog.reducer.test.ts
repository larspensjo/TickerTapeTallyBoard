import { describe, expect, it } from "vitest";
import {
  type AddInstrumentState,
  addInstrumentReducer,
  buildSubmissionMessages,
  createInitialAddInstrumentState,
  guardInstrumentLookup,
  instrumentPriceMappingNote,
  revealAddInstrumentIntent,
  validateInstrumentDraft,
} from "./AddInstrumentDialog";

function filledState(): AddInstrumentState {
  return {
    ...createInitialAddInstrumentState(),
    symbol: "MSFT",
    exchange: "NASDAQ",
    name: "Microsoft",
    instrumentType: "Etf",
    currency: "EUR",
    isin: "US5949181045",
    submitting: true,
    error: "old error",
  };
}

describe("addInstrumentReducer", () => {
  it("updates fields and clears errors", () => {
    const next = addInstrumentReducer(
      { ...createInitialAddInstrumentState(), error: "boom" },
      { type: "fieldChanged", field: "symbol", value: "AAPL" },
    );
    expect(next.symbol).toBe("AAPL");
    expect(next.error).toBeNull();
  });

  it("resets the form on submitSucceeded and stores the submission result", () => {
    const next = addInstrumentReducer(filledState(), {
      type: "submitSucceeded",
      result: { instrumentId: 17, messages: ["already exists"] },
    });

    expect(next.symbol).toBe("");
    expect(next.exchange).toBe("");
    expect(next.name).toBe("");
    expect(next.instrumentType).toBe("Stock");
    expect(next.currency).toBe("USD");
    expect(next.isin).toBe("");
    expect(next.submitting).toBe(false);
    expect(next.error).toBeNull();
    expect(next.result).toEqual({
      instrumentId: 17,
      messages: ["already exists"],
    });
  });

  it("updates the ISIN field and clears errors", () => {
    const next = addInstrumentReducer(
      { ...createInitialAddInstrumentState(), error: "boom" },
      { type: "fieldChanged", field: "isin", value: "US1234567890" },
    );
    expect(next.isin).toBe("US1234567890");
    expect(next.error).toBeNull();
  });
});

describe("instrument lookup guard", () => {
  it("rejects mistyped instruments and allows provider-unavailable lookups with a warning", () => {
    expect(
      guardInstrumentLookup({
        query: "MSFT",
        status: "no_match",
        matches: [],
      }),
    ).toEqual({
      allowCreate: false,
      warning: null,
      error: "No suitable provider match was found for this instrument.",
      sourceNote: null,
    });

    expect(
      guardInstrumentLookup({
        query: "MSFT",
        status: "provider_unavailable",
        matches: [],
      }),
    ).toEqual({
      allowCreate: true,
      warning: "Could not verify instrument - provider unavailable.",
      error: null,
      sourceNote: null,
    });
  });

  it("allows matches and names the highest-precedence source", () => {
    expect(
      guardInstrumentLookup({
        query: "JE00BJ7HNC92",
        status: "matches",
        matches: [
          {
            provider: "NASDAQ_NORDIC",
            provider_symbol: "TX2997672",
            quote_type: null,
            exchange: "Warrants",
            name: "AVA SAMSUNG TRACKER",
            asset_class: "TRACKER_CERTIFICATES",
            currency: "SEK",
          },
        ],
      }),
    ).toEqual({
      allowCreate: true,
      warning: null,
      error: null,
      sourceNote:
        "Provider match: Nasdaq Nordic · TX2997672 · TRACKER_CERTIFICATES · SEK.",
    });
  });

  it("allows a matches response with no entries and omits the source note", () => {
    expect(
      guardInstrumentLookup({
        query: "MSFT",
        status: "matches",
        matches: [],
      }),
    ).toEqual({
      allowCreate: true,
      warning: null,
      error: null,
      sourceNote: null,
    });
  });
});

describe("submission feedback", () => {
  it("validates the required instrument fields before lookup", () => {
    expect(validateInstrumentDraft(createInitialAddInstrumentState())).toBe(
      "Symbol is required.",
    );
    expect(
      validateInstrumentDraft({
        ...createInitialAddInstrumentState(),
        symbol: "MSFT",
      }),
    ).toBe("Exchange is required.");
  });

  it("combines collision and unpriceable-ghost feedback", () => {
    expect(
      buildSubmissionMessages({
        lookupWarning: "Could not verify instrument - provider unavailable.",
        sourceNote: "Provider match: Nasdaq Nordic · TX2997672 · SEK.",
        upsertStatus: 200,
        priceMappingNote: "No price mapping yet - configure provider symbol.",
      }),
    ).toEqual([
      "Could not verify instrument - provider unavailable.",
      "Provider match: Nasdaq Nordic · TX2997672 · SEK.",
      "Instrument already exists.",
      "No price mapping yet - configure provider symbol.",
    ]);
  });

  it("describes a fresh watchlist row with missing price mapping", () => {
    expect(
      instrumentPriceMappingNote(
        {
          refreshing: false,
          latest_run: null,
          instruments: [
            {
              instrument_id: 9,
              exchange: "NASDAQ",
              symbol: "MSFT",
              currency: "USD",
              price_sources: [],
              effective_price_source: null,
              open_quantity: 0,
              latest_price: {
                status: "unmapped",
                date: null,
                value: null,
                provider: null,
                provider_symbol: null,
                reason: null,
              },
              latest_fx: {
                status: "missing",
                date: null,
                value: null,
                provider: null,
                provider_symbol: null,
                reason: null,
              },
            },
          ],
        },
        9,
      ),
    ).toBe("No price mapping yet - configure provider symbol.");
  });

  it("produces a reveal intent that turns the watchlist toggle on and targets the row", () => {
    const state = addInstrumentReducer(createInitialAddInstrumentState(), {
      type: "submitSucceeded",
      result: { instrumentId: 42, messages: [] },
    });

    expect(revealAddInstrumentIntent(state)).toEqual({
      includeWatchlist: true,
      instrumentId: 42,
    });
  });
});
