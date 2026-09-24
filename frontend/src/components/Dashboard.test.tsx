// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { GainsRow, Instrument } from "../api/types";

const useGains = vi.fn();
const usePortfolioValueHistory = vi.fn();
const renderTimeSeriesChart = vi.fn();
const renderGainChart = vi.fn();

vi.mock("../api/queries", () => ({
  useGains: (...args: unknown[]) => useGains(...args),
  usePortfolioValueHistory: (...args: unknown[]) =>
    usePortfolioValueHistory(...args),
}));

vi.mock("./TimeSeriesChart", () => ({
  TimeSeriesChart: (props: unknown) => {
    renderTimeSeriesChart(props);
    return null;
  },
}));

vi.mock("./PortfolioGainChart", () => ({
  PortfolioGainChart: (props: unknown) => {
    renderGainChart(props);
    return null;
  },
}));

class TestResizeObserver {
  observe = vi.fn();
  disconnect = vi.fn();
}

// Import after vi.mock so the mocked queries module is wired up.
import { Dashboard } from "./Dashboard";

function inst(symbol: string): Instrument {
  return {
    id: 1,
    symbol,
    exchange: "NYSE",
    name: symbol,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
  };
}

function openRow(symbol: string, value: string): GainsRow {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: inst(symbol),
    quantity: 10,
    cost_basis_native: "1000",
    cost_basis_base: money,
    performance_start_date: null,
    performance_denominator_base: money,
    capital_gain_base: money,
    capital_gain_percent: money,
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
    currency_gain_base: money,
    currency_gain_percent: money,
    total_return_base: money,
    total_return_percent: { status: "available", value: "5.00" },
    latest_price: null,
    previous_price: null,
    latest_fx: null,
    previous_fx: null,
    market_value_native: money,
    market_value_base: { status: "available", value },
    proceeds_native: money,
    proceeds_base: money,
    unrealized_gain_base: money,
    unrealized_gain_percent: money,
    realized_gain_base: money,
    realized_cost_basis_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    day_change_base: money,
    day_change_percent: money,
    reasons: [],
    position_status: "open",
  };
}

function portfolioWaterfall() {
  const money = { status: "available", value: "0.00" } as const;
  return {
    cost_basis_base: money,
    held_fee_component_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    market_value_base: money,
    realized_gain_base: money,
    realized_fee_base: money,
    realized_cost_basis_base: money,
    brokerage_total_base: money,
    income_base: money,
    unrealized_gain_base: money,
    total_return_base: money,
    income_not_tracked: true,
    excluded_rows: 0,
  };
}

function unavailablePortfolioWaterfall() {
  const money = { status: "unavailable", reasons: ["missing_price"] } as const;
  return {
    cost_basis_base: money,
    held_fee_component_base: money,
    price_effect_base: money,
    fx_effect_base: money,
    market_value_base: money,
    realized_gain_base: money,
    realized_fee_base: money,
    realized_cost_basis_base: money,
    brokerage_total_base: money,
    income_base: money,
    unrealized_gain_base: money,
    total_return_base: money,
    income_not_tracked: false,
    excluded_rows: 1,
  };
}

function renderDashboard() {
  return render(
    <MemoryRouter>
      <Dashboard
        selectedDatePreset="all"
        customRange={{ startDate: null, endDate: null }}
        valuationDate="2026-09-12"
        onDatePresetChange={vi.fn()}
        onDateRangeChange={vi.fn()}
      />
    </MemoryRouter>,
  );
}

