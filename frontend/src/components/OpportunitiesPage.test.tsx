// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  OpportunitiesResponse,
  OpportunityRow,
  OpportunityWindow,
} from "../api/types";
import { opportunityEvidence } from "../test/opportunityFixtures";
import { OpportunitiesPage } from "./OpportunitiesPage";

const useOpportunities = vi.fn();
const useAppMode = vi.fn();

vi.mock("../api/queries", () => ({
  useOpportunities: (...args: unknown[]) => useOpportunities(...args),
}));

vi.mock("./useAppMode", () => ({
  useAppMode: (...args: unknown[]) => useAppMode(...args),
}));

function opportunityRow(
  id: number,
  name: string,
  change: string | string[],
): OpportunityRow {
  return {
    evidence: null,
    instrument: {
      id,
      symbol: name.slice(0, 3).toUpperCase(),
      ticker: name.slice(0, 3).toUpperCase(),
      exchange: "STO",
      name,
      type: "Stock",
      currency: "SEK",
      conviction: "High",
    },
    quantity: 10,
    latest_close: {
      date: "2026-10-15",
      close: "100",
      currency: "SEK",
      source: "YAHOO",
      freshness: "fresh",
    },
    price_change: Array.isArray(change)
      ? { status: "unavailable", reasons: change }
      : {
          status: "available",
          start_date: "2026-10-08",
          start_close: "110",
          end_date: "2026-10-15",
          end_close: "100",
          change_percent: change,
        },
    average_cost_native: "100.00",
    price_vs_average_percent: { status: "available", value: "0.00" },
    below_average: false,
    unrealized_gain_percent: { status: "available", value: "0.00" },
    unrealized_gain_freshness: "fresh",
    conviction_target: {
      conviction: "High",
      status: "no_target",
      target_value_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_base: { status: "unavailable", reasons: ["no_target"] },
      target_gap_percent: { status: "unavailable", reasons: ["no_target"] },
    },
  };
}

const defaultRows = [
  opportunityRow(1, "Gap Holdings", ["start_close_too_old"]),
  opportunityRow(2, "Riser Corp", "5.00"),
  opportunityRow(3, "Faller Inc", "-8.41"),
];

function response(
  window: OpportunityWindow,
  rows: OpportunityRow[] = defaultRows,
): OpportunitiesResponse {
  return {
    data_revision: "test",
    period: {
      window,
      start_date: window === "30d" ? "2026-09-15" : "2026-10-08",
      end_date: "2026-10-15",
    },
    rows,
  };
}

function queryResult(data: OpportunitiesResponse | undefined) {
  return {
    data,
    isPending: data === undefined,
    isFetching: false,
    isError: false,
    error: null,
    refetch: vi.fn(),
  };
}

function renderPage() {
  render(
    <MemoryRouter>
      <OpportunitiesPage />
    </MemoryRouter>,
  );
}

function dataRows() {
  const table = screen.getByRole("table", { name: "Opportunities" });
  return within(table).getAllByRole("row").slice(1);
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  localStorage.clear();
});

