import { describe, expect, it } from "vitest";
import type {
  OpportunitiesResponse,
  OpportunityRow,
  OpportunityWindow,
  PriceChange,
} from "../api/types";
import {
  buildOpportunitiesViewModel,
  priceChangeReasonLabel,
  sortOpportunityRows,
} from "./opportunitiesViewModel";

let nextId = 1;

function opportunityRow(
  symbol: string,
  change: string | string[],
  options: { exchange?: string; freshness?: string; latest?: boolean } = {},
): OpportunityRow {
  const id = nextId++;
  const priceChange: PriceChange = Array.isArray(change)
    ? { status: "unavailable", reasons: change }
    : {
        status: "available",
        start_date: "2026-10-07",
        start_close: "412.512345",
        end_date: "2026-10-15",
        end_close: "377.8",
        change_percent: change,
      };
  return {
    instrument: {
      id,
      symbol,
      exchange: options.exchange ?? "STO",
      name: `${symbol} AB`,
      type: "Stock",
      currency: "SEK",
      conviction: "High",
    },
    quantity: 10,
    latest_close:
      options.latest === false
        ? null
        : {
            date: "2026-10-15",
            close: "377.8",
            currency: "SEK",
            source: "YAHOO",
            freshness: options.freshness ?? "fresh",
          },
    price_change: priceChange,
    average_cost_native: "398.20",
    price_vs_average_percent: { status: "available", value: "-5.12" },
    below_average: true,
    unrealized_gain_percent: { status: "available", value: "-6.03" },
    unrealized_gain_freshness: "fresh",
    conviction_target: {
      conviction: "High",
      status: "on_target",
      target_value_base: { status: "available", value: "1000.00" },
      target_gap_base: { status: "available", value: "0.00" },
      target_gap_percent: { status: "available", value: "0.00" },
    },
  };
}

function response(
  rows: OpportunityRow[],
  window: OpportunityWindow = "7d",
): OpportunitiesResponse {
  const startDates: Record<OpportunityWindow, string> = {
    "7d": "2026-10-08",
    "14d": "2026-10-01",
    "30d": "2026-09-15",
  };
  return {
    data_revision: "test",
    period: { window, start_date: startDates[window], end_date: "2026-10-15" },
    rows,
  };
}

function symbols(rows: { instrument: { symbol: string } }[]): string[] {
  return rows.map((row) => row.instrument.symbol);
}

describe("sortOpportunityRows", () => {
  const rows = [
    opportunityRow("NONE", ["no_price_source"]),
    opportunityRow("UP", "4.20"),
    opportunityRow("DOWN", "-8.41"),
    opportunityRow("AAA", ["start_close_too_old"]),
    opportunityRow("FLAT", "0.00"),
  ];

  it("puts the biggest fallers first, unavailable rows last by symbol", () => {
    expect(symbols(sortOpportunityRows(rows, "fallers_first"))).toEqual([
      "DOWN",
      "FLAT",
      "UP",
      "AAA",
      "NONE",
    ]);
  });

  it("puts the biggest risers first, unavailable rows still last by symbol", () => {
    expect(symbols(sortOpportunityRows(rows, "risers_first"))).toEqual([
      "UP",
      "FLAT",
      "DOWN",
      "AAA",
      "NONE",
    ]);
  });

  it("breaks ties by symbol, then exchange", () => {
    const tied = [
      opportunityRow("BBB", "-1.00"),
      opportunityRow("AAA", "-1.00", { exchange: "NYSE" }),
      opportunityRow("AAA", "-1.00", { exchange: "NASDAQ" }),
    ];

    for (const order of ["fallers_first", "risers_first"] as const) {
      expect(
        sortOpportunityRows(tied, order).map(
          (row) => `${row.instrument.symbol}/${row.instrument.exchange}`,
        ),
      ).toEqual(["AAA/NASDAQ", "AAA/NYSE", "BBB/STO"]);
    }
  });

  it("does not reorder its input", () => {
    const input = [opportunityRow("B", "1.00"), opportunityRow("A", "-1.00")];
    sortOpportunityRows(input, "fallers_first");
    expect(symbols(input)).toEqual(["B", "A"]);
  });
});

