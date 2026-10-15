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
  it("defaults to one week, biggest fallers first", () => {
    expect(initialOpportunitiesPageState).toEqual({
      window: "7d",
      order: "fallers_first",
    });
  });

  it("changes the window without touching the order", () => {
    const next = opportunitiesPageReducer(initialOpportunitiesPageState, {
      type: "windowChanged",
      window: "30d",
    });

    expect(next).toEqual({ window: "30d", order: "fallers_first" });
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
        order: "fallers_first",
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
    localStorage.setItem("opportunities.order", "{not json");

    expect(loadOpportunitiesPageState()).toEqual(initialOpportunitiesPageState);
  });

  it("falls back to the defaults for unknown stored values", () => {
    localStorage.setItem("opportunities.window", JSON.stringify("1w"));
    localStorage.setItem("opportunities.order", JSON.stringify("desc"));

    expect(loadOpportunitiesPageState()).toEqual(initialOpportunitiesPageState);
  });

  it("saves both settings and loads them back", () => {
    saveOpportunitiesPageState({ window: "14d", order: "risers_first" });

    expect(localStorage.getItem("opportunities.window")).toBe('"14d"');
    expect(localStorage.getItem("opportunities.order")).toBe('"risers_first"');
    expect(loadOpportunitiesPageState()).toEqual({
      window: "14d",
      order: "risers_first",
    });
  });
});
