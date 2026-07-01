import {
  type HierarchyRectangularNode,
  hierarchy,
  treemap,
  treemapSquarify,
} from "d3-hierarchy";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { GainsRow } from "../api/types";
import { type TreemapTile, treemapViewModel } from "./treemapViewModel";
import { formatGroupedNumber } from "./valuationDisplay";

// d3-hierarchy datum: the root holds children; each leaf IS a TreemapTile.
type RootDatum = { children: TreemapTile[] };
type Datum = RootDatum | TreemapTile;

function computeLeaves(
  tiles: TreemapTile[],
  width: number,
  height: number,
): HierarchyRectangularNode<Datum>[] {
  if (!tiles.length || width <= 0 || height <= 0) return [];
  const root = hierarchy<Datum>({ children: tiles } as RootDatum)
    .sum((d) => ("marketValueBase" in d ? d.marketValueBase : 0))
    .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
  const laidOut = treemap<Datum>()
    .size([width, height])
    .paddingOuter(2)
    .paddingInner(2)
    .tile(treemapSquarify)(root);
  return laidOut.leaves();
}

function tileModifier(percent: number | null): string {
  if (percent === null) return "";
  if (percent > 0) return " treemap-tile--up";
  if (percent < 0) return " treemap-tile--down";
  return "";
}

function tileColor(percent: number | null): string {
  if (percent === null) return "var(--text-secondary)";
  if (percent > 0) return "var(--up)";
  if (percent < 0) return "var(--down)";
  return "var(--text-secondary)";
}

function formatPct(percent: number | null): string {
  if (percent === null) return "—";
  return `${percent >= 0 ? "+" : ""}${percent.toFixed(2)}%`;
}

export function PortfolioTreemap({ rows }: { rows: GainsRow[] }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    setSize({ width: el.clientWidth, height: el.clientHeight });
    const observer = new ResizeObserver(() =>
      setSize({ width: el.clientWidth, height: el.clientHeight }),
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const tiles = useMemo(() => treemapViewModel(rows), [rows]);
  const leaves = useMemo(
    () => computeLeaves(tiles, size.width, size.height),
    [tiles, size],
  );

  return (
    <div ref={containerRef} className="treemap-container">
      {tiles.length === 0 ? (
        <p className="treemap-empty">No valued open holdings to display.</p>
      ) : (
        <ul className="treemap-list" aria-label="Portfolio holdings treemap">
          {leaves.map((leaf) => {
            const tile = leaf.data as TreemapTile;
            const w = leaf.x1 - leaf.x0;
            const h = leaf.y1 - leaf.y0;
            const isSmall = w < 64 || h < 44;
            const value = `SEK ${formatGroupedNumber(tile.marketValueBase.toFixed(2))}`;
            const displayName =
              tile.name.trim() || `${tile.symbol}.${tile.exchange}`;
            const label = `${displayName}: ${value}, ${formatPct(tile.totalReturnPercent)}`;
            return (
              <li
                key={`${tile.symbol}.${tile.exchange}`}
                className={`treemap-tile${tileModifier(tile.totalReturnPercent)}${isSmall ? " treemap-tile--small" : ""}`}
                style={{ left: leaf.x0, top: leaf.y0, width: w, height: h }}
                title={label}
                aria-label={label}
              >
                <span className="treemap-tile-label">{displayName}</span>
                <span
                  className="treemap-tile-pct"
                  style={{ color: tileColor(tile.totalReturnPercent) }}
                >
                  {formatPct(tile.totalReturnPercent)}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
