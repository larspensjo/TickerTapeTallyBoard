import type {
  Conviction,
  ConvictionTarget,
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
  dayChange: MoneyValue;
  dayChangePercent: PercentValue;
  quantity: number;
  averageCost: MoneyValue;
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

export interface HeaderStatus {
  label: string;
  tone: "neutral" | "warning";
}

export interface SplitEvent {
  id: number;
  tradeDate: string;
  quantityDelta: number;
  beforeQuantity: number;
  afterQuantity: number;
  ratioLabel: string;
  factor: number;
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
  const matching = rows.filter((row) => row.instrument.id === id);
  return (
    matching.find((row) => row.position_status !== "closed") ??
    matching[0] ??
    null
  );
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

export function canDeleteInstrument(args: {
  holding: Holding | null;
  transactions: Transaction[];
}): boolean {
  return args.transactions.length === 0 && (args.holding?.quantity ?? 0) === 0;
}

export function deleteInstrumentDisabledReason(args: {
  holding: Holding | null;
  transactions: Transaction[];
}): string | null {
  if (canDeleteInstrument(args)) {
    return null;
  }

  if (args.transactions.length > 0) {
    return "Only never-traded instruments can be deleted.";
  }

  if ((args.holding?.quantity ?? 0) !== 0) {
    return "Open positions cannot be deleted.";
  }

  return "Only never-traded instruments can be deleted.";
}

export function sharesSold(transactions: Transaction[]): number {
  return transactions
    .filter((transaction) => transaction.type === "Sell")
    .reduce((sum, transaction) => sum + Math.abs(transaction.quantity), 0);
}

export function splitEvents(transactions: Transaction[]): SplitEvent[] {
  const sorted = transactions
    .slice()
    .sort((a, b) => a.trade_date.localeCompare(b.trade_date) || a.id - b.id);
  const events: SplitEvent[] = [];
  let runningQuantity = 0;

  for (const transaction of sorted) {
    if (transaction.type === "Split") {
      const beforeQuantity = runningQuantity;
      const afterQuantity = runningQuantity + transaction.quantity;
      const factor =
        beforeQuantity > 0 && afterQuantity > 0
          ? afterQuantity / beforeQuantity
          : 0;
      events.push({
        id: transaction.id,
        tradeDate: transaction.trade_date,
        quantityDelta: transaction.quantity,
        beforeQuantity,
        afterQuantity,
        ratioLabel: splitRatioLabel(beforeQuantity, afterQuantity),
        factor,
      });
      runningQuantity = afterQuantity;
    } else if (transaction.type === "Buy" || transaction.type === "Sell") {
      runningQuantity += transaction.quantity;
    }
  }

  return events;
}

function splitRatioLabel(
  beforeQuantity: number,
  afterQuantity: number,
): string {
  if (beforeQuantity <= 0 || afterQuantity <= 0) {
    return "n/a";
  }

  const divisor = gcd(Math.abs(beforeQuantity), Math.abs(afterQuantity));
  return `${afterQuantity / divisor}:${beforeQuantity / divisor}`;
}

function gcd(a: number, b: number): number {
  let left = Math.trunc(a);
  let right = Math.trunc(b);

  while (right !== 0) {
    const next = left % right;
    left = right;
    right = next;
  }

  return left || 1;
}

function averageCostBase(holding: Holding | null): MoneyValue {
  if (holding?.base?.status === "available") {
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
      proceeds: gain.proceeds_base,
      costBasis: gain.cost_basis_base,
      sharesSold: sharesSold(transactions),
    };
  }

  return {
    status: "open",
    dayChange: gain.day_change_base,
    dayChangePercent: gain.day_change_percent,
    quantity: gain.quantity,
    averageCost: averageCostBase(holding),
  };
}

export interface ConvictionPanelView {
  /** Saved conviction for the instrument (the source of truth for the editor). */
  conviction: Conviction;
  /** Pool-wide target for the current open holding, or null for closed /
   * no-position instruments which have no current target. */
  target: ConvictionTarget | null;
}

export function convictionPanelView(
  instrument: Instrument,
  holding: Holding | null,
): ConvictionPanelView {
  return {
    conviction: instrument.conviction,
    target: holding?.conviction_target ?? null,
  };
}

/**
 * Reset is offered only when the saved conviction differs from the baseline
 * captured when the page first loaded for the current instrument.
 *
 * The baseline itself is held as component state keyed on the instrument id
 * (the Asset Detail panel remounts per id), so it is re-captured on navigation
 * and discarded on unmount without any render-time bookkeeping here.
 */
export function convictionResetVisible(
  baseline: Conviction,
  saved: Conviction,
): boolean {
  return baseline !== saved;
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
