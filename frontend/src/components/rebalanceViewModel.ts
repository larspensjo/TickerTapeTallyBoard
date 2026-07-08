import { normalizeRebalanceAmount } from "../api/rebalanceAmount";
import type {
  Instrument,
  RebalanceBalanceEntry,
  RebalanceRankBy,
  RebalanceResponse,
  RebalanceRung,
  RebalanceTrade,
  RebalanceUnavailableReason,
  RebalanceUntraded,
} from "../api/types";
import { type GapBarGeometry, gapBarGeometry } from "./holdingsConviction";
import {
  formatGroupedNumber,
  freshnessLabel,
  freshnessTone,
  parseFiniteNumber,
  worstFreshness,
} from "./valuationDisplay";

const REBALANCE_UNAVAILABLE_MESSAGES: Record<
  RebalanceUnavailableReason,
  string
> = {
  empty_pool: "No eligible holdings are available for rebalance.",
  offset_exceeds_pool:
    "The requested offset is at or below the negated pool value, so no valid plan exists.",
};

const REBALANCE_UNTRADED_REASON_LABELS: Record<string, string> = {
  too_small: "Too small",
  clamped: "Clamped",
  on_target: "On target",
};

export type RebalanceFreshnessKind = "fresh" | "minor_stale" | "warning_stale";

export interface RebalanceTradeRowViewModel {
  instrument: Instrument;
  side: "buy" | "sell";
  sideLabel: string;
  shares: number;
  sharesLabel: string;
  priceBaseLabel: string;
  amountBaseLabel: string;
  freshness: string;
  freshnessLabel: string;
  freshnessTone: "warning" | "flat";
  freshnessKind: RebalanceFreshnessKind;
  is_new: boolean;
}

export interface RebalanceBalanceBarViewModel {
  before: GapBarGeometry;
  after: GapBarGeometry;
  tooltip: string;
}

export interface RebalanceBalanceRowViewModel {
  instrument: Instrument;
  actionKind: "trade" | "untraded" | "unselected";
  actionLabel: string;
  bar: RebalanceBalanceBarViewModel | null;
  afterGapLabel: string;
  flipsSide: boolean;
  is_new: boolean;
}

export interface RebalanceSummaryViewModel {
  requestedLabel: string;
  achievedNetLabel: string;
  residualLabel: string;
}

export interface RebalanceSliderViewModel {
  value: number;
  max: number;
  tradeCountLabel: string;
  coverageLabel: string | null;
}

export interface RebalanceWarningBannerViewModel {
  label: string;
  message: string;
}

export const rankByOptions: ReadonlyArray<{
  value: RebalanceRankBy;
  label: string;
}> = [
  { value: "sek", label: "Amount" },
  { value: "percent", label: "Relative" },
];

export type RebalancePageStatus =
  | "prompt"
  | "loading"
  | "error"
  | "unavailable"
  | "available";

export interface RebalancePageViewModel {
  status: RebalancePageStatus;
  message: string | null;
  isRefreshing: boolean;
  summary: RebalanceSummaryViewModel | null;
  slider: RebalanceSliderViewModel | null;
  warningBanner: RebalanceWarningBannerViewModel | null;
  tradeRowsMessage: string | null;
  tradeRows: RebalanceTradeRowViewModel[];
  balanceRows: RebalanceBalanceRowViewModel[];
  balanceTotalLabel: string | null;
  selectedRungFreshness: string | null;
}

export interface BuildRebalancePageViewModelInput {
  amountInput: string;
  committedAmount: string | null;
  response?: RebalanceResponse;
  isFetching: boolean;
  isError: boolean;
  errorMessage: string | null;
  sliderPosition: number;
}

export function clampInteger(value: number, min: number, max: number): number {
  return Math.min(Math.max(Math.trunc(value), min), max);
}

export function formatRebalanceMoney(value: string): string {
  return `SEK ${formatGroupedNumber(value)}`;
}

function freshnessKind(freshness: string): RebalanceFreshnessKind {
  if (freshness.startsWith("warning_stale_")) {
    return "warning_stale";
  }

  if (freshness.startsWith("minor_stale_")) {
    return "minor_stale";
  }

  return "fresh";
}

export function rebalanceUnavailableMessage(
  reasons: RebalanceUnavailableReason[],
): string {
  if (reasons.length === 0) {
    return "Rebalance plan unavailable.";
  }

  return reasons
    .map((reason) => REBALANCE_UNAVAILABLE_MESSAGES[reason] ?? reason)
    .join(" ");
}

