import type {
  GainsRow,
  Holding,
  Instrument,
  MoneyValue,
  PercentValue,
  PriceStatusInstrument,
  Transaction,
} from "../api/types";
import { reasonLabel } from "./valuationDisplay";

export type AssetData =
  | { kind: "not-found" }
  | {
      kind: "no-row";
      instrument: Instrument;
      transactions: Transaction[];
      priceStatus: PriceStatusInstrument | null;
    }
  | {
      kind: "position";
      instrument: Instrument;
      gain: GainsRow;
      holding: Holding | null;
      transactions: Transaction[];
      priceStatus: PriceStatusInstrument | null;
    };

export interface OpenTiles {
  status: "open";
  marketValue: MoneyValue;
  unrealizedGain: MoneyValue;
  unrealizedPercent: PercentValue;
  dayChange: MoneyValue;
  dayChangePercent: PercentValue;
  quantity: number;
  averageCost: MoneyValue;
  costBasis: MoneyValue;
}

export interface ClosedTiles {
  status: "closed";
  realizedGain: MoneyValue;
  realizedPercent: PercentValue;
  proceeds: MoneyValue;
  costBasis: MoneyValue;
  sharesSold: number;
}

export type Tiles = OpenTiles | ClosedTiles;

export interface BreakdownView {
  priceEffect: MoneyValue;
  fxEffect: MoneyValue;
  totalLabel: string;
  total: MoneyValue;
}

export interface HeaderStatus {
  label: string;
  tone: "neutral" | "warning";
}

export function parseInstrumentId(raw: string | undefined): number | null {
  if (raw === undefined) {
    return null;
  }

  const parsed = Number(raw);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : null;
}

export function findInstrument(
  instruments: Instrument[],
  id: number,
): Instrument | null {
  return instruments.find((instrument) => instrument.id === id) ?? null;
}

export function findGainsRow(rows: GainsRow[], id: number): GainsRow | null {
  return rows.find((row) => row.instrument.id === id) ?? null;
}

export function findHolding(holdings: Holding[], id: number): Holding | null {
  return holdings.find((holding) => holding.instrument.id === id) ?? null;
}

export function findPriceStatus(
  instruments: PriceStatusInstrument[],
  id: number,
): PriceStatusInstrument | null {
  return instruments.find((entry) => entry.instrument_id === id) ?? null;
}

export function instrumentTransactions(
  transactions: Transaction[],
  id: number,
): Transaction[] {
  return transactions.filter((transaction) => transaction.instrument_id === id);
}

export function sharesSold(transactions: Transaction[]): number {
  return transactions
    .filter((transaction) => transaction.type === "Sell")
    .reduce((sum, transaction) => sum + Math.abs(transaction.quantity), 0);
}

function averageCostBase(holding: Holding | null): MoneyValue {
  if (holding && holding.base.status === "available") {
    return { status: "available", value: holding.base.average_cost_base };
  }

  return { status: "unavailable", reasons: ["base_cost_basis_unavailable"] };
}

export function deriveAssetData(args: {
  id: number | null;
  instruments: Instrument[];
  gainsRows: GainsRow[];
  holdings: Holding[];
  transactions: Transaction[];
  priceStatus: PriceStatusInstrument[];
}): AssetData {
  if (args.id === null) {
    return { kind: "not-found" };
  }

  const instrument = findInstrument(args.instruments, args.id);
  if (!instrument) {
    return { kind: "not-found" };
  }

  const transactions = instrumentTransactions(args.transactions, args.id);
  const priceStatus = findPriceStatus(args.priceStatus, args.id);
  const gain = findGainsRow(args.gainsRows, args.id);

  if (!gain) {
    return { kind: "no-row", instrument, transactions, priceStatus };
  }

  const holding = findHolding(args.holdings, args.id);
  return {
    kind: "position",
    instrument,
    gain,
    holding,
    transactions,
    priceStatus,
  };
}

export function tilesView(
  gain: GainsRow,
  holding: Holding | null,
  transactions: Transaction[],
): Tiles {
  if (gain.position_status === "closed") {
    return {
      status: "closed",
      realizedGain: gain.unrealized_gain_base,
      realizedPercent: gain.unrealized_gain_percent,
      proceeds: gain.market_value_base,
      costBasis: gain.cost_basis_base,
      sharesSold: sharesSold(transactions),
    };
  }

  return {
    status: "open",
    marketValue: gain.market_value_base,
    unrealizedGain: gain.unrealized_gain_base,
    unrealizedPercent: gain.unrealized_gain_percent,
    dayChange: gain.day_change_base,
    dayChangePercent: gain.day_change_percent,
    quantity: gain.quantity,
    averageCost: averageCostBase(holding),
    costBasis: gain.cost_basis_base,
  };
}

export function breakdownView(gain: GainsRow): BreakdownView {
  return {
    priceEffect: gain.price_effect_base,
    fxEffect: gain.fx_effect_base,
    totalLabel:
      gain.position_status === "closed" ? "Realized total" : "Unrealized total",
    total: gain.unrealized_gain_base,
  };
}

export function headerStatus(
  gain: GainsRow | null,
  priceStatus: PriceStatusInstrument | null,
): HeaderStatus {
  // Closed positions have no live price/FX by design, so the closed-position
  // label must outrank mapping/missing-price warnings (spec: closed rows show
  // "Closed position"). Check it before any price-status warning.
  if (gain?.position_status === "closed") {
    return { label: "Closed position", tone: "neutral" };
  }

  if (priceStatus && !priceStatus.mapping_enabled) {
    return { label: "Mapping disabled", tone: "warning" };
  }

  if (priceStatus && priceStatus.latest_price.status === "unmapped") {
    return { label: "Unmapped", tone: "warning" };
  }

  if (priceStatus && priceStatus.latest_price.status === "missing") {
    return { label: "Missing price", tone: "warning" };
  }

  if (gain) {
    if (gain.reasons.length > 0) {
      return { label: reasonLabel(gain.reasons[0]), tone: "warning" };
    }

    const priceFreshness = gain.latest_price?.freshness;
    if (priceFreshness && priceFreshness !== "fresh") {
      return { label: "Stale price", tone: "warning" };
    }

    const fxFreshness = gain.latest_fx?.freshness;
    if (fxFreshness && fxFreshness !== "fresh") {
      return { label: "Stale FX", tone: "warning" };
    }

    return { label: "Open position", tone: "neutral" };
  }

  return { label: "No position", tone: "neutral" };
}
