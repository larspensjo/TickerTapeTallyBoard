import { describe, expect, it } from "vitest";
import type { OpportunityEvidence } from "../api/types";
import { reportDate } from "../test/companyFiguresFixtures";
import {
  opportunityEvidence,
  opportunityIndexMarket,
} from "../test/opportunityFixtures";
import { opportunityChecksViewModel } from "./opportunityChecksViewModel";

type Checks = OpportunityEvidence["checks"];
function result(key: keyof Checks, checks: Partial<Checks> = {}) {
  const evidence = opportunityEvidence();
  Object.assign(evidence.checks, checks);
  const entry = opportunityChecksViewModel(evidence).find(
    (entry) => entry.key === key,
  );
  if (!entry) throw new Error(`Missing check ${key}`);
  return entry;
}
const text = (entry: ReturnType<typeof result>) =>
  entry.parts.map((p) => p.text).join("");

describe("readable opportunity checks", () => {
  it.each([
    [
      "unusual_drop",
      "price_history_gap",
      "gap in price history starting 1 Oct",
    ],
    ["off_high", "price_history_too_short", "too few closes"],
    [
      "report_in_period",
      "no_fundamentals_source",
      "no company figures source found",
    ],
    [
      "report_coming_soon",
      "too_few_quarters",
      "too few quarters of company figures",
    ],
    [
      "business",
      "newer_report_expected",
      "a newer report was expected; figures have not appeared yet",
    ],
    [
      "market",
      "insufficient_market_rows",
      "too few other holdings for the market comparison",
    ],
    [
      "stopped_falling",
      "missing_window_closes",
      "prices missing at the period boundaries",
    ],
    [
      "ex_dividend",
      "dividend_capture_invalid",
      "dividend capture failed validation",
    ],
    ["room_to_buy", "valuation_unavailable", "holding valuation unavailable"],
  ] as const)(
    "makes a real unknown reason readable for %s",
    (key, code, sentence) => {
      const check = opportunityEvidence().checks[key];
      if (key === "unusual_drop")
        Object.assign(check, {
          usual_move_percent: null,
          threshold_percent: null,
          sample_moves: null,
        });
      if (key === "off_high")
        Object.assign(check, {
          closes: 80,
          latest_close: null,
          high_close: null,
          low_close: null,
          below_high_percent: null,
          above_low_percent: null,
          near_low: null,
        });
      if (key === "report_in_period")
        Object.assign(check, { last_report: null });
      if (key === "report_coming_soon")
        Object.assign(check, { report: reportDate() });
      if (key === "market")
        Object.assign(check, {
          count: 3,
          reference_percent: null,
          difference_percent: null,
        });
      if (key === "room_to_buy")
        Object.assign(check, { target_status: "excluded_unavailable" });
      expect(
        text(
          result(key, {
            [key]: {
              ...check,
              state: "unknown",
              reasons: [
                {
                  code,
                  ...(key === "unusual_drop"
                    ? { start: "2026-10-01" }
                    : key === "off_high"
                      ? { count: 80, required: 200 }
                      : key === "market"
                        ? { count: 3, required: 5 }
                        : {}),
                },
              ],
            },
          }),
        ),
      ).toContain(`unknown; ${sentence}`);
    },
  );

  it("keeps negative business evidence readable when another sub-test is unavailable", () => {
    const check = opportunityEvidence().checks.business;
    check.state = "weakening";
    check.sales_not_shrinking = {
      passes: false,
      value: "-4.00",
      year_ago_value: null,
      approximate: false,
      reasons: [],
    };
    check.earnings_not_sliding = {
      passes: null,
      value: null,
      year_ago_value: null,
      approximate: false,
      reasons: [{ code: "year_ago_quarter_missing" }],
    };
    check.eps_consistency = {
      state: "unavailable",
      gap_percent: null,
      compared_quarters: 0,
    };
    expect(text(result("business", { business: check }))).toContain(
      "weakening: margin 1.00%, sales shrinking -4.00% vs a year ago, EPS unknown; the same quarter a year ago is missing",
    );
  });

  it("keeps derived EPS approximate and severe disagreement unavailable", () => {
    const check = opportunityEvidence().checks.business;
    check.earnings_not_sliding.approximate = true;
    check.earnings_not_sliding.reasons = [
      { code: "eps_derived_from_net_income" },
      { code: "eps_not_cross_checked" },
    ];
    check.eps_consistency = {
      state: "not_cross_checked",
      gap_percent: null,
      compared_quarters: 1,
    };
    let view = result("business", { business: check });
    expect(text(view)).toContain("EPS not cross-checked");
    expect(view.parts.find((p) => p.approximate)?.accessibleLabel).toContain(
      "earnings per share derived from net income and share count",
    );
    check.state = "unknown";
    check.reasons = [{ code: "eps_inconsistent" }];
    check.earnings_not_sliding.passes = null;
    check.earnings_not_sliding.approximate = false;
    check.earnings_not_sliding.reasons = [{ code: "eps_inconsistent" }];
    check.eps_consistency = {
      state: "inconsistent",
      gap_percent: "25.00",
      compared_quarters: 2,
    };
    view = result("business", { business: check });
    expect(text(view)).toContain(
      "unknown; EPS disagrees too much with net income and diluted shares",
    );
    expect(text(view)).toContain(
      "EPS disagreement 25.00%; earnings comparison unavailable",
    );
  });

  it.each(["estimate_passed", "report_date_passed"] as const)(
    "keeps passed next-report reasons visible (%s)",
    (code) => {
      const check = {
        state: "no" as const,
        reasons: [{ code }],
        report: reportDate({
          certainty: code === "report_date_passed" ? "confirmed" : "estimated",
          basis:
            code === "report_date_passed"
              ? "company_announced"
              : "last_year_date",
          date: "2026-10-12",
          days_until: -3,
          reasons: [{ code }],
        }),
      };
      expect(
        text(result("report_coming_soon", { report_coming_soon: check })),
      ).toBe(
        `no; 12 Oct (${code === "report_date_passed" ? "confirmed" : "est."}), figures not seen yet`,
      );
    },
  );

  it("keeps all nine checks in the agreed order", () => {
    expect(
      opportunityChecksViewModel(opportunityEvidence()).map((c) => c.name),
    ).toEqual([
      "Unusual drop",
      "Off its high",
      "Report in the period",
      "Report coming soon",
      "Business",
      "Market",
      "Stopped falling",
      "Ex-dividend",
      "Room to buy",
    ]);
  });

  it.each([
    ["unusual_drop", "yes", "yes; usual move 2.00%"],
    ["unusual_drop", "no", "no; usual move 10.00%"],
    ["off_high", "deep", "deep: 34.29% below the 1-year high"],
    ["off_high", "not_deep", "not deep: 8.00% below the 1-year high"],
    ["business", "intact", "intact: margin 1.00%"],
    ["business", "loss_making", "loss-making: margin -1.00%"],
    ["business", "weakening", "weakening: margin 1.00%"],
    [
      "market",
      "fell_more_than_market",
      "fell 7.00 pts more than the median of your other holdings",
    ],
    [
      "market",
      "moved_with_market",
      "moved with the median of your other holdings",
    ],
    [
      "market",
      "fell_less_than_market",
      "fell 4.00 pts less than the median of your other holdings",
    ],
    ["room_to_buy", "room_to_buy", "below your target"],
    ["room_to_buy", "already_at_target", "at or above your target"],
    ["room_to_buy", "no_target", "no target set"],
  ] as const)("explains %s / %s", (key, state, phrase) => {
    const check = opportunityEvidence().checks[key];
    if (key === "business" && state === "loss_making") {
      Object.assign(check, {
        profitable: {
          ...opportunityEvidence().checks.business.profitable,
          passes: false,
          value: "-1",
        },
      });
    }
    if (key === "business" && state === "weakening")
      Object.assign(check, {
        sales_not_shrinking: {
          ...opportunityEvidence().checks.business.sales_not_shrinking,
          passes: false,
          value: "-2.00",
        },
      });
    if (key === "off_high" && state === "deep")
      Object.assign(check, { high_close: "140", below_high_percent: "34.29" });
    if (key === "unusual_drop" && state === "no")
      Object.assign(check, {
        usual_move_percent: "10.00",
        threshold_percent: "-15.00",
      });
    if (key === "market" && state === "moved_with_market")
      Object.assign(check, {
        reference_percent: "-8.00",
        difference_percent: "0.00",
      });
    if (key === "market" && state === "fell_less_than_market")
      Object.assign(check, {
        reference_percent: "-12.00",
        difference_percent: "4.00",
      });
    if (key === "room_to_buy")
      Object.assign(check, {
        target_status:
          state === "room_to_buy"
            ? "below"
            : state === "no_target"
              ? "no_target"
              : "above",
        reasons: state === "no_target" ? [{ code: "no_target" }] : [],
      });
    expect(text(result(key, { [key]: { ...check, state } }))).toContain(phrase);
  });

  it("shows supplied price values with existing precision and a named market source", () => {
    const checks = opportunityEvidence().checks;
    checks.unusual_drop.threshold_percent = "-3.00";
    checks.unusual_drop.adjusted_change_percent = "-7.12";
    checks.ex_dividend = {
      state: "yes",
      reasons: [],
      events: [{ ex_date: "2026-10-12", amount: "0.88", currency: "SEK" }],
      amount: "0.88",
      percent_of_start_close: "0.88",
    };
    expect(text(result("unusual_drop", checks))).toBe(
      "yes; dividend-adjusted -7.12%; usual move 2.00%",
    );
    expect(text(result("off_high"))).toBe(
      "not deep: 8.00% below the 1-year high",
    );
    expect(text(result("market"))).toContain(
      "fell 7.00 pts more than the median of your other holdings (-1.00%)",
    );
    expect(result("market").tooltip).toBe(
      "The median mixes markets and currencies, includes funds, and can be self-referential in a concentrated portfolio; 8 other holdings; comparison margin 3.00 pts",
    );
  });

  it.each(["yes", "no"] as const)(
    "explains stopped falling %s with the supplied low and age",
    (state) => {
      const check = {
        ...opportunityEvidence().checks.stopped_falling,
        state,
        reasons: [],
        latest_close: "92",
        low_close: "90",
        low_date: "2026-10-12",
        low_age_trading_days: 3,
      };
      if (state === "no")
        Object.assign(check, {
          low_close: "92",
          low_date: "2026-10-15",
          low_age_trading_days: 0,
        });
      expect(text(result("stopped_falling", { stopped_falling: check }))).toBe(
        state === "yes"
          ? "yes: up from the period low of 90 on 12 Oct"
          : "no: low was 15 Oct",
      );
    },
  );

  it.each(["yes", "no"] as const)("explains ex-dividend %s", (state) => {
    const check = {
      ...opportunityEvidence().checks.ex_dividend,
      state,
      reasons: [],
      events:
        state === "yes"
          ? [{ ex_date: "2026-10-12", amount: "1.2", currency: "SEK" }]
          : [],
      amount: state === "yes" ? "1.2" : "0",
      percent_of_start_close: state === "yes" ? "1.20" : "0.00",
    };
    expect(text(result("ex_dividend", { ex_dividend: check }))).toBe(
      state === "yes"
        ? "yes; dividends 1.20% of the start close; 12 Oct: SEK 1.2"
        : "no ex-dividend event in the covered period",
    );
  });

  it.each(["report_in_period", "report_coming_soon", "business"] as const)(
    "keeps %s not applicable as its own style",
    (key) => {
      const check = {
        ...opportunityEvidence().checks[key],
        state: "not_applicable",
        reasons: [{ code: "fund_or_etf" }],
      };
      const view = result(key, { [key]: check });
      expect(text(view)).toBe("n/a");
      expect(view.parts[0].notApplicable).toBe(true);
    },
  );

  it.each(["confirmed", "interval", "upper_bound"] as const)(
    "preserves %s report knowledge",
    (kind) => {
      const check = opportunityEvidence().checks.report_in_period;
      check.state = kind === "upper_bound" ? "unknown" : "yes";
      check.last_report =
        kind === "confirmed"
          ? { kind, date: "2026-10-12" }
          : kind === "interval"
            ? {
                kind,
                period_end: "2026-09-30",
                absent_on: "2026-10-08",
                first_seen: "2026-10-12",
              }
            : { kind, period_end: "2026-09-30", first_seen: "2026-10-12" };
      check.reasons = [
        {
          code:
            kind === "confirmed"
              ? "confirmed_report"
              : kind === "interval"
                ? "observed_report_inside_window"
                : "first_fetch_without_baseline",
        },
      ];
      const sentence = text(
        result("report_in_period", { report_in_period: check }),
      );
      expect(sentence).toContain(
        kind === "confirmed"
          ? "confirmed report on 12 Oct"
          : kind === "interval"
            ? "last report for the quarter ending 30 Sep was between 8 Oct and 12 Oct"
            : "last report for the quarter ending 30 Sep was on or before 12 Oct",
      );
    },
  );

  it("describes a period-only report before the window as a bound, never a report date", () => {
    const check = opportunityEvidence().checks.report_in_period;
    check.last_report = {
      kind: "upper_bound",
      period_end: "2026-06-30",
      first_seen: "2026-09-20",
    };
    expect(text(result("report_in_period", { report_in_period: check }))).toBe(
      "no; last report for the quarter ending 30 Jun was before 1 Oct; no newer report in the period",
    );
  });

  it("keeps relaxed absence approximate with a title and accessible explanation", () => {
    const check = opportunityEvidence().checks.report_in_period;
    check.reasons = [{ code: "report_history_horizon" }];
    check.no_report_expected_before = "2026-11-30";
    const view = result("report_in_period", { report_in_period: check });
    expect(text(view)).toContain("no report expected before ~30 Nov (est.)");
    expect(view.parts.find((p) => p.approximate)).toMatchObject({
      title: "Estimated from report history",
      accessibleLabel: "approximately 30 Nov; Estimated from report history",
    });
  });

  it("says scheduled figures have not appeared", () => {
    const check = opportunityEvidence().checks.report_in_period;
    check.state = "yes";
    check.reasons = [{ code: "scheduled_report_figures_not_seen" }];
    check.scheduled_report = reportDate({
      certainty: "confirmed",
      basis: "company_announced",
      date: "2026-10-12",
      days_until: -3,
      reasons: [{ code: "report_date_passed" }],
    });
    expect(text(result("report_in_period", { report_in_period: check }))).toBe(
      "yes; scheduled 12 Oct (confirmed), figures not seen yet",
    );
  });

  it.each([
    ["confirmed", "company_announced", "confirmed"],
    ["estimated", "nasdaq_expected", "Nasdaq expected"],
    ["estimated", "last_year_date", "est."],
    ["estimated", "typical_gap", "est."],
  ] as const)(
    "reuses Holdings report certainty %s/%s",
    (certainty, basis, tag) => {
      for (const [state, days] of [
        ["yes", 5],
        ["no", 15],
      ] as const) {
        const check = {
          state,
          reasons: [],
          report: reportDate({
            certainty,
            basis,
            date: "2026-10-20",
            days_until: days,
            reasons: [],
          }),
        };
        expect(
          text(result("report_coming_soon", { report_coming_soon: check })),
        ).toBe(`${state}; 20 Oct (${tag}), in ${days} days`);
      }
    },
  );

  it("shows three business tests, approximate figures, and EPS cross-check information", () => {
    const check = opportunityEvidence().checks.business;
    check.profitable = {
      passes: true,
      value: "12.34",
      year_ago_value: null,
      approximate: true,
      reasons: [{ code: "ttm_from_three_quarters" }],
    };
    check.sales_not_shrinking = {
      passes: false,
      value: "-2.00",
      year_ago_value: null,
      approximate: false,
      reasons: [],
    };
    check.state = "weakening";
    check.earnings_not_sliding = {
      passes: false,
      value: "0.81",
      year_ago_value: "1",
      approximate: true,
      reasons: [{ code: "eps_disagree", detail: "12.34" }],
    };
    check.eps_consistency = {
      state: "approximate",
      gap_percent: "12.34",
      compared_quarters: 2,
    };
    const view = result("business", { business: check });
    expect(text(view)).toBe(
      "weakening: margin ~12.34%, sales shrinking -2.00% vs a year ago, EPS sliding ~0.81 vs ~1 a year ago; EPS disagreement 12.34%",
    );
    for (const part of view.parts.filter((p) => p.approximate)) {
      expect(part.title).toBeTruthy();
      expect(part.accessibleLabel).toBe(
        part.title === "EPS disagreement 12.34%"
          ? `approximately ${part.text.slice(1)}`
          : `approximately ${part.text.slice(1)}; ${part.title}`,
      );
    }
    check.earnings_not_sliding.reasons = [{ code: "eps_not_cross_checked" }];
    check.earnings_not_sliding.approximate = false;
    check.eps_consistency = {
      state: "not_cross_checked",
      gap_percent: null,
      compared_quarters: 1,
    };
    expect(text(result("business", { business: check }))).toContain(
      "EPS not cross-checked",
    );
  });

  it.each([
    "unusual_drop",
    "off_high",
    "report_in_period",
    "report_coming_soon",
    "business",
    "market",
    "stopped_falling",
    "ex_dividend",
    "room_to_buy",
  ] as const)("falls back safely for a future reason on %s", (key) => {
    expect(
      text(
        result(key, {
          [key]: {
            ...opportunityEvidence().checks[key],
            state: "unknown",
            reasons: [{ code: "future_reason" }],
          },
        }),
      ),
    ).toContain("unknown (reason not described)");
  });
});

