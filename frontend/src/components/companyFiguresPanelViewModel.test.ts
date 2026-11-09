import { describe, expect, it } from "vitest";
import type { FigureCertainty } from "../api/types";
import {
  fundamentalsQuarterHistory,
  instrumentFundamentals,
} from "../test/instrumentFundamentalsFixture";
import {
  companyFiguresPanelView,
  fundamentalsSourceView,
  quarterChartView,
} from "./companyFiguresPanelViewModel";

describe("company figures panel", () => {
  it.each([8, 9, 12])(
    "fits all %s quarters in chronological slots with compact labels",
    (count) => {
      const data = instrumentFundamentals();
      data.quarters = fundamentalsQuarterHistory(count);
      const view = companyFiguresPanelView(data);
      for (const chart of [view.sales, view.eps]) {
        expect(chart.slotCount).toBe(count);
        expect(chart.bars.map((bar) => bar.periodEnd)).toEqual(
          data.quarters.map((quarter) => quarter.period_end),
        );
        expect(chart.bars.at(-1)?.periodEnd).toBe("2026-09-30");
        expect(chart.bars.map((bar) => bar.dateText)).toEqual(
          data.quarters.map((quarter) => quarter.period_end.slice(0, 7)),
        );
      }
      expect(view.sales.bars.every((bar) => bar.text === "565.9M EUR")).toBe(
        true,
      );
      expect(view.eps.bars.every((bar) => bar.text === "1.22 EUR")).toBe(true);
      expect(view.sales.bars.at(-1)?.label).toBe(
        "Sales for the quarter ending 2026-09-30: 565,871,000 EUR",
      );
    },
  );
  it("compacts large EPS while preserving its full amount in the accessible label", () => {
    const quarters = fundamentalsQuarterHistory(1);
    quarters[0].eps.value = "12345.6789";
    const bar = quarterChartView(quarters, "eps").bars[0];
    expect(bar.text).toBe("12.3k EUR");
    expect(bar.label).toBe(
      "EPS for the quarter ending 2026-09-30: 12,345.6789 EUR",
    );
  });
  it.each([
    "complete",
    "approximate",
    "not_applicable",
    "unavailable",
  ] satisfies FigureCertainty[])(
    "shares %s certainty formatting with Holdings",
    (certainty) => {
      const data = instrumentFundamentals();
      data.pe = {
        certainty,
        value:
          certainty === "complete" || certainty === "approximate"
            ? "12.345"
            : null,
        reasons:
          certainty === "complete"
            ? []
            : [
                {
                  code:
                    certainty === "approximate"
                      ? "ttm_from_three_quarters"
                      : certainty === "not_applicable"
                        ? "fund_or_etf"
                        : "source_unreachable",
                },
              ],
      };
      const view = companyFiguresPanelView(data);
      expect(view.figures[0].value.text).toBe(
        certainty === "not_applicable"
          ? "n/a"
          : certainty === "unavailable"
            ? "—"
            : "12.3",
      );
      expect(view.figures[0].value.prefix).toBe(
        certainty === "approximate" ? "~" : "",
      );
      expect(view.figures[2].value.text).toBe("12.50%");
      expect(view.figures[3].value.text).toBe("25.00%");
    },
  );
  it("names the backend FX rate only across currencies", () => {
    const data = instrumentFundamentals();
    expect(companyFiguresPanelView(data).fxNote).toContain(
      "Converted EUR→SEK at 11.25",
    );
    expect(companyFiguresPanelView(data).fxNote).toContain("2026-11-06");
    if (data.fx_conversion) data.fx_conversion.from = "SEK";
    expect(companyFiguresPanelView(data).fxNote).toBeNull();
    data.fx_conversion = null;
    expect(companyFiguresPanelView(data).fxNote).toBeNull();
  });
  it("maps gaps, currencies and negative EPS around the zero axis", () => {
    const view = companyFiguresPanelView(instrumentFundamentals());
    expect(view.sales.bars[1].label).toBe(
      "No figures reported for the quarter ending 2026-06-30",
    );
    expect(view.sales.bars[1].missing).toBe(true);
    expect(view.eps.bars[0].negative).toBe(true);
    expect(view.eps.bars[0].bottom).toBe(0);
    expect(view.eps.bars[0].height).toBe(view.eps.axis);
    expect(view.eps.bars[2].bottom).toBe(view.eps.axis);
    expect(view.eps.bars[2].text).toContain("EUR");
  });
  it("keeps placeholders inside the plot for wholly negative or empty series", () => {
    const data = instrumentFundamentals();
    data.quarters[2].eps.value = "-10";
    for (const bar of quarterChartView(data.quarters, "eps").bars) {
      expect(bar.bottom).toBeGreaterThanOrEqual(0);
      expect(bar.bottom + bar.height).toBeLessThanOrEqual(100);
    }
    expect(quarterChartView([], "eps").bars).toEqual([]);
    expect(quarterChartView([], "eps").slotCount).toBe(1);
  });
  it("distinguishes excluded reported figures from missing quarters", () => {
    const data = instrumentFundamentals();
    data.quarters[0].eps = {
      certainty: "unavailable",
      value: null,
      reasons: [{ code: "split_since_report" }],
    };
    data.quarters[0].eps_missing = true;
    const chart = quarterChartView(data.quarters, "eps");
    expect(chart.bars[0].label).toBe(
      "Figures excluded for the quarter ending 2026-03-31: A share split occurred after the latest report",
    );
    expect(chart.bars[1].label).toBe(
      "No figures reported for the quarter ending 2026-06-30",
    );
    expect(chart.bars[0].missing).toBe(true);
  });
  it.each(["sales", "eps"] as const)(
    "scales %s only in the latest report currency",
    (metric) => {
      const data = instrumentFundamentals();
      data.quarters[0].currency = "SEK";
      data.quarters[0][metric].value = "999999999999";
      const chart = quarterChartView(data.quarters, metric);
      expect(chart.bars[0].label).toBe(
        "Figures excluded for the quarter ending 2026-03-31: Reporting currency SEK differs from latest report currency EUR",
      );
      expect(chart.bars[0].missing).toBe(true);
      expect(chart.bars[2].height).toBe(100);
      expect(chart.axis).toBeCloseTo(0);
      expect(data.quarters[0][metric].value).toBe("999999999999");
    },
  );
  it("shows awaiting, observed windows, quarter ends and backend days without deriving dates", () => {
    const data = instrumentFundamentals();
    data.source.awaiting_figures = true;
    data.next_report.days_until = 999;
    const view = companyFiguresPanelView(data);
    expect(view.awaiting).toContain("Figures for OLD load on the next refresh");
    expect(view.nextReport.hint).toBe("in 999 days");
    expect(view.lastPeriod).toBe("quarter ending 2026-09-30");
    expect(view.observedWindow).toBe(
      "Observed between 2026-10-18 and 2026-10-22",
    );
  });
  it("explains discovery ambiguity, candidates, unknown type and last attempt", () => {
    const source = instrumentFundamentals().source;
    source.security_type = null;
    source.status = "AMBIGUOUS";
    source.last_attempt_outcome = "FAILED";
    source.last_attempt_reason = "source_ambiguous";
    source.candidates = [{ symbol: "A.ST", name: "Company A" }];
    expect(fundamentalsSourceView(source)).toMatchObject({
      origin: "automatic",
      securityType: "type unknown",
      outcome: "Update failed",
      reason: "More than one company matches",
      candidates: ["A.ST — Company A"],
    });
  });
  it("names security types in plain words", () => {
    const source = instrumentFundamentals().source;
    for (const [type, label] of [
      ["EQUITY", "Equity"],
      ["ETF", "ETF"],
      ["MUTUALFUND", "Fund"],
      ["CRYPTOCURRENCY", "Cryptocurrency"],
    ]) {
      source.security_type = type;
      expect(fundamentalsSourceView(source).securityType).toBe(label);
    }
  });
  it("collapses a same-day successful attempt into one update row", () => {
    const source = instrumentFundamentals().source;
    source.last_success_on = "2026-11-09";
    source.last_attempt_on = "2026-11-09";
    source.last_attempt_outcome = "SUCCEEDED";
    expect(fundamentalsSourceView(source).updateRows).toEqual([
      { label: "Updated", value: "2026-11-09", numeric: true, date: null },
    ]);
  });
  it("keeps both update rows when the last attempt did not succeed", () => {
    const source = instrumentFundamentals().source;
    source.last_success_on = "2026-11-01";
    source.last_attempt_on = "2026-11-09";
    source.last_attempt_outcome = "EMPTY";
    expect(fundamentalsSourceView(source).updateRows).toEqual([
      {
        label: "Last successful update",
        value: "2026-11-01",
        numeric: true,
        date: null,
      },
      {
        label: "Last attempt",
        value: "No figures returned",
        numeric: false,
        date: "2026-11-09",
      },
    ]);
  });
});

it("shows awaiting for a newly selected source before its first figures arrive", () => {
  const data = instrumentFundamentals();
  data.source.awaiting_figures = false;
  data.source.has_active_figures = false;
  data.source.pending_update = true;
  data.quarters = [];
  expect(companyFiguresPanelView(data).awaiting).toContain(
    "Figures for OLD load on the next refresh",
  );
});
