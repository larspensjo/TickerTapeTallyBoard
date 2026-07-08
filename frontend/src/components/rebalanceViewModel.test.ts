import { describe, expect, it } from "vitest";
import { normalizeRebalanceAmount } from "../api/rebalanceAmount";
import type {
  Instrument,
  RebalanceResponse,
  RebalanceRung,
  RebalanceTrade,
} from "../api/types";
import {
  buildRebalanceBalanceRows,
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
  isNew = false,
): RebalanceTrade {
  return {
    instrument: instrument(id, symbol),
    side,
    shares,
    price_base: priceBase,
    amount_base: amountBase,
    freshness,
    is_new: isNew,
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
          balance: [
            {
              instrument: instrument(1, "AAA"),
              gap_before_base: "0.00",
              gap_after_base: "-100000.00",
              gap_before_percent: "0.00",
              gap_after_percent: "-100.00",
              status_before: "on_target",
              status_after: "below",
              is_new: false,
            },
            {
              instrument: instrument(2, "BBB"),
              gap_before_base: "100000.00",
              gap_after_base: "100000.00",
              gap_before_percent: "50.00",
              gap_after_percent: "50.00",
              status_before: "above",
              status_after: "above",
              is_new: false,
            },
            {
              instrument: instrument(3, "CCC"),
              gap_before_base: "-100000.00",
              gap_after_base: "-100000.00",
              gap_before_percent: "-25.00",
              gap_after_percent: "-25.00",
              status_before: "below",
              status_after: "below",
              is_new: false,
            },
          ],
          achieved_net_base: "-100000.00",
          residual_base: "101234.50",
          coverage_percent: "50.00",
          total_gap_before_base: "200000.00",
          total_gap_after_base: "300000.00",
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
              true,
            ),
          ],
          untraded: [{ instrument: instrument(1, "AAA"), reason: "too_small" }],
          balance: [
            {
              instrument: instrument(1, "AAA"),
              gap_before_base: "0.00",
              gap_after_base: "0.00",
              gap_before_percent: "0.00",
              gap_after_percent: "0.00",
              status_before: "on_target",
              status_after: "on_target",
              is_new: false,
            },
            {
              instrument: instrument(2, "BBB"),
              gap_before_base: "100.00",
              gap_after_base: "49.90",
              gap_before_percent: "50.00",
              gap_after_percent: "24.95",
              status_before: "above",
              status_after: "above",
              is_new: false,
            },
            {
              instrument: instrument(3, "CCC"),
              gap_before_base: "-100.00",
              gap_after_base: "0.00",
              gap_before_percent: "-25.00",
              gap_after_percent: "0.00",
              status_before: "below",
              status_after: "on_target",
              is_new: true,
            },
          ],
          achieved_net_base: "9950.10",
          residual_base: "49.90",
          coverage_percent: "82.50",
          total_gap_before_base: "200.00",
          total_gap_after_base: "49.90",
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
    expect(vm.tradeRows[0].is_new).toBe(false);
    expect(vm.tradeRows[1].is_new).toBe(true);
    expect(vm.selectedRungFreshness).toBe("warning_stale_4_days");
    expect(vm.warningBanner?.label).toBe("Stale 4 days");
    expect(vm.warningBanner?.message).toContain("warning-stale trades");
    expect(vm.balanceRows).toHaveLength(3);
    expect(vm.balanceRows[0].actionKind).toBe("untraded");
    expect(vm.balanceRows[0].actionLabel).toBe("Too small");
    expect(vm.balanceRows[1].actionKind).toBe("trade");
    expect(vm.balanceRows[0].is_new).toBe(false);
    expect(vm.balanceRows[2].is_new).toBe(true);
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
              balance: [
                {
                  instrument: instrument(2, "BBB"),
                  gap_before_base: "0.00",
                  gap_after_base: "0.00",
                  gap_before_percent: "0.00",
                  gap_after_percent: "0.00",
                  status_before: "on_target",
                  status_after: "on_target",
                  is_new: false,
                },
              ],
              achieved_net_base: "6147.60",
              residual_base: "-4913.10",
              coverage_percent: "100.00",
              total_gap_before_base: "0.00",
              total_gap_after_base: "0.00",
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
              balance: [
                {
                  instrument: instrument(1, "AAA"),
                  gap_before_base: "0.00",
                  gap_after_base: "0.00",
                  gap_before_percent: "0.00",
                  gap_after_percent: "0.00",
                  status_before: "on_target",
                  status_after: "on_target",
                  is_new: false,
                },
                {
                  instrument: instrument(2, "BBB"),
                  gap_before_base: "0.00",
                  gap_after_base: "0.00",
                  gap_before_percent: "0.00",
                  gap_after_percent: "0.00",
                  status_before: "on_target",
                  status_after: "on_target",
                  is_new: false,
                },
              ],
              achieved_net_base: "0.00",
              residual_base: "1234.50",
              coverage_percent: "0.00",
              total_gap_before_base: "0.00",
              total_gap_after_base: "0.00",
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
              balance: [
                {
                  instrument: instrument(1, "AAA"),
                  gap_before_base: "0.00",
                  gap_after_base: "0.00",
                  gap_before_percent: "0.00",
                  gap_after_percent: "0.00",
                  status_before: "on_target",
                  status_after: "on_target",
                  is_new: false,
                },
                {
                  instrument: instrument(2, "BBB"),
                  gap_before_base: "0.00",
                  gap_after_base: "0.00",
                  gap_before_percent: "0.00",
                  gap_after_percent: "0.00",
                  status_before: "on_target",
                  status_after: "on_target",
                  is_new: false,
                },
              ],
              achieved_net_base: "0.00",
              residual_base: "1234.50",
              coverage_percent: "0.00",
              total_gap_before_base: "0.00",
              total_gap_after_base: "0.00",
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