it.each(["newer_report_expected", "source_unreachable"])(
  "states the business reason once (%s), retaining marker explanations",
  (code) => {
    const check = opportunityEvidence().checks.business;
    check.state = "unknown";
    check.reasons = [{ code }];
    for (const test of [
      check.profitable,
      check.sales_not_shrinking,
      check.earnings_not_sliding,
    ]) {
      test.approximate = true;
      test.reasons = [{ code }];
    }
    const view = result("business", { business: check });
    const reason =
      code === "source_unreachable"
        ? "company figures source could not be reached"
        : "a newer report was expected; figures have not appeared yet";
    expect(text(view)).toBe(
      `unknown; ${reason}; margin ~1.00%, sales ~1.00% vs a year ago, EPS ~1 vs ~1 a year ago`,
    );
    for (const part of view.parts.filter((p) => p.approximate)) {
      expect(part.title).toBe(reason);
      expect(part.accessibleLabel).toBe(`approximately ${part.text.slice(1)}`);
    }
    check.earnings_not_sliding.passes = null;
    expect(
      text(result("business", { business: check })).split(reason),
    ).toHaveLength(2);
  },
);

it.each([
  ["20.00", true, "~20.00%"],
  ["-20.00", true, "~-20.00%"],
  ["0.00", true, "~0.00%"],
  ["20.00", false, "+20.00%"],
  ["-20.00", false, "-20.00%"],
  ["0.00", false, "0.00%"],
] as const)(
  "renders sales %s with approximate=%s as %s",
  (value, approximate, expected) => {
    const check = opportunityEvidence().checks.business;
    check.sales_not_shrinking = {
      passes: !value.startsWith("-"),
      value,
      year_ago_value: null,
      approximate,
      reasons: approximate ? [{ code: "ttm_from_three_quarters" }] : [],
    };
    expect(text(result("business", { business: check }))).toContain(
      `sales${value.startsWith("-") ? " shrinking" : ""} ${expected} vs a year ago`,
    );
  },
);

