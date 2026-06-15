export type TransactionType = "Buy" | "Sell" | "Split" | "Dividend";
export type InstrumentType = "Stock" | "Etf" | "Fund";

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
}

export interface ApiErrorBody {
  error: { code: string; message: string; details?: unknown };
}

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
  new_instruments: number;
  warnings: number;
  errors: number;
}

export interface ImportNewInstrument {
  exchange: string;
  symbol: string;
  name: string;
  currency: string;
}

export interface ImportPreview {
  metadata: { title: string; date_from: string; date_to: string } | null;
  counts: ImportCounts;
  new_instruments: ImportNewInstrument[];
  warnings: ImportRowNote[];
  errors: ImportRowNote[];
  duplicate_of_batch_id: number | null;
}

export interface ImportResult {
  batch_id: number;
  counts: ImportCounts;
}

export interface RollbackResult {
  batch_id: number;
  removed: number;
}
