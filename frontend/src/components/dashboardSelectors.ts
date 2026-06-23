import type { GainsRow, Instrument } from "../api/types";

export interface MoverRow {
  instrument: Instrument;
  percent: number;
}

type Candidate = MoverRow;

function tieBreak(a: Candidate, b: Candidate): number {
  const bySymbol = a.instrument.symbol.localeCompare(b.instrument.symbol);
  if (bySymbol !== 0) return bySymbol;
  return a.instrument.name.localeCompare(b.instrument.name);
}

export function topMovers(rows: GainsRow[]): {
  gainers: MoverRow[];
  losers: MoverRow[];
} {
  const candidates: Candidate[] = [];
  for (const row of rows) {
    if (row.position_status !== "open") continue;
    if (row.day_change_percent.status !== "available") continue;
    const percent = Number(row.day_change_percent.value);
    if (!Number.isFinite(percent)) continue;
    candidates.push({ instrument: row.instrument, percent });
  }

  const gainers = candidates
    .filter((candidate) => candidate.percent > 0)
    .sort((a, b) => b.percent - a.percent || tieBreak(a, b))
    .slice(0, 3);

  const losers = candidates
    .filter((candidate) => candidate.percent < 0)
    .sort((a, b) => a.percent - b.percent || tieBreak(a, b))
    .slice(0, 3);

  return { gainers, losers };
}

export type AllocationDimension = "instrument" | "currency" | "type";

export interface AllocationSlice {
  key: string;
  label: string;
  /** Muted secondary identifier (the ISIN for instrument allocation). */
  secondary?: string;
  valueBase: number;
  weightPercent: number;
}

export interface Allocation {
  slices: AllocationSlice[];
  excludedCount: number;
}

function bucketKey(row: GainsRow, dimension: AllocationDimension): string {
  switch (dimension) {
    case "instrument":
      return row.instrument.symbol;
    case "currency":
      return row.instrument.currency;
    case "type":
      return row.instrument.type;
  }
}

/**
 * Human-facing label for a bucket. For instrument allocation we lead with the
 * recognizable name and keep the ISIN (symbol) as muted secondary text, falling
 * back to the symbol when no name is known.
 */
function bucketLabels(
  row: GainsRow,
  dimension: AllocationDimension,
): { label: string; secondary?: string } {
  if (dimension !== "instrument") {
    return { label: bucketKey(row, dimension) };
  }
  const name = row.instrument.name.trim();
  const symbol = row.instrument.symbol.trim();
  const label = name || symbol;
  return { label, secondary: symbol && symbol !== label ? symbol : undefined };
}

/**
 * Build display-only allocation weights from currently open positions.
 * Unavailable market values are counted as exclusions instead of being treated
 * as zero, while percentage weights intentionally use floats for chart display.
 */
export function allocationBreakdown(
  rows: GainsRow[],
  dimension: AllocationDimension,
): Allocation {
  const totals = new Map<string, number>();
  const labels = new Map<string, { label: string; secondary?: string }>();
  let total = 0;
  let excludedCount = 0;

  for (const row of rows) {
    if (row.position_status !== "open") continue;

    if (row.market_value_base.status !== "available") {
      excludedCount += 1;
      continue;
    }

    const value = Number(row.market_value_base.value);
    if (!Number.isFinite(value) || value === 0) {
      continue;
    }

    const key = bucketKey(row, dimension);
    totals.set(key, (totals.get(key) ?? 0) + value);
    if (!labels.has(key)) {
      labels.set(key, bucketLabels(row, dimension));
    }
    total += value;
  }

  if (total === 0) {
    return { slices: [], excludedCount };
  }

  const slices: AllocationSlice[] = [...totals.entries()]
    .map(([key, valueBase]) => ({
      key,
      label: labels.get(key)?.label ?? key,
      secondary: labels.get(key)?.secondary,
      valueBase,
      weightPercent: (valueBase / total) * 100,
    }))
    .sort(
      (a, b) => b.valueBase - a.valueBase || a.label.localeCompare(b.label),
    );

  return { slices, excludedCount };
}
