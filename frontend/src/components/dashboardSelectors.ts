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