export function rebalanceUntradedReasonLabel(reason: string): string {
  return (
    REBALANCE_UNTRADED_REASON_LABELS[reason] ?? reason.replaceAll("_", " ")
  );
}

export function rebalanceTradeCountLabel(
  selectedCount: number,
  effectiveTradeCount: number,
): string {
  const selectedLabel = `${formatGroupedNumber(selectedCount)} selected`;
  if (selectedCount === effectiveTradeCount) {
    return selectedLabel;
  }

  return `${formatGroupedNumber(effectiveTradeCount)} executed of ${selectedLabel}`;
}

export function selectRebalanceRung(
  response: RebalanceResponse | undefined,
  sliderPosition: number,
): { rung: RebalanceRung; position: number } | null {
  if (response?.plan.status !== "available") {
    return null;
  }

  const rungCount = response.plan.rungs.length;
  if (rungCount === 0) {
    return null;
  }

  const position = clampInteger(sliderPosition, 1, rungCount);
  return { rung: response.plan.rungs[position - 1], position };
}

function tradeRows(trades: RebalanceTrade[]): RebalanceTradeRowViewModel[] {
  return trades.map((trade) => ({
    instrument: trade.instrument,
    side: trade.side,
    sideLabel: trade.side === "buy" ? "Buy" : "Sell",
    shares: trade.shares,
    sharesLabel: formatGroupedNumber(trade.shares),
    priceBaseLabel: formatRebalanceMoney(trade.price_base),
    amountBaseLabel: formatRebalanceMoney(trade.amount_base),
    freshness: trade.freshness,
    freshnessLabel: freshnessLabel(trade.freshness),
    freshnessTone: freshnessTone(trade.freshness),
    freshnessKind: freshnessKind(trade.freshness),
    is_new: trade.is_new,
  }));
}

function balanceBar(
  entry: RebalanceBalanceEntry,
): RebalanceBalanceBarViewModel | null {
  if (entry.gap_before_percent === null || entry.gap_after_percent === null) {
    return null;
  }

  const beforePercent = parseFiniteNumber(entry.gap_before_percent);
  const afterPercent = parseFiniteNumber(entry.gap_after_percent);
  if (beforePercent === null || afterPercent === null) {
    return null;
  }

  return {
    before: gapBarGeometry(beforePercent),
    after: gapBarGeometry(afterPercent),
    tooltip: [
      `Before ${formatRebalanceMoney(entry.gap_before_base)} (${formatGroupedNumber(entry.gap_before_percent)}%)`,
      `After ${formatRebalanceMoney(entry.gap_after_base)} (${formatGroupedNumber(entry.gap_after_percent)}%)`,
    ].join("\n"),
  };
}

export function buildRebalanceBalanceRows(
  rung: RebalanceRung,
): RebalanceBalanceRowViewModel[] {
  const tradesById = new Map(
    rung.trades.map((trade) => [trade.instrument.id, trade]),
  );
  const untradedById = new Map(
    rung.untraded.map((candidate) => [candidate.instrument.id, candidate]),
  );

  return rung.balance.map((entry) => {
    const trade = tradesById.get(entry.instrument.id);
    const untraded = untradedById.get(entry.instrument.id);
    const actionKind: RebalanceBalanceRowViewModel["actionKind"] = trade
      ? "trade"
      : untraded
        ? "untraded"
        : "unselected";
    const actionLabel = trade
      ? `${trade.side === "buy" ? "Buy" : "Sell"} ${formatRebalanceMoney(
          trade.amount_base,
        )}`
      : untraded
        ? rebalanceUntradedReasonLabel(untraded.reason)
        : "—";
    const bar = balanceBar(entry);
    const afterGapLabel =
      entry.gap_after_percent === null
        ? formatRebalanceMoney(entry.gap_after_base)
        : `${formatRebalanceMoney(entry.gap_after_base)} (${formatGroupedNumber(entry.gap_after_percent)}%)`;

    return {
      instrument: entry.instrument,
      actionKind,
      actionLabel,
      bar,
      afterGapLabel,
      flipsSide:
        (entry.status_before === "below" && entry.status_after === "above") ||
        (entry.status_before === "above" && entry.status_after === "below"),
      is_new: entry.is_new,
    };
  });
}

function emptyTradeRowsMessage(untraded: RebalanceUntraded[]): string {
  if (
    untraded.length > 0 &&
    untraded.every((candidate) => candidate.reason === "on_target")
  ) {
    return "Portfolio is on target at this granularity.";
  }

  return "Too small to trade at this granularity.";
}

