// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import type { Holding, Instrument } from "../api/types";
import { holdingsFundamentals } from "../test/companyFiguresFixtures";
import { HoldingsTable } from "./HoldingsTable";

const HOLDINGS_SORTING_KEY = "holdings.sorting";

function seedSorting(sorting: unknown): void {
  localStorage.setItem(HOLDINGS_SORTING_KEY, JSON.stringify(sorting));
}

function instrument(id: number, name: string, symbol: string): Instrument {
  return {
    id,
    symbol,
    ticker: symbol,
    exchange: "NYSE",
    name,
    type: "Stock",
    currency: "USD",
    conviction: "Other",
  };
}

function holding(
  id: number,
  name: string,
  symbol: string,
  marketValueBase: string,
): Holding {
  const money = { status: "available", value: "0.00" } as const;
  return {
    instrument: instrument(id, name, symbol),
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
      market_value_base: { status: "available", value: marketValueBase },
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
    fundamentals: holdingsFundamentals(),
  };
}

function renderHoldingsTable(holdings: Holding[]) {
  return render(
    <MemoryRouter>
      <HoldingsTable
        holdings={holdings}
        filter=""
        onFilterChange={() => undefined}
      />
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("holdings sorting persistence", () => {
  it("falls back to value descending when the stored sort names conviction", () => {
    seedSorting([{ id: "conviction", desc: false }]);
    renderHoldingsTable([
      holding(1, "Small", "SML", "100"),
      holding(2, "Large", "LRG", "200"),
    ]);
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Large", "Small"],
    );
  });
  it("applies saved sorting when the table mounts", () => {
    seedSorting([{ id: "instrument", desc: false }]);

    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
      holding(3, "Metro Ltd", "METRO", "200.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Alpha Corp", "Metro Ltd", "Zulu Inc"],
    );
  });

  it("sorts instrument names case-insensitively in natural order", () => {
    seedSorting([{ id: "instrument", desc: false }]);

    renderHoldingsTable([
      holding(1, "ABB", "ABB", "100.00"),
      holding(2, "Investor B", "INVESTOR-B", "100.00"),
      holding(3, "Volvo B", "VOLVO-B", "100.00"),
      holding(4, "iShares Core", "ISHARES", "100.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["ABB", "Investor B", "iShares Core", "Volvo B"],
    );
  });

  it("ignores a stored retired column id and falls back to value descending", () => {
    seedSorting([{ id: "target_gap_base", desc: true }]);

    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
      holding(3, "Metro Ltd", "METRO", "200.00"),
    ]);

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Zulu Inc", "Metro Ltd", "Alpha Corp"],
    );
  });

  it("saves sorting when the user changes the table sort", () => {
    renderHoldingsTable([
      holding(1, "Zulu Inc", "ZULU", "300.00"),
      holding(2, "Alpha Corp", "ALPHA", "100.00"),
    ]);

    fireEvent.click(screen.getByRole("button", { name: "Instrument" }));

    expect(
      JSON.parse(localStorage.getItem(HOLDINGS_SORTING_KEY) ?? "null"),
    ).toEqual([{ id: "instrument", desc: false }]);
  });

  it("sorts Cost by total purchase cost instead of average unit cost", () => {
    const largeTotal: Holding = {
      ...holding(1, "Large Total", "LARGE", "1000.00"),
      average_cost_native: "10.00",
      cost_basis_native: "1000.00",
    };
    const smallTotal: Holding = {
      ...holding(2, "Small Total", "SMALL", "200.00"),
      average_cost_native: "200.00",
      cost_basis_native: "200.00",
    };

    renderHoldingsTable([largeTotal, smallTotal]);

    fireEvent.click(screen.getByRole("button", { name: "Cost" }));

    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Large Total", "Small Total"],
    );
  });
});

