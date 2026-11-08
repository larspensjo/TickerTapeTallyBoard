import type { FundamentalsPreview, InstrumentFundamentals } from "../api/types";
import { reportDate, unavailableFigure } from "./companyFiguresFixtures";

export function instrumentFundamentals(): InstrumentFundamentals {
  const figure = {
    certainty: "complete",
    value: "12.345",
    reasons: [],
  } as const;
  return {
    pe: { ...figure, reasons: [] },
    ps: { ...figure, reasons: [] },
    profit_margin: { ...figure, value: "0.125", reasons: [] },
    sales_growth: { ...figure, value: "0.25", reasons: [] },
    next_report: reportDate({
      certainty: "estimated",
      basis: "typical_gap",
      date: "2026-11-30",
      days_until: 22,
      reasons: [],
    }),
    last_report: reportDate({
      certainty: "observed",
      date: "2026-10-20",
      period_end: "2026-09-30",
      observed_between: ["2026-10-18", "2026-10-22"],
      reasons: [],
    }),
    source: {
      symbol: "OLD",
      origin: "DISCOVERED",
      security_type: "EQUITY",
      classification: "EQUITY",
      status: "USABLE",
      candidates: [],
      last_success: "2026-11-01T12:00:00Z",
      last_success_on: "2026-11-01",
      last_attempt_on: "2026-11-01",
      last_attempt_at: "2026-11-01T12:00:00Z",
      last_attempt_outcome: "SUCCEEDED",
      last_attempt_reason: null,
      has_active_figures: true,
      awaiting_figures: false,
      pending_update: false,
    },
    quarters: [
      {
        period_end: "2026-03-31",
        missing: false,
        sales_missing: false,
        eps_missing: false,
        currency: "EUR",
        sales: { ...figure, value: "100000", reasons: [] },
        eps: { ...figure, value: "-0.5", reasons: [] },
      },
      {
        period_end: "2026-06-30",
        missing: true,
        sales_missing: true,
        eps_missing: true,
        currency: null,
        sales: unavailableFigure(),
        eps: unavailableFigure(),
      },
      {
        period_end: "2026-09-30",
        missing: false,
        sales_missing: false,
        eps_missing: false,
        currency: "EUR",
        sales: { ...figure, value: "120000", reasons: [] },
        eps: { ...figure, value: "1.5", reasons: [] },
      },
    ],
    fx_conversion: {
      from: "EUR",
      to: "SEK",
      rate: "11.25",
      date: "2026-11-06",
      from_rate_date: "2026-11-06",
      to_rate_date: "2026-11-06",
    },
    share_basis: {
      state: "established",
      ratio: "1",
      evidence_date: "2026-11-06",
    },
    data_revision: "1",
  };
}
export function fundamentalsPreview(): FundamentalsPreview {
  return {
    status: "found",
    symbol: "MSFT",
    name: "Microsoft Corporation",
    security_type: "EQUITY",
    report_currency: "USD",
    quarters: 5,
    latest_period_end: "2026-09-30",
    data_revision: "1",
  };
}

export function fundamentalsQuarterHistory(count: number) {
  const quarter = instrumentFundamentals().quarters[2];
  return [
    "2023-12-31",
    "2024-03-31",
    "2024-06-30",
    "2024-09-30",
    "2024-12-31",
    "2025-03-31",
    "2025-06-30",
    "2025-09-30",
    "2025-12-31",
    "2026-03-31",
    "2026-06-30",
    "2026-09-30",
  ]
    .slice(-count)
    .map((period_end) => ({
      ...quarter,
      period_end,
      sales: { ...quarter.sales, value: "565871000.0" },
      eps: { ...quarter.eps, value: "1.22" },
    }));
}