it.each([false, true])(
  "states overlapping report reasons once (newer reason in sub-test=%s)",
  (inSubtest) => {
    const check = opportunityEvidence().checks.business;
    check.state = "unknown";
    check.reasons = [{ code: "expected_report_passed" }];
    if (inSubtest) {
      check.profitable.passes = null;
      check.profitable.reasons = [{ code: "newer_report_expected" }];
    } else {
      // Deliberately reverse the order: the more specific reason still wins.
      check.reasons.unshift({ code: "newer_report_expected" });
    }
    const view = result("business", { business: check });
    const retained =
      "expected report date has passed; newer figures not seen yet";
    const omitted =
      "a newer report was expected; figures have not appeared yet";
    expect(text(view)).toContain(`unknown; ${retained}; margin`);
    expect(text(view).split(retained)).toHaveLength(2);
    expect(text(view)).not.toContain(omitted);
    expect(view.tooltip).toContain(retained);
    expect(view.tooltip).toContain(omitted);
  },
);

it.each([true, false])(
  "keeps EPS hover explanations and speaks them only when absent (visible=%s)",
  (visible) => {
    const check = opportunityEvidence().checks.business;
    check.earnings_not_sliding = {
      passes: true,
      value: "1.12",
      year_ago_value: "1.12",
      approximate: true,
      reasons: visible
        ? [{ code: "eps_disagree", detail: "11.00" }]
        : [{ code: "eps_derived_from_net_income" }],
    };
    check.eps_consistency = {
      state: visible ? "approximate" : "not_cross_checked",
      gap_percent: visible ? "11.00" : null,
      compared_quarters: visible ? 2 : 1,
    };
    const view = result("business", { business: check });
    expect(text(view)).toContain("EPS ~1.12 vs ~1.12 a year ago");
    expect(text(view).includes("EPS disagreement 11.00%")).toBe(visible);
    const figures = view.parts.filter((part) => part.approximate);
    expect(figures).toHaveLength(2);
    for (const figure of figures) {
      expect(figure.title).toBe(
        visible
          ? "EPS disagreement 11.00%"
          : "earnings per share derived from net income and share count",
      );
      expect(figure.accessibleLabel).toBe(
        visible
          ? "approximately 1.12"
          : "approximately 1.12; earnings per share derived from net income and share count",
      );
    }
  },
);

