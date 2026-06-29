// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DateRangeSelector,
  dateRangeSelectionReducer,
  loadDateRangeSelection,
  presetToRange,
  saveDateRangeSelection,
} from "./DateRangeSelector";

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.useRealTimers();
});

describe("presetToRange", () => {
  it("derives rolling preset ranges from the supplied date", () => {
    const today = new Date(2026, 5, 29);

    expect(presetToRange("today", "", "", today)).toEqual({
      startDate: "2026-06-29",
      endDate: "2026-06-29",
    });
    expect(presetToRange("7d", "", "", today)).toEqual({
      startDate: "2026-06-22",
      endDate: "2026-06-29",
    });
    expect(presetToRange("12m", "", "", today)).toEqual({
      startDate: "2025-06-29",
      endDate: "2026-06-29",
    });
    expect(presetToRange("ytd", "", "", today)).toEqual({
      startDate: "2026-01-01",
      endDate: "2026-06-29",
    });
  });
});

describe("dateRangeSelectionReducer", () => {
  const initialState = {
    datePreset: "all" as const,
    dateRange: { startDate: null, endDate: null },
  };

  it("updates the selected date preset", () => {
    expect(
      dateRangeSelectionReducer(initialState, {
        type: "datePresetChanged",
        datePreset: "ytd",
      }),
    ).toEqual({ ...initialState, datePreset: "ytd" });
  });

  it("updates the active date range", () => {
    const dateRange = { startDate: "2026-01-01", endDate: "2026-06-29" };

    expect(
      dateRangeSelectionReducer(initialState, {
        type: "dateRangeChanged",
        dateRange,
      }),
    ).toEqual({ ...initialState, dateRange });
  });
});

describe("date range persistence", () => {
  it("loads the default all-time selection when nothing is saved", () => {
    expect(loadDateRangeSelection()).toEqual({
      datePreset: "all",
      dateRange: { startDate: null, endDate: null },
    });
  });

  it("round-trips custom selections through localStorage", () => {
    const selection = {
      datePreset: "custom" as const,
      dateRange: { startDate: "2026-02-01", endDate: "2026-06-29" },
    };

    saveDateRangeSelection(selection);

    expect(loadDateRangeSelection()).toEqual(selection);
  });
});

describe("DateRangeSelector", () => {
  it("emits the selected preset and derived range when a preset is clicked", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 5, 29));
    const onDatePresetChange = vi.fn();
    const onDateRangeChange = vi.fn();

    render(
      <DateRangeSelector
        dateRange={{ startDate: null, endDate: null }}
        selectedDatePreset="all"
        onDatePresetChange={onDatePresetChange}
        onDateRangeChange={onDateRangeChange}
        ariaLabel="Test date range"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "YTD" }));

    expect(onDatePresetChange).toHaveBeenCalledWith("ytd");
    expect(onDateRangeChange).toHaveBeenCalledWith({
      startDate: "2026-01-01",
      endDate: "2026-06-29",
    });
  });
});
