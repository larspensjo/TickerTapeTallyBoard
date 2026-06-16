import { type ChangeEvent, useReducer, useState } from "react";
import { ApiError } from "../api/client";
import {
  useCommitImport,
  usePreviewImport,
  useRollbackImport,
} from "../api/queries";
import type { ImportPreview, ImportResult, ImportRowNote } from "../api/types";

type Phase =
  | "idle"
  | "previewing"
  | "previewReady"
  | "committing"
  | "committed"
  | "error";

interface State {
  phase: Phase;
  fileName: string | null;
  preview: ImportPreview | null;
  result: ImportResult | null;
  confirmingDuplicate: boolean;
  error: string | null;
}

type Action =
  | { type: "fileSelected"; fileName: string }
  | { type: "previewReady"; preview: ImportPreview; fileName: string }
  | { type: "confirmDuplicate" }
  | { type: "cancelDuplicate" }
  | { type: "commitStarted" }
  | { type: "committed"; result: ImportResult }
  | { type: "failed"; message: string }
  | { type: "reset" };

const INITIAL_STATE: State = {
  phase: "idle",
  fileName: null,
  preview: null,
  result: null,
  confirmingDuplicate: false,
  error: null,
};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "fileSelected":
      return {
        ...state,
        phase: "previewing",
        fileName: action.fileName,
        preview: null,
        result: null,
        confirmingDuplicate: false,
        error: null,
      };
    case "previewReady":
      return {
        ...state,
        phase: "previewReady",
        fileName: action.fileName,
        preview: action.preview,
        result: null,
        confirmingDuplicate: false,
        error: null,
      };
    case "confirmDuplicate":
      return { ...state, confirmingDuplicate: true, error: null };
    case "cancelDuplicate":
      return { ...state, confirmingDuplicate: false, error: null };
    case "commitStarted":
      return { ...state, phase: "committing", error: null };
    case "committed":
      return {
        ...state,
        phase: "committed",
        preview: null,
        result: action.result,
        confirmingDuplicate: false,
        error: null,
      };
    case "failed":
      return {
        ...state,
        phase: "error",
        error: action.message,
        confirmingDuplicate: false,
      };
    case "reset":
      return INITIAL_STATE;
  }
}

function formatError(error: unknown, fallback: string): string {
  if (error instanceof ApiError) {
    return `${error.code}: ${error.message}`;
  }

  if (error instanceof Error) {
    return error.message;
  }

  return fallback;
}

function noteKey(note: ImportRowNote, index: number) {
  return `${note.code}-${note.row ?? "none"}-${index}`;
}

interface ImportViewProps {
  onViewTransactions: () => void;
}

