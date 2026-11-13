// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import {
  initialOpportunitiesPageState,
  loadOpportunitiesPageState,
  opportunitiesPageReducer,
  saveOpportunitiesPageState,
} from "./OpportunitiesPage";

afterEach(() => {
  localStorage.clear();
});

describe("opportunitiesPageReducer", () => {
  it("defaults to one week, best candidates first", () => {
    expect(initialOpportunitiesPageState).toEqual({
      window: "7d",
      order: "best_candidates_first",
    });
  });

  it("changes the window without touching the order", () => {
    const next = opportunitiesPageReducer(initialOpportunitiesPageState, {
      type: "windowChanged",
      window: "30d",
    });

    expect(next).toEqual({ window: "30d", order: "best_candidates_first" });
  });

  it("changes the order without touching the window", () => {
    const next = opportunitiesPageReducer(initialOpportunitiesPageState, {
      type: "orderChanged",
      order: "risers_first",
    });

    expect(next).toEqual({ window: "7d", order: "risers_first" });
  });

  it("returns the same state when the value does not change", () => {
    expect(
      opportunitiesPageReducer(initialOpportunitiesPageState, {
        type: "windowChanged",
        window: "7d",
      }),
    ).toBe(initialOpportunitiesPageState);
    expect(
      opportunitiesPageReducer(initialOpportunitiesPageState, {
        type: "orderChanged",
        order: "best_candidates_first",
      }),
    ).toBe(initialOpportunitiesPageState);
  });
});

describe("opportunities page persistence", () => {
  it("falls back to the defaults when nothing is stored", () => {
    expect(loadOpportunitiesPageState()).toEqual(initialOpportunitiesPageState);
  });

  it("falls back to the defaults for malformed stored values", () => {
    localStorage.setItem("opportunities.window", "{not json");
    localStorage.setItem("opportunities.ordering", "{not json");

    expect(loadOpportunitiesPageState()).toEqual(initialOpportunitiesPageState);
  });

  it("falls back to the defaults for unknown stored values", () => {
    localStorage.setItem("opportunities.window", JSON.stringify("1w"));
    localStorage.setItem("opportunities.ordering", JSON.stringify("desc"));

    expect(loadOpportunitiesPageState()).toEqual(initialOpportunitiesPageState);
  });

  it("saves both settings and loads them back", () => {
    saveOpportunitiesPageState({ window: "14d", order: "risers_first" });

    expect(localStorage.getItem("opportunities.window")).toBe('"14d"');
    expect(localStorage.getItem("opportunities.ordering")).toBe(
      '"risers_first"',
    );
    expect(loadOpportunitiesPageState()).toEqual({
      window: "14d",
      order: "risers_first",
    });
  });
});

describe("opportunities ordering carry-over", () => {
  it.each([
    ['"fallers_first"', "best_candidates_first"],
    ['"risers_first"', "risers_first"],
    [null, "best_candidates_first"],
    ["{malformed", "best_candidates_first"],
    ['"unknown"', "best_candidates_first"],
    ['"best_candidates_first"', "best_candidates_first"],
    ["null", "best_candidates_first"],
    ["42", "best_candidates_first"],
  ])("carries over old value %s as %s once", (saved, expected) => {
    if (saved !== null) localStorage.setItem("opportunities.order", saved);
    expect(loadOpportunitiesPageState().order).toBe(expected);
    expect(localStorage.getItem("opportunities.ordering")).toBe(
      JSON.stringify(expected),
    );
    expect(localStorage.getItem("opportunities.order")).toBeNull();
  });

  it.each(["best_candidates_first", "fallers_first", "risers_first"] as const)(
    "round-trips %s under the new key and ignores the retired key",
    (order) => {
      saveOpportunitiesPageState({ window: "7d", order });
      localStorage.setItem(
        "opportunities.order",
        JSON.stringify(
          order === "risers_first" ? "fallers_first" : "risers_first",
        ),
      );
      expect(loadOpportunitiesPageState().order).toBe(order);
      expect(localStorage.getItem("opportunities.ordering")).toBe(
        JSON.stringify(order),
      );
    },
  );

  it.each(["{malformed", '"unknown"', "null", "42"])(
    "rejects invalid new value %s even when the retired key has risers",
    (saved) => {
      localStorage.setItem("opportunities.ordering", saved);
      localStorage.setItem("opportunities.order", '"risers_first"');
      expect(loadOpportunitiesPageState().order).toBe("best_candidates_first");
    },
  );

  it("keeps the next choice after the one-time switch", () => {
    localStorage.setItem("opportunities.order", '"fallers_first"');
    const initial = loadOpportunitiesPageState();
    const next = opportunitiesPageReducer(initial, {
      type: "orderChanged",
      order: "fallers_first",
    });
    saveOpportunitiesPageState(next);
    expect(loadOpportunitiesPageState().order).toBe("fallers_first");
  });
});
