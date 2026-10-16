import { describe, expect, it } from "vitest";
import {
  compactPriceFormat,
  noAutoscale,
  percentPriceFormat,
  zeroInViewAutoscale,
} from "./chartTheme";

function base(minValue: number, maxValue: number) {
  return () => ({
    priceRange: { minValue, maxValue },
    margins: { above: 10, below: 10 },
  });
}

describe("noAutoscale", () => {
  it("removes the series from scale aggregation entirely", () => {
    expect(noAutoscale(base(0, 10))).toBeNull();
  });
});

describe("zeroInViewAutoscale", () => {
  it("widens upwards so zero stays visible under a positive series", () => {
    const info = zeroInViewAutoscale(base(1_000, 5_000));

    expect(info?.priceRange).toEqual({ minValue: 0, maxValue: 5_000 });
  });

  it("widens downwards so zero stays visible above a negative series", () => {
    const info = zeroInViewAutoscale(base(-5_000, -1_000));

    expect(info?.priceRange).toEqual({ minValue: -5_000, maxValue: 0 });
  });

  it("keeps a range that already straddles zero", () => {
    const info = zeroInViewAutoscale(base(-200, 300));

    expect(info?.priceRange).toEqual({ minValue: -200, maxValue: 300 });
  });

  it("falls back to a unit range so an all-zero series still draws its baseline", () => {
    const info = zeroInViewAutoscale(base(0, 0));

    expect(info?.priceRange).toEqual({ minValue: -1, maxValue: 1 });
  });

  it("passes through a null autoscale result", () => {
    expect(zeroInViewAutoscale(() => null)).toBeNull();
  });
});

describe("price formats", () => {
  it("renders percent values with two decimals so a sub-point week stays readable", () => {
    expect(percentPriceFormat.formatter(0.5)).toBe("0.50%");
    expect(percentPriceFormat.formatter(-12.345)).toBe("-12.35%");
    expect(percentPriceFormat.minMove).toBe(0.01);
  });

  it("keeps compact whole-unit formatting for currency axes", () => {
    expect(compactPriceFormat.formatter(3_240_000)).toBe("3.24M");
    expect(compactPriceFormat.minMove).toBe(1);
  });
});