it("states a passed app estimate once without calling it scheduled", () => {
  const check = opportunityEvidence().checks.report_in_period;
  check.state = "unknown";
  check.reasons = [{ code: "app_estimate_passed_in_window" }];
  check.scheduled_report = reportDate({
    certainty: "estimated",
    basis: "typical_gap",
    date: "2026-11-04",
    days_until: -3,
    reasons: [{ code: "estimate_passed" }],
  });
  expect(text(result("report_in_period", { report_in_period: check }))).toBe(
    "unknown; estimated 4 Nov (est.), figures not seen yet",
  );
});

it("keeps moved diagnostics in tooltips rather than visible results", () => {
  const unusual = result("unusual_drop");
  expect(unusual.tooltip).toBe(
    "Threshold -3.00%; 250 closes; 240 historical moves",
  );
  expect(text(unusual)).toBe("yes; usual move 2.00%");
  const high = result("off_high");
  expect(high.tooltip).toBe(
    "1-year high 100; 1-year low 80; 15.00% above the low; 250 closes",
  );
  expect(text(high)).toBe("not deep: 8.00% below the 1-year high");
  const market = result("market");
  expect(market.tooltip).toContain(
    "8 other holdings; comparison margin 3.00 pts",
  );
  expect(text(market)).toBe(
    "fell 7.00 pts more than the median of your other holdings (-1.00%)",
  );
  const check = opportunityEvidence().checks.stopped_falling;
  Object.assign(check, {
    state: "no",
    reasons: [],
    latest_close: "92",
    low_close: "90",
    low_date: "2026-10-12",
    low_age_trading_days: 1,
  });
  const stopped = result("stopped_falling", { stopped_falling: check });
  expect(stopped.tooltip).toBe("Low was 1 trading day ago");
  expect(text(stopped)).toBe("no: low was 12 Oct");
});

