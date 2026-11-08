import { describe, expect, it } from "vitest";
import type { Figure, ReportDate } from "../api/types";
import {
  holdingsFundamentals,
  reportDate,
} from "../test/companyFiguresFixtures";
import {
  compareFundamentals,
  holdingFundamentalsView,
  holdingReportSortField,
  holdingValuationSortField,
} from "./holdingsFundamentals";

describe("holdings fundamentals sort fields", () => {
  const figures: Figure[] = [
    {
      certainty: "not_applicable",
      value: null,
      reasons: [{ code: "fund_or_etf" }],
    },
    { certainty: "complete", value: "100", reasons: [] },
    {
      certainty: "unavailable",
      value: null,
      reasons: [{ code: "loss_making" }],
    },
    {
      certainty: "approximate",
      value: "9.5",
      reasons: [{ code: "price_stale" }],
    },
    { certainty: "complete", value: "20", reasons: [] },
  ];
  it("orders P/E numerically with n/a and unavailable last before table reversal", () => {
    const fields = figures.map((pe) => ({
      certainty: pe.certainty,
      value: holdingValuationSortField({
        fundamentals: { ...holdingsFundamentals(), pe },
      }),
    }));
    const sorted = fields.sort((left, right) =>
      compareFundamentals(left.value, right.value),
    );
    expect(sorted.map((field) => field.value)).toEqual([
      9.5,
      20,
      100,
      undefined,
      undefined,
    ]);
    expect(sorted.slice(-2).map((field) => field.certainty)).toEqual([
      "not_applicable",
      "unavailable",
    ]);
  });
  it("uses P/E alone even when P/S has an available value", () => {
    const fundamentals = { ...holdingsFundamentals(), ps: figures[1] };
    expect(holdingValuationSortField({ fundamentals })).toBeUndefined();
    expect(holdingFundamentalsView({ fundamentals }).ps.text).toBe("100.0");
  });
  it("orders reports by backend days with no-date rows last before table reversal", () => {
    const reports: ReportDate[] = [
      reportDate({ certainty: "not_applicable" }),
      reportDate({
        certainty: "estimated",
        basis: "nasdaq_expected",
        date: "2026-12-01",
        days_until: 20,
      }),
      reportDate(),
      reportDate({
        certainty: "confirmed",
        basis: "company_announced",
        date: "2026-11-01",
        days_until: -2,
      }),
      reportDate({
        certainty: "estimated",
        basis: "typical_gap",
        date: "2026-11-08",
        days_until: 0,
      }),
      reportDate({ date: null, days_until: 1 }),
      reportDate({ date: "2026-11-08", days_until: null }),
    ];
    // Sort objects so JavaScript does not move undefined values before calling the comparator.
    const fields = reports.map((next_report) => ({
      value: holdingReportSortField({
        fundamentals: { ...holdingsFundamentals(), next_report },
      }),
    }));
    expect(
      fields
        .sort((left, right) => compareFundamentals(left.value, right.value))
        .map((field) => field.value),
    ).toEqual([-2, 0, 20, undefined, undefined, undefined, undefined]);
  });
});
