// @vitest-environment jsdom

import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TransactionsPage } from "./TransactionsPage";

const useDataVersion = vi.fn();
const useDeleteTransaction = vi.fn();
const useInstruments = vi.fn();
const useTransactions = vi.fn();
const useCreateTransaction = vi.fn();
const useUpsertInstrument = vi.fn();
const useAppMode = vi.fn();

vi.mock("../api/queries", () => ({
  useDataVersion: (...args: unknown[]) => useDataVersion(...args),
  useDeleteTransaction: (...args: unknown[]) => useDeleteTransaction(...args),
  useInstruments: (...args: unknown[]) => useInstruments(...args),
  useTransactions: (...args: unknown[]) => useTransactions(...args),
  useCreateTransaction: (...args: unknown[]) => useCreateTransaction(...args),
  useUpsertInstrument: (...args: unknown[]) => useUpsertInstrument(...args),
}));

vi.mock("./useAppMode", () => ({
  useAppMode: (...args: unknown[]) => useAppMode(...args),
}));

function setupQueries() {
  useDataVersion.mockReturnValue({ data: { valuation_date: "2026-10-04" } });
  useDeleteTransaction.mockReturnValue({
    mutateAsync: vi.fn(),
    isPending: false,
    variables: null,
  });
  useInstruments.mockReturnValue({ data: [] });
  useTransactions.mockReturnValue({
    data: [],
    isPending: false,
    isError: false,
    refetch: vi.fn(),
  });
  useCreateTransaction.mockReturnValue({ mutateAsync: vi.fn() });
  useUpsertInstrument.mockReturnValue({ mutateAsync: vi.fn() });
}

function renderTransactionsPage(canMutate: boolean) {
  setupQueries();
  useAppMode.mockReturnValue({ canMutate });

  render(
    <MemoryRouter>
      <TransactionsPage />
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("TransactionsPage", () => {
  it("returns focus to the add transaction button after Cancel", () => {
    renderTransactionsPage(true);

    const toggle = screen.getByRole("button", { name: "Add transaction" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(
      screen.getByRole("region", { name: "Add transaction" }),
    ).toBeTruthy();

    const cancel = screen.getByRole("button", { name: "Cancel" });
    cancel.focus();
    fireEvent.click(cancel);

    expect(
      screen.queryByRole("region", { name: "Add transaction" }),
    ).toBeNull();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveFocus();
  });

  it("closes the form when the header add transaction button is clicked again", () => {
    renderTransactionsPage(true);

    const toggle = screen.getByRole("button", { name: "Add transaction" });
    fireEvent.click(toggle);

    expect(
      screen.getByRole("region", { name: "Add transaction" }),
    ).toBeTruthy();
    expect(toggle).toHaveAttribute("aria-expanded", "true");

    fireEvent.click(toggle);

    expect(
      screen.queryByRole("region", { name: "Add transaction" }),
    ).toBeNull();
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("returns focus to the add transaction button after Save", async () => {
    renderTransactionsPage(true);
    useUpsertInstrument.mockReturnValue({
      mutateAsync: vi.fn().mockResolvedValue({ instrument: { id: 1 } }),
    });
    useCreateTransaction.mockReturnValue({
      mutateAsync: vi.fn().mockResolvedValue({}),
    });

    const toggle = screen.getByRole("button", { name: "Add transaction" });
    fireEvent.click(toggle);
    fireEvent.change(screen.getByRole("textbox", { name: "Symbol" }), {
      target: { value: "TEST" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Exchange" }), {
      target: { value: "TEST" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Name" }), {
      target: { value: "Test instrument" },
    });
    fireEvent.change(screen.getByRole("spinbutton", { name: "Quantity" }), {
      target: { value: "1" },
    });
    fireEvent.change(screen.getByRole("textbox", { name: "Price (native)" }), {
      target: { value: "10" },
    });

    const save = screen.getByRole("button", { name: "Save transaction" });
    save.focus();
    fireEvent.click(save);

    await waitFor(() => {
      expect(
        screen.queryByRole("region", { name: "Add transaction" }),
      ).toBeNull();
    });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveFocus();
  });

  it("shows the add transaction invitation in the mutable empty state", () => {
    renderTransactionsPage(true);

    expect(screen.getByText(/Use Add transaction above/)).toBeTruthy();
    expect(screen.getByText(/No transactions yet\./)).toBeTruthy();
    expect(
      screen.getByRole("link", { name: "import from Avanza or Sharesight" }),
    ).toHaveAttribute("href", "/import");
  });

  it("omits the add action and invitation in demo mode", () => {
    renderTransactionsPage(false);

    expect(
      screen.queryByRole("button", { name: "Add transaction" }),
    ).toBeNull();
    expect(screen.getByText("No transactions yet.")).toBeTruthy();
    expect(screen.queryByText(/Use Add transaction above/)).toBeNull();
    expect(
      screen.queryByRole("link", { name: "import from Avanza or Sharesight" }),
    ).toBeNull();
  });
});