describe("OpportunitiesPage", () => {
  it("switches the window and requests that window's data", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    useOpportunities.mockImplementation((window: OpportunityWindow) =>
      queryResult(response(window)),
    );

    renderPage();

    expect(screen.getByText("RIS")).toBeTruthy();
    expect(screen.queryByText("STO")).toBeNull();

    const week = screen.getByRole("button", { name: "1 week" });
    const month = screen.getByRole("button", { name: "30 days" });
    expect(week).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "2 weeks" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(month).toHaveAttribute("aria-pressed", "false");
    expect(useOpportunities).toHaveBeenLastCalledWith("7d");
    expect(
      screen.getByRole("columnheader", { name: "1-week price change" }),
    ).toBeTruthy();
    expect(screen.getByText("Closes from 8 Oct to 15 Oct")).toBeTruthy();

    fireEvent.click(month);

    expect(useOpportunities).toHaveBeenLastCalledWith("30d");
    expect(month).toHaveAttribute("aria-pressed", "true");
    expect(week).toHaveAttribute("aria-pressed", "false");
    expect(
      screen.getByRole("columnheader", { name: "30-day price change" }),
    ).toBeTruthy();
    expect(screen.queryByText("Updating…")).toBeNull();
  });

  it("reorders rows from the order switch and keeps unavailable rows last", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    useOpportunities.mockImplementation((window: OpportunityWindow) =>
      queryResult(response(window)),
    );

    renderPage();

    expect(
      screen.getByRole("button", { name: "Best candidates first" }),
    ).toHaveAttribute("aria-pressed", "true");
    const fallers = screen.getByRole("button", { name: "Biggest fallers" });
    expect(fallers).toHaveAttribute("aria-pressed", "false");
    expect(
      screen.getByRole("button", { name: "Biggest risers" }),
    ).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(fallers);
    expect(fallers).toHaveAttribute("aria-pressed", "true");
    let rows = dataRows();
    expect(within(rows[0]).getByText("Faller Inc")).toBeTruthy();
    expect(within(rows[2]).getByText("Gap Holdings")).toBeTruthy();
    expect(within(rows[2]).getByText("No price near 8 Oct")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Biggest risers" }));

    rows = dataRows();
    expect(within(rows[0]).getByText("Riser Corp")).toBeTruthy();
    expect(within(rows[0]).getByText("+5.00%")).toBeTruthy();
    expect(within(rows[2]).getByText("Gap Holdings")).toBeTruthy();
  });

  it("stops naming the period while the previous window's rows are shown", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    useOpportunities.mockReturnValue(queryResult(response("7d")));

    renderPage();
    fireEvent.click(screen.getByRole("button", { name: "30 days" }));

    expect(useOpportunities).toHaveBeenLastCalledWith("30d");
    expect(
      screen.getByRole("columnheader", { name: "Price change" }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("columnheader", { name: "1-week price change" }),
    ).toBeNull();
    expect(screen.getByText("Updating…")).toBeTruthy();
    expect(screen.queryByText(/Closes from/)).toBeNull();
    expect(dataRows()).toHaveLength(3);
  });

  it("restores the persisted window and order", () => {
    localStorage.setItem("opportunities.window", JSON.stringify("14d"));
    localStorage.setItem(
      "opportunities.ordering",
      JSON.stringify("risers_first"),
    );
    useAppMode.mockReturnValue({ canMutate: true });
    useOpportunities.mockImplementation((window: OpportunityWindow) =>
      queryResult(response(window)),
    );

    renderPage();

    expect(useOpportunities).toHaveBeenLastCalledWith("14d");
    expect(screen.getByRole("button", { name: "2 weeks" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "Biggest risers" }),
    ).toHaveAttribute("aria-pressed", "true");
  });

  it("shows a stale latest price signal", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    const stale = opportunityRow(4, "Stale Group", [
      "no_close_since_period_start",
    ]);
    stale.latest_close = {
      date: "2026-10-01",
      close: "100",
      currency: "SEK",
      source: "YAHOO",
      freshness: "warning_stale_10_days",
    };
    useOpportunities.mockReturnValue(queryResult(response("7d", [stale])));

    renderPage();

    const [row] = dataRows();
    expect(within(row).getByText("SEK 100")).toBeTruthy();
    expect(within(row).getByText("Stale 10 days")).toBeTruthy();
    expect(within(row).getByText("No price since 8 Oct")).toBeTruthy();
  });

  it("offers a retry when loading fails", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    const refetch = vi.fn();
    useOpportunities.mockReturnValue({
      data: undefined,
      isPending: false,
      isFetching: false,
      isError: true,
      error: new Error("Server unavailable"),
      refetch,
    });

    renderPage();

    expect(screen.getByText("Server unavailable")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetch).toHaveBeenCalledOnce();
  });

  it.each([true, false])(
    "offers ledger entry for an empty ledger only when mutable (%s)",
    (canMutate) => {
      useAppMode.mockReturnValue({ canMutate });
      useOpportunities.mockReturnValue(queryResult(response("7d", [])));

      renderPage();

      expect(screen.getByText(/No holdings yet\./)).toBeTruthy();
      if (canMutate) {
        expect(
          screen.getByRole("link", { name: "Transactions page" }),
        ).toHaveAttribute("href", "/transactions");
        expect(
          screen.getByRole("link", {
            name: "import from Avanza or Sharesight",
          }),
        ).toHaveAttribute("href", "/import");
      } else {
        expect(screen.queryByRole("link")).toBeNull();
        expect(screen.queryByText(/Add one/)).toBeNull();
      }
    },
  );

  it("shows the context columns, markers, stale signals and footnote", () => {
    useAppMode.mockReturnValue({ canMutate: true });
    const below: OpportunityRow = {
      ...opportunityRow(5, "Below Ltd", "-3.00"),
      price_vs_average_percent: { status: "available", value: "-5.12" },
      below_average: true,
      unrealized_gain_percent: { status: "available", value: "20.00" },
      unrealized_gain_freshness: "warning_stale_8_days",
      conviction_target: {
        conviction: "High",
        status: "above",
        target_value_base: { status: "available", value: "900.00" },
        target_gap_base: { status: "available", value: "300.00" },
        target_gap_percent: { status: "available", value: "33.33" },
      },
    };
    const above: OpportunityRow = {
      ...opportunityRow(6, "Above Plc", "2.00"),
      price_vs_average_percent: { status: "available", value: "4.00" },
      below_average: false,
    };
    useOpportunities.mockReturnValue(
      queryResult(response("7d", [below, above])),
    );

    renderPage();

    const table = screen.getByRole("table", { name: "Opportunities" });
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((header) => header.textContent);
    const column = (name: string) => headers.indexOf(name);
    expect(column("Vs your average price")).toBeGreaterThan(-1);
    expect(column("Your gain incl. fees & currency")).toBeGreaterThan(-1);
    expect(column("Vs target")).toBeGreaterThan(-1);

    const [belowRow, aboveRow] = dataRows();
    const cell = (row: HTMLElement, name: string) =>
      within(row).getAllByRole("cell")[column(name)];

    expect(within(belowRow).getByText("Below your average")).toBeTruthy();
    expect(within(aboveRow).queryByText("Below your average")).toBeNull();

    // A fresh share price with an old exchange rate: the SEK gain carries the
    // stale chip, the latest price does not.
    const gainCell = cell(belowRow, "Your gain incl. fees & currency");
    expect(within(gainCell).getByText("+20.00%")).toBeTruthy();
    expect(within(gainCell).getByText("Stale 8 days")).toBeTruthy();
    expect(
      within(cell(belowRow, "Latest price")).queryByText(/Stale/),
    ).toBeNull();

    expect(
      within(cell(belowRow, "Vs target")).getByRole("img", {
        name: /Gap SEK 300\.00 \(33\.33%\)/,
      }),
    ).toBeTruthy();
    expect(cell(aboveRow, "Vs target")).toBeEmptyDOMElement();

    expect(
      screen.getByText(
        "Price change compares share prices in each asset's own currency, without dividends added back. Ex-dividend days are detected where the price source reports dividends. During market hours the latest price can be intraday, so fell and stopped falling can change before the close.",
      ),
    ).toBeTruthy();
  });
});

