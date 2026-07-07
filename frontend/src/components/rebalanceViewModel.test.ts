import { describe, expect, it } from "vitest";
import { normalizeRebalanceAmount } from "../api/rebalanceAmount";
import type {
  Instrument,
  RebalanceResponse,
  RebalanceTrade,
} from "../api/types";
import {
  buildRebalancePageViewModel,
  rebalanceTradeCountLabel,
  rebalanceUnavailableMessage,
  selectRebalanceRung,
} from "./rebalanceViewModel";

function instrument(
  id: number,
  symbol: string,
  name = symbol,
  exchange = "STO",
): Instrument {
  return {
    id,
    symbol,
    name,
    exchange,
    type: "Stock",
    currency: "SEK",
    conviction: "Low",
  };
}

function trade(
  id: number,
  symbol: string,
  side: "buy" | "sell",
  shares: number,
  priceBase: string,
  amountBase: string,
  freshness = "fresh",
): RebalanceTrade {
  return {
    instrument: instrument(id, symbol),
    side,
    shares,
    price_base: priceBase,
    amount_base: amountBase,
    freshness,
  };
}

function response(): RebalanceResponse {
  return {
    amount_base: "1234.50",
    base_currency: "SEK",
    plan: {
      status: "available",
      pool_value_base: "700000.00",
      candidate_count: 3,
      rungs: [
        {
          selected_count: 1,
          effective_trade_count: 1,
          trades: [trade(1, "AAA", "sell", 100, "1000.00", "100000.00")],
          untraded: [],
          achieved_net_base: "-100000.00",
          residual_base: "101234.50",
          coverage_percent: "50.00",
        },
        {
          selected_count: 3,
          effective_trade_count: 2,
          trades: [
            trade(
              2,
              "BBB",
              "buy",
              12,
              "512.30",
              "6147.60",
              "minor_stale_2_days",
            ),
            trade(
              3,
              "CCC",
              "sell",
              4,
              "950.00",
              "3800.00",
              "warning_stale_4_days",
            ),
          ],
          untraded: [{ instrument: instrument(1, "AAA"), reason: "too_small" }],
          achieved_net_base: "9950.10",
          residual_base: "49.90",
          coverage_percent: "82.50",
        },
      ],
    },
  };
}

