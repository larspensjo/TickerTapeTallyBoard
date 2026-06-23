import { describe, expect, it } from "vitest";
import {
  addTransactionReducer,
  createInitialState,
  type FormState,
} from "./AddTransactionForm";

const base = (): FormState => createInitialState(true);

describe("addTransactionReducer", () => {
  it("updates a text field and clears the error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "boom" },
      { type: "fieldChanged", field: "quantity", value: "10" },
    );
    expect(next.quantity).toBe("10");
    expect(next.error).toBeNull();
  });

  it("updates symbol field and clears error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "prev" },
      { type: "fieldChanged", field: "symbol", value: "AAPL" },
    );
    expect(next.symbol).toBe("AAPL");
    expect(next.error).toBeNull();
  });

  it("switches instrument mode and clears error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "old" },
      { type: "instrumentModeChanged", mode: "new" },
    );
    expect(next.instrumentMode).toBe("new");
    expect(next.error).toBeNull();
  });

  it("sets transaction type and clears error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "old" },
      { type: "transactionTypeChanged", value: "Sell" },
    );
    expect(next.type).toBe("Sell");
    expect(next.error).toBeNull();
  });

  it("sets instrument type and clears error", () => {
    const next = addTransactionReducer(
      { ...base(), error: "old" },
      { type: "instrumentTypeChanged", value: "Etf" },
    );
    expect(next.instrumentType).toBe("Etf");
    expect(next.error).toBeNull();
  });

  it("marks submitting on submitStarted and records a failure message", () => {
    const started = addTransactionReducer(base(), { type: "submitStarted" });
    expect(started.submitting).toBe(true);
    expect(started.error).toBeNull();

    const failed = addTransactionReducer(started, {
      type: "submitFailed",
      message: "nope",
    });
    expect(failed.submitting).toBe(false);
    expect(failed.error).toBe("nope");
  });

  it("resets transaction fields on submitSucceeded and preserves instrumentMode, tradeDate, instrumentCurrency", () => {
    const filled: FormState = {
      ...base(),
      instrumentMode: "new",
      instrumentId: "5",
      symbol: "TSLA",
      exchange: "NASDAQ",
      name: "Tesla",
      instrumentType: "Etf",
      instrumentCurrency: "EUR",
      type: "Sell",
      tradeDate: "2025-01-15",
      quantity: "100",
      price: "200",
      currency: "EUR",
      fxRate: "10.5",
      brokerage: "50",
      note: "test note",
      submitting: true,
      error: "old error",
    };

    const next = addTransactionReducer(filled, { type: "submitSucceeded" });

    // Reset fields
    expect(next.instrumentId).toBe("");
    expect(next.symbol).toBe("");
    expect(next.exchange).toBe("");
    expect(next.name).toBe("");
    expect(next.instrumentType).toBe("Stock");
    expect(next.type).toBe("Buy");
    expect(next.quantity).toBe("");
    expect(next.price).toBe("");
    expect(next.currency).toBe("USD");
    expect(next.fxRate).toBe("");
    expect(next.brokerage).toBe("");
    expect(next.note).toBe("");
    expect(next.submitting).toBe(false);
    expect(next.error).toBeNull();

    // Preserved fields
    expect(next.instrumentMode).toBe("new");
    expect(next.tradeDate).toBe("2025-01-15");
    expect(next.instrumentCurrency).toBe("EUR");
  });

  it("seeds the instrument mode from whether instruments exist", () => {
    expect(createInitialState(true).instrumentMode).toBe("existing");
    expect(createInitialState(false).instrumentMode).toBe("new");
  });
});