describe("holdings company figures", () => {
  it("shows the display ticker without the exchange or private instrument symbol", () => {
    const held = holding(1, "Evolution", "SE0012673267", "100");
    held.instrument.ticker = "EVO";
    held.instrument.exchange = "AVANZA";
    renderHoldingsTable([held]);
    expect(screen.getByRole("link", { name: "Evolution" })).toHaveAttribute(
      "href",
      "/asset/1",
    );
    expect(screen.getByText("EVO")).toBeTruthy();
    expect(screen.queryByText("AVANZA")).toBeNull();
    expect(screen.queryByText("SE0012673267")).toBeNull();
    expect(
      screen.getByRole("combobox", { name: "Conviction for Evolution" }),
    ).toBeTruthy();
  });
  it.each([
    ["  Evolution  ", "EVO", "Evolution"],
    ["   ", "EVO", "EVO"],
    ["   ", null, "Unnamed instrument"],
  ])(
    "names the instrument link and conviction control for %s / %s",
    (name, ticker, label) => {
      const held = holding(1, name, "SE0012673267", "100");
      held.instrument.ticker = ticker;
      renderHoldingsTable([held]);
      expect(screen.getByRole("link", { name: label })).toHaveTextContent(
        label,
      );
      expect(
        screen.getByRole("combobox", { name: `Conviction for ${label}` }),
      ).toBeTruthy();
      expect(screen.queryByText("SE0012673267")).toBeNull();
    },
  );
  it("shows only the name when a ticker is unavailable and can filter on a ticker", () => {
    const held = holding(1, "Evolution", "SE0012673267", "100");
    held.instrument.ticker = null;
    const view = renderHoldingsTable([held]);
    expect(screen.getByRole("link", { name: "Evolution" })).toBeTruthy();
    expect(screen.queryByText("SE0012673267")).toBeNull();
    held.instrument.ticker = "EVO";
    view.rerender(
      <MemoryRouter>
        <HoldingsTable
          holdings={[held]}
          filter="EVO"
          onFilterChange={() => undefined}
        />
      </MemoryRouter>,
    );
    expect(screen.getByRole("link", { name: "Evolution" })).toBeTruthy();
  });
  it("exposes approximation and distinguishes n/a from unavailable by visible text", () => {
    const approximate = holding(1, "Approximate", "APP", "300");
    approximate.fundamentals.pe = {
      certainty: "approximate",
      value: "24.34",
      reasons: [{ code: "ttm_from_three_quarters" }],
    };
    const fund = holding(2, "Fund", "ETF", "200");
    fund.fundamentals.pe = {
      certainty: "not_applicable",
      value: null,
      reasons: [{ code: "fund_or_etf" }],
    };
    const missing = holding(3, "Missing", "MISS", "100");
    renderHoldingsTable([approximate, fund, missing]);
    expect(screen.getByText("~24.3")).toBeTruthy();
    expect(
      screen.getByText(
        "P/E: approximately 24.3; Built from three of four quarters",
      ),
    ).toBeTruthy();
    expect(
      screen.getByTitle("Built from three of four quarters"),
    ).toHaveTextContent("~24.3");
    expect(screen.getByText("n/a")).toBeTruthy();
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
    expect(
      screen.getAllByTitle("No company figures source found").length,
    ).toBeGreaterThan(0);
  });
  it.each(["valuation", "report"])(
    "keeps missing rows last in both sort directions for %s",
    (column) => {
      const missing = holding(1, "Missing", "MISS", "500");
      const first = holding(2, "First", "ONE", "200");
      const second = holding(3, "Second", "TWO", "300");
      const na = holding(4, "Not applicable", "NA", "400");
      first.fundamentals.pe = {
        certainty: "approximate",
        value: "9.5",
        reasons: [{ code: "price_stale" }],
      };
      second.fundamentals.pe = {
        certainty: "complete",
        value: "100",
        reasons: [],
      };
      na.fundamentals.pe = {
        certainty: "not_applicable",
        value: null,
        reasons: [{ code: "fund_or_etf" }],
      };
      first.fundamentals.next_report = {
        ...first.fundamentals.next_report,
        certainty: "estimated",
        date: "2026-11-01",
        basis: "nasdaq_expected",
        days_until: -1,
        reasons: [],
      };
      second.fundamentals.next_report = {
        ...second.fundamentals.next_report,
        certainty: "confirmed",
        date: "2026-11-10",
        basis: "company_announced",
        days_until: 8,
        reasons: [],
      };
      seedSorting([{ id: column, desc: false }]);
      renderHoldingsTable([missing, first, second, na]);
      expect(
        screen.getAllByRole("link").map((link) => link.textContent),
      ).toEqual(["First", "Second", "Missing", "Not applicable"]);
      fireEvent.click(
        screen.getByRole("button", {
          name: column === "valuation" ? "P/E·P/S" : "Next report",
        }),
      );
      expect(
        screen.getAllByRole("link").map((link) => link.textContent),
      ).toEqual(["Second", "First", "Missing", "Not applicable"]);
      if (column === "report") {
        expect(screen.getByText("Nasdaq expected")).toBeTruthy();
        expect(screen.getByText("confirmed")).toBeTruthy();
        expect(screen.getByText("1 day ago")).toBeTruthy();
      }
    },
  );
  it("keeps the target column sorted by signed gap and the filter matches conviction", () => {
    const below = holding(1, "Below", "BLW", "100");
    const above = holding(2, "Above", "ABV", "200");
    for (const [held, gap, conviction] of [
      [below, "-20", "High"],
      [above, "20", "Low"],
    ] as const) {
      held.instrument.conviction = conviction;
      held.conviction_target = {
        conviction,
        status: gap === "-20" ? "below" : "above",
        target_value_base: { status: "available", value: "100" },
        target_gap_base: { status: "available", value: gap },
        target_gap_percent: { status: "available", value: gap },
      };
    }
    seedSorting([{ id: "target", desc: false }]);
    const view = renderHoldingsTable([above, below]);
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Below", "Above"],
    );
    const row = screen.getByRole("row", { name: /Below/ });
    expect(within(row).getByRole("img", { name: /Gap SEK -20/ })).toBeTruthy();
    expect(
      within(row).getByRole("combobox", { name: "Conviction for Below" }),
    ).toBeTruthy();
    view.rerender(
      <MemoryRouter>
        <HoldingsTable
          holdings={[above, below]}
          filter="High"
          onFilterChange={() => undefined}
        />
      </MemoryRouter>,
    );
    expect(screen.getAllByRole("link").map((link) => link.textContent)).toEqual(
      ["Below"],
    );
  });
});

