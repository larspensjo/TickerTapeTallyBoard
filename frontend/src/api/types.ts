export type TransactionType = "Buy" | "Sell" | "Split" | "Dividend";
export type InstrumentType = "Stock" | "Etf" | "Fund";
export type ReturnMethod = "xirr" | "simple" | "modified_dietz";
export type GainsPeriod =
  | "today"
  | "7d"
  | "30d"
  | "12m"
  | "ytd"
  | "all"
  | "custom";
export type Conviction = "Other" | "Low" | "Medium" | "High";
export type TargetStatus =
  | "below"
  | "on_target"
  | "above"
  | "no_target"
  | "excluded_unavailable"
  | "unavailable";

export type AvailabilityValue<T> =
  | { status: "available"; value: T }
  | { status: "unavailable"; reasons: string[] };

export type MoneyValue = AvailabilityValue<string>;
export type PercentValue = AvailabilityValue<string>;

export type Ticker = string | null;
export type FigureCertainty =
  | "complete"
  | "approximate"
  | "not_applicable"
  | "unavailable";
export type FigureReasonCode =
  | "fund_or_etf"
  | "not_a_company"
  | "certificate"
  | "no_fundamentals_source"
  | "source_ambiguous"
  | "source_unreachable"
  | "security_type_unknown"
  | "awaiting_figures"
  | "too_few_quarters"
  | "year_ago_quarter_missing"
  | "latest_quarter_too_old"
  | "report_currency_changed"
  | "loss_making"
  | "non_positive_revenue"
  | "price_unavailable"
  | "fx_unavailable"
  | "share_basis_unverified"
  | "listing_unit_mismatch"
  | "split_since_report"
  | "figures_inconsistent"
  | "implausible_ratio"
  | "ttm_from_three_quarters"
  | "eps_derived_from_net_income"
  | "price_stale"
  | "newer_report_expected"
  | "figures_disagree"
  | "estimate_passed"
  | "report_date_passed"
  | "quarter_missing";
export interface FigureReason {
  code: FigureReasonCode;
  detail?: string;
}
export interface Figure {
  certainty: FigureCertainty;
  value: string | null;
  reasons: FigureReason[];
}
export type ReportCertainty =
  | "confirmed"
  | "estimated"
  | "observed"
  | "period_only"
  | "not_applicable"
  | "unavailable";
export type ReportBasis =
  | "company_announced"
  | "nasdaq_expected"
  | "last_year_date"
  | "typical_gap";
export interface ReportDate {
  certainty: ReportCertainty;
  basis: ReportBasis | null;
  date: string | null;
  days_until: number | null;
  period_end: string | null;
  observed_between: [string, string] | null;
  reasons: FigureReason[];
}
export interface HoldingsFundamentals {
  pe: Figure;
  ps: Figure;
  next_report: ReportDate;
}

export interface Instrument {
  ticker: Ticker;
  id: number;
  symbol: string;
  exchange: string;
  name: string;
  type: InstrumentType;
  currency: string;
  conviction: Conviction;
}

export interface CreateInstrumentInput {
  symbol: string;
  exchange: string;
  name: string;
  type: InstrumentType;
  currency: string;
  isin?: string;
}

export interface ConvictionTarget {
  conviction: Conviction;
  status: TargetStatus;
  target_value_base: MoneyValue;
  target_gap_base: MoneyValue;
  target_gap_percent: PercentValue;
}

export interface Transaction {
  id: number;
  instrument_id: number;
  type: TransactionType;
  trade_date: string;
  quantity: number;
  price: string | null;
  dividend_per_share: string | null;
  currency: string | null;
  fx_rate_to_base: string | null;
  brokerage: string | null;
  brokerage_currency: string | null;
  source_value: string | null;
  source_currency: string | null;
  note: string | null;
  import_batch_id: number | null;
}

export type HoldingBase =
  | {
      status: "available";
      cost_basis_base: string;
      average_cost_base: string;
      fee_component_base: string;
    }
  | {
      status: "unavailable";
      reasons: { code: string; transaction_id: number }[];
    };