function buildAvailableViewModel(
  response: RebalanceResponse,
  sliderPosition: number,
  isFetching: boolean,
): RebalancePageViewModel {
  if (response.plan.status !== "available") {
    return {
      status: "unavailable",
      message: "Rebalance plan unavailable.",
      isRefreshing: isFetching,
      summary: null,
      slider: null,
      warningBanner: null,
      tradeRowsMessage: null,
      tradeRows: [],
      balanceRows: [],
      balanceTotalLabel: null,
      selectedRungFreshness: null,
    };
  }

  const selection = selectRebalanceRung(response, sliderPosition);
  if (!selection) {
    return {
      status: "unavailable",
      message: "Rebalance plan unavailable.",
      isRefreshing: isFetching,
      summary: null,
      slider: null,
      warningBanner: null,
      tradeRowsMessage: null,
      tradeRows: [],
      balanceRows: [],
      balanceTotalLabel: null,
      selectedRungFreshness: null,
    };
  }

  const { rung, position } = selection;
  const tradeCountLabel = rebalanceTradeCountLabel(
    rung.selected_count,
    rung.effective_trade_count,
  );
  const selectedRungFreshness = worstFreshness(
    rung.trades.map((trade) => trade.freshness),
  );
  const tradeRowsView = tradeRows(rung.trades);
  const warningBanner =
    selectedRungFreshness !== null &&
    freshnessKind(selectedRungFreshness) === "warning_stale"
      ? {
          label: freshnessLabel(selectedRungFreshness),
          message:
            "Selected rung includes warning-stale trades. Verify these broker orders before placing.",
        }
      : null;

  return {
    status: "available",
    message: null,
    isRefreshing: isFetching,
    summary: {
      requestedLabel: formatRebalanceMoney(response.amount_base),
      achievedNetLabel: formatRebalanceMoney(rung.achieved_net_base),
      residualLabel: formatRebalanceMoney(rung.residual_base),
    },
    slider: {
      value: position,
      max: response.plan.rungs.length,
      tradeCountLabel,
      coverageLabel:
        rung.coverage_percent === null ? null : `${rung.coverage_percent}%`,
    },
    warningBanner,
    tradeRowsMessage:
      tradeRowsView.length === 0 ? emptyTradeRowsMessage(rung.untraded) : null,
    tradeRows: tradeRowsView,
    balanceRows: buildRebalanceBalanceRows(rung),
    balanceTotalLabel: `${formatRebalanceMoney(
      rung.total_gap_before_base,
    )} → ${formatRebalanceMoney(rung.total_gap_after_base)}`,
    selectedRungFreshness,
  };
}

export function buildRebalancePageViewModel(
  input: BuildRebalancePageViewModelInput,
): RebalancePageViewModel {
  const normalizedAmount = normalizeRebalanceAmount(input.committedAmount);

  if (normalizedAmount === null && !input.isFetching) {
    const hasTypedAmount = input.amountInput.trim().length > 0;
    return {
      status: "prompt",
      message: hasTypedAmount
        ? "Enter a valid decimal amount."
        : "No valid amount entered yet. Enter a signed SEK amount to preview the rebalance ladder.",
      isRefreshing: false,
      summary: null,
      slider: null,
      warningBanner: null,
      tradeRowsMessage: null,
      tradeRows: [],
      balanceRows: [],
      balanceTotalLabel: null,
      selectedRungFreshness: null,
    };
  }

  if (input.response && normalizedAmount !== null) {
    if (input.response.plan.status === "available") {
      return buildAvailableViewModel(
        input.response,
        input.sliderPosition,
        input.isFetching,
      );
    }

    return {
      status: "unavailable",
      message: rebalanceUnavailableMessage(input.response.plan.reasons),
      isRefreshing: input.isFetching,
      summary: null,
      slider: null,
      warningBanner: null,
      tradeRowsMessage: null,
      tradeRows: [],
      balanceRows: [],
      balanceTotalLabel: null,
      selectedRungFreshness: null,
    };
  }

  if (input.isError) {
    return {
      status: "error",
      message: input.errorMessage ?? "Could not load rebalance plan.",
      isRefreshing: false,
      summary: null,
      slider: null,
      warningBanner: null,
      tradeRowsMessage: null,
      tradeRows: [],
      balanceRows: [],
      balanceTotalLabel: null,
      selectedRungFreshness: null,
    };
  }

  return {
    status: "loading",
    message: "Loading rebalance plan...",
    isRefreshing: false,
    summary: null,
    slider: null,
    warningBanner: null,
    tradeRowsMessage: null,
    tradeRows: [],
    balanceRows: [],
    balanceTotalLabel: null,
    selectedRungFreshness: null,
  };
}
