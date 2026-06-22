// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AsyncBoundary } from "./AsyncBoundary";

afterEach(cleanup);

describe("AsyncBoundary", () => {
  it("shows three skeleton bars while pending", () => {
    const { container } = render(<AsyncBoundary isPending />);
    expect(container.querySelectorAll(".skeleton-bar")).toHaveLength(3);
  });

  it("shows a custom error message while errored", () => {
    render(<AsyncBoundary isError errorMessage="Load failed." />);
    expect(screen.getByText("Load failed.")).toBeDefined();
  });

  it("shows default error message when no errorMessage prop is given", () => {
    render(<AsyncBoundary isError />);
    expect(screen.getByText("Could not load data.")).toBeDefined();
  });

  it("calls onRetry when the Retry button is clicked", () => {
    const onRetry = vi.fn();
    render(<AsyncBoundary isError onRetry={onRetry} />);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });

  it("shows empty message when isEmpty", () => {
    render(<AsyncBoundary isEmpty emptyMessage="Nothing here." />);
    expect(screen.getByText("Nothing here.")).toBeDefined();
  });

  it("renders children when not pending, errored, or empty", () => {
    render(
      <AsyncBoundary>
        <span>content</span>
      </AsyncBoundary>,
    );
    expect(screen.getByText("content")).toBeDefined();
  });
});
