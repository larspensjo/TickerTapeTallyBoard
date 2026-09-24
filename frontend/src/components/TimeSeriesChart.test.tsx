// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TimeSeriesChart } from "./TimeSeriesChart";

const chartMocks = vi.hoisted(() => {
  const LineSeries = { seriesType: "Line" };
  const AreaSeries = { seriesType: "Area" };
  const setData = vi.fn();
  const setMarkers = vi.fn();
  const createPriceLine = vi.fn((options?: unknown) => ({ options }));
  const removePriceLine = vi.fn();
  const setVisibleRange = vi.fn();
  const fitContent = vi.fn();
  const applyOptions = vi.fn();
  const remove = vi.fn();
  const subscribeCrosshairMove = vi.fn();
  const timeScale = vi.fn(() => ({ setVisibleRange, fitContent }));
  const setReferenceData = vi.fn();
  const seriesApplyOptions = vi.fn();
  const referenceApplyOptions = vi.fn();
  const areaSeries = {
    setData,
    createPriceLine,
    removePriceLine,
    applyOptions: seriesApplyOptions,
  };
  const lineSeries = {
    setData: setReferenceData,
    applyOptions: referenceApplyOptions,
  };
  const addAreaSeries = vi.fn((_options?: unknown) => areaSeries);
  const addLineSeries = vi.fn((_options?: unknown) => lineSeries);
  const addSeries = vi.fn((definition: unknown, options: unknown) =>
    definition === AreaSeries ? addAreaSeries(options) : addLineSeries(options),
  );
  const createSeriesMarkers = vi.fn(() => ({ setMarkers }));
  const createChart = vi.fn(() => ({
    addSeries,
    applyOptions,
    remove,
    subscribeCrosshairMove,
    timeScale,
  }));

  return {
    AreaSeries,
    LineSeries,
    addAreaSeries,
    addLineSeries,
    addSeries,
    applyOptions,
    createChart,
    createPriceLine,
    createSeriesMarkers,
    fitContent,
    remove,
    removePriceLine,
    referenceApplyOptions,
    setData,
    seriesApplyOptions,
    setMarkers,
    setReferenceData,
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
  LineStyle: {
    Solid: 0,
    Dotted: 1,
    Dashed: 2,
    LargeDashed: 3,
    SparseDotted: 4,
  },
  AreaSeries: chartMocks.AreaSeries,
  LineSeries: chartMocks.LineSeries,
  createChart: chartMocks.createChart,
  createSeriesMarkers: chartMocks.createSeriesMarkers,
}));

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

type AreaSeriesOptions = {
  autoscaleInfoProvider: (
    baseImplementation: () => {
      priceRange: { minValue: number; maxValue: number } | null;
      margins?: { above: number; below: number };
    } | null,
  ) => {
    priceRange: { minValue: number; maxValue: number } | null;
  } | null;
};

