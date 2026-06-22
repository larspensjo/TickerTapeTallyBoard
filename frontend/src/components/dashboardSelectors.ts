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

export function allocationBreakdown(
  rows: GainsRow[],
  dimension: AllocationDimension,
): Allocation {
  const totals = new Map<string, number>();
  let total = 0;
  let excludedCount = 0;

  for (const row of rows) {
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
    total += value;
  }

  if (total === 0) {
    return { slices: [], excludedCount };
  }

  const slices: AllocationSlice[] = [...totals.entries()]
    .map(([key, valueBase]) => ({
      key,
      label: key,
      valueBase,
      weightPercent: (valueBase / total) * 100,
    }))
    .sort(
      (a, b) => b.valueBase - a.valueBase || a.label.localeCompare(b.label),
    );

  return { slices, excludedCount };
}
