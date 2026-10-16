// @vitest-environment jsdom

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { type ChartStepLine, TimeSeriesChart } from "./TimeSeriesChart";

const chartMocks = vi.hoisted(() => {
  const LineSeries = { seriesType: "Line" };
  const AreaSeries = { seriesType: "Area" };
  const LineType = { Simple: 0, WithSteps: 1, Curved: 2 };
  const setData = vi.fn();
  const setMarkers = vi.fn();
  const createPriceLine = vi.fn();
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
    applyOptions: seriesApplyOptions,
  };
  const lineSeries = {
    setData: setReferenceData,
    applyOptions: referenceApplyOptions,
  };
  const addAreaSeries = vi.fn((_options?: unknown) => areaSeries);
  const addLineSeries = vi.fn((_options?: unknown) => lineSeries);
  // Step-line series are told apart from the reference series by their step
  // line type; each gets its own handle so removal can be traced.
  const addStepSeries = vi.fn((_options?: unknown) => ({
    setData: vi.fn(),
    applyOptions: vi.fn(),
    attachPrimitive: vi.fn(),
    detachPrimitive: vi.fn(),
    priceToCoordinate: vi.fn((price: number) => 500 - price),
  }));
  const addSeries = vi.fn((definition: unknown, options: unknown) => {
    if (definition === AreaSeries) return addAreaSeries(options);
    if ((options as { lineType?: number }).lineType === LineType.WithSteps) {
      return addStepSeries(options);
    }
    return addLineSeries(options);
  });
  const createSeriesMarkers = vi.fn(() => ({ setMarkers }));
  // Each chart gets its own series bookkeeping, so a test can tell which
  // chart a series was added to or removed from.
  const createChart = vi.fn(() => ({
    addSeries: vi.fn((definition: unknown, options: unknown) =>
      addSeries(definition, options),
    ),
    applyOptions,
    remove,
    removeSeries: vi.fn(),
    subscribeCrosshairMove,
    timeScale,
  }));

  return {
    AreaSeries,
    LineSeries,
    LineType,
    addAreaSeries,
    addLineSeries,
    addSeries,
    addStepSeries,
    applyOptions,
    createChart,
    createPriceLine,
    createSeriesMarkers,
    fitContent,
    remove,
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
  LineType: chartMocks.LineType,
  AreaSeries: chartMocks.AreaSeries,
  LineSeries: chartMocks.LineSeries,
  createChart: chartMocks.createChart,
  createSeriesMarkers: chartMocks.createSeriesMarkers,
}));

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

type StepSeriesOptions = {
  color: string;
  lineWidth: number;
  lineStyle: number;
  lineType: number;
  priceLineVisible: boolean;
  lastValueVisible: boolean;
  crosshairMarkerVisible: boolean;
  pointMarkersVisible: boolean;
  autoscaleInfoProvider?: unknown;
  priceFormat?: unknown;
};

type StepSeriesMock = {
  setData: ReturnType<typeof vi.fn>;
  attachPrimitive: ReturnType<typeof vi.fn>;
  detachPrimitive: ReturnType<typeof vi.fn>;
};

type ChartMock = {
  addSeries: ReturnType<typeof vi.fn>;
  removeSeries: ReturnType<typeof vi.fn>;
};

function chartAt(index: number): ChartMock {
  const chart = chartMocks.createChart.mock.results[index]?.value as
    | ChartMock
    | undefined;
  if (!chart) throw new Error(`chart ${index} was not created`);
  return chart;
}

function stepSeriesAddedTo(chart: ChartMock): number {
  return chart.addSeries.mock.calls.filter(
    ([, options]) =>
      (options as { lineType?: number }).lineType ===
      chartMocks.LineType.WithSteps,
  ).length;
}

type AttachedTag = {
  attached: (param: { series: unknown }) => void;
  priceAxisViews: () => ReadonlyArray<{
    coordinate: () => number;
    text: () => string;
    textColor: () => string;
    backColor: () => string;
    visible?: () => boolean;
  }>;
};