describe("rebalanceViewModel", () => {
  it("normalizes signed amounts with commas and rejects garbage", () => {
    expect(normalizeRebalanceAmount(" +1,25 ")).toBe("+1.25");
    expect(normalizeRebalanceAmount("1,5")).toBe("1.5");
    expect(normalizeRebalanceAmount("-5.00")).toBe("-5.00");
    expect(normalizeRebalanceAmount("10,000")).toBeNull();
    expect(normalizeRebalanceAmount("1.234,56")).toBeNull();
    expect(normalizeRebalanceAmount("garbage")).toBeNull();
  });

  it("clamps the selected rung and formats the selected plan row data", () => {
    const vm = buildRebalancePageViewModel({
      amountInput: "1234,5",
      committedAmount: "1234.50",
      response: response(),
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 9,
    });

    expect(vm.status).toBe("available");
    expect(vm.summary).toEqual({
      requestedLabel: "SEK 1,234.50",
      achievedNetLabel: "SEK 9,950.10",
      residualLabel: "SEK 49.90",
    });
    expect(vm.slider).toEqual({
      value: 2,
      max: 2,
      tradeCountLabel: "2 executed of 3 selected",
      coverageLabel: "82.50%",
    });
    expect(vm.tradeRows).toHaveLength(2);
    expect(vm.tradeRows[0]).toMatchObject({
      side: "buy",
      sideLabel: "Buy",
      shares: 12,
      sharesLabel: "12",
      priceBaseLabel: "SEK 512.30",
      amountBaseLabel: "SEK 6,147.60",
      freshnessLabel: "Minor stale 2 days",
      freshnessTone: "warning",
      freshnessKind: "minor_stale",
    });
    expect(vm.tradeRows[1]).toMatchObject({
      freshnessLabel: "Stale 4 days",
      freshnessTone: "warning",
      freshnessKind: "warning_stale",
    });
    expect(vm.selectedRungFreshness).toBe("warning_stale_4_days");
    expect(vm.warningBanner?.label).toBe("Stale 4 days");
    expect(vm.warningBanner?.message).toContain("warning-stale trades");
    expect(vm.untradedRows).toHaveLength(1);
    expect(vm.untradedRows[0].instrument.symbol).toBe("AAA");
    expect(vm.untradedRows[0].reason).toBe("too_small");
    expect(vm.untradedRows[0].reasonLabel).toBe("Too small");
  });

  it("keeps minor-stale trades out of the plan-level warning banner", () => {
    const vm = buildRebalancePageViewModel({
      amountInput: "1234.5",
      committedAmount: "1234.50",
      response: {
        ...response(),
        plan: {
          status: "available",
          pool_value_base: "700000.00",
          candidate_count: 1,
          rungs: [
            {
              selected_count: 1,
              effective_trade_count: 1,
              trades: [
                trade(
                  2,
                  "BBB",
                  "buy",
                  12,
                  "512.30",
                  "6147.60",
                  "minor_stale_5_days",
                ),
              ],
              untraded: [],
              achieved_net_base: "6147.60",
              residual_base: "-4913.10",
              coverage_percent: "100.00",
            },
          ],
        },
      },
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(vm.selectedRungFreshness).toBe("minor_stale_5_days");
    expect(vm.warningBanner).toBeNull();
  });

  it("maps unavailable and empty-rung edge states explicitly", () => {
    expect(rebalanceTradeCountLabel(3, 2)).toBe("2 executed of 3 selected");
    expect(rebalanceTradeCountLabel(2, 2)).toBe("2 selected");
    expect(rebalanceUnavailableMessage(["empty_pool"])).toContain(
      "No eligible holdings",
    );
    expect(rebalanceUnavailableMessage(["offset_exceeds_pool"])).toContain(
      "negated pool value",
    );

    const promptVm = buildRebalancePageViewModel({
      amountInput: "",
      committedAmount: null,
      response: undefined,
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(promptVm.status).toBe("prompt");
    expect(promptVm.message).toContain("No valid amount entered yet");

    const unavailableVm = buildRebalancePageViewModel({
      amountInput: "0",
      committedAmount: "0",
      response: {
        amount_base: "0.00",
        base_currency: "SEK",
        plan: {
          status: "unavailable",
          reasons: ["empty_pool"],
        },
      },
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(unavailableVm.status).toBe("unavailable");
    expect(unavailableVm.message).toContain("No eligible holdings");

    const offsetUnavailableVm = buildRebalancePageViewModel({
      amountInput: "0",
      committedAmount: "0",
      response: {
        amount_base: "0.00",
        base_currency: "SEK",
        plan: {
          status: "unavailable",
          reasons: ["offset_exceeds_pool"],
        },
      },
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(offsetUnavailableVm.message).toContain("negated pool value");

    const emptyRung = buildRebalancePageViewModel({
      amountInput: "1234.5",
      committedAmount: "1234.50",
      response: {
        ...response(),
        plan: {
          status: "available",
          pool_value_base: "700000.00",
          candidate_count: 3,
          rungs: [
            {
              selected_count: 2,
              effective_trade_count: 0,
              trades: [],
              untraded: [
                { instrument: instrument(1, "AAA"), reason: "too_small" },
                { instrument: instrument(2, "BBB"), reason: "too_small" },
              ],
              achieved_net_base: "0.00",
              residual_base: "1234.50",
              coverage_percent: "0.00",
            },
          ],
        },
      },
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(emptyRung.status).toBe("available");
    expect(emptyRung.tradeRows).toHaveLength(0);
    expect(emptyRung.tradeRowsMessage).toBe(
      "Too small to trade at this granularity.",
    );
    expect(emptyRung.warningBanner).toBeNull();
  });

  it("uses an on-target empty-trades message when all untraded candidates are on target", () => {
    const vm = buildRebalancePageViewModel({
      amountInput: "1234.5",
      committedAmount: "1234.50",
      response: {
        ...response(),
        plan: {
          status: "available",
          pool_value_base: "700000.00",
          candidate_count: 2,
          rungs: [
            {
              selected_count: 2,
              effective_trade_count: 0,
              trades: [],
              untraded: [
                { instrument: instrument(1, "AAA"), reason: "on_target" },
                { instrument: instrument(2, "BBB"), reason: "on_target" },
              ],
              achieved_net_base: "0.00",
              residual_base: "1234.50",
              coverage_percent: "0.00",
            },
          ],
        },
      },
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 1,
    });

    expect(vm.status).toBe("available");
    expect(vm.tradeRows).toHaveLength(0);
    expect(vm.tradeRowsMessage).toBe(
      "Portfolio is on target at this granularity.",
    );
    expect(vm.warningBanner).toBeNull();
  });

  it("preserves the selected rung selection and clamps the slider", () => {
    const vm = buildRebalancePageViewModel({
      amountInput: "1234.5",
      committedAmount: "1234.50",
      response: response(),
      isFetching: false,
      isError: false,
      errorMessage: null,
      sliderPosition: 2,
    });

    expect(selectRebalanceRung(response(), 9)?.position).toBe(2);
    expect(vm.slider?.value).toBe(2);
  });
});
