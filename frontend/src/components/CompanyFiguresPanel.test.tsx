// @vitest-environment jsdom
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  fundamentalsPreview,
  fundamentalsQuarterHistory,
  instrumentFundamentals,
} from "../test/instrumentFundamentalsFixture";
import { CompanyFiguresPanel } from "./CompanyFiguresPanel";
import { CompanyFiguresSource } from "./CompanyFiguresSource";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
function response(status: number, body?: unknown) {
  return {
    status,
    ok: status < 400,
    text: async () => (body === undefined ? "" : JSON.stringify(body)),
  } as Response;
}
function source(hasFigures = true, canMutate = true, open = canMutate) {
  const data = instrumentFundamentals();
  data.source.has_active_figures = hasFigures;
  const client = new QueryClient({
    defaultOptions: { mutations: { retry: false } },
  });
  const invalidate = vi.spyOn(client, "invalidateQueries");
  const fetch = vi.fn().mockResolvedValue(response(200, fundamentalsPreview()));
  vi.stubGlobal("fetch", fetch);
  render(
    <QueryClientProvider client={client}>
      <CompanyFiguresSource id={1} source={data.source} canMutate={canMutate} />
    </QueryClientProvider>,
  );
  if (open)
    fireEvent.click(screen.getByRole("button", { name: "Change source" }));
  return { fetch, invalidate };
}
describe("company figures panel", () => {
  it.each([8, 12])(
    "renders all %s quarters with the newest full amounts accessible",
    (count) => {
      const data = instrumentFundamentals();
      data.quarters = fundamentalsQuarterHistory(count);
      render(<CompanyFiguresPanel data={data} />);
      const sales = within(screen.getByRole("region", { name: "Sales" }));
      const eps = within(screen.getByRole("region", { name: "EPS" }));
      expect(sales.getAllByRole("img")).toHaveLength(count);
      expect(eps.getAllByRole("img")).toHaveLength(count);
      expect(sales.getAllByText("565.9M EUR")).toHaveLength(count);
      expect(eps.getAllByText("1.22 EUR")).toHaveLength(count);
      expect(
        sales.getByRole("img", {
          name: "Sales for the quarter ending 2026-09-30: 565,871,000 EUR",
        }),
      ).toHaveAttribute(
        "title",
        "Sales for the quarter ending 2026-09-30: 565,871,000 EUR",
      );
      expect(
        eps.getByRole("img", {
          name: "EPS for the quarter ending 2026-09-30: 1.22 EUR",
        }),
      ).toBeInTheDocument();
    },
  );
  it("renders four labelled figures, report details, rate and accessible missing quarters", () => {
    render(<CompanyFiguresPanel data={instrumentFundamentals()} />);
    expect(
      screen.getByRole("heading", { name: "How the company is doing" }),
    ).toBeInTheDocument();
    for (const label of ["P/E", "P/S", "Profit margin", "Sales growth"])
      expect(screen.getByText(label)).toBeInTheDocument();
    expect(screen.getByText("P/E: 12.3")).toBeInTheDocument();
    expect(screen.getByText("Profit margin: 12.50%")).toBeInTheDocument();
    expect(
      screen.getAllByRole("img", {
        name: "No figures reported for the quarter ending 2026-06-30",
      }),
    ).toHaveLength(2);
    expect(
      screen.getByRole("img", {
        name: /EPS for the quarter ending 2026-03-31: -0.5 EUR/,
      }),
    ).toBeInTheDocument();
    expect(screen.getByText("in 22 days")).toBeInTheDocument();
    expect(
      screen.getByText("Observed between 2026-10-18 and 2026-10-22"),
    ).toBeInTheDocument();
    expect(screen.getByText("quarter ending 2026-09-30")).toBeInTheDocument();
    expect(screen.getByText(/Converted EUR→SEK/)).toBeInTheDocument();
  });
  it("shows awaiting figures without old quarters", () => {
    const data = instrumentFundamentals();
    data.source.awaiting_figures = true;
    data.quarters = [];
    render(<CompanyFiguresPanel data={data} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Awaiting figures. Figures for OLD load on the next refresh.",
    );
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });
});
describe("source correction visible states", () => {
  it.each([
    "Same company, different listing",
    "Wrong company — set the old figures aside",
  ])("previews and saves with %s", async (choice) => {
    const { fetch, invalidate } = source();
    fireEvent.change(
      screen.getByRole("textbox", { name: "Company symbol (Yahoo)" }),
      { target: { value: "MSFT" } },
    );
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(
      await screen.findByText("Microsoft Corporation"),
    ).toBeInTheDocument();
    expect(screen.getByText(/USD · 5 quarters found/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    fireEvent.click(screen.getByRole("radio", { name: choice }));
    fetch.mockResolvedValueOnce(response(204));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "Source saved. Figures load on the next refresh.",
      ),
    ).toBeInTheDocument();
    expect(fetch).toHaveBeenLastCalledWith(
      "/api/instruments/1/fundamentals-source",
      expect.objectContaining({
        method: "PUT",
        body: JSON.stringify({
          provider: "YAHOO",
          provider_symbol: "MSFT",
          previous_link: choice.startsWith("Same")
            ? "same_company"
            : "wrong_company",
        }),
      }),
    );
    expect(invalidate).toHaveBeenCalledExactlyOnceWith({
      queryKey: ["data-version"],
    });
  });
  it.each(["Not a company", "Use automatic"])(
    "asks the previous-company choice for %s",
    async (action) => {
      const { fetch } = source();
      fireEvent.click(screen.getByRole("button", { name: action }));
      expect(
        screen.getByRole("group", {
          name: "How should the previous figures be treated?",
        }),
      ).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
      fireEvent.click(
        screen.getByRole("radio", {
          name: "Wrong company — set the old figures aside",
        }),
      );
      fetch.mockResolvedValueOnce(response(204));
      fireEvent.click(screen.getByRole("button", { name: "Save" }));
      expect(
        await screen.findByText(
          action === "Not a company"
            ? "Source saved. Marked as not a company."
            : "Source saved. Figures load on the next refresh.",
        ),
      ).toBeInTheDocument();
      expect(fetch).toHaveBeenLastCalledWith(
        action === "Use automatic"
          ? "/api/instruments/1/fundamentals-source?previous_link=wrong_company"
          : "/api/instruments/1/fundamentals-source",
        expect.objectContaining({
          method: action === "Use automatic" ? "DELETE" : "PUT",
          body: action === "Use automatic" ? undefined : expect.any(String),
        }),
      );
    },
  );
  it("does not ask about old figures when none exist", async () => {
    source(false);
    fireEvent.click(screen.getByRole("button", { name: "Not a company" }));
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save" })).toBeEnabled();
  });
  it("clears preview after typing and displays preview and save errors", async () => {
    const { fetch } = source();
    const field = screen.getByRole("textbox", {
      name: "Company symbol (Yahoo)",
    });
    fireEvent.change(field, { target: { value: "MSFT" } });
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    await screen.findByText("Microsoft Corporation");
    fireEvent.change(field, { target: { value: "WRONG" } });
    expect(screen.queryByText("Microsoft Corporation")).not.toBeInTheDocument();
    fetch.mockResolvedValueOnce(
      response(200, { ...fundamentalsPreview(), status: "no_match" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Preview" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No matching symbol found.",
    );
    fireEvent.click(screen.getByRole("button", { name: "Not a company" }));
    fireEvent.click(
      screen.getByRole("radio", { name: "Same company, different listing" }),
    );
    fetch.mockResolvedValueOnce(
      response(400, {
        error: { code: "previous_link_required", message: "Choose again" },
      }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("Choose again"),
    );
  });
  it("hides correction in demo but shows source details", () => {
    source(true, false);
    expect(
      screen.getByRole("heading", { name: "Company figures source" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/automatic · Equity/)).toBeInTheDocument();
    expect(screen.getByText("Updated")).toBeInTheDocument();
    expect(screen.queryByRole("form")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Change source" }),
    ).not.toBeInTheDocument();
  });
  it("keeps the correction form folded away until asked for", () => {
    source(true, true, false);
    expect(
      screen.queryByRole("textbox", { name: "Company symbol (Yahoo)" }),
    ).not.toBeInTheDocument();
    const toggle = screen.getByRole("button", { name: "Change source" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);
    expect(
      screen.getByRole("textbox", { name: "Company symbol (Yahoo)" }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(
      screen.queryByRole("textbox", { name: "Company symbol (Yahoo)" }),
    ).not.toBeInTheDocument();
  });
  it("displays backend local dates instead of timestamp text", () => {
    const data = instrumentFundamentals();
    data.source.last_success_on = "2026-11-02";
    data.source.last_attempt_on = "2026-11-03";
    render(
      <CompanyFiguresSource id={1} source={data.source} canMutate={false} />,
    );
    expect(screen.getByText("2026-11-02")).toBeInTheDocument();
    expect(screen.getByText("2026-11-03")).toBeInTheDocument();
    expect(screen.queryByText(/T12:00:00Z/)).not.toBeInTheDocument();
  });
  it("separates the last attempt outcome from its backend date", () => {
    const data = instrumentFundamentals();
    data.source.last_attempt_on = "2026-11-08";
    render(
      <CompanyFiguresSource id={1} source={data.source} canMutate={false} />,
    );
    const section = within(
      screen.getByRole("region", { name: "Company figures source" }),
    );
    expect(section.getByText(/^Figures updated ·/)).toHaveTextContent(
      "Figures updated · 2026-11-08",
    );
  });
});
