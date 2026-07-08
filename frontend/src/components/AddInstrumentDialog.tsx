import { type ChangeEvent, type FormEvent, useReducer } from "react";
import { apiGet } from "../api/client";
import { lookupInstrument, useUpsertInstrument } from "../api/queries";
import type {
  InstrumentLookupResponse,
  InstrumentType,
  PriceStatusResponse,
} from "../api/types";

const DEFAULT_PRICE_MAPPING_NOTE =
  "No price mapping yet - configure provider symbol.";
const PROVIDER_UNAVAILABLE_WARNING =
  "Could not verify instrument - provider unavailable.";

export interface AddInstrumentSubmission {
  instrumentId: number;
  messages: string[];
}

export interface AddInstrumentRevealIntent {
  includeWatchlist: true;
  instrumentId: number;
}

export interface AddInstrumentState {
  symbol: string;
  exchange: string;
  name: string;
  instrumentType: InstrumentType;
  currency: string;
  isin: string;
  submitting: boolean;
  error: string | null;
  result: AddInstrumentSubmission | null;
}

export type AddInstrumentAction =
  | {
      type: "fieldChanged";
      field: "symbol" | "exchange" | "name" | "currency" | "isin";
      value: string;
    }
  | { type: "instrumentTypeChanged"; value: InstrumentType }
  | { type: "submitStarted" }
  | { type: "submitFailed"; message: string }
  | { type: "submitSucceeded"; result: AddInstrumentSubmission };

export function createInitialAddInstrumentState(): AddInstrumentState {
  return {
    symbol: "",
    exchange: "",
    name: "",
    instrumentType: "Stock",
    currency: "USD",
    isin: "",
    submitting: false,
    error: null,
    result: null,
  };
}

export function addInstrumentReducer(
  state: AddInstrumentState,
  action: AddInstrumentAction,
): AddInstrumentState {
  switch (action.type) {
    case "fieldChanged":
      return { ...state, [action.field]: action.value, error: null };
    case "instrumentTypeChanged":
      return { ...state, instrumentType: action.value, error: null };
    case "submitStarted":
      return { ...state, submitting: true, error: null, result: null };
    case "submitFailed":
      return {
        ...state,
        submitting: false,
        error: action.message,
        result: null,
      };
    case "submitSucceeded":
      return {
        ...createInitialAddInstrumentState(),
        result: action.result,
      };
  }

  return state;
}

export interface InstrumentLookupGuard {
  allowCreate: boolean;
  warning: string | null;
  error: string | null;
}

export function guardInstrumentLookup(
  response: InstrumentLookupResponse,
): InstrumentLookupGuard {
  if (response.status === "provider_unavailable") {
    return {
      allowCreate: true,
      warning: PROVIDER_UNAVAILABLE_WARNING,
      error: null,
    };
  }

  if (response.status === "no_match") {
    return {
      allowCreate: false,
      warning: null,
      error: "No suitable provider match was found for this instrument.",
    };
  }

  return { allowCreate: true, warning: null, error: null };
}

export function revealAddInstrumentIntent(
  state: AddInstrumentState,
): AddInstrumentRevealIntent | null {
  if (!state.result) {
    return null;
  }

  return {
    includeWatchlist: true,
    instrumentId: state.result.instrumentId,
  };
}

export function instrumentPriceMappingNote(
  priceStatus: PriceStatusResponse | null,
  instrumentId: number,
): string | null {
  const instrument = priceStatus?.instruments.find(
    (candidate) => candidate.instrument_id === instrumentId,
  );
  if (!instrument) {
    return null;
  }

  if (
    !instrument.mapping_enabled ||
    instrument.provider_symbol === null ||
    instrument.latest_price.status === "unmapped"
  ) {
    return DEFAULT_PRICE_MAPPING_NOTE;
  }

  if (instrument.latest_price.status === "missing") {
    return "Price mapping exists, but the latest price is missing.";
  }

  return null;
}

export function buildSubmissionMessages(args: {
  lookupWarning: string | null;
  upsertStatus: number;
  priceMappingNote: string | null;
}): string[] {
  const messages: string[] = [];
  if (args.lookupWarning) {
    messages.push(args.lookupWarning);
  }
  if (args.upsertStatus === 200) {
    messages.push("Instrument already exists.");
  }
  if (args.priceMappingNote) {
    messages.push(args.priceMappingNote);
  }
  return messages;
}

