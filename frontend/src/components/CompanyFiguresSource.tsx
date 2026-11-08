import { useId, useReducer } from "react";
import {
  useCorrectFundamentalsSource,
  usePreviewFundamentalsSource,
} from "../api/queries";
import type { FundamentalsSource } from "../api/types";
import { fundamentalsSourceView } from "./companyFiguresPanelViewModel";
import {
  correctionRequest,
  fundamentalsCorrectionReducer,
  initialCorrectionState,
} from "./fundamentalsCorrectionReducer";

export function CompanyFiguresSource({
  id,
  source,
  canMutate,
}: {
  id: number;
  source: FundamentalsSource;
  canMutate: boolean;
}) {
  const view = fundamentalsSourceView(source);
  return (
    <section
      className="asset-panel-section"
      aria-label="Company figures source"
    >
      <h3>Company figures source</h3>
      <p>
        <span className="number">{view.symbol}</span> · {view.origin} ·{" "}
        {view.securityType}
      </p>
      <dl className="data-list">
        <div className="data-row">
          <dt>Last successful update</dt>
          <dd className="number">{source.last_success_on ?? "Never"}</dd>
        </div>
        <div className="data-row">
          <dt>Last attempt</dt>
          <dd>
            {view.outcome}
            {source.last_attempt_on ? (
              <>
                {" · "}
                <span className="asset-subtle number">
                  {source.last_attempt_on}
                </span>
              </>
            ) : null}
          </dd>
        </div>
      </dl>
      {view.reason ? <p className="asset-subtle">{view.reason}</p> : null}
      {view.candidates.length ? (
        <ul aria-label="Source candidates">
          {view.candidates.map((candidate) => (
            <li key={candidate}>{candidate}</li>
          ))}
        </ul>
      ) : null}
      {source.pending_update ? (
        <p role="status">Figures load on the next refresh</p>
      ) : null}
      {canMutate ? (
        <FundamentalsCorrectionForm
          key={id}
          id={id}
          hasFigures={source.has_active_figures}
        />
      ) : null}
    </section>
  );
}

export function FundamentalsCorrectionForm({
  id,
  hasFigures,
}: {
  id: number;
  hasFigures: boolean;
}) {
  const [state, dispatch] = useReducer(
    fundamentalsCorrectionReducer,
    initialCorrectionState,
  );
  const preview = usePreviewFundamentalsSource(id);
  const save = useCorrectFundamentalsSource(id);
  const fieldId = useId();
  const request = correctionRequest(state, hasFigures);
  async function handlePreview() {
    const symbol = state.symbol;
    dispatch({ type: "preview_requested" });
    try {
      dispatch({
        type: "preview_received",
        symbol,
        preview: await preview.mutateAsync(symbol),
      });
    } catch (error) {
      dispatch({
        type: "preview_failed",
        symbol,
        error: error instanceof Error ? error.message : "Preview failed",
      });
    }
  }
  async function handleSave() {
    if (!request) return;
    dispatch({ type: "save_requested", hasFigures });
    try {
      await save.mutateAsync(request);
      dispatch({ type: "save_succeeded" });
    } catch (error) {
      dispatch({
        type: "save_failed",
        error: error instanceof Error ? error.message : "Save failed",
      });
    }
  }
  return (
    <form
      className="fundamentals-correction"
      aria-label="Correct company figures source"
      onSubmit={(event) => {
        event.preventDefault();
        void handleSave();
      }}
    >
      <label htmlFor={fieldId}>Company symbol (Yahoo)</label>
      <div className="fundamentals-source-actions">
        <input
          id={fieldId}
          value={state.symbol}
          disabled={state.phase === "saving"}
          placeholder="e.g. MSFT"
          onChange={(event) =>
            dispatch({ type: "symbol_changed", symbol: event.target.value })
          }
        />
        <button
          className="button outline"
          type="button"
          disabled={
            state.mode !== "symbol" ||
            !state.symbol.trim() ||
            state.phase === "saving" ||
            state.phase === "previewing"
          }
          onClick={() => void handlePreview()}
        >
          {state.phase === "previewing" ? "Previewing…" : "Preview"}
        </button>
      </div>
      <div className="fundamentals-source-actions">
        <button
          className="button outline"
          type="button"
          disabled={state.phase === "saving"}
          onClick={() =>
            dispatch({ type: "mode_changed", mode: "not_company" })
          }
        >
          Not a company
        </button>
        <button
          className="button outline"
          type="button"
          disabled={state.phase === "saving"}
          onClick={() => dispatch({ type: "mode_changed", mode: "automatic" })}
        >
          Use automatic
        </button>
      </div>
      {state.preview?.status === "found" ? (
        <div role="status">
          <strong>{state.preview.name ?? state.preview.symbol}</strong>
          <p>
            {state.preview.symbol} ·{" "}
            {state.preview.security_type ?? "type unknown"} ·{" "}
            {state.preview.report_currency ?? "report currency unknown"} ·{" "}
            {state.preview.quarters} quarters found
            {state.preview.latest_period_end
              ? ` · latest quarter ending ${state.preview.latest_period_end}`
              : ""}
          </p>
        </div>
      ) : null}
      {state.mode === "not_company" ? (
        <p>Mark this instrument as not a company.</p>
      ) : state.mode === "automatic" ? (
        <p>
          Find the company figures source automatically on the next refresh.
        </p>
      ) : null}
      {state.phase === "ready" && hasFigures ? (
        <fieldset>
          <legend>How should the previous figures be treated?</legend>
          <label>
            <input
              type="radio"
              name={`${fieldId}-previous`}
              checked={state.previous === "same_company"}
              onChange={() =>
                dispatch({ type: "previous_chosen", previous: "same_company" })
              }
            />
            Same company, different listing
          </label>
          <label>
            <input
              type="radio"
              name={`${fieldId}-previous`}
              checked={state.previous === "wrong_company"}
              onChange={() =>
                dispatch({ type: "previous_chosen", previous: "wrong_company" })
              }
            />
            Wrong company — set the old figures aside
          </label>
        </fieldset>
      ) : null}
      {state.phase === "ready" || state.phase === "saving" ? (
        <button className="button primary" type="submit" disabled={!request}>
          {state.phase === "saving" ? "Saving…" : "Save"}
        </button>
      ) : null}
      {state.phase === "saved" ? (
        <p role="status">
          {state.mode === "not_company"
            ? "Source saved. Marked as not a company."
            : "Source saved. Figures load on the next refresh."}
        </p>
      ) : null}
      {state.error ? (
        <p className="down" role="alert">
          {state.error}
        </p>
      ) : null}
    </form>
  );
}
