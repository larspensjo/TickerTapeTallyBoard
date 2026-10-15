import { useEffect, useReducer } from "react";
import { useOpportunities } from "../api/queries";
import {
  OPPORTUNITY_ORDERS,
  OPPORTUNITY_WINDOWS,
  type OpportunityOrder,
  type OpportunityWindow,
} from "../api/types";
import { InstrumentCell } from "./InstrumentCell";
import { LedgerEntryInvitation } from "./LedgerEntryInvitation";
import {
  buildOpportunitiesViewModel,
  type OpportunityLatestPriceViewModel,
  opportunityOrderOptions,
  opportunityWindowOptions,
} from "./opportunitiesViewModel";
import { isOneOf, loadSetting, saveSetting } from "./persistence";
import { useAppMode } from "./useAppMode";
import { StaleSignal } from "./valuationDisplay";

export interface OpportunitiesPageState {
  window: OpportunityWindow;
  order: OpportunityOrder;
}

export type OpportunitiesPageAction =
  | { type: "windowChanged"; window: OpportunityWindow }
  | { type: "orderChanged"; order: OpportunityOrder };

export const initialOpportunitiesPageState: OpportunitiesPageState = {
  window: "7d",
  order: "fallers_first",
};

const WINDOW_KEY = "opportunities.window";
const ORDER_KEY = "opportunities.order";
const isOpportunityWindow = isOneOf(OPPORTUNITY_WINDOWS);
const isOpportunityOrder = isOneOf(OPPORTUNITY_ORDERS);

export function loadOpportunitiesPageState(): OpportunitiesPageState {
  return {
    window: loadSetting(
      WINDOW_KEY,
      isOpportunityWindow,
      initialOpportunitiesPageState.window,
    ),
    order: loadSetting(
      ORDER_KEY,
      isOpportunityOrder,
      initialOpportunitiesPageState.order,
    ),
  };
}

export function saveOpportunitiesPageState(state: OpportunitiesPageState) {
  saveSetting(WINDOW_KEY, state.window, isOpportunityWindow);
  saveSetting(ORDER_KEY, state.order, isOpportunityOrder);
}

export function opportunitiesPageReducer(
  state: OpportunitiesPageState,
  action: OpportunitiesPageAction,
): OpportunitiesPageState {
  switch (action.type) {
    case "windowChanged":
      if (state.window === action.window) {
        return state;
      }
      return { ...state, window: action.window };
    case "orderChanged":
      if (state.order === action.order) {
        return state;
      }
      return { ...state, order: action.order };
  }
}

export function OpportunitiesPage() {
  const [state, dispatch] = useReducer(
    opportunitiesPageReducer,
    undefined,
    loadOpportunitiesPageState,
  );
  const opportunitiesQuery = useOpportunities(state.window);
  const { canMutate } = useAppMode();

  useEffect(() => {
    saveOpportunitiesPageState(state);
  }, [state]);

  const viewModel = buildOpportunitiesViewModel({
    response: opportunitiesQuery.data,
    requestedWindow: state.window,
    order: state.order,
    isError: opportunitiesQuery.isError,
    errorMessage: opportunitiesQuery.error?.message ?? null,
  });

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Opportunities</h1>
            {viewModel.periodSubtitle ? (
              <p className="opportunities-period">{viewModel.periodSubtitle}</p>
            ) : null}
            {viewModel.isUpdating ? (
              <span className="status-chip compact opportunities-updating">
                Updating…
              </span>
            ) : null}
          </div>
          <div className="opportunities-controls">
            <fieldset className="segmented-control" aria-label="Price window">
              <legend className="sr-only">Price window</legend>
              {opportunityWindowOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={
                    state.window === option.value ? "active" : undefined
                  }
                  aria-pressed={state.window === option.value}
                  onClick={() =>
                    dispatch({ type: "windowChanged", window: option.value })
                  }
                >
                  {option.label}
                </button>
              ))}
            </fieldset>
            <fieldset className="segmented-control" aria-label="Order">
              <legend className="sr-only">Order</legend>
              {opportunityOrderOptions.map((option) => (
                <button
                  key={option.value}
                  type="button"
                  className={
                    state.order === option.value ? "active" : undefined
                  }
                  aria-pressed={state.order === option.value}
                  onClick={() =>
                    dispatch({ type: "orderChanged", order: option.value })
                  }
                >
                  {option.label}
                </button>
              ))}
            </fieldset>
          </div>
        </div>

        {viewModel.status === "loading" ? (
          <div className="board-state" aria-busy="true">
            <div className="skeleton-bar" />
            <div className="skeleton-bar" />
            <div className="skeleton-bar" />
          </div>
        ) : null}

        {viewModel.status === "error" ? (
          <div className="board-state error">
            <p className="down">{viewModel.message}</p>
            <button
              type="button"
              className="button outline"
              onClick={() => void opportunitiesQuery.refetch()}
            >
              Retry
            </button>
          </div>
        ) : null}

        {viewModel.status === "empty" ? (
          <div className="board-state muted">
            <p className="ledger-empty-state">
              {viewModel.message}{" "}
              <LedgerEntryInvitation
                variant="elsewhere"
                canMutate={canMutate}
              />
            </p>
          </div>
        ) : null}

        {viewModel.status === "available" ? (
          <div className="table-wrap">
            <table aria-label="Opportunities">
              <thead>
                <tr>
                  <th>Instrument</th>
                  <th className="number-head">{viewModel.changeColumnLabel}</th>
                  <th className="number-head">Latest price</th>
                </tr>
              </thead>
              <tbody>
                {viewModel.rows.map((row) => (
                  <tr key={row.instrument.id}>
                    <td>
                      <InstrumentCell
                        instrumentId={row.instrument.id}
                        name={row.instrument.name}
                        symbol={row.instrument.symbol}
                        exchange={row.instrument.exchange}
                      />
                    </td>
                    <td className="number">
                      {row.changeAvailable ? (
                        <span
                          className={`number ${row.changeTone}`}
                          title={row.changeTooltip}
                        >
                          {row.changeLabel}
                        </span>
                      ) : (
                        <span
                          className="status-chip compact"
                          title={row.changeTooltip}
                        >
                          {row.changeLabel}
                        </span>
                      )}
                    </td>
                    <td className="number">
                      {row.latestPrice ? latestPriceCell(row.latestPrice) : "—"}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </article>
    </section>
  );
}

function latestPriceCell(price: OpportunityLatestPriceViewModel) {
  return (
    <span className="opportunities-price">
      {price.label}
      <StaleSignal kind={price.freshnessKind} label={price.freshnessLabel} />
    </span>
  );
}
