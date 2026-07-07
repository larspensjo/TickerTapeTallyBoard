import { Clock3 } from "lucide-react";
import { type CSSProperties, useEffect, useReducer, useRef } from "react";
import { useRebalancePlan } from "../api/queries";
import { normalizeRebalanceAmount } from "../api/rebalanceAmount";
import { InstrumentCell } from "./InstrumentCell";
import {
  buildRebalancePageViewModel,
  clampInteger,
} from "./rebalanceViewModel";

export interface RebalancePageState {
  amountInput: string;
  committedAmount: string | null;
  sliderPosition: number;
  lastAvailableRungCount: number | null;
}

export type RebalancePageAction =
  | { type: "amountInputChanged"; amountInput: string }
  | { type: "amountCommitted"; amount: string | null }
  | { type: "sliderChanged"; sliderPosition: number }
  | { type: "planChanged"; rungCount: number | null };

const initialState: RebalancePageState = {
  amountInput: "",
  committedAmount: null,
  sliderPosition: 1,
  lastAvailableRungCount: null,
};

export function rebalancePageReducer(
  state: RebalancePageState,
  action: RebalancePageAction,
): RebalancePageState {
  switch (action.type) {
    case "amountInputChanged":
      if (state.amountInput === action.amountInput) {
        return state;
      }
      return { ...state, amountInput: action.amountInput };
    case "amountCommitted":
      if (state.committedAmount === action.amount) {
        return state;
      }
      return { ...state, committedAmount: action.amount };
    case "sliderChanged": {
      const max =
        state.lastAvailableRungCount ?? Math.max(1, action.sliderPosition);
      const sliderPosition = clampInteger(action.sliderPosition, 1, max);
      if (state.sliderPosition === sliderPosition) {
        return state;
      }
      return {
        ...state,
        sliderPosition,
      };
    }
    case "planChanged": {
      if (action.rungCount === null) {
        return state;
      }

      const rungCount = Math.max(1, Math.trunc(action.rungCount));
      if (state.lastAvailableRungCount === null) {
        return {
          ...state,
          lastAvailableRungCount: rungCount,
          sliderPosition: rungCount,
        };
      }

      const sliderPosition = clampInteger(state.sliderPosition, 1, rungCount);
      if (
        state.lastAvailableRungCount === rungCount &&
        state.sliderPosition === sliderPosition
      ) {
        return state;
      }

      return {
        ...state,
        lastAvailableRungCount: rungCount,
        sliderPosition,
      };
    }
  }
}