describe("Dashboard chart panel", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
  });

  it("uses the gains report period for chart filtering and its visible start", () => {
    usePortfolioValueHistory.mockReturnValue({
      data: {
        start_date: "2025-01-01",
        points: [
          {
            date: "2026-05-31",
            value_base: "100.00",
            invested_base: "80.00",
            incomplete: false,
            included_count: 1,
            excluded_count: 0,
          },
          {
            date: "2026-06-01",
            value_base: "110.00",
            invested_base: "80.00",
            incomplete: false,
            included_count: 1,
            excluded_count: 0,
          },
          {
            date: "2026-09-12",
            value_base: "120.00",
            invested_base: "80.00",
            incomplete: false,
            included_count: 1,
            excluded_count: 0,
          },
        ],
      },
      isPending: false,
      isError: false,
    });
    useGains.mockReturnValue({
      data: {
        rows: [],
        portfolio_waterfall: portfolioWaterfall(),
        report_period: {
          start_date: "2026-06-01",
          end_date: "2026-09-12",
        },
      },
      isPending: false,
      isError: false,
    });

    renderDashboard();

    expect(renderTimeSeriesChart).toHaveBeenCalledWith(
      expect.objectContaining({
        data: [
          { time: "2026-06-01", value: 110 },
          { time: "2026-09-12", value: 120 },
        ],
        visibleStart: "2026-06-01",
      }),
    );
  });

  it("keeps the Treemap view reachable when value-history fails", () => {
    // Value-history query is in error (owns the default "value" view), while
    // the gains query has usable holdings for the treemap.
    usePortfolioValueHistory.mockReturnValue({
      data: undefined,
      isPending: false,
      isError: true,
      refetch: vi.fn(),
    });
    useGains.mockReturnValue({
      data: {
        rows: [openRow("MSFT", "5000.00")],
        portfolio_waterfall: portfolioWaterfall(),
        report_period: { start_date: null, end_date: "2026-09-12" },
      },
      isPending: false,
      isError: false,
    });

    renderDashboard();

    // The error state still renders the view controls, so Treemap is reachable.
    expect(screen.getByText("Could not load portfolio value.")).toBeTruthy();
    const treemapButton = screen.getByRole("button", { name: "Treemap" });

    fireEvent.click(treemapButton);

    // Switched to the treemap, driven by the successful gains query rather than
    // the failed value-history query.
    expect(screen.getByRole("heading", { name: "Portfolio map" })).toBeTruthy();
    expect(screen.queryByText("Could not load portfolio value.")).toBeNull();
    expect(
      screen.queryByText("No valued open holdings to display."),
    ).toBeNull();
  });

  it("shows the portfolio waterfall when closed activity leaves no open rows", () => {
    usePortfolioValueHistory.mockReturnValue({
      data: { points: [] },
      isPending: false,
      isError: false,
    });
    useGains.mockReturnValue({
      data: {
        rows: [],
        portfolio_waterfall: {
          ...portfolioWaterfall(),
          realized_gain_base: { status: "available", value: "250.00" },
          realized_cost_basis_base: { status: "available", value: "1000.00" },
          total_return_base: { status: "available", value: "250.00" },
        },
      },
      isPending: false,
      isError: false,
    });

    renderDashboard();

    expect(
      screen.getByRole("heading", { name: "Portfolio gains breakdown" }),
    ).toBeTruthy();
    expect(screen.getByText("Realized gain")).toBeTruthy();
    expect(
      screen.queryByText("No valued holdings in this interval."),
    ).toBeNull();
  });

  it("shows the portfolio waterfall empty state when the aggregate block is unavailable", () => {
    usePortfolioValueHistory.mockReturnValue({
      data: { points: [] },
      isPending: false,
      isError: false,
    });
    useGains.mockReturnValue({
      data: {
        rows: [openRow("MSFT", "5000.00")],
        portfolio_waterfall: unavailablePortfolioWaterfall(),
      },
      isPending: false,
      isError: false,
    });

    renderDashboard();

    expect(
      screen.getByText("No valued holdings in this interval."),
    ).toBeTruthy();
    expect(screen.queryByText("Realized gain")).toBeNull();
  });
});

type ChartProps = {
  data: { time: string; value: number }[];
  visibleStart?: string;
};

function lastGainChartProps(): ChartProps {
  const calls = renderGainChart.mock.calls as unknown as Array<[ChartProps]>;
  const props = calls.at(-1)?.[0];
  if (!props) throw new Error("gain chart was not rendered");
  return props;
}

function lastValueChartProps(): ChartProps {
  const calls = renderTimeSeriesChart.mock.calls as unknown as Array<
    [ChartProps]
  >;
  const props = calls.at(-1)?.[0];
  if (!props) throw new Error("value chart was not rendered");
  return props;
}

function historyPoint(
  date: string,
  value: string,
  invested: string | null,
  incomplete = false,
) {
  return {
    date,
    value_base: value,
    invested_base: invested,
    incomplete,
    included_count: incomplete ? 1 : 2,
    excluded_count: incomplete ? 1 : 0,
  };
}

function mockHistory(
  points: ReturnType<typeof historyPoint>[],
  startDate: string,
) {
  usePortfolioValueHistory.mockReturnValue({
    data: { start_date: startDate, points },
    isPending: false,
    isError: false,
    refetch: vi.fn(),
  });
}

