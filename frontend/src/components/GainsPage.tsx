import { useReducer, useState } from "react";
import { type ReturnMethod, useGains } from "../api/queries";
import type { DateRange } from "../api/types";
import { AsyncBoundary } from "./AsyncBoundary";
import { type DatePreset, GainsTable, loadReturnMethod } from "./GainsTable";

export interface GainsPageState {
  includeClosedPositions: boolean;
  datePreset: DatePreset;
  dateRange: DateRange;
  returnMethod: ReturnMethod;
}

export type GainsPageAction =
  | { type: "closedPositionsToggled"; includeClosedPositions: boolean }
  | { type: "datePresetChanged"; datePreset: DatePreset }
  | { type: "dateRangeChanged"; dateRange: DateRange }
  | { type: "returnMethodChanged"; returnMethod: ReturnMethod };

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
    case "datePresetChanged":
      return { ...state, datePreset: action.datePreset };
    case "dateRangeChanged":
      return { ...state, dateRange: action.dateRange };
    case "returnMethodChanged":
      return { ...state, returnMethod: action.returnMethod };
  }
}

export function GainsPage() {
  const [filter, setFilter] = useState("");
  const [state, dispatch] = useReducer(gainsPageReducer, {
    includeClosedPositions: false,
    datePreset: "all",
    dateRange: { startDate: null, endDate: null },
    returnMethod: loadReturnMethod(),
  });

  const gainsQuery = useGains({
    includeClosedPositions: state.includeClosedPositions,
    startDate: state.dateRange.startDate,
    endDate: state.dateRange.endDate ?? undefined,
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
            onIncludeClosedPositionsChange={(includeClosedPositions) =>
              dispatch({
                type: "closedPositionsToggled",
                includeClosedPositions,
              })
            }
            dateRange={state.dateRange}
            selectedDatePreset={state.datePreset}
            onDatePresetChange={(datePreset) =>
              dispatch({ type: "datePresetChanged", datePreset })
            }
            onDateRangeChange={(dateRange) =>
              dispatch({ type: "dateRangeChanged", dateRange })
            }
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
