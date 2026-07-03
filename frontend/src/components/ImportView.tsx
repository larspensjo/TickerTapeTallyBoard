import { RefreshCw } from "lucide-react";
import { type ChangeEvent, useReducer, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ApiError } from "../api/client";
import {
  useCommitImport,
  usePreviewImport,
  useRefreshPrices,
  useRollbackImport,
} from "../api/queries";
import type {
  ImportAssetGroup,
  ImportCounts,
  ImportPreview,
  ImportResult,
  ImportRowNote,
  ImportSource,
} from "../api/types";
import { formatGroupedNumber } from "./valuationDisplay";

export type Phase =
  | "idle"
  | "previewing"
  | "previewReady"
  | "committing"
  | "committed"
  | "error";

export interface State {
  phase: Phase;
  source: ImportSource;
  fileName: string | null;
  preview: ImportPreview | null;
  result: ImportResult | null;
  confirmingDuplicate: boolean;
  confirmingAppend: boolean;
  error: string | null;
  selected: Record<string, boolean>;
}

export type Action =
  | { type: "sourceSelected"; source: ImportSource }
  | { type: "fileSelected"; fileName: string }
  | { type: "previewReady"; preview: ImportPreview; fileName: string }
  | { type: "confirmDuplicate" }
  | { type: "cancelDuplicate" }
  | { type: "confirmAppend" }
  | { type: "cancelAppend" }
  | { type: "toggleAsset"; assetKey: string }
  | { type: "setAllAssets"; selected: boolean }
  | { type: "commitStarted" }
  | { type: "committed"; result: ImportResult }
  | { type: "failed"; message: string }
  | { type: "reset" };

export const INITIAL_STATE: State = {
  phase: "idle",
  source: "avanza",
  fileName: null,
  preview: null,
  result: null,
  confirmingDuplicate: false,
  confirmingAppend: false,
  error: null,
  selected: {},
};

export function selectedFromPreview(
  preview: ImportPreview,
): Record<string, boolean> {
  return preview.assets.reduce<Record<string, boolean>>(
    (accumulator, asset) => {
      accumulator[asset.asset_key] = asset.default_selected;
      return accumulator;
    },
    {},
  );
}