export interface Holding {
  fundamentals: HoldingsFundamentals;
  instrument: Instrument;
  quantity: number;
  cost_basis_native: string | null;
  average_cost_native: string | null;
  base: HoldingBase | null;
  valuation?: {
    market_value_base: MoneyValue;
    unrealized_gain_base: MoneyValue;
    unrealized_gain_percent: PercentValue;
    day_change_base: MoneyValue;
  } | null;
  conviction_target: ConvictionTarget;
  row_kind: "open" | "watchlist";
}

export interface HoldingsResponse {
  data_revision: string;
  holdings: Holding[];
  hidden_watchlist_pool_count: number;
}

export type InstrumentLookupStatus =
  | "matches"
  | "no_match"
  | "provider_unavailable";

export interface InstrumentLookupMatch {
  provider: string;
  provider_symbol: string;
  quote_type: string | null;
  exchange: string | null;
  name: string | null;
  asset_class: string | null;
  currency: string | null;
}

export interface InstrumentLookupResponse {
  query: string;
  status: InstrumentLookupStatus;
  matches: InstrumentLookupMatch[];
}

export interface PriceSnapshot {
  date: string;
  close: string;
  currency: string;
  source: string;
  freshness: string;
}

export interface FxSnapshot {
  date: string;
  rate: string;
  base: string;
  quote: string;
  freshness: string;
}

export interface PriceHistoryPoint {
  date: string;
  close: string;
  source: string;
  close_base: MoneyValue;
  fx?: { rate: string; date: string };
}

export interface PriceHistoryResponse {
  instrument_id: number;
  currency: string;
  base_currency: string;
  points: PriceHistoryPoint[];
  running_average_cost: RunningAverageCost;
}

/** End-of-day running weighted-average native cost per share after a buy day. */
export interface RunningAverageCostStep {
  date: string;
  /** Native currency, split-adjusted, fees excluded. */
  average: string;
}

/** One held period; `closed_on` is null while the position is still open. */
export interface RunningAverageCostSegment {
  steps: RunningAverageCostStep[];
  closed_on: string | null;
}

export interface RunningAverageCost {
  segments: RunningAverageCostSegment[];
  /** Display text for the open position's native average (as Holdings shows it), or null when flat. */
  current_average: string | null;
}

export interface ValueHistoryPoint {
  date: string;
  value_base: string;
  invested_base: string | null;
  incomplete: boolean;
  included_count: number;
  excluded_count: number;
}

export interface ValueHistoryResponse {
  data_revision: string;
  base_currency: string;
  start_date: string | null;
  points: ValueHistoryPoint[];
}

export interface GainsSummary {
  market_value_base: MoneyValue;
  cost_basis_base: MoneyValue;
  unrealized_gain_base: MoneyValue;
  unrealized_gain_percent: PercentValue;
  price_effect_base: MoneyValue;
  fx_effect_base: MoneyValue;
  day_change_base: MoneyValue;
  day_change_percent: PercentValue;
  excluded_rows: number;
}

export interface GainsTotals {
  capital_gain_base: MoneyValue;
  capital_gain_percent: PercentValue;
  income_base: MoneyValue;
  income_percent: PercentValue;
  currency_gain_base: MoneyValue;
  currency_gain_percent: PercentValue;
  total_return_base: MoneyValue;
  total_return_percent: PercentValue;
  excluded_rows: number;
}

export interface PortfolioWaterfall {
  cost_basis_base: MoneyValue;
  held_fee_component_base: MoneyValue;
  price_effect_base: MoneyValue;
  fx_effect_base: MoneyValue;
  market_value_base: MoneyValue;
  realized_gain_base: MoneyValue;
  realized_fee_base: MoneyValue;
  realized_cost_basis_base: MoneyValue;
  brokerage_total_base: MoneyValue;
  income_base: MoneyValue;
  unrealized_gain_base: MoneyValue;
  total_return_base: MoneyValue;
  income_not_tracked: boolean;
  excluded_rows: number;
}

