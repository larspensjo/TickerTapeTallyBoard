// @vitest-environment jsdom

import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TimeSeriesChart } from "./TimeSeriesChart";

const chartMocks = vi.hoisted(() => {
  const setData = vi.fn();
  const setMarkers = vi.fn();
  const setVisibleRange = vi.fn();
  const fitContent = vi.fn();
  const applyOptions = vi.fn();
  const remove = vi.fn();
  const subscribeCrosshairMove = vi.fn();
  const timeScale = vi.fn(() => ({ setVisibleRange, fitContent }));
  const addAreaSeries = vi.fn(() => ({ setData, setMarkers }));
  const createChart = vi.fn(() => ({
    addAreaSeries,
    applyOptions,
    remove,
    subscribeCrosshairMove,
    timeScale,
  }));

  return {
    addAreaSeries,
    applyOptions,
    createChart,
    fitContent,
    remove,
    setData,
    setMarkers,
    subscribeCrosshairMove,
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
  createChart: chartMocks.createChart,
}));

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

describe("TimeSeriesChart", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("fills missing calendar days so the x-axis is linear over time", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        visibleStart="2025-09-16"
        data={[
          { time: "2025-09-19", value: 157.99 },
          { time: "2025-09-22", value: 208.43 },
        ]}
      />,
    );

    expect(chartMocks.setData).toHaveBeenLastCalledWith([
      { time: "2025-09-16" },
      { time: "2025-09-17" },
      { time: "2025-09-18" },
      { time: "2025-09-19", value: 157.99 },
      { time: "2025-09-20" },
      { time: "2025-09-21" },
      { time: "2025-09-22", value: 208.43 },
    ]);
    expect(chartMocks.setVisibleRange).toHaveBeenLastCalledWith({
      from: "2025-09-16",
      to: "2025-09-22",
    });
    expect(chartMocks.fitContent).not.toHaveBeenCalled();
  });

  it("formats day ticks with a month name so first-history labels are not ambiguous", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2025-05-11", value: 1346.33 }]}
      />,
    );

    type ChartOptions = {
      timeScale: {
        tickMarkFormatter: (
          time: string,
          tickMarkType: number,
        ) => string | null;
      };
    };
    const calls = chartMocks.createChart.mock.calls as unknown as Array<
      [unknown, ChartOptions]
    >;
    const options = calls[0]?.[1];
    expect(options).toBeDefined();
    if (!options) return;

    expect(options.timeScale.tickMarkFormatter("2025-05-11", 2)).toBe("May 11");
    expect(options.timeScale.tickMarkFormatter("2025-05-01", 1)).toBeNull();
  });

  it("pins the y-axis floor to zero even when markers request lower padding", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[
          { time: "2026-06-01", value: 300 },
          { time: "2026-06-02", value: 224.43 },
        ]}
        markers={[
          {
            time: "2026-06-01",
            side: "buy",
            title: "Buy",
            rows: [],
          },
        ]}
      />,
    );

    type ChartOptions = {
      rightPriceScale: {
        scaleMargins: {
          top: number;
          bottom: number;
        };
      };
    };
    type AreaSeriesOptions = {
      autoscaleInfoProvider: (
        baseImplementation: () => {
          priceRange: { minValue: number; maxValue: number };
          margins?: { above: number; below: number };
        } | null,
      ) => {
        priceRange: { minValue: number; maxValue: number };
        margins?: { above: number; below: number };
      } | null;
    };
    const chartCalls = chartMocks.createChart.mock.calls as unknown as Array<
      [unknown, ChartOptions]
    >;
    const seriesCalls = chartMocks.addAreaSeries.mock.calls as unknown as Array<
      [AreaSeriesOptions]
    >;

    expect(chartCalls[0]?.[1].rightPriceScale.scaleMargins.bottom).toBe(0);

    const autoscale = seriesCalls[0]?.[0].autoscaleInfoProvider(() => ({
      priceRange: { minValue: 224.43, maxValue: 300 },
      margins: { above: 12, below: 24 },
    }));

    expect(autoscale).toEqual({
      priceRange: { minValue: 0, maxValue: 300 },
      margins: { above: 12, below: 0 },
    });
  });

  it("falls back to fitContent when the requested visible start is after the data", () => {
    const data = [
      { time: "2026-01-02", value: 9150 },
      { time: "2026-01-18", value: 10868.39 },
    ];

    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        visibleStart="2026-02-01"
        data={data}
      />,
    );

    const chartData = chartMocks.setData.mock.calls.at(-1)?.[0] as Array<{
      time: string;
      value?: number;
    }>;
    expect(chartData[0]).toEqual(data[0]);
    expect(chartData.at(-1)).toEqual(data[1]);
    expect(chartData).toHaveLength(17);
    expect(chartMocks.setVisibleRange).not.toHaveBeenCalled();
    expect(chartMocks.fitContent).toHaveBeenCalledTimes(1);
  });
});
