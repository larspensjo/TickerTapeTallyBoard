import { describe, expect, it } from "vitest";
import type { GainsRow, PriceStatusInstrument } from "../api/types";
import {
  effectivePriceSourceRow,
  priceAvailabilityLabel,
  priceSourceLabel,
  priceSourceRows,
} from "./priceSourceViewModel";

function makePriceStatus(
  priceSources: PriceStatusInstrument["price_sources"],
  effectivePriceSource: string | null,
): PriceStatusInstrument {
  return {
    instrument_id: 32,
    exchange: "AVANZA",
    symbol: "AVA SAMSUNG TRACKER",
    currency: "SEK",
    price_sources: priceSources,
    effective_price_source: effectivePriceSource,
    open_quantity: 10,
    latest_price: {
      status: "available",
      date: "2026-08-28",
      value: "100",
      provider: effectivePriceSource,
      provider_symbol: "TX2997672",
      reason: null,
    },
    latest_fx: {
      status: "available",
      date: "2026-08-28",
      value: "1",
      provider: "FRANKFURTER",
      provider_symbol: "SEK",
      reason: null,
    },
  };
}

const yahooSource = {
  provider: "YAHOO",
  provider_symbol: "AVA.ST",
  asset_class: null,
  currency: "SEK",
  enabled: true,
};

const nasdaqSource = {
  provider: "NASDAQ_NORDIC",
  provider_symbol: "TX2997672",
  asset_class: "TRACKER_CERTIFICATES",
  currency: "SEK",
  enabled: true,
};

function makeGainWithSource(source: string): Pick<GainsRow, "latest_price"> {
  return {
    latest_price: {
      date: "2026-08-27",
      close: "99",
      currency: "SEK",
      source,
      freshness: "fresh",
    },
  };
}

describe("priceSourceLabel", () => {
  it("names known price sources", () => {
    expect(priceSourceLabel("YAHOO")).toBe("Yahoo");
    expect(priceSourceLabel("NASDAQ_NORDIC")).toBe("Nasdaq Nordic");
    expect(priceSourceLabel("MANUAL")).toBe("Hand-entered");
  });

  it("prettifies an unknown source code", () => {
    expect(priceSourceLabel("SOME_NEW_PROVIDER")).toBe("Some New Provider");
  });
});

describe("priceAvailabilityLabel", () => {
  it("has no availability label without price status", () => {
    expect(priceAvailabilityLabel(null, null)).toBeNull();
  });

  it("distinguishes no mappings from disabled mappings", () => {
    expect(priceAvailabilityLabel(makePriceStatus([], null), null)).toBe(
      "No price source",
    );
    expect(
      priceAvailabilityLabel(
        makePriceStatus([{ ...yahooSource, enabled: false }], null),
        null,
      ),
    ).toBe("Price sources disabled");
  });

  it("names an enabled source", () => {
    expect(
      priceAvailabilityLabel(makePriceStatus([yahooSource], "YAHOO"), null),
    ).toBe("Yahoo");
  });

  it("names the lower-precedence source when it is effective", () => {
    expect(
      priceAvailabilityLabel(
        makePriceStatus([yahooSource, nasdaqSource], "NASDAQ_NORDIC"),
        null,
      ),
    ).toBe("Nasdaq Nordic");
  });

  it("describes enabled sources when none is effective", () => {
    expect(
      priceAvailabilityLabel(makePriceStatus([yahooSource], null), null),
    ).toBe("No effective price source");
  });
});

describe("priceSourceRows", () => {
  it("marks the effective row and omits absent asset classes", () => {
    const rows = priceSourceRows(
      makePriceStatus([yahooSource, nasdaqSource], "NASDAQ_NORDIC"),
    );

    expect(rows).toEqual([
      {
        provider: "YAHOO",
        label: "Yahoo",
        identifier: "AVA.ST",
        enabled: true,
        effective: false,
      },
      {
        provider: "NASDAQ_NORDIC",
        label: "Nasdaq Nordic",
        identifier: "TX2997672",
        assetClass: "TRACKER_CERTIFICATES",
        enabled: true,
        effective: true,
      },
    ]);
    expect(rows.filter((row) => row.effective)).toHaveLength(1);
  });

  it("uses the valuation source for both rows and the effective selector", () => {
    const priceStatus = makePriceStatus([yahooSource, nasdaqSource], "YAHOO");
    const gain = makeGainWithSource("NASDAQ_NORDIC");

    const rows = priceSourceRows(priceStatus, gain);

    expect(rows.find((row) => row.provider === "YAHOO")?.effective).toBe(false);
    expect(
      rows.find((row) => row.provider === "NASDAQ_NORDIC")?.effective,
    ).toBe(true);
    expect(effectivePriceSourceRow(priceStatus, gain)?.provider).toBe(
      "NASDAQ_NORDIC",
    );
  });
});
