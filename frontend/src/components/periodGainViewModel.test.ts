import { describe, expect, it } from "vitest";
import type { PortfolioWaterfall, ValueHistoryPoint } from "../api/types";
import {
  chartPanelHeading,
  dividendCaveatApplies,
  periodGainSeries,
  periodValueSeries,
  valueHistoryWindow,
} from "./portfolioValueViewModel";

function pt(
  date: string,
  value: number,
  invested: number | null,
  incomplete = false,
): ValueHistoryPoint {
  return {
    date,
    value_base: String(value),
    invested_base: invested === null ? null : String(invested),
    incomplete,
    included_count: incomplete ? 1 : 2,
    excluded_count: incomplete ? 1 : 0,
  };
}

const range = (startDate: string | null, endDate: string | null) => ({
  startDate,
  endDate,
});

function windowOf(
  points: ValueHistoryPoint[],
  startDate: string | null,
  endDate: string | null,
  historyStart: string | null = points[0]?.date ?? null,
) {
  return valueHistoryWindow(points, range(startDate, endDate), historyStart);
}

function gainOf(
  points: ValueHistoryPoint[],
  startDate: string | null,
  endDate: string | null,
  historyStart: string | null = points[0]?.date ?? null,
) {
  return periodGainSeries(windowOf(points, startDate, endDate, historyStart));
}

describe("valueHistoryWindow", () => {
  const points = [
    pt("2026-01-02", 100, 100),
    pt("2026-01-03", 110, 100),
    pt("2026-01-04", 120, 100),
  ];

  it("anchors on the last observation strictly before the range start", () => {
    const win = windowOf(points, "2026-01-03", "2026-01-04");

    expect(win.anchor?.date).toBe("2026-01-02");
    expect(win.measured.map((p) => p.date)).toEqual([
      "2026-01-03",
      "2026-01-04",
    ]);
  });

  it("skips an incomplete candidate when choosing the anchor", () => {
    const win = windowOf(
      [
        pt("2026-01-01", 90, 100),
        pt("2026-01-02", 55, 100, true),
        pt("2026-01-03", 110, 100),
      ],
      "2026-01-03",
      "2026-01-03",
    );

    expect(win.anchor?.date).toBe("2026-01-01");
  });

  it("treats the opening as known empty when the range reaches the first trade", () => {
    const win = windowOf(points, "2026-01-02", "2026-01-04", "2026-01-02");

    expect(win.anchor).toBeNull();
    expect(win.openingKnownEmpty).toBe(true);
  });

  it("treats the opening as unknown when holdings predate the stored history", () => {
    const win = windowOf(points, "2026-01-02", "2026-01-04", "2025-11-01");

    expect(win.anchor).toBeNull();
    expect(win.openingKnownEmpty).toBe(false);
  });

  it("reports the first date invested capital went unavailable in the whole history", () => {
    const win = windowOf(
      [
        pt("2026-01-01", 90, null),
        pt("2026-01-02", 100, null),
        pt("2026-01-03", 110, null),
      ],
      "2026-01-03",
      "2026-01-03",
    );

    expect(win.investedUnavailableFrom).toBe("2026-01-01");
  });

  it("counts incomplete days inside the range", () => {
    const win = windowOf(
      [
        pt("2026-01-02", 100, 100),
        pt("2026-01-03", 55, 100, true),
        pt("2026-01-04", 120, 100),
      ],
      "2026-01-02",
      "2026-01-04",
      "2026-01-02",
    );

    expect(win.incompleteCount).toBe(1);
    expect(win.measured).toHaveLength(2);
    expect(win.rangePoints).toHaveLength(3);
  });
});

describe("periodValueSeries", () => {
  it("plots measured points only, so an incomplete day shows no plunge", () => {
    const series = periodValueSeries(
      windowOf(
        [
          pt("2026-01-02", 100, 100),
          pt("2026-01-03", 55, 100, true),
          pt("2026-01-04", 120, 100),
        ],
        "2026-01-02",
        "2026-01-04",
        "2026-01-02",
      ),
    );

    expect(series.value).toEqual([
      { time: "2026-01-02", value: 100 },
      { time: "2026-01-04", value: 120 },
    ]);
    expect(series.invested).toEqual([
      { time: "2026-01-02", value: 100 },
      { time: "2026-01-04", value: 100 },
    ]);
  });

  it("drops invested points the backend could not derive", () => {
    const series = periodValueSeries(
      windowOf(
        [pt("2026-01-02", 100, null), pt("2026-01-03", 110, null)],
        "2026-01-02",
        "2026-01-03",
        "2026-01-02",
      ),
    );

    expect(series.value).toHaveLength(2);
    expect(series.invested).toEqual([]);
  });
});

