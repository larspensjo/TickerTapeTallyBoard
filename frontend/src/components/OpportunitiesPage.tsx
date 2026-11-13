import { Fragment, useEffect, useReducer } from "react";
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
  type OpportunityPercentViewModel,
  opportunityOrderOptions,
  opportunityWindowOptions,
} from "./opportunitiesViewModel";
import type { OpportunityCheckEntry } from "./opportunityChecksViewModel";
import {
  hasSetting,
  isOneOf,
  loadSetting,
  removeSetting,
  saveSetting,
} from "./persistence";
import { TargetGapBarView } from "./TargetGapBarView";
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
  order: "best_candidates_first",
};

const WINDOW_KEY = "opportunities.window";
const ORDER_KEY = "opportunities.ordering";
const RETIRED_ORDER_KEY = "opportunities.order";
const isOpportunityWindow = isOneOf(OPPORTUNITY_WINDOWS);
const isOpportunityOrder = isOneOf(OPPORTUNITY_ORDERS);

function loadOpportunityOrder(): OpportunityOrder {
  if (hasSetting(ORDER_KEY)) {
    return loadSetting(ORDER_KEY, isOpportunityOrder, "best_candidates_first");
  }
  const retiredOrder = loadSetting(
    RETIRED_ORDER_KEY,
    isOpportunityOrder,
    "best_candidates_first",
  );
  const order =
    retiredOrder === "risers_first" ? "risers_first" : "best_candidates_first";
  saveSetting(ORDER_KEY, order, isOpportunityOrder);
  removeSetting(RETIRED_ORDER_KEY);
  return order;
}

export function loadOpportunitiesPageState(): OpportunitiesPageState {
  return {
    window: loadSetting(
      WINDOW_KEY,
      isOpportunityWindow,
      initialOpportunitiesPageState.window,
    ),
    order: loadOpportunityOrder(),
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
          <>
            <div className="table-wrap opportunities-table">
              <table aria-label="Opportunities">
                <thead>
                  <tr>
                    <th>Instrument</th>
                    <th>Verdict</th>
                    <th className="number-head">
                      {viewModel.changeColumnLabel}
                    </th>
                    <th className="number-head">Latest price</th>
                    <th className="number-head">Vs your average price</th>
                    <th className="number-head">
                      Your gain incl. fees &amp; currency
                    </th>
                    <th className="number-head">Vs target</th>
                  </tr>
                </thead>
                <tbody>
                  {viewModel.rows.map((row) => (
                    <Fragment key={row.instrument.id}>
                      <tr
                        className={
                          row.verdict ? "opportunity-with-evidence" : undefined
                        }
                      >
                        <td
                          aria-describedby={
                            row.verdict
                              ? `opportunity-evidence-${row.instrument.id}`
                              : undefined
                          }
                        >
                          <InstrumentCell
                            instrumentId={row.instrument.id}
                            name={row.instrument.name}
                            ticker={row.instrument.ticker}
                          />
                        </td>
                        <td>
                          {row.verdict ? (
                            <div className="opportunity-verdict">
                              <span
                                className={`status-chip opportunity-verdict-chip ${row.verdict.tone}`}
                              >
                                {row.verdict.label}
                              </span>
                              <span className="opportunity-because">
                                {row.verdict.because}
                              </span>
                              {row.verdict.cautions.length > 0 ? (
                                <div className="opportunity-cautions">
                                  {row.verdict.cautions.map((caution) => (
                                    <span
                                      key={caution}
                                      className="status-chip compact"
                                    >
                                      {caution}
                                    </span>
                                  ))}
                                </div>
                              ) : null}
                            </div>
                          ) : null}
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
                          {row.latestPrice
                            ? latestPriceCell(row.latestPrice)
                            : "—"}
                        </td>
                        <td className="number">
                          <span className="opportunities-context">
                            {percentCell(row.vsAverage)}
                            {row.belowAverage ? (
                              <span className="status-chip compact">
                                Below your average
                              </span>
                            ) : null}
                          </span>
                        </td>
                        <td className="number">
                          <span className="opportunities-context">
                            {percentCell(row.gain)}
                            {row.gainFreshness ? (
                              <StaleSignal
                                kind={row.gainFreshness.kind}
                                label={row.gainFreshness.label}
                              />
                            ) : null}
                          </span>
                        </td>
                        <td className="number">
                          {row.targetBar ? (
                            <TargetGapBarView bar={row.targetBar} />
                          ) : null}
                        </td>
                      </tr>
                      {row.verdict ? (
                        // biome-ignore lint/a11y/noInteractiveElementToNoninteractiveRole: This continuation contains evidence text, not a second holding or interactive table row.
                        <tr
                          role="presentation"
                          className="opportunity-evidence-row"
                        >
                          <td role="presentation" colSpan={7}>
                            <EvidenceLine
                              id={`opportunity-evidence-${row.instrument.id}`}
                              checks={row.checks}
                              name={row.instrument.name}
                            />
                          </td>
                        </tr>
                      ) : null}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            </div>
            {viewModel.hasVerdicts ? (
              <p className="opportunities-note opportunities-verdict-note">
                A verdict describes the dip: how unusual the fall is and what is
                known about the company. It does not predict the price. A replay
                of stored price history found no measurable difference in the
                following month between dips the price checks flagged and other
                dips; the company checks could not be replayed.
              </p>
            ) : null}
            <p className="opportunities-note">
              Price change compares share prices in each asset's own currency,
              without dividends added back. Ex-dividend days are detected where
              the price source reports dividends. During market hours the latest
              price can be intraday, so fell and stopped falling can change
              before the close.
            </p>
          </>
        ) : null}
      </article>
    </section>
  );
}

function percentCell(value: OpportunityPercentViewModel) {
  if (!value.available) {
    return (
      <span className="status-chip compact" title={value.tooltip ?? undefined}>
        {value.label}
      </span>
    );
  }

  return <span className={`number ${value.tone}`}>{value.label}</span>;
}

function latestPriceCell(price: OpportunityLatestPriceViewModel) {
  return (
    <span className="opportunities-price">
      {price.label}
      <StaleSignal kind={price.freshnessKind} label={price.freshnessLabel} />
    </span>
  );
}

function EvidenceLine({
  id,
  checks,
  name,
}: {
  id: string;
  checks: OpportunityCheckEntry[];
  name: string;
}) {
  return (
    // biome-ignore lint/a11y/useSemanticElements: Evidence is a labelled text group, not form fields.
    <div
      id={id}
      className="opportunity-evidence"
      role="group"
      aria-label={`Evidence for ${name}`}
    >
      {checks.map((check) => (
        <span
          className="opportunity-check"
          key={check.key}
          title={check.tooltip}
          aria-description={check.tooltip}
        >
          <span className="opportunity-check-name">{check.name}: </span>
          <span className="opportunity-check-result">
            {check.parts.map((part) => (
              <Fragment key={part.key}>
                {part.accessibleLabel ? (
                  <span className="sr-only">{part.accessibleLabel}</span>
                ) : null}
                <span
                  className={`${part.figure || part.notApplicable ? "number" : ""}${part.approximate ? " approximate" : ""}${part.notApplicable ? " not-applicable" : ""}`}
                  title={part.title}
                  aria-hidden={part.accessibleLabel ? true : undefined}
                >
                  {part.text}
                </span>
              </Fragment>
            ))}
          </span>
        </span>
      ))}
    </div>
  );
}
