// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  activeDateRange,
  DateRangeSelector,
  dateRangeSelectionReducer,
  loadDateRangeSelection,
  presetToRange,
  saveDateRangeSelection,
} from "./DateRangeSelector";
import { useLocalDate } from "./useLocalDate";

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

describe("activeDateRange", () => {
  it("resolves rolling presets against the current day, not the day they were chosen", () => {
    const selection = {
      datePreset: "all" as const,
      dateRange: { startDate: null, endDate: "2026-09-10" },
    };

    expect(activeDateRange(selection, "2026-09-11")).toEqual({
      startDate: null,
      endDate: "2026-09-11",
    });
    expect(
      activeDateRange({ ...selection, datePreset: "7d" }, "2026-09-11"),
    ).toEqual({ startDate: "2026-09-04", endDate: "2026-09-11" });
  });

  it("keeps a custom range and treats an open end as today", () => {
    expect(
      activeDateRange(
        {
          datePreset: "custom",
          dateRange: { startDate: "2026-02-01", endDate: "2026-06-29" },
        },
        "2026-09-11",
      ),
    ).toEqual({ startDate: "2026-02-01", endDate: "2026-06-29" });
    expect(
      activeDateRange(
        {
          datePreset: "custom",
          dateRange: { startDate: "2026-02-01", endDate: null },
        },
        "2026-09-11",
      ),
    ).toEqual({ startDate: "2026-02-01", endDate: "2026-09-11" });
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
  it("emits only the preset when a rolling preset is clicked", () => {
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
    expect(onDateRangeChange).not.toHaveBeenCalled();
  });
});

describe("useLocalDate", () => {
  it("rolls over to the new day at local midnight", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 8, 10, 23, 59, 30));

    const { result } = renderHook(() => useLocalDate());
    expect(result.current).toBe("2026-09-10");

    act(() => {
      vi.advanceTimersByTime(60_000);
    });

    expect(result.current).toBe("2026-09-11");

    act(() => {
      vi.advanceTimersByTime(24 * 60 * 60 * 1000);
    });

    expect(result.current).toBe("2026-09-12");
  });

  it("catches up when the page becomes visible after the day changed", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 8, 10, 12, 0, 0));

    const { result } = renderHook(() => useLocalDate());

    act(() => {
      vi.setSystemTime(new Date(2026, 8, 11, 8, 0, 0));
      document.dispatchEvent(new Event("visibilitychange"));
    });

    expect(result.current).toBe("2026-09-11");
  });
});