describe("periodGainSeries — the opening origin", () => {
  it("starts the line at zero on the calendar day before the range", () => {
    const gain = gainOf(
      [pt("2026-01-02", 100, 100), pt("2026-01-05", 130, 100)],
      "2026-01-05",
      "2026-01-05",
      "2026-01-02",
    );

    expect(gain.status).toBe("available");
    expect(gain.sek[0]).toEqual({ time: "2026-01-04", value: 0 });
    expect(gain.originDate).toBe("2026-01-04");
    expect(gain.percent[0]).toEqual({ time: "2026-01-04", value: 0 });
  });

  it("never lets the origin collide with a measured point", () => {
    const gain = gainOf(
      [pt("2026-01-04", 100, 100), pt("2026-01-05", 130, 100)],
      "2026-01-05",
      "2026-01-05",
      "2026-01-04",
    );

    const dates = gain.sek.map((point) => point.time);
    expect(new Set(dates).size).toBe(dates.length);
    expect(dates[0]).toBe("2026-01-04");
  });

  it("carries the first measured point's move from the anchor, not from zero", () => {
    const gain = gainOf(
      [pt("2026-01-04", 100, 100), pt("2026-01-05", 130, 100)],
      "2026-01-05",
      "2026-01-05",
      "2026-01-04",
    );

    expect(gain.sek.at(-1)).toEqual({ time: "2026-01-05", value: 30 });
  });
});

describe("periodGainSeries — rebased SEK", () => {
  it("does not count a mid-period deposit as gain", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 100, 100),
        pt("2026-01-03", 1_100, 1_100),
      ],
      "2026-01-02",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.sek.at(-1)?.value).toBe(0);
  });

  it("reproduces the plain cumulative series for an all-time range", () => {
    const points = [
      pt("2026-01-01", 100, 100),
      pt("2026-01-02", 150, 120),
      pt("2026-01-03", 220, 120),
    ];
    const gain = gainOf(points, null, null, "2026-01-01");

    expect(gain.sek.slice(1)).toEqual([
      { time: "2026-01-01", value: 0 },
      { time: "2026-01-02", value: 30 },
      { time: "2026-01-03", value: 100 },
    ]);
  });

  it("is unchanged by a skipped incomplete point, being endpoint arithmetic", () => {
    const withGap = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 60, 100, true),
        pt("2026-01-03", 130, 100),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );
    const without = gainOf(
      [pt("2026-01-01", 100, 100), pt("2026-01-03", 130, 100)],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(withGap.sek.at(-1)?.value).toBe(30);
    expect(withGap.sek.at(-1)?.value).toBe(without.sek.at(-1)?.value);
  });
});

describe("periodGainSeries — percent chaining", () => {
  it("chains consecutive observations into the compounded return", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 110, 100),
        pt("2026-01-03", 121, 100),
      ],
      "2026-01-02",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.percent.at(-1)?.value).toBeCloseTo(21, 6);
    expect(gain.approximate).toBe(false);
  });

  it("leaves the return unmoved by a deposit between two observations", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 110, 100),
        pt("2026-01-03", 1_110, 1_100),
      ],
      "2026-01-02",
      "2026-01-03",
      "2026-01-01",
    );

    // The 1000 deposit on the 3rd is ten times the portfolio and must not
    // register as performance: the reading is unchanged from the day before.
    expect(gain.percent.at(-1)?.value).toBeCloseTo(10, 6);
    expect(gain.percent.at(-1)?.value).toBeCloseTo(
      gain.percent[1]?.value ?? Number.NaN,
      6,
    );
    expect(gain.approximate).toBe(false);
  });

  it("reads zero percent at the first point of a known-empty opening, exactly", () => {
    const gain = gainOf(
      [pt("2026-01-01", 100, 100), pt("2026-01-02", 110, 100)],
      "2026-01-01",
      "2026-01-02",
      "2026-01-01",
    );

    expect(gain.percent[1]).toEqual({ time: "2026-01-01", value: 0 });
    expect(gain.percent.at(-1)?.value).toBeCloseTo(10, 6);
    expect(gain.approximate).toBe(false);
  });

  it("chains normally across a weekend, which is absence and not missing data", () => {
    const gain = gainOf(
      [
        pt("2026-01-02", 100, 100),
        pt("2026-01-05", 110, 100),
        pt("2026-01-06", 121, 100),
      ],
      "2026-01-05",
      "2026-01-06",
      "2026-01-02",
    );

    expect(gain.percent.at(-1)?.value).toBeCloseTo(21, 6);
    expect(gain.approximate).toBe(false);
  });
});