export interface GainsRow {
  instrument: Instrument;
  quantity: number;
  cost_basis_native: string;
  cost_basis_base: MoneyValue;
  performance_start_date: string | null;
  performance_denominator_base: MoneyValue;
  capital_gain_base: MoneyValue;
  capital_gain_percent: PercentValue;
  income_base: MoneyValue;
  currency_gain_base: MoneyValue;
  currency_gain_percent: PercentValue;
  total_return_base: MoneyValue;
  total_return_percent: PercentValue;
  latest_price: PriceSnapshot | null;
  previous_price: PriceSnapshot | null;
  latest_fx: FxSnapshot | null;
  previous_fx: FxSnapshot | null;
  market_value_native: MoneyValue;
  market_value_base: MoneyValue;
  proceeds_native: MoneyValue;
  proceeds_base: MoneyValue;
  held_fee_component_base?: MoneyValue;
  realized_fee_base?: MoneyValue;
  realized_sell_brokerage_base?: MoneyValue;
  brokerage_total_base?: MoneyValue;
  unrealized_price_effect_base?: MoneyValue;
  unrealized_fx_effect_base?: MoneyValue;
  unrealized_gain_base: MoneyValue;
  unrealized_gain_percent: PercentValue;
  realized_gain_base: MoneyValue;
  realized_cost_basis_base: MoneyValue;
  price_effect_base: MoneyValue;
  fx_effect_base: MoneyValue;
  day_change_base: MoneyValue;
  day_change_percent: PercentValue;
  reasons: string[];
  position_status: "open" | "closed";
}

export interface ReportPeriod {
  start_date: string | null;
  end_date: string;
}

export interface DateRange {
  startDate: string | null;
  endDate: string | null;
}

export interface GainsResponse {
  data_revision: string;
  as_of_date: string;
  base_currency: string;
  include_closed_positions: boolean;
  summary: GainsSummary;
  totals: GainsTotals;
  rows: GainsRow[];
  portfolio_waterfall: PortfolioWaterfall;
  report_period: ReportPeriod;
  percentage_method: "money_weighted" | "simple" | "modified_dietz";
  display_percent_kind: string;
}

export interface RebalanceResponse {
  data_revision: string;
  amount_base: string;
  base_currency: string;
  plan: RebalancePlan;
}

export const REBALANCE_RANK_BY_VALUES = ["sek", "percent"] as const;
export type RebalanceRankBy = (typeof REBALANCE_RANK_BY_VALUES)[number];

export type RebalanceUnavailableReason = "empty_pool" | "offset_exceeds_pool";

export type RebalancePlan =
  | {
      status: "available";
      pool_value_base: string;
      candidate_count: number;
      rungs: RebalanceRung[];
    }
  | {
      status: "unavailable";
      reasons: RebalanceUnavailableReason[];
    };

export interface RebalanceRung {
  selected_count: number;
  effective_trade_count: number;
  trades: RebalanceTrade[];
  untraded: RebalanceUntraded[];
  balance: RebalanceBalanceEntry[];
  achieved_net_base: string;
  residual_base: string;
  coverage_percent: string | null;
  total_gap_before_base: string;
  total_gap_after_base: string;
}

export interface RebalanceTrade {
  instrument: Instrument;
  side: "buy" | "sell";
  shares: number;
  price_base: string;
  amount_base: string;
  freshness: string;
  is_new: boolean;
}

export interface RebalanceUntraded {
  instrument: Instrument;
  reason: string;
}

export interface RebalanceBalanceEntry {
  instrument: Instrument;
  gap_before_base: string;
  gap_after_base: string;
  gap_before_percent: string | null;
  gap_after_percent: string | null;
  status_before: TargetStatus;
  status_after: TargetStatus;
  is_new: boolean;
}

export const OPPORTUNITY_WINDOWS = ["7d", "14d", "30d"] as const;
export type OpportunityWindow = (typeof OPPORTUNITY_WINDOWS)[number];

export const OPPORTUNITY_ORDERS = [
  "best_candidates_first",
  "fallers_first",
  "risers_first",
] as const;
export type OpportunityOrder = (typeof OPPORTUNITY_ORDERS)[number];

