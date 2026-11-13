import "../styles.css";
import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";
import { reportDate } from "../test/companyFiguresFixtures";
import {
  opportunityEvidence,
  opportunityFixture,
} from "../test/opportunityFixtures";
import { OpportunitiesPage } from "./OpportunitiesPage";

vi.mock("../api/queries", () => ({
  useOpportunities: () => {
    const company = opportunityEvidence();
    company.checks.business.profitable.approximate = true;
    company.checks.business.profitable.reasons = [
      { code: "ttm_from_three_quarters" },
    ];
    const fund = opportunityEvidence();
    fund.verdict.deciding = ["unusual_drop", "report_in_period", "business"];
    fund.checks.report_in_period = {
      ...fund.checks.report_in_period,
      state: "not_applicable",
      reasons: [{ code: "fund_or_etf" }],
      last_report: null,
    };
    fund.checks.report_coming_soon = {
      state: "not_applicable",
      reasons: [{ code: "fund_or_etf" }],
      report: reportDate({
        certainty: "not_applicable",
        reasons: [{ code: "fund_or_etf" }],
      }),
    };
    const missing = {
      passes: null,
      value: null,
      year_ago_value: null,
      approximate: false,
      reasons: [{ code: "fund_or_etf" }],
    };
    fund.checks.business = {
      state: "not_applicable",
      reasons: [{ code: "fund_or_etf" }],
      profitable: missing,
      sales_not_shrinking: missing,
      earnings_not_sliding: {
        ...missing,
        reasons: [{ code: "year_ago_quarter_missing" }],
      },
      eps_consistency: {
        state: "unavailable",
        gap_percent: null,
        compared_quarters: 0,
      },
    };
    return {
      data: {
        data_revision: "evidence-colours",
        period: {
          window: "7d",
          start_date: "2026-10-08",
          end_date: "2026-10-15",
        },
        rows: [opportunityFixture(1, company), opportunityFixture(2, fund)],
      },
      isError: false,
      error: null,
    };
  },
}));
vi.mock("./useAppMode", () => ({ useAppMode: () => ({ canMutate: false }) }));

afterEach(() => {
  cleanup();
  localStorage.clear();
});

function tokenColour(token: string): string {
  const probe = document.createElement("span");
  probe.style.color = `var(${token})`;
  document.body.append(probe);
  const colour = getComputedStyle(probe).color;
  probe.remove();
  return colour;
}

it("keeps evidence readable, approximation amber and n/a italic", () => {
  render(
    <MemoryRouter>
      <div className="app-shell">
        <main className="workspace">
          <OpportunitiesPage />
        </main>
      </div>
    </MemoryRouter>,
  );
  const table = screen.getByRole("table", { name: "Opportunities" });
  const company = within(table).getByRole("group", {
    name: "Evidence for Company 1",
  });
  for (const result of [
    "the dividend range is not fully covered",
    "below your target",
    "intact",
    "2.00%",
  ]) {
    const style = getComputedStyle(within(company).getByText(result));
    expect(style.color).toBe(tokenColour("--text-secondary"));
    expect(style.fontSize).toBe("13px");
  }
  const marker = within(company).getByText("~1.00%");
  expect(getComputedStyle(marker).color).toBe(tokenColour("--warning"));
  expect(marker.title).toBe("built from three of four quarters");
  expect(
    within(company).getByText(
      "approximately 1.00%; built from three of four quarters",
    ),
  ).toBeTruthy();
  const fund = within(table).getByRole("group", {
    name: "Evidence for Company 2",
  });
  for (const entry of within(fund).getAllByText("n/a")) {
    expect(getComputedStyle(entry).fontStyle).toBe("italic");
    expect(getComputedStyle(entry).color).toBe(tokenColour("--text-secondary"));
  }
});

it("keeps evidence within a 640px panel while its columns scroll sideways", async () => {
  await page.viewport(800, 1138);
  render(
    <MemoryRouter>
      <div style={{ width: 640 }}>
        <OpportunitiesPage />
      </div>
    </MemoryRouter>,
  );
  const table = screen.getByRole("table", { name: "Opportunities" });
  const wrapper = table.closest<HTMLDivElement>(".table-wrap");
  if (!wrapper) throw new Error("Missing table scroll container");
  expect(wrapper.scrollWidth).toBeGreaterThan(wrapper.clientWidth);
  const groups = within(table).getAllByRole("group", { name: /Evidence for/ });
  for (const scrollLeft of [0, wrapper.scrollWidth / 2, wrapper.scrollWidth]) {
    wrapper.scrollLeft = scrollLeft;
    const visibleLeft =
      wrapper.getBoundingClientRect().left + wrapper.clientLeft;
    const visibleRight = visibleLeft + wrapper.clientWidth;
    for (const group of groups) {
      expect(group.getBoundingClientRect().right).toBeLessThanOrEqual(
        visibleRight + 1,
      );
      expect(group.getBoundingClientRect().left).toBeGreaterThanOrEqual(
        visibleLeft - 1,
      );
      expect(group.getBoundingClientRect().width).toBeGreaterThan(0);
    }
  }
});