export function validateInstrumentDraft(
  state: AddInstrumentState,
): string | null {
  if (state.symbol.trim() === "") {
    return "Symbol is required.";
  }
  if (state.exchange.trim() === "") {
    return "Exchange is required.";
  }
  if (state.name.trim() === "") {
    return "Name is required.";
  }
  if (state.currency.trim() === "") {
    return "Currency is required.";
  }

  return null;
}

function trimmedOrUndefined(value: string): string | undefined {
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
}

async function lookupGuard(symbol: string): Promise<InstrumentLookupGuard> {
  const response = await lookupInstrument(symbol);
  return guardInstrumentLookup(response);
}

async function priceMappingNoteFor(
  instrumentId: number,
): Promise<string | null> {
  const response = await apiGet<PriceStatusResponse>("/api/prices/status");
  return instrumentPriceMappingNote(response, instrumentId);
}

export function AddInstrumentDialog({
  onClose,
  onCreated,
}: {
  onClose: () => void;
  onCreated: (result: AddInstrumentSubmission) => void;
}) {
  const [state, dispatch] = useReducer(
    addInstrumentReducer,
    undefined,
    createInitialAddInstrumentState,
  );
  const upsertInstrument = useUpsertInstrument();

  const setField =
    (field: "symbol" | "exchange" | "name" | "currency" | "isin") =>
    (event: ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
      dispatch({ type: "fieldChanged", field, value: event.target.value });

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    dispatch({ type: "submitStarted" });

    try {
      const validationError = validateInstrumentDraft(state);
      if (validationError) {
        dispatch({ type: "submitFailed", message: validationError });
        return;
      }

      const symbol = state.symbol.trim();
      const lookup = await lookupGuard(symbol);
      if (!lookup.allowCreate) {
        dispatch({
          type: "submitFailed",
          message: lookup.error ?? "Could not verify the instrument.",
        });
        return;
      }

      const result = await upsertInstrument.mutateAsync({
        symbol,
        exchange: state.exchange.trim(),
        name: state.name.trim(),
        type: state.instrumentType,
        currency: state.currency.trim(),
        isin: trimmedOrUndefined(state.isin),
      });

      let priceMappingNote: string | null = null;
      try {
        priceMappingNote = await priceMappingNoteFor(result.instrument.id);
      } catch {
        priceMappingNote = null;
      }
      const messages = buildSubmissionMessages({
        lookupWarning: lookup.warning,
        upsertStatus: result.status,
        priceMappingNote,
      });
      const submission = { instrumentId: result.instrument.id, messages };

      dispatch({ type: "submitSucceeded", result: submission });
      onCreated(submission);
      onClose();
    } catch (error) {
      dispatch({
        type: "submitFailed",
        message:
          error instanceof Error ? error.message : "Could not add instrument.",
      });
    }
  }

  return (
    <div className="dialog-backdrop">
      <section
        className="panel dialog-panel"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-instrument-title"
      >
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h2 id="add-instrument-title">Add instrument</h2>
          </div>
          <button
            type="button"
            className="button outline"
            onClick={onClose}
            aria-label="Close add instrument dialog"
          >
            Close
          </button>
        </div>

        <form className="transaction-form dialog-form" onSubmit={handleSubmit}>
          <div className="form-row">
            <label className="form-field">
              <span>Symbol</span>
              <input value={state.symbol} onChange={setField("symbol")} />
            </label>
            <label className="form-field">
              <span>Exchange</span>
              <input value={state.exchange} onChange={setField("exchange")} />
            </label>
            <label className="form-field grow">
              <span>Name</span>
              <input value={state.name} onChange={setField("name")} />
            </label>
          </div>

          <div className="form-row">
            <label className="form-field grow">
              <span>ISIN</span>
              <input value={state.isin} onChange={setField("isin")} />
              <p className="form-hint">
                Optional. Enables matching with broker imports.
              </p>
            </label>
          </div>

          <div className="form-row">
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
              <input value={state.currency} onChange={setField("currency")} />
            </label>
          </div>

          {state.error ? <p className="form-error">{state.error}</p> : null}

          <div className="form-actions">
            <button
              type="button"
              className="button secondary"
              onClick={onClose}
            >
              Cancel
            </button>
            <button
              type="submit"
              className="button primary"
              disabled={state.submitting}
            >
              {state.submitting ? "Saving..." : "Save instrument"}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
