// @vitest-environment jsdom

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { GainsWaterfall } from "./GainsWaterfall";
import type { WaterfallView } from "./waterfallViewModel";

const view: WaterfallView = {
  mode: "open",
  currency: "SEK",
  minValue: 0,
  maxValue: 335000,
  rows: [
    {
      key: "cost-basis",
      label: "Cost basis (held)",
      kind: "base",
      value: { status: "available", value: "265582.94" },
      direction: null,
      percent: null,
      span: { from: 0, to: 265582.94 },
    },
    {
      key: "price",
      label: "Price effect",
      kind: "effect",
      value: { status: "available", value: "53546.54" },
      direction: "up",
      percent: { status: "available", value: "20.16" },
      span: { from: 265582.94, to: 319129.48 },
    },
    {
      key: "income",
      label: "Dividend income",
      kind: "placeholder",
      value: { status: "unavailable", reasons: ["income_not_tracked"] },
      direction: null,
      percent: null,
      span: null,
    },
    {
      key: "total-return",
      label: "Total return",
      kind: "total",
      value: { status: "available", value: "62964.73" },
      direction: "up",
      percent: { status: "available", value: "23.71" },
      span: { from: 265582.94, to: 328547.67 },
    },
  ],
};

describe("GainsWaterfall", () => {
  it("renders each row label, the currency header, and the income placeholder", () => {
    render(
      <GainsWaterfall
        view={view}
        title="Portfolio gains breakdown"
        className="panel dashboard-waterfall"
        headerRight={
          <span className="status-chip warning compact">1 incomplete</span>
        }
      />,
    );
    expect(
      screen.getByRole("heading", { name: "Portfolio gains breakdown" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Cost basis (held)")).toBeInTheDocument();
    expect(screen.getByText("Dividend income")).toBeInTheDocument();
    expect(screen.getByText("Total return")).toBeInTheDocument();
    expect(screen.getByText("SEK")).toBeInTheDocument();
    expect(screen.getByText("% of cost")).toBeInTheDocument();
    expect(screen.getByText("1 incomplete")).toBeInTheDocument();
    // Income placeholder is a calm "not tracked" note, not a warning chip.
    expect(screen.getByText("Not tracked yet")).toBeInTheDocument();
  });

  it("renders held and sold capital layers with an accessible overflow break", () => {
    const brokenView: WaterfallView = {
      ...view,
      rows: view.rows.map((row) =>
        row.key === "total-return"
          ? {
              ...row,
              stackedSegments: [
                {
                  key: "stacked-base",
                  direction: null,
                  span: { from: 0, to: 265582.94 },
                },
                {
                  key: "stacked-price",
                  direction: "up",
                  span: { from: 265582.94, to: 319129.48 },
                },
              ],
              capitalStack: {
                held: 265582.94,
                sold: 2000000,
                heldUnits: 1,
                soldUnits: 7.5306,
                displayedSoldUnits: 2,
                isBroken: true,
              },
            }
          : row,
      ),
    };
    const { container } = render(<GainsWaterfall view={brokenView} />);

    const capital = container.querySelector("[data-capital-stack='true']");
    expect(capital).toHaveAttribute("data-broken", "true");
    expect(capital).toHaveAccessibleName(
      /SEK 265,582\.94 held and SEK 2,000,000 previously sold/i,
    );
    expect(screen.getByText("sold ×7.5")).toBeInTheDocument();
  });
});
