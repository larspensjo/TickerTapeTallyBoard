import { type ChangeEvent, type FormEvent, useReducer } from "react";
import {
  type NewTransactionInput,
  useCreateTransaction,
  useUpsertInstrument,
} from "../api/queries";
import type { Instrument, InstrumentType, TransactionType } from "../api/types";

type InstrumentMode = "existing" | "new";

type TextField =
  | "instrumentId"
  | "symbol"
  | "exchange"
  | "name"
  | "instrumentCurrency"
  | "tradeDate"
  | "quantity"
  | "price"
  | "currency"
  | "fxRate"
  | "brokerage"
  | "note";

interface FormState {
  instrumentMode: InstrumentMode;
  instrumentId: string;
  symbol: string;
  exchange: string;
  name: string;
  instrumentType: InstrumentType;
  instrumentCurrency: string;
  type: TransactionType;
  tradeDate: string;
  quantity: string;
  price: string;
  currency: string;
  fxRate: string;
  brokerage: string;
  note: string;
  error: string | null;
  submitting: boolean;
}

type FormAction =
  | { type: "fieldChanged"; field: TextField; value: string }
  | { type: "instrumentModeChanged"; mode: InstrumentMode }
  | { type: "instrumentTypeChanged"; value: InstrumentType }
  | { type: "transactionTypeChanged"; value: TransactionType }
  | { type: "submitStarted" }
  | { type: "submitFailed"; message: string }
  | { type: "submitSucceeded" };

function createInitialState(hasInstruments: boolean): FormState {
  return {
    instrumentMode: hasInstruments ? "existing" : "new",
    instrumentId: "",
    symbol: "",
    exchange: "",
    name: "",
    instrumentType: "Stock",
    instrumentCurrency: "USD",
    type: "Buy",
    tradeDate: new Date().toISOString().slice(0, 10),
    quantity: "",
    price: "",
    currency: "USD",
    fxRate: "",
    brokerage: "",
    note: "",
    error: null,
    submitting: false,
  };
}

function reducer(state: FormState, action: FormAction): FormState {
  switch (action.type) {
    case "fieldChanged":
      return { ...state, [action.field]: action.value, error: null };
    case "instrumentModeChanged":
      return { ...state, instrumentMode: action.mode, error: null };
    case "instrumentTypeChanged":
      return { ...state, instrumentType: action.value, error: null };
    case "transactionTypeChanged":
      return { ...state, type: action.value, error: null };
    case "submitStarted":
      return { ...state, submitting: true, error: null };
    case "submitFailed":
      return { ...state, submitting: false, error: action.message };
    case "submitSucceeded":
      return {
        ...state,
        instrumentId: "",
        symbol: "",
        exchange: "",
        name: "",
        instrumentType: "Stock",
        type: "Buy",
        quantity: "",
        price: "",
        currency: "USD",
        fxRate: "",
        brokerage: "",
        note: "",
        error: null,
        submitting: false,
      };
  }

  return state;
}

function trimmedOrUndefined(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
}

function toNumber(value: string, label: string): number {
  if (value.trim() === "") {
    throw new Error(`${label} is required.`);
  }

  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`${label} must be a number.`);
  }
  return parsed;
}