export function RebalancePage() {
  const [state, dispatch] = useReducer(rebalancePageReducer, initialState);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rebalanceQuery = useRebalancePlan(state.committedAmount);

  useEffect(() => {
    if (debounceRef.current !== null) {
      clearTimeout(debounceRef.current);
    }

    debounceRef.current = setTimeout(() => {
      debounceRef.current = null;
      dispatch({
        type: "amountCommitted",
        amount: normalizeRebalanceAmount(state.amountInput),
      });
    }, 400);

    return () => {
      if (debounceRef.current !== null) {
        clearTimeout(debounceRef.current);
        debounceRef.current = null;
      }
    };
  }, [state.amountInput]);

  useEffect(() => {
    if (rebalanceQuery.data?.plan.status === "available") {
      dispatch({
        type: "planChanged",
        rungCount: rebalanceQuery.data.plan.rungs.length,
      });
    }
  }, [rebalanceQuery.data]);

  const viewModel = buildRebalancePageViewModel({
    amountInput: state.amountInput,
    committedAmount: state.committedAmount,
    response: rebalanceQuery.data,
    isFetching: rebalanceQuery.isFetching,
    isError: rebalanceQuery.isError,
    errorMessage: rebalanceQuery.error?.message ?? null,
    sliderPosition: state.sliderPosition,
  });

  const commitNow = () => {
    if (debounceRef.current !== null) {
      clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }

    dispatch({
      type: "amountCommitted",
      amount: normalizeRebalanceAmount(state.amountInput),
    });
  };

  const sliderProgress =
    viewModel.slider && viewModel.slider.max > 1
      ? ((viewModel.slider.value - 1) / (viewModel.slider.max - 1)) * 100
      : 100;

  return (
    <section className="board-grid single">
      <article className="panel ledger-panel rebalance-panel">
        <div className="panel-header">
          <div>
            <p className="eyebrow">Portfolio</p>
            <h1>Rebalance</h1>
          </div>
          {viewModel.isRefreshing && viewModel.status === "available" ? (
            <span className="status-chip compact">Updating...</span>
          ) : null}
        </div>

        <div className="rebalance-controls">
          <label className="form-field grow rebalance-amount-field">
            <span>Amount (SEK)</span>
            <input
              className="filter-input rebalance-amount-input"
              type="text"
              inputMode="decimal"
              autoComplete="off"
              spellCheck={false}
              placeholder="10000.50 or -5000"
              value={state.amountInput}
              onChange={(event) =>
                dispatch({
                  type: "amountInputChanged",
                  amountInput: event.target.value,
                })
              }
              onBlur={commitNow}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  commitNow();
                }
              }}
            />
          </label>

          {viewModel.slider ? (
            <div className="rebalance-slider-block">
              <div className="rebalance-slider-header">
                <span className="conviction-field-label">Trades</span>
                <span className="status-chip compact">
                  {viewModel.slider.tradeCountLabel}
                </span>
                <span className="status-chip compact">
                  Coverage {viewModel.slider.coverageLabel ?? "--"}
                </span>
              </div>
              <input
                className="rebalance-slider"
                type="range"
                min={1}
                max={viewModel.slider.max}
                step={1}
                value={viewModel.slider.value}
                aria-label="Rebalance trade count slider"
                style={
                  {
                    "--rebalance-slider-progress": `${sliderProgress}%`,
                  } as CSSProperties
                }
                onChange={(event) =>
                  dispatch({
                    type: "sliderChanged",
                    sliderPosition: Number(event.target.value),
                  })
                }
              />
            </div>
          ) : null}
        </div>

        {viewModel.warningBanner ? (
          <div className="rebalance-banner" role="alert">
            <span className="status-chip warning compact">
              {viewModel.warningBanner.label}
            </span>
            <p>{viewModel.warningBanner.message}</p>
          </div>
        ) : null}

        {viewModel.summary ? (
          <div className="summary-metrics rebalance-summary">
            <span>
              Requested{" "}
              <strong className="number">
                {viewModel.summary.requestedLabel}
              </strong>
            </span>
            <span>
              Achieved net{" "}
              <strong className="number">
                {viewModel.summary.achievedNetLabel}
              </strong>
            </span>
            <span>
              Residual{" "}
              <strong className="number">
                {viewModel.summary.residualLabel}
              </strong>
            </span>
          </div>
        ) : null}

        {viewModel.status === "loading" ? (
          <div className="board-state">
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
              onClick={() => void rebalanceQuery.refetch()}
            >
              Retry
            </button>
          </div>
        ) : null}

        {viewModel.status === "prompt" || viewModel.status === "unavailable" ? (
          <div className="board-state muted">
            <p>{viewModel.message}</p>
          </div>
        ) : null}

        {viewModel.status === "available" ? (
          <>
            {viewModel.tradeRows.length > 0 ? (
              <div className="table-wrap rebalance-table">
                <table aria-label="Rebalance trades">
                  <thead>
                    <tr>
                      <th>Instrument</th>
                      <th>Side</th>
                      <th className="number-head">Shares</th>
                      <th className="number-head">Price (SEK)</th>
                      <th className="number-head">Amount (SEK)</th>
                      <th>Freshness</th>
                    </tr>
                  </thead>
                  <tbody>
                    {viewModel.tradeRows.map((row) => (
                      <tr
                        key={`${row.instrument.id}-${row.side}-${row.shares}`}
                      >
                        <td>
                          <InstrumentCell
                            instrumentId={row.instrument.id}
                            name={row.instrument.name}
                            symbol={row.instrument.symbol}
                            exchange={row.instrument.exchange}
                          />
                        </td>
                        <td>
                          <span className="type-chip">{row.sideLabel}</span>
                        </td>
                        <td className="number">{row.sharesLabel}</td>
                        <td className="number">{row.priceBaseLabel}</td>
                        <td className="number">{row.amountBaseLabel}</td>
                        <td>{tradeFreshnessCell(row)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <div className="board-state muted rebalance-empty-state">
                <p>{viewModel.tradeRowsMessage}</p>
              </div>
            )}

            {viewModel.untradedRows.length > 0 ? (
              <div className="rebalance-untraded">
                <div className="panel-header compact">
                  <div>
                    <p className="eyebrow">Untraded</p>
                    <h2>Selected but not executed</h2>
                  </div>
                  <span className="status-chip compact">
                    {viewModel.slider?.tradeCountLabel}
                  </span>
                </div>
                <div className="table-wrap">
                  <table aria-label="Rebalance untraded candidates">
                    <thead>
                      <tr>
                        <th>Instrument</th>
                        <th>Reason</th>
                      </tr>
                    </thead>
                    <tbody>
                      {viewModel.untradedRows.map((row) => (
                        <tr key={`${row.instrument.id}-${row.reason}`}>
                          <td>
                            <InstrumentCell
                              instrumentId={row.instrument.id}
                              name={row.instrument.name}
                              symbol={row.instrument.symbol}
                              exchange={row.instrument.exchange}
                            />
                          </td>
                          <td>
                            <span className="status-chip">
                              {row.reasonLabel}
                            </span>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
            ) : null}
          </>
        ) : null}
      </article>
    </section>
  );
}

function tradeFreshnessCell({
  freshnessLabel,
  freshnessTone,
  freshnessKind,
}: {
  freshnessLabel: string;
  freshnessTone: "warning" | "flat";
  freshnessKind: "fresh" | "minor_stale" | "warning_stale";
}) {
  if (freshnessKind === "minor_stale") {
    return (
      <span className="rebalance-freshness-icon" title={freshnessLabel}>
        <Clock3 aria-hidden="true" size={12} />
        <span className="sr-only">{freshnessLabel}</span>
      </span>
    );
  }

  return (
    <span
      className={
        freshnessTone === "warning"
          ? "status-chip warning compact"
          : "status-chip compact"
      }
      title={freshnessLabel}
    >
      {freshnessLabel}
    </span>
  );
}
