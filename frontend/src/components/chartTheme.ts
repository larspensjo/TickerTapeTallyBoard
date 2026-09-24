import type { AutoscaleInfoProvider } from "lightweight-charts";

/**
 * Shared chart appearance and scaling behavior.
 *
 * Kept free of runtime `lightweight-charts` imports so the scaling rules can be
 * unit-tested as plain functions; the chart components supply the enums.
 */

export const chartColors = {
  text: "#9aa4b2",
  grid: "rgba(148, 163, 184, 0.08)",
  border: "rgba(148, 163, 184, 0.2)",
  reference: "#e0b15e",
  value: "#4f9cff",
  valueTop: "rgba(79, 156, 255, 0.30)",
  valueBottom: "rgba(79, 156, 255, 0.02)",
  gainUp: "#16c784",
  gainUpTop: "rgba(22, 199, 132, 0.30)",
  gainUpBottom: "rgba(22, 199, 132, 0.02)",
  gainDown: "#ff4d4f",
  gainDownTop: "rgba(255, 77, 79, 0.02)",
  gainDownBottom: "rgba(255, 77, 79, 0.30)",
} as const;

/**
 * Symmetric margins keep a fitted axis from drawing the line onto the frame.
 * The old zero-pinned axis used `bottom: 0` because the floor was fixed.
 */
export const priceScaleMargins = { top: 0.12, bottom: 0.12 } as const;

const compactNumberFormatter = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumSignificantDigits: 3,
});

export const compactPriceFormat = {
  type: "custom" as const,
  formatter: (value: number) => compactNumberFormatter.format(value),
  minMove: 1,
};

/**
 * Percent axes need their own format: `compactPriceFormat`'s `minMove: 1` would
 * put every gridline a whole percentage point apart and collapse a sub-point
 * period to a single "0" tick.
 */
export const percentPriceFormat = {
  type: "custom" as const,
  formatter: (value: number) => `${value.toFixed(2)}%`,
  minMove: 0.01,
};

export function baseChartOptions(height: number) {
  return {
    height,
    layout: {
      background: { color: "transparent" },
      textColor: chartColors.text,
      attributionLogo: false,
    },
    grid: {
      vertLines: { color: chartColors.grid },
      horzLines: { color: chartColors.grid },
    },
    rightPriceScale: {
      borderColor: chartColors.border,
      scaleMargins: { ...priceScaleMargins },
    },
    timeScale: { borderColor: chartColors.border },
    handleScale: false,
    handleScroll: false,
  };
}

/**
 * Fit the axis to the drawn data, optionally widening it to keep extra anchor
 * values in view. Price-line primitives (the cost-basis break-even line) do not
 * participate in scale aggregation, so their value is passed in as an anchor.
 */
export function fitToDataAutoscale(
  anchorValues?: () => number[],
): AutoscaleInfoProvider {
  return (baseImplementation) => {
    const autoscale = baseImplementation();
    if (autoscale === null || autoscale.priceRange === null) return autoscale;

    const anchors = (anchorValues?.() ?? []).filter((value) =>
      Number.isFinite(value),
    );
    if (anchors.length === 0) return autoscale;

    return {
      ...autoscale,
      priceRange: {
        ...autoscale.priceRange,
        minValue: Math.min(autoscale.priceRange.minValue, ...anchors),
        maxValue: Math.max(autoscale.priceRange.maxValue, ...anchors),
      },
    };
  };
}

/** Remove a series from scale aggregation so it never drags the axis. */
export const noAutoscale: AutoscaleInfoProvider = () => null;

/**
 * Extend the fitted range to include zero, so a gain baseline is always in
 * view. This is not the same as pinning the floor to zero: negative values
 * still scale normally and draw below the baseline.
 */
export const zeroInViewAutoscale: AutoscaleInfoProvider = (
  baseImplementation,
) => {
  const autoscale = baseImplementation();
  if (autoscale === null || autoscale.priceRange === null) return autoscale;

  const minValue = Math.min(autoscale.priceRange.minValue, 0);
  const maxValue = Math.max(autoscale.priceRange.maxValue, 0);

  return {
    ...autoscale,
    priceRange:
      minValue === 0 && maxValue === 0
        ? { ...autoscale.priceRange, minValue: -1, maxValue: 1 }
        : { ...autoscale.priceRange, minValue, maxValue },
  };
};