it("retains counts for unknown checks in their tooltips", () => {
  const check = opportunityEvidence().checks.off_high;
  check.state = "unknown";
  check.reasons = [
    { code: "price_history_too_short", count: 80, required: 200 },
  ];
  const view = result("off_high", { off_high: check });
  expect(text(view)).toBe("unknown; too few closes");
  expect(view.tooltip).toContain("too few closes: 80; need 200");
});

it("keeps timing words in the UI font and deep price context visible", () => {
  const soon = result("report_coming_soon");
  expect(
    soon.parts.find((p) => p.text.includes("in 31 days"))?.figure,
  ).toBeUndefined();
  const check = opportunityEvidence().checks.off_high;
  Object.assign(check, {
    state: "deep",
    below_high_percent: "32.00",
    near_low: true,
  });
  expect(text(result("off_high", { off_high: check }))).toBe(
    "deep: 32.00% below the 1-year high, near the 1-year low",
  );
});

it.each(["market_index", "other_holdings_median"] as const)(
  "names the reference for all three market states (%s)",
  (source) => {
    const check =
      source === "market_index"
        ? opportunityIndexMarket()
        : opportunityEvidence().checks.market;
    const reference =
      source === "market_index"
        ? "OMX Stockholm all-share"
        : "the median of your other holdings";
    for (const [state, difference, expected] of [
      ["fell_more_than_market", "-7.00", "fell 7.00 pts more than"],
      ["fell_less_than_market", "7.00", "fell 7.00 pts less than"],
      ["moved_with_market", "0.00", "moved with"],
    ] as const) {
      check.state = state;
      check.difference_percent = difference;
      const view = result("market", { market: check });
      expect(text(view)).toBe(`${expected} ${reference} (-1.00%)`);
      if (source === "market_index") {
        expect(view.tooltip).toContain(
          "OMX Stockholm all-share is a price index without dividends; ",
        );
        expect(view.tooltip).toContain(
          "Index closes paired on 8 Oct and 14 Oct",
        );
        expect(view.tooltip).not.toContain("other holdings");
      } else {
        expect(view.tooltip).toContain(
          "The median mixes markets and currencies, includes funds, and can be self-referential in a concentrated portfolio; ",
        );
        expect(view.tooltip).not.toContain("price index");
      }
    }
  },
);