export function AddTransactionForm({
  instruments,
  onClose,
}: {
  instruments: Instrument[];
  onClose: () => void;
}) {
  const [state, dispatch] = useReducer(
    reducer,
    instruments.length > 0,
    createInitialState,
  );
  const upsertInstrument = useUpsertInstrument();
  const createTransaction = useCreateTransaction();

  const isSplit = state.type === "Split";
  const isDividend = state.type === "Dividend";

  const setField =
    (field: TextField) =>
    (event: ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
      dispatch({ type: "fieldChanged", field, value: event.target.value });

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    dispatch({ type: "submitStarted" });

    try {
      let instrumentId: number;

      if (state.instrumentMode === "existing") {
        instrumentId = Number(state.instrumentId);
        if (!instrumentId) {
          throw new Error("Select an instrument.");
        }
      } else {
        const instrument = await upsertInstrument.mutateAsync({
          symbol: state.symbol.trim(),
          exchange: state.exchange.trim(),
          name: state.name.trim(),
          type: state.instrumentType,
          currency: state.instrumentCurrency.trim(),
        });
        instrumentId = instrument.id;
      }

      const input: NewTransactionInput = {
        instrument_id: instrumentId,
        type: state.type,
        trade_date: state.tradeDate,
        quantity: toNumber(state.quantity, "Quantity"),
        price: isSplit ? undefined : trimmedOrUndefined(state.price),
        currency: isSplit ? undefined : trimmedOrUndefined(state.currency),
        fx_rate_to_base: isSplit ? undefined : trimmedOrUndefined(state.fxRate),
        brokerage:
          isSplit || isDividend
            ? undefined
            : trimmedOrUndefined(state.brokerage),
        note: trimmedOrUndefined(state.note),
      };

      await createTransaction.mutateAsync(input);
      dispatch({ type: "submitSucceeded" });
      onClose();
    } catch (error) {
      dispatch({
        type: "submitFailed",
        message:
          error instanceof Error
            ? error.message
            : "Could not save transaction.",
      });
    }
  }

  return (
    <form className="transaction-form" onSubmit={handleSubmit}>
      <div className="form-row">
        <label className="form-field">
          <span>Instrument source</span>
          <select
            value={state.instrumentMode}
            onChange={(event) =>
              dispatch({
                type: "instrumentModeChanged",
                mode: event.target.value as InstrumentMode,
              })
            }
          >
            <option value="existing">Pick existing</option>
            <option value="new">Create new</option>
          </select>
        </label>

        {state.instrumentMode === "existing" ? (
          <label className="form-field">
            <span>Instrument</span>
            <select
              value={state.instrumentId}
              onChange={setField("instrumentId")}
            >
              <option value="">Select...</option>
              {instruments.map((instrument) => (
                <option key={instrument.id} value={instrument.id}>
                  {instrument.symbol} - {instrument.exchange}
                </option>
              ))}
            </select>
          </label>
        ) : null}
      </div>

      {state.instrumentMode === "new" ? (
        <div className="form-row">
          <label className="form-field">
            <span>Symbol</span>
            <input value={state.symbol} onChange={setField("symbol")} />
          </label>
          <label className="form-field">
            <span>Exchange</span>
            <input value={state.exchange} onChange={setField("exchange")} />
          </label>
          <label className="form-field">
            <span>Name</span>
            <input value={state.name} onChange={setField("name")} />
          </label>
          <label className="form-field">
            <span>Type</span>
            <select
              value={state.instrumentType}
              onChange={(event) =>
                dispatch({
                  type: "instrumentTypeChanged",
                  value: event.target.value as InstrumentType,
                })
              }
            >
              <option value="Stock">Stock</option>
              <option value="Etf">Etf</option>
              <option value="Fund">Fund</option>
            </select>
          </label>
          <label className="form-field">
            <span>Currency</span>
            <input
              value={state.instrumentCurrency}
              onChange={setField("instrumentCurrency")}
            />
          </label>
        </div>
      ) : null}

      <div className="form-row">
        <label className="form-field">
          <span>Type</span>
          <select
            value={state.type}
            onChange={(event) =>
              dispatch({
                type: "transactionTypeChanged",
                value: event.target.value as TransactionType,
              })
            }
          >
            <option value="Buy">Buy</option>
            <option value="Sell">Sell</option>
            <option value="Split">Split</option>
            <option value="Dividend">Dividend</option>
          </select>
        </label>
        <label className="form-field">
          <span>Trade date</span>
          <input
            type="date"
            value={state.tradeDate}
            onChange={setField("tradeDate")}
          />
        </label>
        <label className="form-field">
          <span>
            {isSplit
              ? "Quantity delta"
              : isDividend
                ? "Shares eligible"
                : "Quantity"}
          </span>
          <input
            type="number"
            value={state.quantity}
            onChange={setField("quantity")}
          />
        </label>
      </div>

      {!isSplit ? (
        <div className="form-row">
          <label className="form-field">
            <span>
              {isDividend ? "Dividend per share (native)" : "Price (native)"}
            </span>
            <input value={state.price} onChange={setField("price")} />
          </label>
          <label className="form-field">
            <span>Currency</span>
            <input value={state.currency} onChange={setField("currency")} />
          </label>
          <label className="form-field">
            <span>FX to SEK (optional)</span>
            <input value={state.fxRate} onChange={setField("fxRate")} />
          </label>
          {!isDividend ? (
            <label className="form-field">
              <span>Brokerage (SEK)</span>
              <input value={state.brokerage} onChange={setField("brokerage")} />
            </label>
          ) : null}
        </div>
      ) : null}

      <div className="form-row">
        <label className="form-field grow">
          <span>Note (optional)</span>
          <input value={state.note} onChange={setField("note")} />
        </label>
      </div>

      {state.error ? <p className="form-error">{state.error}</p> : null}

      <div className="form-actions">
        <button type="button" className="button secondary" onClick={onClose}>
          Cancel
        </button>
        <button
          type="submit"
          className="button primary"
          disabled={state.submitting}
        >
          {state.submitting ? "Saving..." : "Save transaction"}
        </button>
      </div>
    </form>
  );
}
