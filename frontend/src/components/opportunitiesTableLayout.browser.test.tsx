import "@fontsource/inter/latin-400.css";
import "@fontsource/inter/latin-600.css";
import "@fontsource/jetbrains-mono/latin-500.css";
import "../styles.css";
import { cleanup, render, screen, within } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";
import { opportunitiesWithLongVerdicts } from "../test/opportunityFixtures";
import { OpportunitiesPage } from "./OpportunitiesPage";

vi.mock("../api/queries", () => ({
  useOpportunities: () => ({
    data: {
      data_revision: "layout",
      period: {
        window: "7d",
        start_date: "2026-10-08",
        end_date: "2026-10-15",
      },
      rows: opportunitiesWithLongVerdicts(),
    },
    isError: false,
    error: null,
  }),
}));
vi.mock("./useAppMode", () => ({ useAppMode: () => ({ canMutate: false }) }));

afterEach(() => {
  cleanup();
  localStorage.clear();
});

async function renderPage(width: number) {
  await page.viewport(width, 1138);
  render(
    <MemoryRouter>
      <div className="app-shell">
        <main className="workspace">
          <OpportunitiesPage />
        </main>
      </div>
    </MemoryRouter>,
  );
  await document.fonts.ready;
  const table = screen.getByRole("table", { name: "Opportunities" });
  const wrapper = table.closest<HTMLDivElement>(".table-wrap");
  if (!wrapper) throw new Error("Opportunities table has no scroll wrapper");
  return { table, wrapper };
}

function expectNoPageOverflow() {
  expect(document.body.scrollWidth).toBeLessThanOrEqual(
    document.documentElement.clientWidth,
  );
  expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(
    document.documentElement.clientWidth,
  );
}

it("fits long verdicts and full instrument names at the desktop width", async () => {
  const { table, wrapper } = await renderPage(1258);
  expect(screen.getByRole("main").clientWidth).toBe(1180);
  expect(wrapper.scrollWidth).toBeLessThanOrEqual(wrapper.clientWidth);

  for (const name of [
    "Alba Renewables",
    "Harbor Utilities",
    "Nordic Automation",
    "Sprout Diagnostics",
    "Varvsgatan Industri",
  ]) {
    const link = within(table).getByRole("link", { name });
    expect(link.clientWidth).toBeGreaterThan(0);
    expect(link.scrollWidth).toBeLessThanOrEqual(link.clientWidth);
  }
  for (const label of [
    "Possible candidate, report timing unknown",
    "Report coming soon (Nasdaq expected)",
    "Already at target",
  ]) {
    expect(within(table).getByText(label)).toBeTruthy();
  }
  // Populated target bars keep the last column's real minimum width in play.
  expect(within(table).getAllByRole("img")).toHaveLength(5);

  const long = within(table).getByText(
    "Unusual drop, far below its high, no report in the period, sales and earnings holding up",
  );
  const short = within(table).getByText("Report in the period");
  const lineHeight = Number.parseFloat(getComputedStyle(short).lineHeight);
  expect(short.getBoundingClientRect().height).toBeCloseTo(lineHeight, 1);
  expect(long.getBoundingClientRect().height).toBeGreaterThan(
    short.getBoundingClientRect().height,
  );
  expect(long.getBoundingClientRect().height).toBeCloseTo(lineHeight * 2, 1);
  expectNoPageOverflow();
});

it("keeps sideways scrolling inside the table at a narrower window", async () => {
  const { wrapper } = await renderPage(640);
  expect(wrapper.scrollWidth).toBeGreaterThan(wrapper.clientWidth);
  wrapper.scrollLeft = wrapper.scrollWidth;
  expect(wrapper.scrollLeft).toBeGreaterThan(0);
  expectNoPageOverflow();
});
