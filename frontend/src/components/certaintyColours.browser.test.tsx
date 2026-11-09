import "../styles.css";
import { cleanup, render, screen, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type {
  Figure,
  Holding,
  HoldingsFundamentals,
  ReportDate,
} from "../api/types";
import { reportDate } from "../test/companyFiguresFixtures";
import { instrumentFundamentals } from "../test/instrumentFundamentalsFixture";
import { CompanyFiguresPanel } from "./CompanyFiguresPanel";
import { HoldingsTable } from "./HoldingsTable";

// Guards the rendered certainty tones against the real stylesheet cascade. A
// general number colour once overrode them while every jsdom test passed.

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

function colourOf(element: Element): string {
  return getComputedStyle(element).color;
}

/** The visible value that follows a figure's screen-reader label. */
function visibleValue(scope: HTMLElement, label: string): Element {
  const srLabel = within(scope).getByText(
    (_, element) =>
      element?.classList.contains("sr-only") === true &&
      element.textContent?.startsWith(`${label}: `) === true,
  );
  const value = srLabel.nextElementSibling;
  if (!value) throw new Error(`No visible value follows ${label}`);
  return value;
}

function figure(certainty: Figure["certainty"], value: string | null): Figure {
  return {
    certainty,
    value,
    reasons: certainty === "approximate" ? [{ code: "quarter_missing" }] : [],
  };
}

function scheduled(
  certainty: ReportDate["certainty"],
  basis: ReportDate["basis"],
  date: string,
): ReportDate {
  return reportDate({ certainty, basis, date, days_until: 20, reasons: [] });
}

function holding(
  id: number,
  name: string,
  fundamentals: HoldingsFundamentals,
): Holding {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: {
      id,
      symbol: name.toUpperCase(),
      ticker: name.toUpperCase(),
      exchange: "NYSE",
      name,
      type: "Stock",
      currency: "USD",
      conviction: "Other",
    },
    quantity: 1,
    cost_basis_native: "100.00",
    average_cost_native: "100.00",
    base: {
      status: "available",
      cost_basis_base: "100.00",
      average_cost_base: "100.00",
      fee_component_base: "0.00",
    },
    valuation: {
      market_value_base: { status: "available", value: `${id}00.00` },
      unrealized_gain_base: money,
      unrealized_gain_percent: money,
      day_change_base: money,
    },
    conviction_target: {
      conviction: "Other",
      status: "no_target",
      target_value_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_percent: { status: "unavailable", reasons: ["no_target"] },
    },
    row_kind: "open",
    fundamentals,
  };
}

/** Mounts content inside the same page containers the app uses. */
function inApp(page: ReactNode) {
  return render(
    <MemoryRouter>
      <div className="app-shell">
        <main className="workspace">{page}</main>
      </div>
    </MemoryRouter>,
  );
}

function rowFor(name: string): HTMLElement {
  const row = screen.getByRole("link", { name }).closest("tr");
  if (!row) throw new Error(`No row for ${name}`);
  return row;
}

describe("certainty colours", () => {
  it("keeps Holdings figure tones and muted report labels", () => {
    inApp(
      <section className="board-grid single">
        <article className="panel ledger-panel">
          <HoldingsTable
            holdings={[
              holding(1, "Complete", {
                pe: figure("complete", "11.111"),
                ps: figure("approximate", "22.222"),
                next_report: scheduled(
                  "confirmed",
                  "company_announced",
                  "2026-12-01",
                ),
              }),
              holding(2, "Missing", {
                pe: figure("unavailable", null),
                ps: figure("not_applicable", null),
                next_report: scheduled(
                  "estimated",
                  "nasdaq_expected",
                  "2026-12-02",
                ),
              }),
              holding(3, "Estimated", {
                pe: figure("approximate", "33.333"),
                ps: figure("complete", "44.444"),
                next_report: scheduled(
                  "estimated",
                  "typical_gap",
                  "2026-12-03",
                ),
              }),
            ]}
            filter=""
            onFilterChange={() => undefined}
          />
        </article>
      </section>,
    );
    const primary = tokenColour("--text-primary");
    const warning = tokenColour("--warning");
    const muted = tokenColour("--text-muted");
    expect(new Set([primary, warning, muted]).size).toBe(3);

    const complete = rowFor("Complete");
    const missing = rowFor("Missing");
    const estimated = rowFor("Estimated");

    expect(colourOf(visibleValue(complete, "P/E"))).toBe(primary);
    expect(colourOf(visibleValue(complete, "P/S"))).toBe(warning);
    expect(colourOf(visibleValue(estimated, "P/E"))).toBe(warning);
    expect(colourOf(visibleValue(estimated, "P/S"))).toBe(primary);

    const dash = visibleValue(missing, "P/E");
    expect(dash.textContent).toBe("—");
    expect(colourOf(dash)).toBe(muted);
    const notApplicable = visibleValue(missing, "P/S");
    expect(notApplicable.textContent).toBe("n/a");
    expect(colourOf(notApplicable)).toBe(muted);
    expect(getComputedStyle(notApplicable).fontStyle).toBe("italic");

    for (const [row, date, tag] of [
      [complete, "2026-12-01", "confirmed"],
      [missing, "2026-12-02", "Nasdaq expected"],
      [estimated, "2026-12-03", "est."],
    ] as const) {
      expect(colourOf(within(row).getByText(date))).toBe(primary);
      expect(colourOf(within(row).getByText(tag))).toBe(muted);
    }
  });

  it("keeps stock-page figure tones and muted report labels", () => {
    const data = instrumentFundamentals();
    data.pe = figure("approximate", "12.345");
    data.ps = figure("unavailable", null);
    data.profit_margin = figure("not_applicable", null);
    data.sales_growth = figure("complete", "0.25");
    data.next_report = scheduled("estimated", "nasdaq_expected", "2026-12-02");
    inApp(
      <article className="asset-page">
        <CompanyFiguresPanel data={data} />
      </article>,
    );
    const panel = screen.getByRole("region", {
      name: "How the company is doing",
    });
    const primary = tokenColour("--text-primary");
    const muted = tokenColour("--text-muted");

    expect(colourOf(visibleValue(panel, "P/E"))).toBe(tokenColour("--warning"));
    expect(colourOf(visibleValue(panel, "P/S"))).toBe(muted);
    const notApplicable = visibleValue(panel, "Profit margin");
    expect(colourOf(notApplicable)).toBe(muted);
    expect(getComputedStyle(notApplicable).fontStyle).toBe("italic");
    expect(colourOf(visibleValue(panel, "Sales growth"))).toBe(primary);
    expect(colourOf(within(panel).getByText("2026-12-02"))).toBe(primary);
    expect(colourOf(within(panel).getByText("Nasdaq expected"))).toBe(muted);
  });
});