/** The period the backend resolved for the requested window. */
export interface OpportunityPeriod {
  window: OpportunityWindow;
  start_date: string;
  end_date: string;
}

/**
 * Close-to-close share-price change in the instrument's own currency. The
 * closes are the actual dated closes used, at full stored precision.
 */
export type PriceChange =
  | {
      status: "available";
      start_date: string;
      start_close: string;
      end_date: string;
      end_close: string;
      change_percent: string;
    }
  | { status: "unavailable"; reasons: string[] };

export type OpportunityVerdictCode =
  | "sentiment_dip_candidate"
  | "possible_candidate_report_timing_unknown"
  | "not_enough_data"
  | "fell_for_a_reason"
  | "normal_movement";
export type OpportunityDecidingCheck =
  | "unusual_drop"
  | "off_high"
  | "report_in_period"
  | "business";
export type OpportunityCaution =
  | "loss_making"
  | "report_coming_soon"
  | "already_at_target"
  | "no_target";

export interface OpportunityEvidenceReason {
  code: string;
  detail?: string;
  start?: string;
  date?: string;
  count?: number;
  required?: number;
}

export interface OpportunityCheck<State extends string> {
  state: State;
  reasons: OpportunityEvidenceReason[];
}
type OpportunityYesNoUnknown = "yes" | "no" | "unknown";
type OpportunityCompanyCheckState = OpportunityYesNoUnknown | "not_applicable";

export type OpportunityLastReport =
  | { kind: "confirmed"; date: string }
  | {
      kind: "interval";
      period_end: string;
      absent_on: string;
      first_seen: string;
    }
  | { kind: "upper_bound"; period_end: string; first_seen: string };

export interface OpportunityBusinessSubtest {
  passes: boolean | null;
  value: string | null;
  year_ago_value: string | null;
  approximate: boolean;
  reasons: OpportunityEvidenceReason[];
}

export interface OpportunityEvidence {
  verdict: {
    code: OpportunityVerdictCode;
    deciding: OpportunityDecidingCheck[];
    missing: OpportunityDecidingCheck[];
    cautions: OpportunityCaution[];
  };
  checks: {
    unusual_drop: OpportunityCheck<OpportunityYesNoUnknown> & {
      change_percent: string;
      adjusted_change_percent: string | null;
      usual_move_percent: string | null;
      threshold_percent: string | null;
      closes: number;
      sample_moves: number | null;
    };
    off_high: OpportunityCheck<"deep" | "not_deep" | "unknown"> & {
      closes: number;
      latest_close: string | null;
      high_close: string | null;
      low_close: string | null;
      below_high_percent: string | null;
      above_low_percent: string | null;
      near_low: boolean | null;
    };
    report_in_period: OpportunityCheck<OpportunityCompanyCheckState> & {
      window_start: string;
      window_end: string;
      last_report: OpportunityLastReport | null;
      scheduled_report: ReportDate | null;
      no_report_expected_before: string | null;
    };
    report_coming_soon: OpportunityCheck<OpportunityCompanyCheckState> & {
      report: ReportDate;
    };
    business: OpportunityCheck<
      "intact" | "weakening" | "loss_making" | "unknown" | "not_applicable"
    > & {
      profitable: OpportunityBusinessSubtest;
      sales_not_shrinking: OpportunityBusinessSubtest;
      earnings_not_sliding: OpportunityBusinessSubtest;
      eps_consistency: {
        state:
          | "unavailable"
          | "not_cross_checked"
          | "consistent"
          | "approximate"
          | "inconsistent";
        gap_percent: string | null;
        compared_quarters: number;
      };
    };
    market: OpportunityCheck<
      | "fell_more_than_market"
      | "moved_with_market"
      | "fell_less_than_market"
      | "unknown"
    > & {
      change_percent: string;
      reference_percent: string | null;
      difference_percent: string | null;
      margin_percent: string;
      count: number;
    } & (
        | {
            source: "market_index";
            index_code: string;
            index_name: string;
            index_start_date: string;
            index_end_date: string;
          }
        | { source: "other_holdings_median" }
      );
    stopped_falling: OpportunityCheck<OpportunityYesNoUnknown> & {
      latest_close: string | null;
      low_close: string | null;
      low_date: string | null;
      low_age_trading_days: number | null;
    };
    ex_dividend: OpportunityCheck<OpportunityYesNoUnknown> & {
      events: { ex_date: string; amount: string; currency: string }[] | null;
      amount: string | null;
      percent_of_start_close: string | null;
    };
    room_to_buy: OpportunityCheck<
      "room_to_buy" | "already_at_target" | "no_target" | "unknown"
    > & {
      target_status: TargetStatus;
    };
  };
}

