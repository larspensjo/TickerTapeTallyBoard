import type { GainsRow } from "../api/types";

export interface TreemapTile {
  symbol: string;
  exchange: string;
  marketValueBase: number;
  totalReturnPercent: number | null;
}

export function treemapViewModel(rows: GainsRow[]): TreemapTile[] {
  const tiles: TreemapTile[] = [];

  for (const row of rows) {
    if (row.position_status !== "open") continue;
    if (row.market_value_base.status !== "available") continue;

    const marketValueBase = Number(row.market_value_base.value);
    if (!Number.isFinite(marketValueBase) || marketValueBase <= 0) continue;

    let totalReturnPercent: number | null = null;
    const pct = row.total_return_percent;
    if (pct.status === "available") {
      const v = Number(pct.value);
      if (Number.isFinite(v)) totalReturnPercent = v;
    }

    tiles.push({
      symbol: row.instrument.symbol,
      exchange: row.instrument.exchange,
      marketValueBase,
      totalReturnPercent,
    });
  }

  return tiles.sort((a, b) => b.marketValueBase - a.marketValueBase);
}
