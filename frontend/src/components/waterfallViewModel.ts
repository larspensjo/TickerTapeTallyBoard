import type { GainsRow, MoneyValue, PercentValue } from "../api/types";

export type WaterfallKind =
  | "base"
  | "effect"
  | "subtotal"
  | "total"
  | "placeholder";
export type WaterfallDirection = "up" | "down" | "flat";

export interface WaterfallRow {
  key: string;
  label: string;
  kind: WaterfallKind;
  value: MoneyValue;
  /** Coloring hint for effect/total rows; null for base/subtotal/placeholder. */
  direction: WaterfallDirection | null;
  /** Display-only percent vs cost basis; null when the row has no meaningful percent. */
  percent: PercentValue | null;
  /** Display-only bar geometry in base-currency units; null when there is no bar. */
  span: { from: number; to: number } | null;
}

export interface WaterfallView {
  mode: "open" | "closed";
  currency: string;
  rows: WaterfallRow[];
  /** Normalized geometry domain (base-currency units); minValue ≤ 0 ≤ maxValue. */
  minValue: number;
  maxValue: number;
}

const CURRENCY = "SEK";

function toNumber(value: MoneyValue): number | null {
  if (value.status !== "available") {
    return null;
  }
  const parsed = Number(value.value);
  return Number.isFinite(parsed) ? parsed : null;
}

function directionOf(n: number): WaterfallDirection {
  if (n > 0) return "up";
  if (n < 0) return "down";
  return "flat";
}

// Display-only percentage vs a population-matched cost basis (decision 2026-06-18,
// denominator contract from review finding #3). Never money of record.
function displayPercent(
  value: MoneyValue,
  costBasis: MoneyValue,
): PercentValue {
  if (value.status !== "available") {
    return { status: "unavailable", reasons: value.reasons };
  }
  const v = Number(value.value);
  // A zero numerator is a calm 0.00%, even when the matching cost basis is also zero
  // (e.g. a never-sold position's realized row): never surface 0/0 as "n/a".
  if (Number.isFinite(v) && v === 0) {
    return { status: "available", value: "0.00" };
  }
  if (costBasis.status !== "available") {
    return { status: "unavailable", reasons: costBasis.reasons };
  }
  const cb = Number(costBasis.value);
  if (!Number.isFinite(v) || !Number.isFinite(cb)) {
    return { status: "unavailable", reasons: ["unavailable"] };
  }
  if (cb === 0) {
    return { status: "unavailable", reasons: ["zero_cost_basis"] };
  }
  return { status: "available", value: ((v / cb) * 100).toFixed(2) };
}

// Display-only sum for the open-position total-return terminus (decision 2026-06-18).
function displaySum(a: MoneyValue, b: MoneyValue): MoneyValue {
  if (a.status !== "available" && b.status !== "available") {
    const reasons = [...new Set([...a.reasons, ...b.reasons])];
    return { status: "unavailable", reasons };
  }
  if (a.status !== "available") return a;
  if (b.status !== "available") return b;
  return {
    status: "available",
    value: (Number(a.value) + Number(b.value)).toFixed(2),
  };
}

function levelRow(
  key: string,
  label: string,
  kind: "base" | "subtotal",
  value: MoneyValue,
): WaterfallRow {
  const n = toNumber(value);
  return {
    key,
    label,
    kind,
    value,
    direction: null,
    percent: null,
    span: n === null ? null : { from: 0, to: n },
  };
}

function placeholderRow(key: string, label: string): WaterfallRow {
  return {
    key,
    label,
    kind: "placeholder",
    value: { status: "unavailable", reasons: ["income_not_tracked"] },
    direction: null,
    percent: null,
    span: null,
  };
}

// Pushes an income row or a placeholder when income is not tracked.
// When income is tracked (available or unavailable for a reason other than income_not_tracked),
// delegates to pushEffect and returns the updated running total.
function incomeRow(
  gain: GainsRow,
  costBasis: MoneyValue,
  running: number,
  rows: WaterfallRow[],
): number {
  if (
    gain.income_base.status === "unavailable" &&
    gain.income_base.reasons.includes("income_not_tracked")
  ) {
    rows.push(placeholderRow("income", "Dividend income"));
    return running;
  }
  return pushEffect(
    rows,
    "income",
    "Dividend income",
    gain.income_base,
    costBasis,
    running,
  );
}

// Pushes an effect row, advances the running total, and returns the new running total.
// An unavailable effect renders without a bar and does not move the running total.
function pushEffect(
  rows: WaterfallRow[],
  key: string,
  label: string,
  value: MoneyValue,
  costBasis: MoneyValue,
  running: number,
): number {
  const n = toNumber(value);
  if (n === null) {
    rows.push({
      key,
      label,
      kind: "effect",
      value,
      direction: null,
      percent: displayPercent(value, costBasis),
      span: null,
    });
    return running;
  }
  const to = running + n;
  rows.push({
    key,
    label,
    kind: "effect",
    value,
    direction: directionOf(n),
    percent: displayPercent(value, costBasis),
    span: { from: running, to },
  });
  return to;
}

