export type TransactionType = "Buy" | "Sell" | "Split" | "Dividend";
export type InstrumentType = "Stock" | "Etf" | "Fund";
export type ReturnMethod = "xirr" | "simple" | "modified_dietz";
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

export interface Instrument {
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
  close_base: MoneyValue;
  fx?: { rate: string; date: string };
}

export interface PriceHistoryResponse {
  instrument_id: number;
  currency: string;
  base_currency: string;
  points: PriceHistoryPoint[];
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
  as_of_date: string;
  base_currency: string;
  include_closed_positions: boolean;
  summary: GainsSummary;
  totals: GainsTotals;
  rows: GainsRow[];
  report_period: ReportPeriod;
  percentage_method: "money_weighted" | "simple" | "modified_dietz";
  display_percent_kind: string;
}

export interface RebalanceResponse {
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

export interface PriceStatusInstrument {
  instrument_id: number;
  exchange: string;
  symbol: string;
  currency: string;
  mapping_enabled: boolean;
  provider_symbol: string | null;
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
  demo: boolean;
  build: { package: string; profile: string };
}

export type RefreshMode = "latest" | "backfill";
export type RefreshTrigger = "manual" | "backfill" | "launch";
export type RefreshRunStatus = "running" | "succeeded" | "partial" | "failed";
export type RefreshItemKind = "price" | "fx";
export type RefreshItemStatus = "fetched" | "missing" | "failed" | "unmapped";

export interface RefreshPricesInput {
  mode: RefreshMode;
  start_date?: string | null;
  end_date?: string | null;
}

export interface RefreshItem {
  kind: RefreshItemKind;
  instrument_id: number | null;
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