export interface OpportunityRow {
  evidence: OpportunityEvidence | null;
  instrument: Instrument;
  quantity: number;
  latest_close: PriceSnapshot | null;
  price_change: PriceChange;
  average_cost_native: string;
  price_vs_average_percent: PercentValue;
  below_average: boolean | null;
  unrealized_gain_percent: PercentValue;
  unrealized_gain_freshness: string | null;
  conviction_target: ConvictionTarget;
}

export interface OpportunitiesResponse {
  data_revision: string;
  period: OpportunityPeriod;
  rows: OpportunityRow[];
}

export interface RefreshRunSummary {
  run_id: number;
  trigger: RefreshTrigger;
  mode: RefreshMode;
  status: RefreshRunStatus;
  started_at: string;
  finished_at: string | null;
  message: string | null;
  prices_written: number;
  fx_rates_written: number;
  unmapped_instruments: number;
  failed_items: number;
}

export type PriceSnapshotStatus = "available" | "missing" | "unmapped";

export interface PriceSnapshotState {
  status: PriceSnapshotStatus;
  date: string | null;
  value: string | null;
  provider: string | null;
  provider_symbol: string | null;
  reason: string | null;
}

export interface PriceSourceStatus {
  provider: string;
  provider_symbol: string;
  asset_class: string | null;
  currency: string | null;
  enabled: boolean;
}

export interface PriceStatusInstrument {
  instrument_id: number;
  exchange: string;
  symbol: string;
  currency: string;
  price_sources: PriceSourceStatus[];
  effective_price_source: string | null;
  open_quantity: number;
  latest_price: PriceSnapshotState;
  latest_fx: PriceSnapshotState;
}

export interface PriceStatusResponse {
  refreshing: boolean;
  latest_run: RefreshRunSummary | null;
  instruments: PriceStatusInstrument[];
}

export interface HealthResponse {
  status: string;
  version: string;
  mode: "production" | "development" | "demo";
  ledger: { mode: "file" | "memory"; path: string | null };
  backup: {
    directory: string;
    last_snapshot_at: string | null;
    snapshot_count: number;
    launch_status: "succeeded" | "failed" | "skipped" | "disabled";
    launch_error: string | null;
    listing_error: string | null;
  };
  build: { package: string; profile: string };
}

export type RefreshMode = "latest" | "backfill";
export type RefreshTrigger = "manual" | "backfill" | "launch";
export type RefreshRunStatus = "running" | "succeeded" | "partial" | "failed";
export type RefreshItemKind =
  | "price"
  | "fx"
  | "history"
  | "dividend"
  | "market_index";
export type RefreshItemStatus =
  | "fetched"
  | "missing"
  | "failed"
  | "unmapped"
  | "ambiguous"
  | "unavailable";

export interface RefreshPricesInput {
  mode: RefreshMode;
  start_date?: string | null;
  end_date?: string | null;
}

export interface RefreshItem {
  kind: RefreshItemKind;
  instrument_id: number | null;
  provider: string | null;
  symbol_or_pair: string;
  status: RefreshItemStatus;
  reason: string | null;
  rows_written: number;
}

export interface RefreshPricesResult {
  run_id: number;
  trigger: RefreshTrigger;
  mode: RefreshMode;
  status: RefreshRunStatus;
  started_at: string;
  finished_at: string | null;
  message: string | null;
  prices_written: number;
  fx_rates_written: number;
  unmapped_instruments: number;
  failed_items: number;
  items: RefreshItem[];
}

export interface ApiErrorBody {
  error: { code: string; message: string; details?: unknown };
}

