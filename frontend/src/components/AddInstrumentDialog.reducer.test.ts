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
    expect(next.submitting).toBe(false);
    expect(next.error).toBeNull();
    expect(next.result).toEqual({
      instrumentId: 17,
      messages: ["already exists"],
    });
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
        upsertStatus: 200,
        priceMappingNote: "No price mapping yet - configure provider symbol.",
      }),
    ).toEqual([
      "Could not verify instrument - provider unavailable.",
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
              mapping_enabled: false,
              provider_symbol: null,
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
