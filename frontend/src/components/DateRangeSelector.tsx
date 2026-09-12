import { useState } from "react";
import type { DateRange, GainsPeriod } from "../api/types";

const DATE_RANGE_SELECTION_KEY = "portfolio.dateRangeSelection";

export type DatePreset = GainsPeriod;

export interface DateRangeSelection {
  datePreset: DatePreset;
  customRange: DateRange;
}

export type DateRangeSelectionAction =
  | { type: "datePresetChanged"; datePreset: DatePreset }
  | { type: "dateRangeChanged"; dateRange: DateRange };

const DEFAULT_SELECTION: DateRangeSelection = {
  datePreset: "all",
  customRange: { startDate: null, endDate: null },
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

export function dateRangeSelectionReducer(
  state: DateRangeSelection,
  action: DateRangeSelectionAction,
): DateRangeSelection {
  switch (action.type) {
    case "datePresetChanged":
      return { ...state, datePreset: action.datePreset };
    case "dateRangeChanged":
      return { ...state, customRange: action.dateRange };
  }
}

export function loadDateRangeSelection(): DateRangeSelection {
  const saved = storage()?.getItem(DATE_RANGE_SELECTION_KEY);
  if (!saved) return DEFAULT_SELECTION;

  try {
    const parsed = JSON.parse(saved) as {
      datePreset?: unknown;
      customRange?: unknown;
      dateRange?: unknown;
    };
    if (!isDatePreset(parsed.datePreset)) return DEFAULT_SELECTION;

    if (parsed.datePreset !== "custom") {
      return {
        datePreset: parsed.datePreset,
        customRange: DEFAULT_SELECTION.customRange,
      };
    }

    const customRange =
      coerceDateRange(parsed.customRange) ?? coerceDateRange(parsed.dateRange);
    return customRange
      ? { datePreset: parsed.datePreset, customRange }
      : DEFAULT_SELECTION;
  } catch {
    return DEFAULT_SELECTION;
  }
}

export function saveDateRangeSelection(selection: DateRangeSelection): void {
  storage()?.setItem(DATE_RANGE_SELECTION_KEY, JSON.stringify(selection));
}

export function DateRangeSelector({
  customRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
  valuationDate,
  ariaLabel,
}: {
  customRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (preset: DatePreset) => void;
  onDateRangeChange: (range: DateRange) => void;
  valuationDate: string | null;
  ariaLabel: string;
}) {
  const [customStart, setCustomStart] = useState(
    selectedDatePreset === "custom" ? (customRange.startDate ?? "") : "",
  );
  const [customEnd, setCustomEnd] = useState(
    selectedDatePreset === "custom" ? (customRange.endDate ?? "") : "",
  );

  return (
    <fieldset className="date-range-presets">
      <legend className="sr-only">{ariaLabel}</legend>
      {PRESETS.map((preset) => (
        <button
          key={preset}
          type="button"
          className={`preset-btn${selectedDatePreset === preset ? " active" : ""}`}
          aria-pressed={selectedDatePreset === preset}
          onClick={() => {
            onDatePresetChange(preset);
            if (preset === "custom") {
              onDateRangeChange({
                startDate: customStart || null,
                endDate: customEnd || null,
              });
            }
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
              onDateRangeChange({
                startDate: nextStart || null,
                endDate: customEnd || null,
              });
            }}
          />
          <input
            className="date-range-input"
            type="date"
            aria-label="End date"
            value={customEnd}
            max={valuationDate ?? undefined}
            onChange={(event) => {
              const nextEnd = event.target.value;
              setCustomEnd(nextEnd);
              onDateRangeChange({
                startDate: customStart || null,
                endDate: nextEnd || null,
              });
            }}
          />
        </>
      )}
    </fieldset>
  );
}
