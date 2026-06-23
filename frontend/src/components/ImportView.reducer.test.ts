import { describe, expect, it } from "vitest";
import type { ImportPreview, ImportResult } from "../api/types";
import {
  INITIAL_STATE,
  importReducer,
  selectedFromPreview,
} from "./ImportView";

function makePreview(
  assets: Array<{
    asset_key: string;
    default_selected: boolean;
    skipped_reason: string | null;
  }> = [],
): ImportPreview {
  return {
    metadata: null,
    counts: {
      rows: 10,
      buys: 5,
      sells: 2,
      splits: 1,
      dividends: 1,
      new_instruments: 0,
      skipped: 1,
      warnings: 0,
      errors: 0,
    },
    assets: assets.map((a) => ({
      ...a,
      name: a.asset_key,
      currency: "USD",
      buys: 1,
      sells: 0,
      splits: 0,
      dividends: 0,
      warnings: [],
      errors: [],
      is_new_instrument: false,
    })),
    new_instruments: [],
    warnings: [],
    errors: [],
    duplicate_of_batch_id: null,
  };
}

const ASSET_A = {
  asset_key: "AAPL",
  default_selected: true,
  skipped_reason: null,
};
const ASSET_B = {
  asset_key: "TSLA",
  default_selected: false,
  skipped_reason: null,
};
const ASSET_SKIP = {
  asset_key: "SKIP",
  default_selected: false,
  skipped_reason: "no_mapping",
};

describe("selectedFromPreview", () => {
  it("maps each asset key to its default_selected value", () => {
    const preview = makePreview([ASSET_A, ASSET_B, ASSET_SKIP]);
    expect(selectedFromPreview(preview)).toEqual({
      AAPL: true,
      TSLA: false,
      SKIP: false,
    });
  });

  it("returns empty record for empty assets", () => {
    expect(selectedFromPreview(makePreview())).toEqual({});
  });
});

describe("importReducer", () => {
  it("resets to idle and clears fields on sourceSelected", () => {
    const withPreview = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview: makePreview([ASSET_A]),
      fileName: "trades.csv",
    });
    const next = importReducer(withPreview, {
      type: "sourceSelected",
      source: "sharesight",
    });
    expect(next.phase).toBe("idle");
    expect(next.source).toBe("sharesight");
    expect(next.fileName).toBeNull();
    expect(next.preview).toBeNull();
    expect(next.result).toBeNull();
    expect(next.error).toBeNull();
    expect(next.selected).toEqual({});
    expect(next.confirmingDuplicate).toBe(false);
  });

  it("moves to previewing on fileSelected", () => {
    const next = importReducer(INITIAL_STATE, {
      type: "fileSelected",
      fileName: "trades.csv",
    });
    expect(next.phase).toBe("previewing");
    expect(next.fileName).toBe("trades.csv");
    expect(next.preview).toBeNull();
    expect(next.result).toBeNull();
    expect(next.error).toBeNull();
    expect(next.selected).toEqual({});
  });

  it("seeds selection from the preview on previewReady", () => {
    const preview = makePreview([ASSET_A, ASSET_B]);
    const next = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview,
      fileName: "trades.csv",
    });
    expect(next.phase).toBe("previewReady");
    expect(next.fileName).toBe("trades.csv");
    expect(next.preview).toBe(preview);
    expect(next.selected).toEqual(selectedFromPreview(preview));
    expect(next.result).toBeNull();
    expect(next.error).toBeNull();
    expect(next.confirmingDuplicate).toBe(false);
  });

  it("sets confirmingDuplicate on confirmDuplicate and clears error", () => {
    const withError = { ...INITIAL_STATE, error: "old" };
    const next = importReducer(withError, { type: "confirmDuplicate" });
    expect(next.confirmingDuplicate).toBe(true);
    expect(next.error).toBeNull();
  });

  it("clears confirmingDuplicate on cancelDuplicate and clears error", () => {
    const confirming = {
      ...INITIAL_STATE,
      confirmingDuplicate: true,
      error: "old",
    };
    const next = importReducer(confirming, { type: "cancelDuplicate" });
    expect(next.confirmingDuplicate).toBe(false);
    expect(next.error).toBeNull();
  });

  it("toggles a selectable asset", () => {
    const preview = makePreview([ASSET_A]);
    const ready = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview,
      fileName: "trades.csv",
    });
    expect(ready.selected.AAPL).toBe(true);

    const toggled = importReducer(ready, {
      type: "toggleAsset",
      assetKey: "AAPL",
    });
    expect(toggled.selected.AAPL).toBe(false);

    const toggledBack = importReducer(toggled, {
      type: "toggleAsset",
      assetKey: "AAPL",
    });
    expect(toggledBack.selected.AAPL).toBe(true);
  });

  it("ignores toggling an unknown asset", () => {
    const preview = makePreview([ASSET_A]);
    const ready = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview,
      fileName: "trades.csv",
    });
    const same = importReducer(ready, {
      type: "toggleAsset",
      assetKey: "does-not-exist",
    });
    expect(same).toEqual(ready);
  });

  it("ignores toggling an asset with a skipped_reason", () => {
    const preview = makePreview([ASSET_SKIP]);
    const ready = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview,
      fileName: "trades.csv",
    });
    const same = importReducer(ready, {
      type: "toggleAsset",
      assetKey: "SKIP",
    });
    expect(same).toEqual(ready);
  });

  it("returns state unchanged if toggleAsset is called with no preview", () => {
    const same = importReducer(INITIAL_STATE, {
      type: "toggleAsset",
      assetKey: "AAPL",
    });
    expect(same).toBe(INITIAL_STATE);
  });

  it("moves to committing on commitStarted", () => {
    const next = importReducer(INITIAL_STATE, { type: "commitStarted" });
    expect(next.phase).toBe("committing");
    expect(next.error).toBeNull();
  });

  it("stores result and moves to committed on committed", () => {
    const result: ImportResult = {
      batch_id: 42,
      counts: {
        rows: 10,
        buys: 5,
        sells: 2,
        splits: 1,
        dividends: 1,
        new_instruments: 0,
        skipped: 1,
        warnings: 0,
        errors: 0,
      },
      warnings: [],
    };
    const next = importReducer(INITIAL_STATE, { type: "committed", result });
    expect(next.phase).toBe("committed");
    expect(next.result).toBe(result);
    expect(next.preview).toBeNull();
    expect(next.error).toBeNull();
    expect(next.selected).toEqual({});
    expect(next.confirmingDuplicate).toBe(false);
  });

  it("moves to error phase on failed", () => {
    const next = importReducer(INITIAL_STATE, {
      type: "failed",
      message: "something went wrong",
    });
    expect(next.phase).toBe("error");
    expect(next.error).toBe("something went wrong");
    expect(next.confirmingDuplicate).toBe(false);
  });

  it("returns to INITIAL_STATE on reset", () => {
    const withPreview = importReducer(INITIAL_STATE, {
      type: "previewReady",
      preview: makePreview([ASSET_A]),
      fileName: "trades.csv",
    });
    const reset = importReducer(withPreview, { type: "reset" });
    expect(reset).toEqual(INITIAL_STATE);
  });
});