describe("periodGainSeries — when the percentage is approximate", () => {
  it("stays exact when no money moved across a skipped incomplete day", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 60, 100, true),
        pt("2026-01-03", 121, 100),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.approximate).toBe(false);
  });

  it("marks the range approximate when a purchase straddles a skipped day", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 200, 200, true),
        pt("2026-01-03", 220, 200),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.approximate).toBe(true);
    expect(gain.percent).toHaveLength(3);
  });

  it("marks the range approximate when a sale straddles a skipped day", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 200, 200),
        pt("2026-01-02", 100, 100, true),
        pt("2026-01-03", 110, 100),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.approximate).toBe(true);
  });

  it("never marks the SEK line approximate, which is exact by construction", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 200, 200, true),
        pt("2026-01-03", 220, 200),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.sek.at(-1)?.value).toBe(20);
  });
});

describe("periodGainSeries — the link ladder", () => {
  it("falls back to the start-of-period form when a flow exceeds the ending value", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 1_000, 1_000),
        pt("2026-01-02", 400, 1_500),
        pt("2026-01-03", 440, 1_500),
      ],
      "2026-01-02",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.approximate).toBe(true);
    const first = gain.percent[1]?.value ?? 0;
    expect(first).toBeGreaterThan(-100);
    expect(first).toBeLessThan(0);
    expect(gain.percent.at(-1)?.value).toBeGreaterThan(first);
  });

  it("never produces a loss worse than minus one hundred percent", () => {
    const gain = gainOf(
      [pt("2026-01-01", 1_000, 1_000), pt("2026-01-02", 1, 1_500)],
      "2026-01-02",
      "2026-01-02",
      "2026-01-01",
    );

    expect(gain.percent.at(-1)?.value).toBeGreaterThan(-100);
  });

  it("continues the chain through an unmeasurable link instead of erasing it", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 0, 0),
        pt("2026-01-02", 0, 500),
        pt("2026-01-03", 550, 500),
      ],
      "2026-01-02",
      "2026-01-03",
      "2025-06-01",
    );

    expect(gain.status).toBe("available");
    expect(gain.approximate).toBe(true);
    expect(gain.percent).toHaveLength(3);
  });
});

describe("periodGainSeries — availability states", () => {
  it("reports no history when the range holds no stored points", () => {
    const gain = gainOf(
      [pt("2026-01-01", 100, 100)],
      "2026-02-01",
      "2026-02-05",
      "2026-01-01",
    );

    expect(gain.status).toBe("no_history");
    expect(gain.sek).toEqual([]);
  });

  it("distinguishes an all-incomplete range from missing exchange rates", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 50, 100, true),
        pt("2026-01-03", 60, 100, true),
      ],
      "2026-01-02",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.status).toBe("all_incomplete");
  });

  it("reports an unknown opening rather than presenting inception gains as period gains", () => {
    const gain = gainOf(
      [pt("2026-03-01", 500, 200), pt("2026-03-02", 520, 200)],
      "2026-03-01",
      "2026-03-02",
      "2025-06-01",
    );

    expect(gain.status).toBe("unknown_opening");
    expect(gain.sek).toEqual([]);
  });

  it("names the historical first-null date when invested capital is unknown", () => {
    const gain = gainOf(
      [
        pt("2025-12-20", 90, null),
        pt("2026-01-02", 100, null),
        pt("2026-01-03", 110, null),
      ],
      "2026-01-02",
      "2026-01-03",
      "2025-06-01",
    );

    expect(gain.status).toBe("missing_trade_fx");
    expect(gain.investedUnavailableFrom).toBe("2025-12-20");
  });

  it("truncates the series when invested capital runs out part-way through", () => {
    const gain = gainOf(
      [
        pt("2026-01-01", 100, 100),
        pt("2026-01-02", 110, 100),
        pt("2026-01-03", 120, null),
      ],
      "2026-01-01",
      "2026-01-03",
      "2026-01-01",
    );

    expect(gain.status).toBe("available");
    expect(gain.investedUnavailableAfter).toBe("2026-01-02");
    expect(gain.sek.at(-1)?.time).toBe("2026-01-02");
  });

  it("reports when the chart ends before the period does", () => {
    const gain = gainOf(
      [pt("2026-01-01", 100, 100), pt("2026-01-02", 110, 100)],
      "2026-01-01",
      "2026-01-31",
      "2026-01-01",
    );

    expect(gain.endsEarlyAt).toBe("2026-01-02");
  });

  it("does not claim an early end when the last point reaches the range end", () => {
    const gain = gainOf(
      [pt("2026-01-01", 100, 100), pt("2026-01-02", 110, 100)],
      "2026-01-01",
      "2026-01-02",
      "2026-01-01",
    );

    expect(gain.endsEarlyAt).toBeNull();
  });

  it("draws a real two-point line for a single-observation Today range", () => {
    const gain = gainOf(
      [pt("2026-01-01", 100, 100), pt("2026-01-02", 130, 100)],
      "2026-01-02",
      "2026-01-02",
      "2026-01-01",
    );

    expect(gain.sek).toEqual([
      { time: "2026-01-01", value: 0 },
      { time: "2026-01-02", value: 30 },
    ]);
  });
});