export type ImportSource = "sharesight" | "avanza";

export interface ImportRowNote {
  row: number | null;
  code: string;
  message: string;
}

export interface ImportCounts {
  rows: number;
  buys: number;
  sells: number;
  splits: number;
  dividends: number;
  new_instruments: number;
  skipped: number;
  warnings: number;
  errors: number;
}

export interface ImportNewInstrument {
  exchange: string;
  symbol: string;
  name: string;
  currency: string;
  isin: string | null;
}

export interface ImportAssetGroup {
  asset_key: string;
  name: string;
  currency: string;
  buys: number;
  sells: number;
  splits: number;
  dividends: number;
  already_imported_buys: number;
  already_imported_sells: number;
  already_imported_splits: number;
  already_imported_dividends: number;
  default_selected: boolean;
  skipped_reason: string | null;
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  is_new_instrument: boolean;
}

export interface ConvictionClosePosition {
  instrument_id: number;
  asset_key: string;
  symbol: string;
  conviction: Conviction;
}

export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  assets: ImportAssetGroup[];
  already_imported_assets: ImportAssetGroup[];
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
  /** Batch id to target for a refresh commit. Null when no prior Avanza batch exists. */
  replace_candidate_batch_id: number | null;
  /** Non-blocking warning when multiple live Avanza batches exist. */
  replace_candidate_warning: string | null;
  /** Convicted open positions this import would close; each needs a keep/clear
   * choice before commit. */
  conviction_close_positions: ConvictionClosePosition[];
}

export interface ImportResult {
  batch_id: number;
  counts: ImportCounts;
  warnings: ImportRowNote[];
}

export interface RollbackResult {
  batch_id: number;
  removed: number;
}

export type PreviousFundamentalsLink = "same_company" | "wrong_company";
export interface FundamentalsCorrection {
  provider?: "YAHOO" | null;
  provider_symbol?: string;
  not_a_company?: boolean;
  previous_link?: PreviousFundamentalsLink;
}
export interface FundamentalsPreview {
  status: "found" | "no_match" | "provider_unavailable";
  symbol: string;
  name: string | null;
  security_type: string | null;
  report_currency: string | null;
  quarters: number;
  latest_period_end: string | null;
  data_revision: string;
}
export interface FundamentalsSource {
  symbol: string | null;
  origin: "DISCOVERED" | "MANUAL" | null;
  security_type: string | null;
  classification:
    | "EQUITY"
    | "FUND"
    | "CERTIFICATE"
    | "NOT_COMPANY"
    | "UNKNOWN"
    | "FIGURES";
  status:
    | "PENDING"
    | "USABLE"
    | "NOT_APPLICABLE"
    | "AMBIGUOUS"
    | "NO_MATCH"
    | "UNAVAILABLE";
  candidates: {
    symbol: string;
    name: string | null;
    security_type?: string | null;
    exchange?: string | null;
  }[];
  last_success: string | null;
  last_success_on: string | null;
  last_attempt_on: string | null;
  last_attempt_at: string | null;
  last_attempt_outcome: "SUCCEEDED" | "EMPTY" | "FAILED" | null;
  last_attempt_reason: string | null;
  awaiting_figures: boolean;
  has_active_figures: boolean;
  pending_update: boolean;
}
export interface FundamentalsQuarter {
  period_end: string;
  missing: boolean;
  sales_missing: boolean;
  eps_missing: boolean;
  currency: string | null;
  sales: Figure;
  eps: Figure;
}
export interface InstrumentFundamentals {
  pe: Figure;
  ps: Figure;
  profit_margin: Figure;
  sales_growth: Figure;
  next_report: ReportDate;
  last_report: ReportDate;
  source: FundamentalsSource;
  quarters: FundamentalsQuarter[];
  fx_conversion: {
    from: string;
    to: string;
    rate: string;
    date: string;
    from_rate_date: string;
    to_rate_date: string;
  } | null;
  share_basis: {
    state: "established" | "listing_unit_mismatch" | "share_basis_unverified";
    ratio: string | null;
    evidence_date: string | null;
  };
  data_revision: string;
}