function mockGains(
  reportPeriod: { start_date: string | null; end_date: string } | undefined,
  extra: Record<string, unknown> = {},
) {
  useGains.mockReturnValue({
    data: reportPeriod
      ? {
          rows: [openRow("MSFT", "5000.00")],
          portfolio_waterfall: portfolioWaterfall(),
          report_period: reportPeriod,
        }
      : undefined,
    isPending: false,
    isError: false,
    refetch: vi.fn(),
    ...extra,
  });
}

function showGain() {
  fireEvent.click(screen.getByRole("button", { name: "Gain" }));
}

describe("Dashboard gain view", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", TestResizeObserver);
    localStorage.clear();
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
    vi.clearAllMocks();
    localStorage.clear();
  });

  it("plots only in-range measured points on the value view", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
        historyPoint("2026-06-02", "55.00", "80.00", true),
        historyPoint("2026-06-03", "120.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-03" });

    renderDashboard();

    expect(lastValueChartProps().data).toEqual([
      { time: "2026-06-01", value: 110 },
      { time: "2026-06-03", value: 120 },
    ]);
  });

  it("opens the gain line at a zero origin the day before the period", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    renderDashboard();
    showGain();

    const props = lastGainChartProps();
    expect(props.data[0]).toEqual({ time: "2026-05-31", value: 0 });
    expect(props.data.at(-1)).toEqual({ time: "2026-06-01", value: 10 });
    expect(props.visibleStart).toBe("2026-05-31");
  });

  it("offers the unit toggle only on the gain view and switches the series", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    renderDashboard();
    expect(screen.queryByRole("button", { name: "Percent" })).toBeNull();

    showGain();
    expect(
      screen.getByRole("heading", { name: "Portfolio gain, all time (SEK)" }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Percent" }));

    expect(
      screen.getByRole("heading", { name: "Portfolio gain, all time (%)" }),
    ).toBeTruthy();
    expect(lastGainChartProps().data.at(-1)?.value).toBeCloseTo(10, 6);
  });

  it("remembers the unit choice across a remount", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    const first = renderDashboard();
    showGain();
    fireEvent.click(screen.getByRole("button", { name: "Percent" }));
    first.unmount();

    renderDashboard();

    expect(
      screen.getByRole("heading", { name: "Portfolio gain, all time (%)" }),
    ).toBeTruthy();
  });

  it("drops the period name while a previous selection's response is displayed", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains(
      { start_date: "2026-06-01", end_date: "2026-06-01" },
      { isPlaceholderData: true },
    );

    renderDashboard();

    expect(
      screen.getByRole("heading", { name: "Portfolio value (SEK)" }),
    ).toBeTruthy();
    expect(screen.getByText("Updating")).toBeTruthy();
  });

  it("names the period once the matching response arrives", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains(
      { start_date: "2026-06-01", end_date: "2026-06-01" },
      { isPlaceholderData: false },
    );

    renderDashboard();

    expect(
      screen.getByRole("heading", { name: "Portfolio value, all time (SEK)" }),
    ).toBeTruthy();
    expect(screen.queryByText("Updating")).toBeNull();
  });

  it("refuses to plot lifetime history when no period could be resolved", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains(undefined);

    renderDashboard();

    expect(
      screen.getByText("Could not work out the selected period."),
    ).toBeTruthy();
    expect(renderTimeSeriesChart).not.toHaveBeenCalled();

    // The treemap remains reachable from that state.
    fireEvent.click(screen.getByRole("button", { name: "Treemap" }));
    expect(screen.getByRole("heading", { name: "Portfolio map" })).toBeTruthy();
  });

  it("surfaces a failed period request instead of holding a stale Updating chip", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    // A preset change whose gains request failed: the previous response is
    // still held as placeholder data, so a period-name guard alone would leave
    // the panel updating forever with no way out.
    mockGains(
      { start_date: "2026-06-01", end_date: "2026-06-01" },
      { isPlaceholderData: true, isError: true },
    );

    renderDashboard();

    const panel = screen.getByRole("region", { name: "Portfolio value" });
    expect(
      within(panel).getByText("Could not work out the selected period."),
    ).toBeTruthy();
    expect(within(panel).getByRole("button", { name: "Retry" })).toBeTruthy();
    expect(renderTimeSeriesChart).not.toHaveBeenCalled();
  });

  it("announces invested capital when it falls outside the drawn band", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "3950000.00", "760000.00"),
        historyPoint("2026-06-01", "4100000.00", "760000.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    renderDashboard();

    const props = lastValueChartProps() as unknown as {
      edgeTag?: { side: string; label: string; description: string };
    };
    expect(props.edgeTag?.side).toBe("below");
    expect(props.edgeTag?.label).toBe("Invested 760K");
    expect(props.edgeTag?.description).toContain("below the visible range");
  });

  it("leaves the invested line untagged when it crosses the drawn band", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100000.00", "80000.00"),
        historyPoint("2026-06-01", "110000.00", "150000.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-05-31", end_date: "2026-06-01" });

    renderDashboard();

    const props = lastValueChartProps() as unknown as { edgeTag?: unknown };
    expect(props.edgeTag).toBeUndefined();
  });

  it("explains an unknown opening rather than showing inception gains", () => {
    mockHistory(
      [
        historyPoint("2026-06-01", "500.00", "200.00"),
        historyPoint("2026-06-02", "520.00", "200.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-02" });

    renderDashboard();
    showGain();

    expect(
      screen.getByText(/value at the start of this period is not known/),
    ).toBeTruthy();
    expect(renderGainChart).not.toHaveBeenCalled();
  });

  it("explains a period whose every day is missing prices", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "50.00", "80.00", true),
        historyPoint("2026-06-02", "60.00", "80.00", true),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-02" });

    renderDashboard();
    showGain();

    expect(
      screen.getByText(/missing prices for at least one holding/),
    ).toBeTruthy();
    expect(renderGainChart).not.toHaveBeenCalled();
  });

  it("names the historical date a trade lost its exchange rate", () => {
    mockHistory(
      [
        historyPoint("2025-12-20", "90.00", null),
        historyPoint("2026-06-01", "100.00", null),
        historyPoint("2026-06-02", "110.00", null),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-02" });

    renderDashboard();
    showGain();

    expect(screen.getByText(/from 2025-12-20 onwards/)).toBeTruthy();
    expect(renderGainChart).not.toHaveBeenCalled();
  });

  it("marks the percentage approximate only when money moved around a gap", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "100.00"),
        historyPoint("2026-06-01", "200.00", "200.00", true),
        historyPoint("2026-06-02", "220.00", "200.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-02" });

    renderDashboard();
    showGain();

    // Never in SEK mode: the money line is exact endpoint arithmetic.
    expect(screen.queryByText("Approximate")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Percent" }));

    expect(screen.getByText("Approximate")).toBeTruthy();
    expect(screen.getByText(/close estimate/)).toBeTruthy();
  });

  it("leaves the percentage unmarked when nothing moved around a gap", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "100.00"),
        historyPoint("2026-06-01", "60.00", "100.00", true),
        historyPoint("2026-06-02", "121.00", "100.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-02" });

    renderDashboard();
    showGain();
    fireEvent.click(screen.getByRole("button", { name: "Percent" }));

    expect(screen.queryByText("Approximate")).toBeNull();
  });

  it("distinguishes the two percentages without needing a hover", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    renderDashboard();
    showGain();
    fireEvent.click(screen.getByRole("button", { name: "Percent" }));

    expect(
      screen.getByText(/answer a different question: how your money performed/),
    ).toBeTruthy();
  });

  it("says when the chart ends before the period does", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-30" });

    renderDashboard();
    showGain();

    expect(
      screen.getByText(/Chart ends 2026-06-01 — no valued holdings/),
    ).toBeTruthy();
  });

  it("shows the dividend caveat only when the period received dividends", () => {
    mockHistory(
      [
        historyPoint("2026-05-31", "100.00", "80.00"),
        historyPoint("2026-06-01", "110.00", "80.00"),
      ],
      "2025-01-01",
    );
    mockGains({ start_date: "2026-06-01", end_date: "2026-06-01" });

    renderDashboard();
    showGain();
    expect(screen.queryByText(/Dividends are not included here/)).toBeNull();

    cleanup();
    useGains.mockReturnValue({
      data: {
        rows: [openRow("MSFT", "5000.00")],
        portfolio_waterfall: {
          ...portfolioWaterfall(),
          income_not_tracked: false,
          income_base: { status: "available", value: "1200.00" },
        },
        report_period: { start_date: "2026-06-01", end_date: "2026-06-01" },
      },
      isPending: false,
      isError: false,
      refetch: vi.fn(),
    });

    renderDashboard();
    expect(screen.getByText(/Dividends are not included here/)).toBeTruthy();
  });
});