describe("buildOpportunitiesViewModel", () => {
  it("names the column from the response window when it matches the request", () => {
    for (const [window, label, subtitle] of [
      ["7d", "1-week price change", "Closes from 8 Oct to 15 Oct"],
      ["14d", "2-week price change", "Closes from 1 Oct to 15 Oct"],
      ["30d", "30-day price change", "Closes from 15 Sep to 15 Oct"],
    ] as const) {
      const model = buildOpportunitiesViewModel({
        response: response([opportunityRow("A", "1.00")], window),
        requestedWindow: window,
        order: "fallers_first",
        isError: false,
        errorMessage: null,
      });

      expect(model.status).toBe("available");
      expect(model.changeColumnLabel).toBe(label);
      expect(model.periodSubtitle).toBe(subtitle);
      expect(model.isUpdating).toBe(false);
    }
  });

  it("stops naming a period while the rows belong to another window", () => {
    const model = buildOpportunitiesViewModel({
      response: response([opportunityRow("A", "1.00")], "7d"),
      requestedWindow: "30d",
      order: "fallers_first",
      isError: false,
      errorMessage: null,
    });

    expect(model.status).toBe("available");
    expect(model.rows).toHaveLength(1);
    expect(model.changeColumnLabel).toBe("Price change");
    expect(model.isUpdating).toBe(true);
    expect(model.periodSubtitle).toBeNull();
  });

  it("reports an error when the requested window fails while another window's rows are shown", () => {
    const model = buildOpportunitiesViewModel({
      response: response([opportunityRow("A", "1.00")], "7d"),
      requestedWindow: "30d",
      order: "fallers_first",
      isError: true,
      errorMessage: "Server unavailable",
    });

    expect(model).toMatchObject({
      status: "error",
      message: "Server unavailable",
      rows: [],
      periodSubtitle: null,
    });
  });

  it("labels an available change with sign, tone and the closes used", () => {
    const model = buildOpportunitiesViewModel({
      response: response([
        opportunityRow("DOWN", "-8.41"),
        opportunityRow("UP", "1234.50"),
        opportunityRow("FLAT", "0.00"),
      ]),
      requestedWindow: "7d",
      order: "fallers_first",
      isError: false,
      errorMessage: null,
    });

    const [down, flat, up] = model.rows;
    expect(down).toMatchObject({
      changeAvailable: true,
      changeLabel: "-8.41%",
      changeTone: "down",
      changeTooltip: "From 7 Oct close 412.51 to 15 Oct close 377.8",
    });
    expect(flat).toMatchObject({ changeLabel: "0.00%", changeTone: "flat" });
    expect(up).toMatchObject({ changeLabel: "+1,234.50%", changeTone: "up" });
    expect(down.latestPrice).toEqual({
      label: "SEK 377.8",
      freshnessKind: "fresh",
      freshnessLabel: "Fresh",
    });
  });

  it("renders unavailable reasons as plain language with the start date", () => {
    expect(priceChangeReasonLabel("no_price_source", "2026-10-08")).toBe(
      "No price source",
    );
    expect(priceChangeReasonLabel("missing_latest_close", "2026-10-08")).toBe(
      "No price yet",
    );
    expect(
      priceChangeReasonLabel("no_close_since_period_start", "2026-10-08"),
    ).toBe("No price since 8 Oct");
    expect(priceChangeReasonLabel("missing_start_close", "2026-10-08")).toBe(
      "No price before 8 Oct",
    );
    expect(priceChangeReasonLabel("start_close_too_old", "2026-10-08")).toBe(
      "No price near 8 Oct",
    );
    expect(
      priceChangeReasonLabel("start_close_source_mismatch", "2026-10-08"),
    ).toBe("Prices from different sources");
    expect(
      priceChangeReasonLabel("start_close_currency_mismatch", "2026-10-08"),
    ).toBe("Prices in different currencies");
    expect(priceChangeReasonLabel("zero_start_close", "2026-10-08")).toBe(
      "Start price is zero",
    );
    expect(priceChangeReasonLabel("split_in_period", "2026-10-08")).toBe(
      "Share split in this period",
    );

    const model = buildOpportunitiesViewModel({
      response: response([opportunityRow("GAP", ["start_close_too_old"])]),
      requestedWindow: "7d",
      order: "fallers_first",
      isError: false,
      errorMessage: null,
    });
    expect(model.rows[0]).toMatchObject({
      changeAvailable: false,
      changeLabel: "No price near 8 Oct",
    });
  });

  it("keeps the warning stale signal on the latest price when the change is unavailable", () => {
    const model = buildOpportunitiesViewModel({
      response: response([
        opportunityRow("OLD", ["no_close_since_period_start"], {
          freshness: "warning_stale_10_days",
        }),
        opportunityRow("MINOR", "-1.00", { freshness: "minor_stale_1_days" }),
        opportunityRow("NONE", ["no_price_source"], { latest: false }),
      ]),
      requestedWindow: "7d",
      order: "fallers_first",
      isError: false,
      errorMessage: null,
    });

    const byName = new Map(
      model.rows.map((row) => [row.instrument.symbol, row]),
    );
    expect(byName.get("OLD")?.latestPrice).toEqual({
      label: "SEK 377.8",
      freshnessKind: "warning_stale",
      freshnessLabel: "Stale 10 days",
    });
    expect(byName.get("MINOR")?.latestPrice?.freshnessKind).toBe("minor_stale");
    expect(byName.get("NONE")?.latestPrice).toBeNull();
  });

  it("reports an empty ledger as empty", () => {
    const model = buildOpportunitiesViewModel({
      response: response([]),
      requestedWindow: "7d",
      order: "fallers_first",
      isError: false,
      errorMessage: null,
    });

    expect(model.status).toBe("empty");
    expect(model.message).toBe("No holdings yet.");
  });

  it("reports an error with its message, and loading before any response", () => {
    expect(
      buildOpportunitiesViewModel({
        requestedWindow: "7d",
        order: "fallers_first",
        isError: true,
        errorMessage: "Server unavailable",
      }),
    ).toMatchObject({ status: "error", message: "Server unavailable" });

    expect(
      buildOpportunitiesViewModel({
        requestedWindow: "7d",
        order: "fallers_first",
        isError: false,
        errorMessage: null,
      }),
    ).toMatchObject({ status: "loading", rows: [] });
  });
});
