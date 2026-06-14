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