describe("buildRebalanceBalanceRows", () => {
  function balanceRung(): RebalanceRung {
    return {
      selected_count: 2,
      effective_trade_count: 2,
      trades: [
        {
          instrument: instrument(1, "AAA"),
          side: "sell",
          shares: 65,
          price_base: "0.50",
          amount_base: "32.50",
          freshness: "fresh",
          is_new: false,
        },
      ],
      untraded: [{ instrument: instrument(2, "BBB"), reason: "too_small" }],
      balance: [
        {
          instrument: instrument(1, "AAA"),
          gap_before_base: "40.00",
          gap_after_base: "7.50",
          gap_before_percent: "40.00",
          gap_after_percent: "7.50",
          status_before: "above",
          status_after: "above",
          is_new: false,
        },
        {
          instrument: instrument(2, "BBB"),
          gap_before_base: "-25.00",
          gap_after_base: "7.50",
          gap_before_percent: "-25.00",
          gap_after_percent: "7.50",
          status_before: "below",
          status_after: "above",
          is_new: false,
        },
        {
          instrument: instrument(3, "CCC"),
          gap_before_base: "-15.00",
          gap_after_base: "-15.00",
          gap_before_percent: "-7.50",
          gap_after_percent: "-7.50",
          status_before: "below",
          status_after: "below",
          is_new: false,
        },
      ],
      achieved_net_base: "0.00",
      residual_base: "0.00",
      coverage_percent: "62.50",
      total_gap_before_base: "80.00",
      total_gap_after_base: "30.00",
    };
  }

  it("joins trades and untraded reasons onto balance rows in balance order", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows.map((row) => row.actionKind)).toEqual([
      "trade",
      "untraded",
      "unselected",
    ]);
    expect(rows[0].actionLabel).toBe("Sell SEK 32.50");
    expect(rows[1].actionLabel).toBe("Too small");
    expect(rows[2].actionLabel).toBe("—");
  });

  it("flags only below↔above flips and builds before/after bar geometry", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows.map((row) => row.flipsSide)).toEqual([false, true, false]);
    expect(rows[1].bar?.before).toEqual({ side: "below", widthPercent: 25 });
    expect(rows[1].bar?.after).toEqual({ side: "above", widthPercent: 7.5 });
  });

  it("renders an after-gap label with amount and percent", () => {
    const rows = buildRebalanceBalanceRows(balanceRung());
    expect(rows[1].afterGapLabel).toBe("SEK 7.50 (7.50%)");
  });
});