export function importReducer(state: State, action: Action): State {
  switch (action.type) {
    case "sourceSelected":
      return {
        ...state,
        source: action.source,
        phase: "idle",
        fileName: null,
        preview: null,
        result: null,
        confirmingDuplicate: false,
        error: null,
        selected: {},
      };
    case "fileSelected":
      return {
        ...state,
        phase: "previewing",
        fileName: action.fileName,
        preview: null,
        result: null,
        confirmingDuplicate: false,
        error: null,
        selected: {},
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
        selected: selectedFromPreview(action.preview),
      };
    case "confirmDuplicate":
      return {
        ...state,
        confirmingDuplicate: true,
        confirmingAppend: false,
        error: null,
      };
    case "cancelDuplicate":
      return { ...state, confirmingDuplicate: false, error: null };
    case "confirmAppend":
      return {
        ...state,
        confirmingAppend: true,
        confirmingDuplicate: false,
        error: null,
      };
    case "cancelAppend":
      return { ...state, confirmingAppend: false, error: null };
    case "toggleAsset": {
      if (!state.preview) {
        return state;
      }

      const asset = state.preview.assets.find(
        (candidate) => candidate.asset_key === action.assetKey,
      );

      if (!asset || asset.skipped_reason) {
        return state;
      }

      const current = state.selected[action.assetKey] ?? asset.default_selected;

      return {
        ...state,
        selected: {
          ...state.selected,
          [action.assetKey]: !current,
        },
      };
    }
    case "setAllAssets": {
      if (!state.preview) {
        return state;
      }

      const selected = { ...state.selected };
      for (const asset of state.preview.assets) {
        if (asset.skipped_reason) {
          continue;
        }
        selected[asset.asset_key] = action.selected;
      }

      return { ...state, selected };
    }
    case "commitStarted":
      return { ...state, phase: "committing", error: null };
    case "committed":
      return {
        ...state,
        phase: "committed",
        preview: null,
        result: action.result,
        confirmingDuplicate: false,
        confirmingAppend: false,
        error: null,
        selected: {},
      };
    case "failed":
      return {
        ...state,
        phase: "error",
        error: action.message,
        confirmingDuplicate: false,
        confirmingAppend: false,
      };
    case "reset":
      return INITIAL_STATE;
  }

  return state;
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

function sourceLabel(source: ImportSource) {
  return source === "avanza" ? "Avanza" : "Sharesight";
}

function isAssetSelected(
  asset: ImportAssetGroup,
  selected: Record<string, boolean>,
) {
  return selected[asset.asset_key] ?? asset.default_selected;
}

/**
 * True when every selectable (non-locked) asset in the preview is selected.
 * Returns false when there are no selectable assets, so the header checkbox
 * stays unchecked and disabled in that case.
 */
export function allSelectableSelected(
  preview: ImportPreview,
  selected: Record<string, boolean>,
): boolean {
  const selectable = preview.assets.filter((asset) => !asset.skipped_reason);
  if (selectable.length === 0) {
    return false;
  }

  return selectable.every((asset) => isAssetSelected(asset, selected));
}

function noteFingerprint(note: ImportRowNote): string {
  return `${note.row ?? "none"}:${note.code}:${note.message}`;
}

function hasBlockingErrors(
  preview: ImportPreview | null,
  selected: Record<string, boolean>,
): boolean {
  if (!preview) {
    return false;
  }

  const selectedHasErrors = preview.assets.some(
    (asset) => isAssetSelected(asset, selected) && asset.errors.length > 0,
  );
  const assetErrorKeys = new Set(
    preview.assets.flatMap((asset) => asset.errors.map(noteFingerprint)),
  );
  const hasGlobalErrors = preview.errors.some(
    (note) => !assetErrorKeys.has(noteFingerprint(note)),
  );

  return selectedHasErrors || hasGlobalErrors;
}

export function ImportView() {
  const navigate = useNavigate();
  const [state, dispatch] = useReducer(importReducer, INITIAL_STATE);
  const [fileBytes, setFileBytes] = useState<ArrayBuffer | null>(null);
  const [showAlreadyImported, setShowAlreadyImported] = useState(false);
  const previewImport = usePreviewImport();
  const commitImport = useCommitImport();
  const rollbackImport = useRollbackImport();

  async function previewFile(
    source: ImportSource,
    fileName: string,
    bytes: ArrayBuffer,
  ) {
    const preview = await previewImport.mutateAsync({
      source,
      file: bytes.slice(0),
    });
    dispatch({ type: "previewReady", preview, fileName });
  }

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
      await previewFile(state.source, file.name, bytes);
    } catch (error) {
      dispatch({
        type: "failed",
        message: formatError(error, "Preview failed."),
      });
    }
  }

  async function onSourceChange(source: ImportSource) {
    if (state.source === source) {
      return;
    }

    dispatch({ type: "sourceSelected", source });
    setFileBytes(null);
  }

  async function onCommit(allowDuplicate: boolean, refresh = false) {
    if (!fileBytes || !state.preview) {
      return;
    }

    dispatch({ type: "commitStarted" });

    const exclude = state.preview.assets
      .filter((asset) => !isAssetSelected(asset, state.selected))
      .map((asset) => asset.asset_key);

    const replaceBatchId =
      state.preview.replace_candidate_batch_id ?? undefined;

    try {
      const result = await commitImport.mutateAsync({
        source: state.source,
        file: fileBytes.slice(0),
        allowDuplicate,
        exclude,
        ...(refresh && replaceBatchId !== undefined
          ? { mode: "replace" as const, replaceBatchId }
          : {}),
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
  const isRefreshMode =
    state.source === "avanza" && preview?.replace_candidate_batch_id != null;
  const isDuplicate = preview?.duplicate_of_batch_id != null && !isRefreshMode;
  const commitBlockedByErrors = hasBlockingErrors(preview, state.selected);
  const noWritableAssets =
    preview !== null &&
    preview.assets.length === 0 &&
    preview.already_imported_assets.length > 0;
  const hasSelectableAssets =
    preview?.assets.some((asset) => !asset.skipped_reason) ?? false;
  const allAssetsSelected =
    preview !== null && allSelectableSelected(preview, state.selected);
  const isBusy =
    state.phase === "previewing" ||
    state.phase === "committing" ||
    previewImport.isPending ||
    commitImport.isPending ||
    rollbackImport.isPending;

  return (
    <>
      <section className="panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">{sourceLabel(state.source)}</p>
            <h1>Import All Trades CSV</h1>
          </div>

          <fieldset className="segmented-control" aria-label="Import source">
            <legend className="sr-only">Import source</legend>
            <button
              className={state.source === "sharesight" ? "active" : undefined}
              type="button"
              aria-pressed={state.source === "sharesight"}
              disabled={isBusy}
              onClick={() => {
                void onSourceChange("sharesight");
              }}
            >
              Sharesight
            </button>
            <button
              className={state.source === "avanza" ? "active" : undefined}
              type="button"
              aria-pressed={state.source === "avanza"}
              disabled={isBusy}
              onClick={() => {
                void onSourceChange("avanza");
              }}
            >
              Avanza
            </button>
          </fieldset>
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
              <section className="board-state muted import-summary">
                <p className="total-value">
                  {formatGroupedNumber(preview.counts.rows)} trades
                </p>
                <ImportCountsMetrics counts={preview.counts} />
                {preview.metadata ? (
                  <span className="status-chip">{preview.metadata.title}</span>
                ) : null}
                {isRefreshMode ? (
                  <span className="status-chip">
                    Will refresh Avanza batch{" "}
                    {preview.replace_candidate_batch_id}
                  </span>
                ) : null}
                {isDuplicate ? (
                  <span className="status-chip warning">
                    Already imported as batch {preview.duplicate_of_batch_id}
                  </span>
                ) : null}
                {preview.replace_candidate_warning ? (
                  <span className="status-chip warning">
                    {preview.replace_candidate_warning}
                  </span>
                ) : null}
              </section>

              {preview.assets.length > 0 ? (
                <>
                  <p className="eyebrow">Assets</p>
                  {isRefreshMode ? (
                    <p className="form-note muted">
                      Unchecked assets will be removed from the refreshed batch.
                    </p>
                  ) : null}
                  <div className="table-wrap asset-table">
                    <table>
                      <thead>
                        <tr>
                          <th className="checkbox-head">
                            <input
                              type="checkbox"
                              className="asset-check"
                              checked={allAssetsSelected}
                              disabled={!hasSelectableAssets || isBusy}
                              aria-label="Select all assets"
                              onChange={() => {
                                dispatch({
                                  type: "setAllAssets",
                                  selected: !allAssetsSelected,
                                });
                              }}
                            />
                          </th>
                          <th>Asset</th>
                          <th>Currency</th>
                          <th>Buys</th>
                          <th>Sells</th>
                          <th>Splits</th>
                          <th>Dividends</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.assets.map((asset) => {
                          const checked = isAssetSelected(
                            asset,
                            state.selected,
                          );
                          const locked = asset.skipped_reason != null;

                          return (
                            <tr key={asset.asset_key}>
                              <td className="checkbox-cell">
                                <input
                                  type="checkbox"
                                  className="asset-check"
                                  checked={checked}
                                  disabled={locked || isBusy}
                                  aria-label={`Include ${asset.name}`}
                                  onChange={() => {
                                    dispatch({
                                      type: "toggleAsset",
                                      assetKey: asset.asset_key,
                                    });
                                  }}
                                />
                              </td>
                              <td>
                                <div className="asset-name-cell">
                                  <strong>{asset.name}</strong>
                                  <div className="asset-meta-line">
                                    <span>{asset.asset_key}</span>
                                    <div className="asset-badges">
                                      {asset.is_new_instrument ? (
                                        <span className="asset-badge">
                                          New instrument
                                        </span>
                                      ) : null}
                                      {asset.skipped_reason ? (
                                        <span className="asset-badge warning">
                                          {asset.skipped_reason}
                                        </span>
                                      ) : null}
                                      {asset.errors.length > 0 ? (
                                        <span className="asset-badge warning">
                                          {formatGroupedNumber(
                                            asset.errors.length,
                                          )}{" "}
                                          error
                                          {asset.errors.length === 1 ? "" : "s"}
                                        </span>
                                      ) : null}
                                    </div>
                                  </div>
                                </div>
                              </td>
                              <td>{asset.currency}</td>
                              <td className="number">
                                {formatGroupedNumber(asset.buys)}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(asset.sells)}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(asset.splits)}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(asset.dividends)}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </>
              ) : null}

              {preview.already_imported_assets.length > 0 ? (
                <>
                  <button
                    type="button"
                    className="button secondary"
                    onClick={() => setShowAlreadyImported((v) => !v)}
                  >
                    {showAlreadyImported ? "Hide" : "Show"} already imported (
                    {preview.already_imported_assets.length} asset
                    {preview.already_imported_assets.length === 1 ? "" : "s"})
                  </button>
                  {showAlreadyImported ? (
                    <div className="table-wrap asset-table">
                      <table>
                        <thead>
                          <tr>
                            <th className="checkbox-head">
                              <span className="sr-only">Select</span>
                            </th>
                            <th>Asset</th>
                            <th>Currency</th>
                            <th>Buys</th>
                            <th>Sells</th>
                            <th>Splits</th>
                            <th>Dividends</th>
                          </tr>
                        </thead>
                        <tbody>
                          {preview.already_imported_assets.map((asset) => (
                            <tr key={asset.asset_key}>
                              <td className="checkbox-cell">
                                <input
                                  type="checkbox"
                                  className="asset-check"
                                  checked={false}
                                  disabled
                                  aria-label={`Include ${asset.name}`}
                                  onChange={() => undefined}
                                />
                              </td>
                              <td>
                                <div className="asset-name-cell">
                                  <strong>{asset.name}</strong>
                                  <div className="asset-meta-line">
                                    <span>{asset.asset_key}</span>
                                  </div>
                                </div>
                              </td>
                              <td>{asset.currency}</td>
                              <td className="number">
                                {formatGroupedNumber(
                                  asset.already_imported_buys,
                                )}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(
                                  asset.already_imported_sells,
                                )}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(
                                  asset.already_imported_splits,
                                )}
                              </td>
                              <td className="number">
                                {formatGroupedNumber(
                                  asset.already_imported_dividends,
                                )}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  ) : null}
                </>
              ) : null}

              {preview.errors.length > 0 ? (
                <ImportNoteSection title="Errors" notes={preview.errors} />
              ) : null}

              {preview.warnings.length > 0 ? (
                <ImportNoteSection title="Warnings" notes={preview.warnings} />
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
                          <th>ISIN</th>
                          <th>Name</th>
                          <th>Currency</th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.new_instruments.map((instrument) => (
                          <tr
                            key={`${instrument.exchange}-${instrument.symbol}`}
                          >
                            <td>{instrument.exchange}</td>
                            <td>{instrument.symbol}</td>
                            <td>{instrument.isin ?? "-"}</td>
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
                  second batch. Click "Import anyway" to confirm.
                </p>
              ) : null}

              {isRefreshMode && state.confirmingAppend ? (
                <p className="form-error">
                  This will append a new Avanza batch alongside the existing
                  batch {preview.replace_candidate_batch_id}. The existing batch
                  will not be modified.
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
                {isRefreshMode ? (
                  state.confirmingAppend ? (
                    <>
                      <button
                        type="button"
                        className="button secondary"
                        disabled={isBusy}
                        onClick={() => {
                          dispatch({ type: "cancelAppend" });
                        }}
                      >
                        Back
                      </button>
                      <button
                        type="button"
                        className="button outline danger"
                        disabled={
                          commitBlockedByErrors || isBusy || noWritableAssets
                        }
                        onClick={() => {
                          void onCommit(
                            preview.duplicate_of_batch_id != null,
                            false,
                          );
                        }}
                      >
                        Confirm append
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        type="button"
                        className="button secondary"
                        disabled={
                          commitBlockedByErrors || isBusy || noWritableAssets
                        }
                        onClick={() => {
                          dispatch({ type: "confirmAppend" });
                        }}
                      >
                        Append as new batch…
                      </button>
                      <button
                        type="button"
                        className="button primary"
                        disabled={commitBlockedByErrors || isBusy}
                        onClick={() => {
                          void onCommit(false, true);
                        }}
                      >
                        Update Avanza import
                      </button>
                    </>
                  )
                ) : isDuplicate && !state.confirmingDuplicate ? (
                  <button
                    type="button"
                    className="button outline danger"
                    disabled={commitBlockedByErrors || isBusy}
                    onClick={() => {
                      dispatch({ type: "confirmDuplicate" });
                    }}
                  >
                    Import anyway...
                  </button>
                ) : (
                  <button
                    type="button"
                    className="button primary"
                    disabled={
                      commitBlockedByErrors || isBusy || noWritableAssets
                    }
                    title={
                      noWritableAssets
                        ? "All rows are already imported — nothing to commit"
                        : undefined
                    }
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
            <>
              <div className="board-state">
                <p className="total-value">
                  Imported batch {formatGroupedNumber(state.result.batch_id)}
                </p>
                <ImportCountsMetrics counts={state.result.counts} showRows />
                <div className="form-actions">
                  <button
                    type="button"
                    className="button primary"
                    onClick={() => navigate("/transactions")}
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
              {state.result.warnings.length > 0 ? (
                <ImportNoteSection
                  title="Warnings"
                  notes={state.result.warnings}
                />
              ) : null}
            </>
          ) : null}
        </div>
      </section>

      <BackfillPanel />
    </>
  );
}

function BackfillPanel() {
  const refreshPrices = useRefreshPrices();
  const isBackfilling = refreshPrices.isPending;
  const result = refreshPrices.data;

  return (
    <section className="panel">
      <div className="panel-header">
        <div>
          <p className="eyebrow">Maintenance</p>
          <h2>Price history</h2>
        </div>
      </div>
      <div className="transaction-form">
        <p className="muted">
          Fetches daily prices and FX from your earliest transaction up to
          today. Normally only needed once, after setting up your portfolio.
        </p>
        <div className="form-actions">
          <button
            type="button"
            className="button secondary"
            disabled={isBackfilling}
            onClick={() => {
              void refreshPrices.mutateAsync({ mode: "backfill" });
            }}
          >
            <RefreshCw
              aria-hidden="true"
              className={isBackfilling ? "spin" : undefined}
              size={16}
            />
            <span>Backfill full price history</span>
          </button>
        </div>
        {refreshPrices.isError ? (
          <p className="form-error">
            {refreshPrices.error instanceof ApiError
              ? refreshPrices.error.message
              : "Backfill failed. Please try again."}
          </p>
        ) : null}
        {result ? (
          <p className="muted">
            Backfill {result.status}: wrote{" "}
            <strong className="number">
              {formatGroupedNumber(result.prices_written)}
            </strong>{" "}
            price rows and{" "}
            <strong className="number">
              {formatGroupedNumber(result.fx_rates_written)}
            </strong>{" "}
            FX rates.
          </p>
        ) : null}
      </div>
    </section>
  );
}

function ImportCountsMetrics({
  counts,
  showRows = false,
}: {
  counts: ImportCounts;
  showRows?: boolean;
}) {
  return (
    <div className="summary-metrics">
      {showRows ? (
        <span>
          Rows{" "}
          <strong className="number">{formatGroupedNumber(counts.rows)}</strong>
        </span>
      ) : null}
      <span>
        Buys{" "}
        <strong className="number">{formatGroupedNumber(counts.buys)}</strong>
      </span>
      <span>
        Sells{" "}
        <strong className="number">{formatGroupedNumber(counts.sells)}</strong>
      </span>
      <span>
        Splits{" "}
        <strong className="number">{formatGroupedNumber(counts.splits)}</strong>
      </span>
      <span>
        Dividends{" "}
        <strong className="number">
          {formatGroupedNumber(counts.dividends)}
        </strong>
      </span>
      <span>
        New instruments{" "}
        <strong className="number">
          {formatGroupedNumber(counts.new_instruments)}
        </strong>
      </span>
      <span>
        Skipped{" "}
        <strong className="number">
          {formatGroupedNumber(counts.skipped)}
        </strong>
      </span>
      <span>
        Warnings{" "}
        <strong className="number">
          {formatGroupedNumber(counts.warnings)}
        </strong>
      </span>
      <span>
        Errors{" "}
        <strong className="number">{formatGroupedNumber(counts.errors)}</strong>
      </span>
    </div>
  );
}

function ImportNoteSection({
  title,
  notes,
}: {
  title: string;
  notes: ImportRowNote[];
}) {
  return (
    <>
      <p className="eyebrow">{title}</p>
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
            {notes.map((note, index) => (
              <tr key={noteKey(note, index)}>
                <td>
                  {note.row == null ? "-" : formatGroupedNumber(note.row)}
                </td>
                <td>{note.code}</td>
                <td>{note.message}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  );
}
