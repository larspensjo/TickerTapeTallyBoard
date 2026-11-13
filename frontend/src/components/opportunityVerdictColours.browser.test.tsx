import "../styles.css";
import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import type { OpportunityVerdictCode } from "../api/types";
import { reportDate } from "../test/companyFiguresFixtures";
import {
  opportunityEvidence,
  opportunityFixture,
} from "../test/opportunityFixtures";
import { OpportunitiesPage } from "./OpportunitiesPage";

const codes: OpportunityVerdictCode[] = [
  "sentiment_dip_candidate",
  "possible_candidate_report_timing_unknown",
  "not_enough_data",
  "fell_for_a_reason",
  "normal_movement",
];

vi.mock("../api/queries", () => ({
  useOpportunities: () => ({
    data: {
      data_revision: "colours",
      period: {
        window: "7d",
        start_date: "2026-10-08",
        end_date: "2026-10-15",
      },
      rows: codes.map((code, index) => {
        const evidence = opportunityEvidence(code);
        if (index === 0) {
          evidence.verdict.cautions = [
            "loss_making",
            "report_coming_soon",
            "already_at_target",
            "no_target",
          ];
          evidence.checks.report_coming_soon.report = reportDate({
            certainty: "confirmed",
            basis: "company_announced",
            date: "2026-10-20",
            days_until: 5,
          });
        }
        return opportunityFixture(index + 1, evidence);
      }),
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

function tokenColour(token: string, alpha?: string): string {
  const probe = document.createElement("span");
  probe.style.color = alpha
    ? `rgb(from var(${token}) r g b / ${alpha})`
    : `var(${token})`;
  document.body.append(probe);
  const colour = getComputedStyle(probe).color;
  probe.remove();
  return colour;
}

it("keeps verdict chip fills, neutral cautions and readable evidence colours", () => {
  render(
    <MemoryRouter>
      <div className="app-shell">
        <main className="workspace">
          <OpportunitiesPage />
        </main>
      </div>
    </MemoryRouter>,
  );
  for (const [label, foreground, background] of [
    ["Sentiment-dip candidate", "--up", "--up-soft"],
    ["Fell for a reason?", "--warning", "--warning-soft"],
    [
      "Possible candidate, report timing unknown",
      "--text-secondary",
      "--surface-2",
    ],
    ["Not enough data", "--text-secondary", "--surface-2"],
    ["Normal movement", "--text-secondary", "--surface-2"],
    ["Loss-making", "--text-secondary", "--surface-2"],
    ["Report coming soon (confirmed)", "--text-secondary", "--surface-2"],
    ["Already at target", "--text-secondary", "--surface-2"],
    ["No target", "--text-secondary", "--surface-2"],
  ] as const) {
    const style = getComputedStyle(screen.getByText(label));
    expect(style.color).toBe(tokenColour(foreground));
    expect(style.backgroundColor).toBe(tokenColour(background));
  }

  for (const [label, token] of [
    ["Sentiment-dip candidate", "--up"],
    ["Fell for a reason?", "--warning"],
  ]) {
    expect(getComputedStyle(screen.getByText(label)).borderTopColor).toBe(
      tokenColour(token, "28%"),
    );
  }

  for (const sentence of [
    "Unusual drop, no report in the period, business intact",
    "Unusual drop, report timing unknown, business intact",
    "Less than a year of prices, report timing unknown",
    "Report in the period, business weakening",
    "Fall within usual moves, not far below its high",
  ]) {
    const style = getComputedStyle(screen.getByText(sentence));
    expect(style.color).toBe(tokenColour("--text-secondary"));
    expect(style.fontSize).toBe("13px");
  }
  const note = screen.getByText(/A verdict describes the dip:/);
  expect(getComputedStyle(note).color).toBe(tokenColour("--text-secondary"));
});
