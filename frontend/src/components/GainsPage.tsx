import { useReducer, useState } from "react";
import { type ReturnMethod, useGains } from "../api/queries";
import type { DateRange } from "../api/types";
import { AsyncBoundary } from "./AsyncBoundary";
import type { DatePreset } from "./DateRangeSelector";
import { GainsTable, loadReturnMethod } from "./GainsTable";
import { isBoolean, loadSetting, saveSetting } from "./persistence";

const INCLUDE_CLOSED_POSITIONS_KEY = "gains.includeClosedPositions";

export interface GainsPageState {
  includeClosedPositions: boolean;
  returnMethod: ReturnMethod;
}

export type GainsPageAction =
  | { type: "closedPositionsToggled"; includeClosedPositions: boolean }
  | { type: "returnMethodChanged"; returnMethod: ReturnMethod };

export interface GainsPageProps {
  dateRange: DateRange;
  selectedDatePreset: DatePreset;
  onDatePresetChange: (datePreset: DatePreset) => void;
  onDateRangeChange: (dateRange: DateRange) => void;
}

export function gainsPageReducer(
  state: GainsPageState,
  action: GainsPageAction,
): GainsPageState {
  switch (action.type) {
    case "closedPositionsToggled":
      return {
        ...state,
        includeClosedPositions: action.includeClosedPositions,
      };
    case "returnMethodChanged":
      return { ...state, returnMethod: action.returnMethod };
  }
}

export function GainsPage({
  dateRange,
  selectedDatePreset,
  onDatePresetChange,
  onDateRangeChange,
}: GainsPageProps) {
  const [filter, setFilter] = useState("");
  const [state, dispatch] = useReducer(gainsPageReducer, {
    includeClosedPositions: loadSetting(
      INCLUDE_CLOSED_POSITIONS_KEY,
      isBoolean,
      false,
    ),
    returnMethod: loadReturnMethod(),
  });

  const gainsQuery = useGains({
    includeClosedPositions: state.includeClosedPositions,
    startDate: dateRange.startDate,
    endDate: dateRange.endDate ?? undefined,
    method: state.returnMethod,
  });

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Gains</h1>
          </div>
        </div>
        <AsyncBoundary
          isPending={gainsQuery.isPending}
          isError={gainsQuery.isError}
          isEmpty={(gainsQuery.data?.rows.length ?? 0) === 0}
          onRetry={() => void gainsQuery.refetch()}
          emptyMessage="No valued holdings yet. Add a Buy and refresh prices."
        >
          <GainsTable
            rows={gainsQuery.data?.rows ?? []}
            totals={gainsQuery.data?.totals}
            percentageMethod={gainsQuery.data?.percentage_method}
            filter={filter}
            onFilterChange={setFilter}
            includeClosedPositions={state.includeClosedPositions}
            onIncludeClosedPositionsChange={(includeClosedPositions) => {
              saveSetting(
                INCLUDE_CLOSED_POSITIONS_KEY,
                includeClosedPositions,
                isBoolean,
              );
              dispatch({
                type: "closedPositionsToggled",
                includeClosedPositions,
              });
            }}
            dateRange={dateRange}
            selectedDatePreset={selectedDatePreset}
            onDatePresetChange={onDatePresetChange}
            onDateRangeChange={onDateRangeChange}
            displayPercentKind={gainsQuery.data?.display_percent_kind}
            returnMethod={state.returnMethod}
            onReturnMethodChange={(returnMethod) =>
              dispatch({ type: "returnMethodChanged", returnMethod })
            }
          />
        </AsyncBoundary>
      </article>
    </section>
  );
}