// `denominator` drives the display-only % (population-matched, finding #3); `baseline`
// is the held cost basis the delta bar floats from. They differ for an open partial sell.
function totalRow(
  label: string,
  value: MoneyValue,
  denominator: MoneyValue,
  baseline: MoneyValue,
): WaterfallRow {
  const amount = toNumber(value);
  const base = toNumber(baseline);
  const span =
    amount === null || base === null ? null : { from: base, to: base + amount };
  return {
    key: "total-return",
    label,
    kind: "total",
    value,
    direction: amount === null ? null : directionOf(amount),
    percent: displayPercent(value, denominator),
    span,
  };
}

// Normalized geometry domain. Tracks both a minimum and a maximum so a span that crosses
// below zero (a realized loss larger than the held cost basis) still renders in-track
// (review finding #4). `minValue` is clamped at 0 or below so the baseline stays anchored.
function computeDomain(rows: WaterfallRow[]): {
  minValue: number;
  maxValue: number;
} {
  let min = 0;
  let max = 0;
  for (const row of rows) {
    if (row.span) {
      min = Math.min(min, row.span.from, row.span.to);
      max = Math.max(max, row.span.from, row.span.to);
    }
  }
  if (max <= min) {
    return { minValue: min, maxValue: min + 1 };
  }
  // Pad the high end slightly so the tallest bar never touches the track edge.
  return { minValue: min, maxValue: max + (max - min) * 0.02 };
}

function openWaterfall(gain: GainsRow): WaterfallView {
  const costBasis = gain.cost_basis_base;
  const rows: WaterfallRow[] = [];
  let running = toNumber(costBasis) ?? 0;

  rows.push(levelRow("cost-basis", "Cost basis (held)", "base", costBasis));
  running = pushEffect(
    rows,
    "price",
    "Price effect",
    gain.price_effect_base,
    costBasis,
    running,
  );
  running = pushEffect(
    rows,
    "fx",
    "FX effect",
    gain.fx_effect_base,
    costBasis,
    running,
  );
  rows.push(
    levelRow(
      "market-value",
      "Market value",
      "subtotal",
      gain.market_value_base,
    ),
  );
  // Realized gain belongs to sold shares: its % is vs the sold cost basis.
  running = pushEffect(
    rows,
    "realized",
    "Realized gain",
    gain.realized_gain_base,
    gain.realized_cost_basis_base,
    running,
  );
  running = incomeRow(gain, costBasis, running, rows);

  // For total-return, income_not_tracked means no dividend history exists;
  // treat it as zero rather than making total-return unavailable.
  const incomeForSum: MoneyValue =
    gain.income_base.status === "unavailable" &&
    gain.income_base.reasons.includes("income_not_tracked")
      ? { status: "available", value: "0.00" }
      : gain.income_base;
  const totalReturn = displaySum(
    displaySum(gain.unrealized_gain_base, gain.realized_gain_base),
    incomeForSum,
  );
  // Total-return % is vs total capital deployed = held + sold cost basis; the delta bar
  // still floats from the held cost basis baseline.
  const totalCostBasis = displaySum(costBasis, gain.realized_cost_basis_base);
  rows.push(totalRow("Total return", totalReturn, totalCostBasis, costBasis));

  const { minValue, maxValue } = computeDomain(rows);
  return { mode: "open", currency: CURRENCY, rows, minValue, maxValue };
}

function closedWaterfall(gain: GainsRow): WaterfallView {
  const costBasis = gain.cost_basis_base;
  const rows: WaterfallRow[] = [];
  let running = toNumber(costBasis) ?? 0;

  rows.push(levelRow("cost-basis", "Cost basis (sold)", "base", costBasis));
  running = pushEffect(
    rows,
    "price",
    "Price effect",
    gain.price_effect_base,
    costBasis,
    running,
  );
  running = pushEffect(
    rows,
    "fx",
    "FX effect",
    gain.fx_effect_base,
    costBasis,
    running,
  );
  rows.push(levelRow("proceeds", "Proceeds", "subtotal", gain.proceeds_base));
  incomeRow(gain, costBasis, running, rows);
  // Closed: cost_basis_base already represents the full sold cost basis, so it serves as
  // both the denominator and the baseline (do not re-add realized_cost_basis_base).
  rows.push(
    totalRow("Total return", gain.total_return_base, costBasis, costBasis),
  );

  const { minValue, maxValue } = computeDomain(rows);
  return { mode: "closed", currency: CURRENCY, rows, minValue, maxValue };
}

export function waterfallView(gain: GainsRow): WaterfallView {
  return gain.position_status === "closed"
    ? closedWaterfall(gain)
    : openWaterfall(gain);
}