describe("chartPanelHeading", () => {
  it("names the period in plain language for each preset", () => {
    const heading = (preset: "today" | "7d" | "12m" | "ytd" | "all") =>
      chartPanelHeading({
        view: "gain",
        unit: "sek",
        period: { preset, startDate: "2026-01-01", endDate: "2026-09-24" },
      });

    expect(heading("today")).toBe("Portfolio gain, today (SEK)");
    expect(heading("7d")).toBe("Portfolio gain, last 7 days (SEK)");
    expect(heading("12m")).toBe("Portfolio gain, last 12 months (SEK)");
    expect(heading("ytd")).toBe("Portfolio gain, year to date (SEK)");
    expect(heading("all")).toBe("Portfolio gain, all time (SEK)");
  });

  it("uses the resolved dates for a custom range", () => {
    expect(
      chartPanelHeading({
        view: "value",
        unit: "sek",
        period: {
          preset: "custom",
          startDate: "2026-02-01",
          endDate: "2026-03-01",
        },
      }),
    ).toBe("Portfolio value, 2026-02-01 to 2026-03-01 (SEK)");
  });

  it("falls back to a generic custom label when a resolved date is missing", () => {
    expect(
      chartPanelHeading({
        view: "gain",
        unit: "percent",
        period: { preset: "custom", startDate: null, endDate: "2026-03-01" },
      }),
    ).toBe("Portfolio gain, custom range (%)");
  });

  it("drops the period clause entirely when the period is stale or absent", () => {
    expect(
      chartPanelHeading({ view: "gain", unit: "percent", period: null }),
    ).toBe("Portfolio gain (%)");
    expect(
      chartPanelHeading({ view: "value", unit: "sek", period: null }),
    ).toBe("Portfolio value (SEK)");
  });
});

describe("dividendCaveatApplies", () => {
  const waterfall = (
    income: PortfolioWaterfall["income_base"],
    notTracked = false,
  ) => ({ income_base: income, income_not_tracked: notTracked });

  it("is false when income is not tracked at all", () => {
    expect(
      dividendCaveatApplies(
        waterfall({ status: "available", value: "1200" }, true),
      ),
    ).toBe(false);
  });

  it("is false when the period received no dividends", () => {
    expect(
      dividendCaveatApplies(waterfall({ status: "available", value: "0" })),
    ).toBe(false);
  });

  it("is true when the period actually received dividends", () => {
    expect(
      dividendCaveatApplies(waterfall({ status: "available", value: "1200" })),
    ).toBe(true);
  });

  it("is false when the income figure itself is unavailable", () => {
    expect(
      dividendCaveatApplies(waterfall({ status: "unavailable", reasons: [] })),
    ).toBe(false);
  });

  it("is false when there is no waterfall yet", () => {
    expect(dividendCaveatApplies(undefined)).toBe(false);
  });
});