function areaSeriesOptions(): AreaSeriesOptions {
  const calls = chartMocks.addAreaSeries.mock.calls as unknown as Array<
    [AreaSeriesOptions]
  >;
  const options = calls[0]?.[0];
  if (!options) throw new Error("area series was not created");
  return options;
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

  it("fits the axis to the drawn window instead of pinning the floor to zero", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[
          { time: "2026-06-01", value: 300 },
          { time: "2026-06-02", value: 224.43 },
        ]}
      />,
    );

    const autoscale = areaSeriesOptions().autoscaleInfoProvider(() => ({
      priceRange: { minValue: 224.43, maxValue: 300 },
      margins: { above: 12, below: 24 },
    }));

    expect(autoscale?.priceRange).toEqual({
      minValue: 224.43,
      maxValue: 300,
    });
  });

  it("gives the axis symmetric margins so a fitted line is not drawn on the frame", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-06-01", value: 300 }]}
      />,
    );

    type ChartOptions = {
      rightPriceScale: { scaleMargins: { top: number; bottom: number } };
    };
    const chartCalls = chartMocks.createChart.mock.calls as unknown as Array<
      [unknown, ChartOptions]
    >;

    expect(chartCalls[0]?.[1].rightPriceScale.scaleMargins).toEqual({
      top: 0.12,
      bottom: 0.12,
    });
  });

  it("widens the axis to keep the break-even line on screen", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        costBasisLine={120}
        data={[
          { time: "2026-06-01", value: 300 },
          { time: "2026-06-02", value: 224.43 },
        ]}
      />,
    );

    const autoscale = areaSeriesOptions().autoscaleInfoProvider(() => ({
      priceRange: { minValue: 224.43, maxValue: 300 },
    }));

    expect(autoscale?.priceRange).toEqual({ minValue: 120, maxValue: 300 });
  });

  it("keeps the invested reference line out of axis scaling entirely", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[{ time: "2026-06-01", value: 4_000_000 }]}
        referenceData={[{ time: "2026-06-01", value: 760_000 }]}
      />,
    );

    type LineSeriesOptions = {
      autoscaleInfoProvider: (base: () => unknown) => unknown;
    };
    const calls = chartMocks.addLineSeries.mock.calls as unknown as Array<
      [LineSeriesOptions]
    >;
    const provider = calls[0]?.[0].autoscaleInfoProvider;

    expect(provider).toBeDefined();
    expect(
      provider?.(() => ({
        priceRange: { minValue: 760_000, maxValue: 760_000 },
      })),
    ).toBeNull();
  });

  it("shows point markers when the window holds a single observation", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[{ time: "2026-06-01", value: 4_000_000 }]}
      />,
    );

    expect(chartMocks.seriesApplyOptions).toHaveBeenCalledWith({
      pointMarkersVisible: true,
    });
  });

  it("leaves point markers off once there is a line to draw", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[
          { time: "2026-06-01", value: 4_000_000 },
          { time: "2026-06-02", value: 4_050_000 },
        ]}
      />,
    );

    expect(chartMocks.seriesApplyOptions).toHaveBeenCalledWith({
      pointMarkersVisible: false,
    });
  });

  it("announces a reference line that falls outside the drawn band", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[{ time: "2026-06-01", value: 4_000_000 }]}
        edgeTag={{
          side: "below",
          label: "Invested 760K",
          description: "Net invested capital is 760K, below the visible range",
        }}
      />,
    );

    const tag = screen.getByRole("note", {
      name: "Net invested capital is 760K, below the visible range",
    });

    expect(tag).toHaveTextContent("Invested 760K");
  });

  it("renders no edge tag when the reference line is on screen", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[{ time: "2026-06-01", value: 4_000_000 }]}
      />,
    );

    expect(screen.queryByRole("note")).toBeNull();
  });

  it("uses compact axis labels with three significant digits when requested", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        data={[{ time: "2026-07-14", value: 6_365_180.76 }]}
        referenceData={[{ time: "2026-07-14", value: 4_100_000 }]}
        compactValueAxis
      />,
    );

    type SeriesOptions = {
      priceFormat: {
        formatter: (value: number) => string;
        minMove: number;
        type: string;
      };
    };
    const areaOptions = chartMocks.addAreaSeries.mock.calls[0]?.[0] as
      | SeriesOptions
      | undefined;
    const lineOptions = chartMocks.addLineSeries.mock.calls[0]?.[0] as
      | SeriesOptions
      | undefined;

    expect(areaOptions?.priceFormat.type).toBe("custom");
    expect(areaOptions?.priceFormat.minMove).toBe(1);
    expect(areaOptions?.priceFormat.formatter(1_000_000)).toBe("1M");
    expect(areaOptions?.priceFormat.formatter(6_365_180.76)).toBe("6.37M");
    expect(lineOptions?.priceFormat.formatter(4_100_000)).toBe("4.1M");
  });

  it("anchors trade arrows to their transaction prices instead of the series", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-07-14", value: 193.92 }]}
        markers={[
          {
            time: "2026-07-14",
            side: "buy",
            price: 168.72,
            title: "Buy",
            rows: [],
          },
          {
            time: "2026-07-15",
            side: "sell",
            price: 201.25,
            title: "Sell",
            rows: [],
          },
        ]}
      />,
    );

    expect(chartMocks.setMarkers).toHaveBeenLastCalledWith([
      {
        time: "2026-07-14",
        position: "atPriceBottom",
        shape: "arrowUp",
        color: "#16c784",
        price: 168.72,
      },
      {
        time: "2026-07-15",
        position: "atPriceTop",
        shape: "arrowDown",
        color: "#ff4d4f",
        price: 201.25,
      },
    ]);
  });

  it("renders split markers as neutral in-bar circles", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-06-01", value: 300 }]}
        markers={[
          {
            time: "2026-06-01",
            side: "split",
            title: "Split",
            rows: [],
          },
        ]}
      />,
    );

    expect(chartMocks.setMarkers).toHaveBeenLastCalledWith([
      {
        time: "2026-06-01",
        position: "inBar",
        shape: "circle",
        color: "#a8acb3",
      },
    ]);
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

  it("renders the invested reference as a dashed line on the same calendar spine", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Portfolio value"
        visibleStart="2026-01-02"
        data={[
          { time: "2026-01-02", value: 1000 },
          { time: "2026-01-04", value: 1100 },
        ]}
        referenceData={[
          { time: "2026-01-02", value: 1000 },
          { time: "2026-01-04", value: 1000 },
        ]}
      />,
    );

    type LineSeriesOptions = { lineStyle: number };
    const lineCalls = chartMocks.addLineSeries.mock.calls as unknown as Array<
      [LineSeriesOptions]
    >;
    expect(lineCalls[0]?.[0].lineStyle).toBe(2); // LineStyle.Dashed

    // Reference data is gap-filled onto the same daily spine as the value line.
    expect(chartMocks.setReferenceData).toHaveBeenLastCalledWith([
      { time: "2026-01-02", value: 1000 },
      { time: "2026-01-03" },
      { time: "2026-01-04", value: 1000 },
    ]);
  });

  it("manages the cost basis price line without stacking or leaking handles", () => {
    const { rerender } = render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-06-01", value: 300 }]}
        costBasisLine={250}
      />,
    );

    expect(chartMocks.createPriceLine).toHaveBeenCalledTimes(1);
    expect(chartMocks.createPriceLine).toHaveBeenLastCalledWith({
      price: 250,
      color: "#e0b15e",
      lineStyle: 1,
      lineWidth: 1,
      axisLabelVisible: true,
      title: "",
      lineVisible: true,
    });

    const firstHandle = chartMocks.createPriceLine.mock.results[0]?.value;
    expect(chartMocks.removePriceLine).not.toHaveBeenCalled();

    rerender(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-06-01", value: 300 }]}
        costBasisLine={275}
      />,
    );

    expect(chartMocks.removePriceLine).toHaveBeenCalledTimes(1);
    expect(chartMocks.removePriceLine).toHaveBeenLastCalledWith(firstHandle);
    expect(chartMocks.createPriceLine).toHaveBeenCalledTimes(2);
    expect(chartMocks.createPriceLine).toHaveBeenLastCalledWith({
      price: 275,
      color: "#e0b15e",
      lineStyle: 1,
      lineWidth: 1,
      axisLabelVisible: true,
      title: "",
      lineVisible: true,
    });

    const secondHandle = chartMocks.createPriceLine.mock.results[1]?.value;

    rerender(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[{ time: "2026-06-01", value: 300 }]}
      />,
    );

    expect(chartMocks.removePriceLine).toHaveBeenCalledTimes(2);
    expect(chartMocks.removePriceLine).toHaveBeenLastCalledWith(secondHandle);
    expect(chartMocks.createPriceLine).toHaveBeenCalledTimes(2);
  });
});
