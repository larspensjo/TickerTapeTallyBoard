import { useState } from "react";
import type { DateRange } from "../api/types";

const DATE_RANGE_SELECTION_KEY = "portfolio.dateRangeSelection";

export type DatePreset = "today" | "7d" | "12m" | "ytd" | "all" | "custom";

export interface DateRangeSelection {
  datePreset: DatePreset;
  dateRange: DateRange;
}

export type DateRangeSelectionAction =
  | { type: "datePresetChanged"; datePreset: DatePreset }
  | { type: "dateRangeChanged"; dateRange: DateRange };

const DEFAULT_SELECTION: DateRangeSelection = {
  datePreset: "all",
  dateRange: { startDate: null, endDate: null },
};

const PRESETS: DatePreset[] = ["today", "7d", "12m", "ytd", "all", "custom"];

const PRESET_LABELS: Record<DatePreset, string> = {
  today: "Today",
  "7d": "7D",
  "12m": "12M",
  ytd: "YTD",
  all: "All",
  custom: "Custom",
};

function storage(): Storage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

function localDateString(date: Date): string {
  return date.toLocaleDateString("sv-SE");
}

function isDatePreset(value: unknown): value is DatePreset {
  return typeof value === "string" && PRESETS.includes(value as DatePreset);
}

function isDateStringOrNull(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function coerceDateRange(value: unknown): DateRange | null {
  if (typeof value !== "object" || value === null) return null;

  const candidate = value as Partial<DateRange>;
  if (
    !isDateStringOrNull(candidate.startDate) ||
    !isDateStringOrNull(candidate.endDate)
  ) {
    return null;
  }

  return {
    startDate: candidate.startDate,
    endDate: candidate.endDate,
  };
}

export function presetToRange(
  preset: DatePreset,
  customStart: string,
  customEnd: string,
  today = new Date(),
): DateRange {
  const fmt = localDateString;

  switch (preset) {
    case "today":
      return { startDate: fmt(today), endDate: fmt(today) };
    case "7d": {
      const start = new Date(today);
      start.setDate(start.getDate() - 7);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "12m": {
      const start = new Date(today);
      start.setFullYear(start.getFullYear() - 1);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "ytd":
      return { startDate: `${today.getFullYear()}-01-01`, endDate: fmt(today) };
    case "all":
      return { startDate: null, endDate: fmt(today) };
    case "custom":
      return {
        startDate: customStart || null,
        endDate: customEnd || fmt(today),
      };
  }
}

export function dateRangeSelectionReducer(
  state: DateRangeSelection,
  action: DateRangeSelectionAction,
): DateRangeSelection {
  switch (action.type) {
    case "datePresetChanged":
      return { ...state, datePreset: action.datePreset };
    case "dateRangeChanged":
      return { ...state, dateRange: action.dateRange };
  }
}

export function loadDateRangeSelection(): DateRangeSelection {
  const saved = storage()?.getItem(DATE_RANGE_SELECTION_KEY);
  if (!saved) return DEFAULT_SELECTION;

  try {
    const parsed = JSON.parse(saved) as Partial<DateRangeSelection>;
    if (!isDatePreset(parsed.datePreset)) return DEFAULT_SELECTION;

    if (parsed.datePreset !== "custom") {
      return {
        datePreset: parsed.datePreset,
        dateRange: presetToRange(parsed.datePreset, "", ""),
      };
    }

    const dateRange = coerceDateRange(parsed.dateRange);
    return dateRange
      ? { datePreset: parsed.datePreset, dateRange }
      : DEFAULT_SELECTION;
  } catch {
    return DEFAULT_SELECTION;
  }
}

export function saveDateRangeSelection(selection: DateRangeSelection): void {
  storage()?.setItem(DATE_RANGE_SELECTION_KEY, JSON.stringify(selection));
}

export function DateRangeSelector({
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
  ariaLabel,
}: {
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (preset: DatePreset) => void;
  onDateRangeChange: (range: DateRange) => void;
  ariaLabel: string;
}) {
  const [customStart, setCustomStart] = useState(
    selectedDatePreset === "custom" ? (dateRange.startDate ?? "") : "",
  );
  const [customEnd, setCustomEnd] = useState(
    selectedDatePreset === "custom" ? (dateRange.endDate ?? "") : "",
  );

  return (
    <fieldset className="date-range-presets">
      <legend className="sr-only">{ariaLabel}</legend>
      {PRESETS.map((preset) => (
        <button
          key={preset}
          type="button"
          className={`preset-btn${
            selectedDatePreset === preset ? " active" : ""
          }`}
          aria-pressed={selectedDatePreset === preset}
          onClick={() => {
            onDatePresetChange(preset);
            onDateRangeChange(presetToRange(preset, customStart, customEnd));
          }}
        >
          {PRESET_LABELS[preset]}
        </button>
      ))}
      {selectedDatePreset === "custom" && (
        <>
          <input
            className="date-range-input"
            type="date"
            aria-label="Start date"
            value={customStart}
            onChange={(event) => {
              const nextStart = event.target.value;
              setCustomStart(nextStart);
              onDateRangeChange(presetToRange("custom", nextStart, customEnd));
            }}
          />
          <input
            className="date-range-input"
            type="date"
            aria-label="End date"
            value={customEnd}
            onChange={(event) => {
              const nextEnd = event.target.value;
              setCustomEnd(nextEnd);
              onDateRangeChange(presetToRange("custom", customStart, nextEnd));
            }}
          />
        </>
      )}
    </fieldset>
  );
}