describe("holdings consolidated columns", () => {
  it("renders the eight consolidated column headers", () => {
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "100.00")]);

    for (const name of [
      "Instrument",
      "Qty",
      "Cost",
      "Value (SEK)",
      "P&L (SEK)",
      "Target & conviction",
      "P/E·P/S",
      "Next report",
    ]) {
      expect(screen.getByRole("button", { name })).toBeTruthy();
    }

    for (const gone of [
      "Avg cost/share",
      "Cost basis",
      "Portfolio %",
      "P&L hint",
      "Target (SEK)",
      "Target status",
    ]) {
      expect(screen.queryByRole("button", { name: gone })).toBeNull();
    }
  });

  it("explains every consolidated column with an accessible tooltip", () => {
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "100.00")]);

    const descriptions = new Map([
      ["Instrument", "The instrument name and display ticker."],
      ["Qty", "The number of shares or units currently held."],
      [
        "Cost",
        "Average purchase price per unit (top) and total purchase cost (bottom), both in the instrument's trading currency.",
      ],
      [
        "Value (SEK)",
        "Current market value in SEK (top) and the holding's share of the portfolio (bottom).",
      ],
      [
        "P&L (SEK)",
        "Unrealized profit or loss in SEK: current market value minus cost basis, including brokerage fees and currency effects (top), with the return on cost basis below.",
      ],
      [
        "Target & conviction",
        "Current value minus the conviction target, as a percentage of the target. Green/right is above target; red/left is below target. Sorts by signed gap percent. Low, Medium, and High use 1x, 2x, and 4x target weights; Other has no target. The target value in SEK appears beneath the selector.",
      ],
      [
        "P/E·P/S",
        "Price to trailing earnings (top) and sales (bottom), shown to one decimal. Sorts by P/E. ~ means approximate, with the reason on hover; n/a means not applicable; — means unavailable, with the reason on hover. Missing values sort last.",
      ],
      [
        "Next report",
        "Next report date and days away, sorted soonest first. confirmed means company-announced; Nasdaq expected means Nasdaq's estimate; est. means the app's estimate. n/a means not applicable; — means unavailable, with the reason on hover. Dates without a day count sort last.",
      ],
    ]);

    for (const [name, description] of descriptions) {
      const header = screen.getByRole("button", { name });
      expect(header).toHaveAttribute("title", description);
      expect(header).toHaveAccessibleDescription(description);
    }
  });

  it("still renders regrouped sub-content (portfolio % and target details)", () => {
    const baseHolding = holding(1, "Alpha Corp", "ALPHA", "1000.00");
    const held: Holding = {
      ...baseHolding,
      valuation: {
        market_value_base: { status: "available", value: "1000.00" },
        unrealized_gain_base: { status: "available", value: "123.45" },
        unrealized_gain_percent: { status: "available", value: "14.08" },
        day_change_base: { status: "available", value: "0.00" },
      },
      conviction_target: {
        conviction: "High",
        status: "below",
        target_value_base: { status: "available", value: "2000.00" },
        target_gap_base: { status: "available", value: "-1000.00" },
        target_gap_percent: { status: "available", value: "-50.0" },
      },
    };

    renderHoldingsTable([held]);

    // Portfolio-% sub-line: a single valued holding is 100% of the portfolio.
    expect(screen.getAllByText("100.0%").length).toBeGreaterThan(0);
    // Target value moved under the conviction selector.
    expect(screen.getByText("2,000.00")).toBeTruthy();
    const holdingRow = screen
      .getByRole("link", { name: "Alpha Corp" })
      .closest("tr");
    expect(holdingRow).toHaveTextContent("123.45");
    // SEK belongs to the two headers, not to each row's P&L/target values.
    expect(holdingRow).not.toHaveTextContent("SEK");
    // The gap bar carries the full detail in its accessible label / tooltip.
    expect(
      screen.getByRole("img", {
        name: "Target SEK 2,000.00\nGap SEK -1,000.00 (-50.0%)\nBelow",
      }),
    ).toBeTruthy();
  });

  it("keeps the average-cost currency and amount on one line", () => {
    const held: Holding = {
      ...holding(1, "Alpha Corp", "ALPHA", "1000.00"),
      average_cost_native: "12.34",
      cost_basis_native: "1234.00",
    };

    renderHoldingsTable([held]);

    const holdingRow = screen
      .getByRole("link", { name: "Alpha Corp" })
      .closest("tr");
    const averageCostCurrency = holdingRow?.querySelector(
      "td:nth-child(3) .metric-stack > .number .number-prefix",
    );
    expect(averageCostCurrency?.parentElement).toHaveTextContent("USD12.34");
  });

  it("leaves the target gap cell empty when no target exists", () => {
    // The holding factory has a no_target conviction target.
    renderHoldingsTable([holding(1, "Alpha Corp", "ALPHA", "1000.00")]);

    expect(screen.queryByRole("img")).toBeNull();
  });
});