it("keeps the fallback reason visible alongside the median, also when unknown", () => {
  const check = opportunityEvidence().checks.market;
  check.reasons = [
    { code: "market_index_unavailable", detail: "OMX Stockholm all-share" },
  ];
  const view = result("market", { market: check });
  expect(text(view)).toBe(
    "fell 7.00 pts more than the median of your other holdings (-1.00%); OMX Stockholm all-share has no usable closes for these dates",
  );
  expect(view.tooltip).toContain(
    "OMX Stockholm all-share has no usable closes for these dates",
  );
  check.state = "unknown";
  check.reasons.unshift({
    code: "insufficient_market_rows",
    count: 4,
    required: 5,
  });
  expect(text(result("market", { market: check }))).toContain(
    "OMX Stockholm all-share has no usable closes for these dates",
  );
});

it.each([
  ["2026-10-08", "2026-10-15", false],
  ["2026-10-07", "2026-10-15", true],
  ["2026-10-08", "2026-10-14", true],
])(
  "explains paired index dates only when different (%s, %s)",
  (start, end, showDates) => {
    const evidence = opportunityEvidence();
    evidence.checks.market = opportunityIndexMarket({
      index_start_date: start,
      index_end_date: end,
    });
    const view = opportunityChecksViewModel(evidence, {
      start_date: "2026-10-08",
      end_date: "2026-10-15",
    }).find((check) => check.key === "market");
    expect(view?.tooltip).toContain("price index without dividends");
    expect(view?.tooltip?.includes("Index closes paired on")).toBe(showDates);
  },
);
