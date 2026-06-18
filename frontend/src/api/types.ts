export type TransactionType = "Buy" | "Sell" | "Split" | "Dividend";
export type InstrumentType = "Stock" | "Etf" | "Fund";

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
}

export interface Transaction {
  id: number;
  instrument_id: number;
  type: TransactionType;
  trade_date: string;
  quantity: number;
  price: string | null;
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
  cost_basis_native: string;
  average_cost_native: string;
  base: HoldingBase;
  valuation?: {
    market_value_base: MoneyValue;
    unrealized_gain_base: MoneyValue;
    unrealized_gain_percent: PercentValue;
    day_change_base: MoneyValue;
  } | null;
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

export interface GainsSummary {
  market_value_base: MoneyValue;
  cost_basis_base: MoneyValue;
  unrealized_gain_base: MoneyValue;
  unrealized_gain_percent: PercentValue;
  day_change_base: MoneyValue;
  day_change_percent: PercentValue;
  excluded_rows: number;
}

export interface GainsRow {
  instrument: Instrument;
  quantity: number;
  cost_basis_native: string;
  cost_basis_base: MoneyValue;
  latest_price: PriceSnapshot | null;
  previous_price: PriceSnapshot | null;
  latest_fx: FxSnapshot | null;
  previous_fx: FxSnapshot | null;
  market_value_native: MoneyValue;
  market_value_base: MoneyValue;
  unrealized_gain_base: MoneyValue;
  unrealized_gain_percent: PercentValue;
  day_change_base: MoneyValue;
  day_change_percent: PercentValue;
  reasons: string[];
}

export interface GainsResponse {
  as_of_date: string;
  base_currency: string;
  summary: GainsSummary;
  rows: GainsRow[];
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
  default_selected: boolean;
  skipped_reason: string | null;
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  is_new_instrument: boolean;
}

export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  assets: ImportAssetGroup[];
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
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
