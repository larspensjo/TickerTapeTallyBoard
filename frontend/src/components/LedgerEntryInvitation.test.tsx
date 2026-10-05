// @vitest-environment jsdom

import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vitest";
import { LedgerEntryInvitation } from "./LedgerEntryInvitation";

afterEach(cleanup);

describe("LedgerEntryInvitation", () => {
  it("links to manual entry and both import sources from elsewhere", () => {
    render(
      <MemoryRouter>
        <LedgerEntryInvitation variant="elsewhere" canMutate />
      </MemoryRouter>,
    );

    expect(
      screen.getByRole("link", { name: "Transactions page" }),
    ).toHaveAttribute("href", "/transactions");
    expect(
      screen.getByRole("link", { name: "import from Avanza or Sharesight" }),
    ).toHaveAttribute("href", "/import");
    expect(screen.getByText(/Add one on the/)).toHaveTextContent(
      "Add one on the Transactions page or import from Avanza or Sharesight.",
    );
  });

  it("refers to the header action on the Transactions page", () => {
    render(
      <MemoryRouter>
        <LedgerEntryInvitation variant="transactions" canMutate />
      </MemoryRouter>,
    );

    expect(screen.getByText(/Use Add transaction above/)).toHaveTextContent(
      "Use Add transaction above, or import from Avanza or Sharesight.",
    );
    expect(
      screen.getByRole("link", { name: "import from Avanza or Sharesight" }),
    ).toHaveAttribute("href", "/import");
    expect(
      screen.queryByRole("link", { name: "Transactions page" }),
    ).toBeNull();
  });

  it.each(["elsewhere", "transactions"] as const)(
    "omits the %s invitation when data cannot be changed",
    (variant) => {
      render(
        <MemoryRouter>
          <LedgerEntryInvitation variant={variant} canMutate={false} />
        </MemoryRouter>,
      );

      expect(screen.queryByRole("link")).toBeNull();
      expect(screen.queryByText(/Add one|Use Add transaction/)).toBeNull();
    },
  );
});