function stepSeriesOptions(): StepSeriesOptions[] {
  const calls = chartMocks.addStepSeries.mock.calls as unknown as Array<
    [StepSeriesOptions]
  >;
  return calls.map(([options]) => options);
}

function addedStepSeries(): StepSeriesMock[] {
  return chartMocks.addStepSeries.mock.results.map((result) => result.value);
}

const closedLine: ChartStepLine = {
  key: "running-average-cost:2026-01-05",
  points: [
    { time: "2026-01-05", value: 100 },
    { time: "2026-02-10", value: 110 },
    { time: "2026-03-01", value: 110 },
  ],
  showLastValue: false,
};

const openLine: ChartStepLine = {
  key: "running-average-cost:2026-04-01",
  points: [
    { time: "2026-04-01", value: 112 },
    { time: "2026-04-15", value: 112.125 },
    { time: "2026-06-01", value: 112.125 },
  ],
  showLastValue: true,
  lastValueLabel: "112.12",
};

const priceData = [
  { time: "2026-01-05", value: 105 },
  { time: "2026-06-01", value: 130 },
];

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

  it("fits the axis to the drawn data instead of pinning the floor to zero", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={[
          { time: "2026-06-01", value: 300 },
          { time: "2026-06-02", value: 224.43 },
        ]}
      />,
    );

    // No provider means the library's own fit to the drawn data.
    expect(chartMocks.addAreaSeries.mock.calls[0]?.[0]).not.toHaveProperty(
      "autoscaleInfoProvider",
    );
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

  it("draws one dotted gold step series per step line", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[closedLine, openLine]}
      />,
    );

    const options = stepSeriesOptions();
    expect(options).toHaveLength(2);
    for (const option of options) {
      expect(option).toMatchObject({
        color: "#e0b15e",
        lineWidth: 1,
        lineStyle: 1, // LineStyle.Dotted
        lineType: chartMocks.LineType.WithSteps,
        priceLineVisible: false,
        crosshairMarkerVisible: false,
        pointMarkersVisible: false,
      });
    }
    // The library's own tag would take the price scale's half-up formatter.
    expect(options.map((option) => option.lastValueVisible)).toEqual([
      false,
      false,
    ]);

    const [closedSeries, openSeries] = addedStepSeries();
    expect(closedSeries?.setData).toHaveBeenCalledWith(closedLine.points);
    expect(openSeries?.setData).toHaveBeenCalledWith(openLine.points);
  });

  it("adds step series after the reference and area series", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[openLine]}
      />,
    );

    const stepOrder = chartMocks.addStepSeries.mock.invocationCallOrder[0];
    const areaOrder = chartMocks.addAreaSeries.mock.invocationCallOrder[0];
    const referenceOrder = chartMocks.addLineSeries.mock.invocationCallOrder[0];
    expect(stepOrder).toBeDefined();
    expect(stepOrder).toBeGreaterThan(areaOrder ?? Number.POSITIVE_INFINITY);
    expect(stepOrder).toBeGreaterThan(
      referenceOrder ?? Number.POSITIVE_INFINITY,
    );
  });

  it("shows a point marker for a step line with a single point", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[
          {
            key: "running-average-cost:2026-06-01",
            points: [{ time: "2026-06-01", value: 120 }],
            showLastValue: false,
          },
        ]}
      />,
    );

    expect(stepSeriesOptions()[0]?.pointMarkersVisible).toBe(true);
  });

  it("tags an open line with the supplied average text, not a half-up rounding", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[closedLine, openLine]}
      />,
    );

    const [closedSeries, openSeries] = addedStepSeries();
    expect(closedSeries?.attachPrimitive).not.toHaveBeenCalled();
    expect(openSeries?.attachPrimitive).toHaveBeenCalledTimes(1);

    const tag = openSeries?.attachPrimitive.mock.calls[0]?.[0] as AttachedTag;
    tag.attached({ series: openSeries });
    const [view] = tag.priceAxisViews();
    // The library's default formatter would render 112.13 here.
    expect(view?.text()).toBe("112.12");
    expect(view?.backColor()).toBe("#e0b15e");
    expect(view?.visible?.()).toBe(true);
    expect(view?.coordinate()).toBe(500 - 112.125);
  });

  it("lets step lines take part in axis scaling while the reference stays out", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        referenceData={[{ time: "2026-01-05", value: 100 }]}
        stepLines={[closedLine, openLine]}
      />,
    );

    for (const option of stepSeriesOptions()) {
      expect(option).not.toHaveProperty("autoscaleInfoProvider");
    }
    type ReferenceOptions = {
      autoscaleInfoProvider: (base: () => unknown) => unknown;
    };
    const reference = chartMocks.addLineSeries.mock.calls[0]?.[0] as
      | ReferenceOptions
      | undefined;
    expect(
      reference?.autoscaleInfoProvider(() => ({
        priceRange: { minValue: 100, maxValue: 100 },
      })),
    ).toBeNull();
  });

  it("replaces step series on change without leaking, and removes them with the prop", () => {
    const { rerender } = render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[closedLine, openLine]}
      />,
    );

    const chart = chartAt(0);
    const firstSeries = addedStepSeries();
    expect(firstSeries).toHaveLength(2);
    expect(chart.removeSeries).not.toHaveBeenCalled();

    rerender(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[openLine]}
      />,
    );

    expect(chart.removeSeries).toHaveBeenCalledTimes(2);
    expect(chart.removeSeries).toHaveBeenCalledWith(firstSeries[0]);
    expect(chart.removeSeries).toHaveBeenCalledWith(firstSeries[1]);
    expect(firstSeries[1]?.detachPrimitive).toHaveBeenCalledWith(
      firstSeries[1]?.attachPrimitive.mock.calls[0]?.[0],
    );
    expect(addedStepSeries()).toHaveLength(3);

    rerender(<TimeSeriesChart ariaLabel="Price history" data={priceData} />);

    expect(chart.removeSeries).toHaveBeenCalledTimes(3);
    expect(chart.removeSeries).toHaveBeenLastCalledWith(addedStepSeries()[2]);
    expect(addedStepSeries()).toHaveLength(3);
    expect(chartMocks.createPriceLine).not.toHaveBeenCalled();
  });

  it("redraws step lines on a recreated chart without touching the removed one", () => {
    const { rerender } = render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[closedLine, openLine]}
      />,
    );

    // Height recreates the chart while the step lines change in the same render.
    rerender(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[openLine]}
        height={320}
      />,
    );

    expect(chartMocks.createChart).toHaveBeenCalledTimes(2);
    expect(chartMocks.remove).toHaveBeenCalledTimes(1);
    const removedChart = chartAt(0);
    const liveChart = chartAt(1);
    expect(removedChart.removeSeries).not.toHaveBeenCalled();
    expect(stepSeriesAddedTo(removedChart)).toBe(2);
    expect(stepSeriesAddedTo(liveChart)).toBe(1);
  });

  it("shows the average after a buy in the trade tooltip", () => {
    render(
      <TimeSeriesChart
        ariaLabel="Price history"
        data={priceData}
        stepLines={[openLine]}
        markers={[
          {
            time: "2026-04-15",
            side: "buy",
            price: 112.25,
            title: "Buy",
            rows: [
              { label: "Avg price", value: "112.25 USD" },
              { label: "Avg after", value: "112.13 USD" },
            ],
          },
        ]}
      />,
    );

    const onCrosshairMove = chartMocks.subscribeCrosshairMove.mock
      .calls[0]?.[0] as ((param: unknown) => void) | undefined;
    expect(onCrosshairMove).toBeDefined();
    act(() => {
      onCrosshairMove?.({ time: "2026-04-15", point: { x: 40, y: 60 } });
    });

    const tooltip = screen.getByRole("tooltip");
    expect(tooltip).toHaveTextContent("Avg after");
    expect(tooltip).toHaveTextContent("112.13 USD");
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
});
