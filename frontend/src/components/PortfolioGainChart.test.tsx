// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PortfolioGainChart } from "./PortfolioGainChart";

const chartMocks = vi.hoisted(() => {
  const BaselineSeries = { seriesType: "Baseline" };
  const setData = vi.fn();
  const applyOptions = vi.fn();
  const setVisibleRange = vi.fn();
  const fitContent = vi.fn();
  const remove = vi.fn();
  const chartApplyOptions = vi.fn();
  const timeScale = vi.fn(() => ({ setVisibleRange, fitContent }));
  const baselineSeries = { setData, applyOptions };
  const addSeries = vi.fn(
    (_definition: unknown, _options: unknown) => baselineSeries,
  );
  const createChart = vi.fn(() => ({
    addSeries,
    applyOptions: chartApplyOptions,
    remove,
    timeScale,
  }));

  return {
    BaselineSeries,
    addSeries,
    createChart,
    fitContent,
    remove,
    setData,
    setVisibleRange,
    timeScale,
  };
});

vi.mock("lightweight-charts", () => ({
  TickMarkType: {
    Year: 0,
    Month: 1,
    DayOfMonth: 2,
    Time: 3,
    TimeWithSeconds: 4,
  },
  BaselineSeries: chartMocks.BaselineSeries,
  createChart: chartMocks.createChart,
}));

class TestResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}

type BaselineOptions = {
  baseValue: { type: string; price: number };
  autoscaleInfoProvider: (
    base: () => { priceRange: { minValue: number; maxValue: number } | null },
  ) => { priceRange: { minValue: number; maxValue: number } | null } | null;
  priceFormat: { formatter: (value: number) => string; minMove: number };
};

function baselineOptions(): BaselineOptions {
  const calls = chartMocks.addSeries.mock.calls as unknown as Array<
    [unknown, BaselineOptions]
  >;
  const options = calls[0]?.[1];
  if (!options) throw new Error("baseline series was not created");
  return options;
}

const sekSeries = [
  { time: "2025-12-31", value: 0 },
  { time: "2026-01-02", value: 1_200 },
];

describe("PortfolioGainChart", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("anchors the series on a zero baseline so losses draw below the line", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={sekSeries}
        unit="sek"
        visibleStart="2025-12-31"
      />,
    );

    expect(chartMocks.addSeries).toHaveBeenCalledWith(
      chartMocks.BaselineSeries,
      expect.anything(),
    );
    expect(baselineOptions().baseValue).toEqual({ type: "price", price: 0 });
  });

  it("keeps zero in view without pinning the axis floor to it", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={sekSeries}
        unit="sek"
        visibleStart="2025-12-31"
      />,
    );

    const widened = baselineOptions().autoscaleInfoProvider(() => ({
      priceRange: { minValue: 1_000, maxValue: 5_000 },
    }));
    expect(widened?.priceRange).toEqual({ minValue: 0, maxValue: 5_000 });

    const negative = baselineOptions().autoscaleInfoProvider(() => ({
      priceRange: { minValue: -5_000, maxValue: -1_000 },
    }));
    expect(negative?.priceRange).toEqual({ minValue: -5_000, maxValue: 0 });
  });

  it("formats a percent axis to two decimals rather than whole points", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio performance"
        data={[
          { time: "2025-12-31", value: 0 },
          { time: "2026-01-02", value: 0.5 },
        ]}
        unit="percent"
        visibleStart="2025-12-31"
      />,
    );

    const format = baselineOptions().priceFormat;
    expect(format.formatter(0.5)).toBe("0.50%");
    expect(format.minMove).toBe(0.01);
  });

  it("formats a currency axis compactly", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={sekSeries}
        unit="sek"
        visibleStart="2025-12-31"
      />,
    );

    const format = baselineOptions().priceFormat;
    expect(format.formatter(3_240_000)).toBe("3.24M");
    expect(format.minMove).toBe(1);
  });

  it("draws on the calendar spine from the zero origin", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={sekSeries}
        unit="sek"
        visibleStart="2025-12-31"
      />,
    );

    expect(chartMocks.setData).toHaveBeenLastCalledWith([
      { time: "2025-12-31", value: 0 },
      { time: "2026-01-01" },
      { time: "2026-01-02", value: 1_200 },
    ]);
    expect(chartMocks.setVisibleRange).toHaveBeenLastCalledWith({
      from: "2025-12-31",
      to: "2026-01-02",
    });
  });

  it("passes negative gain through unclamped", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={[
          { time: "2025-12-31", value: 0 },
          { time: "2026-01-01", value: -4_500 },
        ]}
        unit="sek"
        visibleStart="2025-12-31"
      />,
    );

    expect(chartMocks.setData).toHaveBeenLastCalledWith([
      { time: "2025-12-31", value: 0 },
      { time: "2026-01-01", value: -4_500 },
    ]);
  });

  it("falls back to fitting the content when the origin is after the data", () => {
    render(
      <PortfolioGainChart
        ariaLabel="Portfolio gain"
        data={[{ time: "2026-01-02", value: 10 }]}
        unit="sek"
        visibleStart="2026-02-01"
      />,
    );

    expect(chartMocks.fitContent).toHaveBeenCalled();
    expect(chartMocks.setVisibleRange).not.toHaveBeenCalled();
  });
});
