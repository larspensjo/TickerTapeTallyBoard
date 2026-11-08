import { describe, expect, it } from "vitest";
import { fundamentalsPreview } from "../test/instrumentFundamentalsFixture";
import {
  correctionRequest,
  initialCorrectionState as initial,
  fundamentalsCorrectionReducer as reduce,
} from "./fundamentalsCorrectionReducer";

function ready() {
  const editing = reduce(initial, { type: "symbol_changed", symbol: "MSFT" });
  return reduce(reduce(editing, { type: "preview_requested" }), {
    type: "preview_received",
    symbol: "MSFT",
    preview: fundamentalsPreview(),
  });
}
describe("source correction reducer", () => {
  it("requires preview and a previous-company choice whenever active figures exist", () => {
    expect(correctionRequest(initial, false)).toBeNull();
    const state = ready();
    expect(correctionRequest(state, true)).toBeNull();
    expect(
      reduce(state, { type: "save_requested", hasFigures: true }).phase,
    ).toBe("ready");
    expect(correctionRequest(state, false)).toEqual({
      automatic: false,
      input: { provider: "YAHOO", provider_symbol: "MSFT" },
    });
    for (const previous of ["same_company", "wrong_company"] as const) {
      const chosen = reduce(state, { type: "previous_chosen", previous });
      expect(correctionRequest(chosen, true)?.input.previous_link).toBe(
        previous,
      );
      const saving = reduce(chosen, {
        type: "save_requested",
        hasFigures: true,
      });
      expect(saving.phase).toBe("saving");
      expect(
        reduce(saving, { type: "symbol_changed", symbol: "OTHER" }),
      ).toEqual(saving);
      expect(reduce(saving, { type: "save_succeeded" }).phase).toBe("saved");
      expect(
        reduce(saving, { type: "save_failed", error: "Try again" }),
      ).toMatchObject({ phase: "ready", error: "Try again" });
    }
  });
  it.each(["not_company", "automatic"] as const)(
    "requires the same choice for %s",
    (mode) => {
      const state = reduce(initial, { type: "mode_changed", mode });
      expect(correctionRequest(state, true)).toBeNull();
      const chosen = reduce(state, {
        type: "previous_chosen",
        previous: "wrong_company",
      });
      expect(correctionRequest(chosen, true)).toEqual({
        automatic: mode === "automatic",
        input:
          mode === "automatic"
            ? { previous_link: "wrong_company" }
            : {
                provider: null,
                not_a_company: true,
                previous_link: "wrong_company",
              },
      });
      expect(correctionRequest(state, false)).not.toBeNull();
    },
  );
  it("invalidates old previews and ignores stale asynchronous results", () => {
    const changed = reduce(ready(), { type: "symbol_changed", symbol: "NEW" });
    expect(changed.preview).toBeNull();
    expect(correctionRequest(changed, false)).toBeNull();
    const pending = reduce(changed, { type: "preview_requested" });
    expect(
      reduce(pending, {
        type: "preview_received",
        symbol: "MSFT",
        preview: fundamentalsPreview(),
      }),
    ).toEqual(pending);
    expect(
      reduce(pending, {
        type: "preview_failed",
        symbol: "MSFT",
        error: "Old failure",
      }),
    ).toEqual(pending);
    const failed = reduce(pending, {
      type: "preview_failed",
      symbol: "NEW",
      error: "Network failed",
    });
    expect(failed).toMatchObject({ phase: "editing", error: "Network failed" });
    for (const status of ["no_match", "provider_unavailable"] as const) {
      const noMatch = reduce(pending, {
        type: "preview_received",
        symbol: "NEW",
        preview: { ...fundamentalsPreview(), status },
      });
      expect(noMatch.error).not.toBeNull();
      expect(correctionRequest(noMatch, false)).toBeNull();
    }
    expect(reduce(initial, { type: "preview_requested" })).toEqual(initial);
  });
});