export function ImportView({ onViewTransactions }: ImportViewProps) {
  const [state, dispatch] = useReducer(reducer, INITIAL_STATE);
  const [fileBytes, setFileBytes] = useState<ArrayBuffer | null>(null);
  const previewImport = usePreviewImport();
  const commitImport = useCommitImport();
  const rollbackImport = useRollbackImport();

  async function onFileChange(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";

    if (!file) {
      return;
    }

    dispatch({ type: "fileSelected", fileName: file.name });

    try {
      const bytes = await file.arrayBuffer();
      setFileBytes(bytes);
      const preview = await previewImport.mutateAsync(bytes.slice(0));
      dispatch({ type: "previewReady", preview, fileName: file.name });
    } catch (error) {
      dispatch({
        type: "failed",
        message: formatError(error, "Preview failed."),
      });
    }
  }

  async function onCommit(allowDuplicate: boolean) {
    if (!fileBytes) {
      return;
    }

    dispatch({ type: "commitStarted" });

    try {
      const result = await commitImport.mutateAsync({
        file: fileBytes.slice(0),
        allowDuplicate,
      });
      dispatch({ type: "committed", result });
    } catch (error) {
      dispatch({
        type: "failed",
        message: formatError(error, "Commit failed."),
      });
    }
  }

  async function onRollback(batchId: number) {
    try {
      await rollbackImport.mutateAsync(batchId);
      dispatch({ type: "reset" });
      setFileBytes(null);
    } catch (error) {
      dispatch({
        type: "failed",
        message: formatError(error, "Undo failed."),
      });
    }
  }

  const preview = state.preview;
  const isDuplicate = preview?.duplicate_of_batch_id != null;
  const hasErrors = (preview?.counts.errors ?? 0) > 0;
  const isBusy =
    state.phase === "previewing" ||
    state.phase === "committing" ||
    previewImport.isPending ||
    commitImport.isPending ||
    rollbackImport.isPending;

  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Sharesight</p>
          <h1>Import All Trades CSV</h1>
        </div>
      </div>

      <div className="transaction-form">
        <div className="form-row">
          <label className="form-field grow">
            <span>CSV file</span>
            <input
              type="file"
              accept=".csv,text/csv"
              onChange={(event) => {
                void onFileChange(event);
              }}
              disabled={isBusy}
            />
          </label>
          <div className="form-field">
            <span>Selected</span>
            <div className="status-chip">
              {state.fileName ?? "No file selected"}
            </div>
          </div>
        </div>

        {state.phase === "previewing" ? (
          <div className="board-state muted">
            <div className="skeleton-bar" />
            <div className="skeleton-bar" />
            <div className="skeleton-bar" />
          </div>
        ) : null}

        {preview && state.phase !== "previewing" ? (
          <>
            <section className="board-state muted">
              <p className="total-value">{preview.counts.rows} trades</p>
              <div className="summary-metrics">
                <span>
                  Buys <strong className="number">{preview.counts.buys}</strong>
                </span>
                <span>
                  Sells{" "}
                  <strong className="number">{preview.counts.sells}</strong>
                </span>
                <span>
                  Splits{" "}
                  <strong className="number">{preview.counts.splits}</strong>
                </span>
                <span>
                  New instruments{" "}
                  <strong className="number">
                    {preview.counts.new_instruments}
                  </strong>
                </span>
                <span>
                  Warnings{" "}
                  <strong className="number">{preview.counts.warnings}</strong>
                </span>
                <span>
                  Errors{" "}
                  <strong className="number">{preview.counts.errors}</strong>
                </span>
              </div>
              {preview.metadata ? (
                <span className="status-chip">{preview.metadata.title}</span>
              ) : null}
              {isDuplicate ? (
                <span className="status-chip warning">
                  Already imported as batch {preview.duplicate_of_batch_id}
                </span>
              ) : null}
            </section>

            {preview.errors.length > 0 ? (
              <>
                <p className="eyebrow">Errors</p>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>Row</th>
                        <th>Code</th>
                        <th>Message</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.errors.map((note, index) => (
                        <tr key={noteKey(note, index)}>
                          <td>{note.row ?? "-"}</td>
                          <td>{note.code}</td>
                          <td>{note.message}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            ) : null}

            {preview.warnings.length > 0 ? (
              <>
                <p className="eyebrow">Warnings</p>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>Row</th>
                        <th>Code</th>
                        <th>Message</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.warnings.map((note, index) => (
                        <tr key={noteKey(note, index)}>
                          <td>{note.row ?? "-"}</td>
                          <td>{note.code}</td>
                          <td>{note.message}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            ) : null}

            {preview.new_instruments.length > 0 ? (
              <>
                <p className="eyebrow">New instruments</p>
                <div className="table-wrap">
                  <table>
                    <thead>
                      <tr>
                        <th>Exchange</th>
                        <th>Symbol</th>
                        <th>Name</th>
                        <th>Currency</th>
                      </tr>
                    </thead>
                    <tbody>
                      {preview.new_instruments.map((instrument) => (
                        <tr key={`${instrument.exchange}-${instrument.symbol}`}>
                          <td>{instrument.exchange}</td>
                          <td>{instrument.symbol}</td>
                          <td>{instrument.name}</td>
                          <td>{instrument.currency}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </>
            ) : null}

            {state.error ? <p className="form-error">{state.error}</p> : null}

            {isDuplicate && state.confirmingDuplicate ? (
              <p className="form-error">
                This file was already imported as batch{" "}
                {preview.duplicate_of_batch_id}. Importing again will create a
                second batch. Click “Import anyway” to confirm.
              </p>
            ) : null}

            <div className="form-actions">
              <button
                type="button"
                className="button secondary"
                onClick={() => {
                  dispatch({ type: "reset" });
                  setFileBytes(null);
                }}
                disabled={isBusy}
              >
                Cancel
              </button>
              {isDuplicate && !state.confirmingDuplicate ? (
                <button
                  type="button"
                  className="button outline danger"
                  disabled={hasErrors || isBusy}
                  onClick={() => dispatch({ type: "confirmDuplicate" })}
                >
                  Import anyway...
                </button>
              ) : (
                <button
                  type="button"
                  className="button primary"
                  disabled={hasErrors || isBusy}
                  onClick={() => {
                    void onCommit(isDuplicate);
                  }}
                >
                  {isDuplicate ? "Import anyway" : "Commit import"}
                </button>
              )}
            </div>
          </>
        ) : null}

        {state.phase === "error" && !preview ? (
          <>
            <p className="form-error">{state.error}</p>
            <div className="form-actions">
              <button
                type="button"
                className="button secondary"
                onClick={() => {
                  dispatch({ type: "reset" });
                  setFileBytes(null);
                }}
              >
                Choose another CSV
              </button>
            </div>
          </>
        ) : null}

        {state.phase === "committed" && state.result ? (
          <div className="board-state">
            <p className="total-value">
              Imported batch {state.result.batch_id}
            </p>
            <div className="summary-metrics">
              <span>
                Rows{" "}
                <strong className="number">{state.result.counts.rows}</strong>
              </span>
              <span>
                Buys{" "}
                <strong className="number">{state.result.counts.buys}</strong>
              </span>
              <span>
                Sells{" "}
                <strong className="number">{state.result.counts.sells}</strong>
              </span>
              <span>
                Splits{" "}
                <strong className="number">{state.result.counts.splits}</strong>
              </span>
            </div>
            <div className="form-actions">
              <button
                type="button"
                className="button primary"
                onClick={onViewTransactions}
                disabled={rollbackImport.isPending}
              >
                View transactions
              </button>
              <button
                type="button"
                className="button secondary"
                onClick={() => {
                  dispatch({ type: "reset" });
                  setFileBytes(null);
                }}
                disabled={rollbackImport.isPending}
              >
                Import another file
              </button>
              <button
                type="button"
                className="button outline danger"
                onClick={() => {
                  void onRollback(state.result?.batch_id ?? 0);
                }}
                disabled={rollbackImport.isPending}
              >
                Undo this import
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </section>
  );
}
