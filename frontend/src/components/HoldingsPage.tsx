import { Plus } from "lucide-react";
import { useEffect, useReducer, useRef, useState } from "react";
import { useHoldings, useUpdateInstrumentConvictions } from "../api/queries";
import {
  AddInstrumentDialog,
  type AddInstrumentSubmission,
  createInitialAddInstrumentState,
  revealAddInstrumentIntent,
} from "./AddInstrumentDialog";
import { AsyncBoundary } from "./AsyncBoundary";
import { HoldingsTable } from "./HoldingsTable";
import { useAppMode } from "./useAppMode";

export interface HoldingsPageState {
  includeWatchlist: boolean;
}

export type HoldingsPageAction = {
  type: "includeWatchlistChanged";
  includeWatchlist: boolean;
};

export const initialHoldingsPageState: HoldingsPageState = {
  includeWatchlist: false,
};

export function holdingsPageReducer(
  state: HoldingsPageState,
  action: HoldingsPageAction,
): HoldingsPageState {
  switch (action.type) {
    case "includeWatchlistChanged":
      if (state.includeWatchlist === action.includeWatchlist) {
        return state;
      }
      return { ...state, includeWatchlist: action.includeWatchlist };
  }

  return state;
}

export function HoldingsPage() {
  const [filter, setFilter] = useState("");
  const [addInstrumentOpen, setAddInstrumentOpen] = useState(false);
  const [addInstrumentNotice, setAddInstrumentNotice] = useState<string | null>(
    null,
  );
  const [highlightRequest, setHighlightRequest] = useState<{
    instrumentId: number;
    token: number;
  } | null>(null);
  const nextHighlightToken = useRef(0);
  const [state, dispatch] = useReducer(
    holdingsPageReducer,
    initialHoldingsPageState,
  );
  const holdingsQuery = useHoldings(state.includeWatchlist);
  const { canMutate } = useAppMode();
  const applyConvictions = useUpdateInstrumentConvictions();
  const holdings = holdingsQuery.data?.holdings ?? [];
  const hiddenWatchlistPoolCount =
    holdingsQuery.data?.hidden_watchlist_pool_count ?? 0;

  useEffect(() => {
    if (!addInstrumentNotice) {
      return;
    }

    const timer = setTimeout(() => setAddInstrumentNotice(null), 6000);
    return () => clearTimeout(timer);
  }, [addInstrumentNotice]);

  const handleInstrumentCreated = (result: AddInstrumentSubmission) => {
    const revealIntent = revealAddInstrumentIntent({
      ...createInitialAddInstrumentState(),
      result,
    });

    if (revealIntent) {
      dispatch({
        type: "includeWatchlistChanged",
        includeWatchlist: revealIntent.includeWatchlist,
      });
      nextHighlightToken.current += 1;
      setHighlightRequest({
        instrumentId: revealIntent.instrumentId,
        token: nextHighlightToken.current,
      });
    }
    setAddInstrumentNotice(
      result.messages.length > 0 ? result.messages.join(" ") : null,
    );
    setAddInstrumentOpen(false);
  };

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Holdings</h1>
          </div>
          {canMutate ? (
            <button
              type="button"
              className="button secondary"
              onClick={() => setAddInstrumentOpen(true)}
            >
              <Plus aria-hidden="true" size={16} />
              <span>Add instrument</span>
            </button>
          ) : null}
        </div>
        {addInstrumentNotice ? (
          <div className="holdings-banner" role="status">
            <span className="status-chip compact">Instrument added</span>
            <p>{addInstrumentNotice}</p>
          </div>
        ) : null}
        <AsyncBoundary
          isPending={holdingsQuery.isPending}
          isError={holdingsQuery.isError}
          isEmpty={holdings.length === 0 && hiddenWatchlistPoolCount === 0}
          onRetry={() => void holdingsQuery.refetch()}
          emptyMessage="No holdings yet. Add a Buy to get started."
        >
          <HoldingsTable
            holdings={holdings}
            filter={filter}
            onFilterChange={setFilter}
            includeWatchlist={state.includeWatchlist}
            onIncludeWatchlistChange={(includeWatchlist) =>
              dispatch({ type: "includeWatchlistChanged", includeWatchlist })
            }
            hiddenWatchlistPoolCount={hiddenWatchlistPoolCount}
            highlightRequest={highlightRequest}
            canEditConviction={canMutate}
            onApplyConvictions={async (changes) => {
              await applyConvictions.mutateAsync(changes);
            }}
            isApplyingConvictions={applyConvictions.isPending}
            applyError={
              applyConvictions.isError
                ? `Could not apply conviction changes: ${applyConvictions.error.message}`
                : null
            }
          />
        </AsyncBoundary>
        {addInstrumentOpen && canMutate ? (
          <AddInstrumentDialog
            onClose={() => setAddInstrumentOpen(false)}
            onCreated={handleInstrumentCreated}
          />
        ) : null}
      </article>
    </section>
  );
}
