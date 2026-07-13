// @vitest-environment jsdom

import type { SortingState } from "@tanstack/react-table";
import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  isBoolean,
  isOneOf,
  isSortingStateFor,
  loadSetting,
  saveSetting,
  usePersistentSetting,
  usePersistentSorting,
} from "./persistence";

const KEY = "test.setting";

afterEach(() => {
  localStorage.clear();
});

describe("loadSetting", () => {
  it("returns the fallback when the key is absent", () => {
    expect(loadSetting(KEY, isBoolean, true)).toBe(true);
  });

  it("returns the fallback when the stored value is malformed JSON", () => {
    localStorage.setItem(KEY, "{not json");
    expect(loadSetting(KEY, isBoolean, false)).toBe(false);
  });

  it("returns the fallback when the stored value fails validation", () => {
    localStorage.setItem(KEY, JSON.stringify("nope"));
    expect(loadSetting(KEY, isBoolean, false)).toBe(false);
  });

  it("returns the stored value when it is valid", () => {
    localStorage.setItem(KEY, JSON.stringify(true));
    expect(loadSetting(KEY, isBoolean, false)).toBe(true);
  });
});

describe("saveSetting", () => {
  it("round-trips a valid value", () => {
    saveSetting(KEY, true, isBoolean);
    expect(loadSetting(KEY, isBoolean, false)).toBe(true);
  });

  it("does not write a value that fails validation", () => {
    saveSetting(KEY, "invalid" as unknown as boolean, isBoolean);
    expect(localStorage.getItem(KEY)).toBeNull();
  });
});

describe("isOneOf", () => {
  const isColor = isOneOf(["red", "green"] as const);

  it("accepts allowed members and rejects everything else", () => {
    expect(isColor("red")).toBe(true);
    expect(isColor("green")).toBe(true);
    expect(isColor("blue")).toBe(false);
    expect(isColor(1)).toBe(false);
    expect(isColor(null)).toBe(false);
  });
});

describe("isSortingStateFor", () => {
  const isValid = isSortingStateFor(new Set(["a", "b"]));

  it("accepts a valid sorting state", () => {
    expect(isValid([{ id: "a", desc: true }])).toBe(true);
    expect(isValid([])).toBe(true);
  });

  it("rejects unknown, duplicate, and malformed columns", () => {
    expect(isValid([{ id: "c", desc: true }])).toBe(false);
    expect(
      isValid([
        { id: "a", desc: true },
        { id: "a", desc: false },
      ]),
    ).toBe(false);
    expect(isValid([{ id: "a" }])).toBe(false);
    expect(isValid([{ id: "a", desc: "yes" }])).toBe(false);
    expect(isValid("a")).toBe(false);
  });
});

describe("usePersistentSetting", () => {
  it("loads the initial value from storage and writes updates back", () => {
    localStorage.setItem(KEY, JSON.stringify("green"));
    const isColor = isOneOf(["red", "green"] as const);

    const { result } = renderHook(() =>
      usePersistentSetting(KEY, isColor, "red"),
    );

    expect(result.current[0]).toBe("green");

    act(() => result.current[1]("red"));

    expect(result.current[0]).toBe("red");
    expect(localStorage.getItem(KEY)).toBe(JSON.stringify("red"));
  });
});

describe("usePersistentSorting", () => {
  const SORTABLE = new Set(["a", "b"]);
  const DEFAULT: SortingState = [{ id: "a", desc: true }];

  it("falls back to the default when nothing valid is stored", () => {
    localStorage.setItem(KEY, JSON.stringify([{ id: "retired", desc: true }]));

    const { result } = renderHook(() =>
      usePersistentSorting(KEY, SORTABLE, DEFAULT),
    );

    expect(result.current[0]).toEqual(DEFAULT);
  });

  it("persists both direct and updater-form changes", () => {
    const { result } = renderHook(() =>
      usePersistentSorting(KEY, SORTABLE, DEFAULT),
    );

    act(() => result.current[1]([{ id: "b", desc: false }]));
    expect(result.current[0]).toEqual([{ id: "b", desc: false }]);
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual([
      { id: "b", desc: false },
    ]);

    act(() =>
      result.current[1]((current) => [...current, { id: "a", desc: true }]),
    );
    expect(JSON.parse(localStorage.getItem(KEY) ?? "null")).toEqual([
      { id: "b", desc: false },
      { id: "a", desc: true },
    ]);
  });
});