const verdictNote =
  "A verdict describes the dip: how unusual the fall is and what is known about the company. It does not predict the price. A replay of stored price history found no measurable difference in the following month between dips the price checks flagged and other dips; the company checks could not be replayed.";

describe("Opportunities verdict presentation", () => {
  it("exposes all evidence in the table only for a verdict row, keeping real row counts and sorting", () => {
    useAppMode.mockReturnValue({ canMutate: false });
    const candidate = opportunityRow(10, "Candidate Company", "-8.00");
    candidate.evidence = opportunityEvidence();
    const riser = opportunityRow(11, "Rising Company", "2.00");
    useOpportunities.mockReturnValue(
      queryResult(response("7d", [riser, candidate])),
    );
    renderPage();
    const table = screen.getByRole("table", { name: "Opportunities" });
    const line = within(table).getByRole("group", {
      name: "Evidence for Candidate Company",
    });
    for (const label of [
      "Unusual drop:",
      "Off its high:",
      "Report in the period:",
      "Report coming soon:",
      "Business:",
      "Market:",
      "Stopped falling:",
      "Ex-dividend:",
      "Room to buy:",
    ]) {
      expect(within(line).getByText(label)).toBeTruthy();
    }
    expect(line).toHaveTextContent(
      "unknown; the dividend range is not fully covered",
    );
    expect(within(line).getByText("below your target")).toBeTruthy();
    expect(
      within(table).queryByRole("group", {
        name: "Evidence for Rising Company",
      }),
    ).toBeNull();
    expect(dataRows()).toHaveLength(2);
    expect(
      within(dataRows()[0]).getByRole("cell", { name: /Candidate Company/ }),
    ).toHaveAttribute("aria-describedby", line.id);
    const market = within(line).getByText("Market:").parentElement;
    expect(market).toHaveAttribute(
      "title",
      expect.stringContaining("8 other holdings; comparison margin 3.00 pts"),
    );
    expect(market).toHaveAttribute("aria-description", market?.title);
    expect(
      within(dataRows()[0]).getByRole("link", { name: "Candidate Company" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Biggest risers" }));
    expect(dataRows()).toHaveLength(2);
    expect(
      within(dataRows()[0]).getByRole("link", { name: "Rising Company" }),
    ).toBeTruthy();
  });

  it.each([
    "sentiment_dip_candidate",
    "possible_candidate_report_timing_unknown",
    "not_enough_data",
    "fell_for_a_reason",
    "normal_movement",
  ] as const)("shows the honest note when a %s verdict is shown", (code) => {
    useAppMode.mockReturnValue({ canMutate: false });
    const row = opportunityRow(10, "Verdict Company", "-8.00");
    row.evidence = opportunityEvidence(code);
    useOpportunities.mockReturnValue(queryResult(response("7d", [row])));
    renderPage();
    expect(screen.getByRole("columnheader", { name: "Verdict" })).toBeTruthy();
    expect(screen.getByText(verdictNote)).toBeTruthy();
  });

  it("leaves the verdict empty and omits its note when no evidence is supplied", () => {
    useAppMode.mockReturnValue({ canMutate: false });
    useOpportunities.mockReturnValue(queryResult(response("7d")));
    renderPage();
    expect(screen.queryByText(verdictNote)).toBeNull();
    expect(screen.queryByText("Sentiment-dip candidate")).toBeNull();
    expect(screen.queryByText("Normal movement")).toBeNull();
  });

  it("renders the explanation and caution labels supplied by the view-model", () => {
    useAppMode.mockReturnValue({ canMutate: false });
    const row = opportunityRow(10, "Candidate Company", "-8.00");
    row.evidence = opportunityEvidence();
    row.evidence.verdict.cautions = ["loss_making", "no_target"];
    row.evidence.checks.business.state = "loss_making";
    useOpportunities.mockReturnValue(queryResult(response("7d", [row])));
    renderPage();
    expect(screen.getByText("Sentiment-dip candidate")).toBeTruthy();
    expect(
      screen.getByText(
        "Unusual drop, no report in the period, sales and earnings holding up",
      ),
    ).toBeTruthy();
    expect(screen.getByText("Loss-making")).toBeTruthy();
    expect(screen.getByText("No target")).toBeTruthy();
  });
});
