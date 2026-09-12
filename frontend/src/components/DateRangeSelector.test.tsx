// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DateRangeSelector,
  dateRangeSelectionReducer,
  loadDateRangeSelection,
  saveDateRangeSelection,
} from "./DateRangeSelector";

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("dateRangeSelectionReducer", () => {
  const initialState = {
    datePreset: "all" as const,
    customRange: { startDate: null, endDate: null },
  };

  it("updates the selected date preset", () => {
    expect(
      dateRangeSelectionReducer(initialState, {
        type: "datePresetChanged",
        datePreset: "ytd",
      }),
    ).toEqual({ ...initialState, datePreset: "ytd" });
  });

  it("updates the custom range", () => {
    const dateRange = { startDate: "2026-01-01", endDate: "2026-06-29" };

    expect(
      dateRangeSelectionReducer(initialState, {
        type: "dateRangeChanged",
        dateRange,
      }),
    ).toEqual({ ...initialState, customRange: dateRange });
  });
});

describe("date range persistence", () => {
  it("loads the default all-time selection when nothing is saved", () => {
    expect(loadDateRangeSelection()).toEqual({
      datePreset: "all",
      customRange: { startDate: null, endDate: null },
    });
  });

  it("round-trips custom selections through localStorage", () => {
    const selection = {
      datePreset: "custom" as const,
      customRange: { startDate: "2026-02-01", endDate: "2026-06-29" },
    };

    saveDateRangeSelection(selection);

    expect(loadDateRangeSelection()).toEqual(selection);
  });

  it("loads a custom selection saved in the previous shape", () => {
    localStorage.setItem(
      "portfolio.dateRangeSelection",
      JSON.stringify({
        datePreset: "custom",
        dateRange: { startDate: "2026-02-01", endDate: "2026-06-29" },
      }),
    );

    expect(loadDateRangeSelection()).toEqual({
      datePreset: "custom",
      customRange: { startDate: "2026-02-01", endDate: "2026-06-29" },
    });
  });

  it("rejects an unknown saved preset", () => {
    localStorage.setItem(
      "portfolio.dateRangeSelection",
      JSON.stringify({ datePreset: "last-tuesday", customRange: {} }),
    );

    expect(loadDateRangeSelection()).toEqual({
      datePreset: "all",
      customRange: { startDate: null, endDate: null },
    });
  });
});

describe("DateRangeSelector", () => {
  it("emits only the preset when a rolling preset is clicked", () => {
    const onDatePresetChange = vi.fn();
    const onDateRangeChange = vi.fn();

    render(
      <DateRangeSelector
        customRange={{ startDate: null, endDate: null }}
        selectedDatePreset="all"
        onDatePresetChange={onDatePresetChange}
        onDateRangeChange={onDateRangeChange}
        valuationDate="2026-09-12"
        ariaLabel="Test date range"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "YTD" }));

    expect(onDatePresetChange).toHaveBeenCalledWith("ytd");
    expect(onDateRangeChange).not.toHaveBeenCalled();
  });

  it("limits a custom end date to the server valuation date", () => {
    render(
      <DateRangeSelector
        customRange={{ startDate: "2026-01-01", endDate: null }}
        selectedDatePreset="custom"
        onDatePresetChange={vi.fn()}
        onDateRangeChange={vi.fn()}
        valuationDate="2026-09-12"
        ariaLabel="Test date range"
      />,
    );

    expect(screen.getByLabelText("End date")).toHaveAttribute(
      "max",
      "2026-09-12",
    );
  });
});
