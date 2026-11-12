# Plan: Opportunities Page Identifies Sentiment-Driven Dips

Date: 2026-11-11 · Status: proposed (revised after review) · Commit headlines:
`Plan.OpportunitySentimentDips Phase N: ...`

This plan is ephemeral. Durable documents and code describe the behaviour (for example
"the latest refresh tops up a year of daily history"), never the phase numbers used here.

## Goal

For every held asset whose share price fell in the chosen window, the Opportunities page
shows a small set of named evidence checks, each with a plain-language result and an
honest "unknown" or "not applicable", plus one verdict built from them by explicit rules.
The page opens on **Best candidates first**. The owner can see why any row got its verdict
without opening anything else. The thresholds behind the price checks are replayed against
stored price history before the verdicts are presented as trustworthy, and the findings are
recorded as a sanitized spike document.

## Fixed inputs

Owner decisions (not re-opened here): evidence checks plus a rule-based verdict and no
numeric score; unknown evidence is never a pass; new default order "Best candidates first"
with "Biggest fallers" and "Biggest risers" kept, reversing the ordering clause of the
2026-10-15 Opportunities entry; held assets only, no watchlist; no paid services and no new
third-party components; the nine checks listed below; a later market-index data set from the
existing price source; a calibration step; future candidates are listed, not designed.

Owner decision taken during planning (2026-11-11): **a loss-making company whose sales are
not shrinking and whose losses are not deepening can be a Sentiment-dip candidate, with a
visible "loss-making" caution.** A loss-making company whose sales shrink or whose losses
deepen goes to "Fell for a reason?". This knowingly loosens the "profitable" part of the
brief's description of Business intact.

Owner decisions after the readiness measurement (2026-11-11), which supersede any
conflicting text below:

- **A fifth verdict, "Possible candidate, report timing unknown"**, is added between
  Sentiment-dip candidate and Not enough data (Open Question 1, answered).
- **Unusual drop works from 120 closes; only the 1-year range ("off its high") needs
  full-year coverage.** The gap between the look-back start and the first close blocks the
  1-year range only.

Outcome of the readiness measurement: the **relaxed** "no report" rule is selected (the
strict rule answered 2 of 20 held company rows at 30 days).

Outcome of the calibration replay (2026-11-11): **Calibrated**, thresholds unchanged, and
**no measurable edge** for flagged dips over the following 20 trading days.

Owner decision after the replay (2026-11-12, relayed by the orchestrator and confirmed by
the owner in the implementation session): **the verdicts are shown as planned**, with a visible, honest note that a verdict describes
the dip and does not predict the price, and the replay is repeated as history grows. The
owner also said that 20 trading days may be too short a horizon and that more evaluations
of this kind may be needed; see "Proposal for the owner: longer horizons and repeat
replays" below.

Settled by the session lead (open to challenge): the "no report in the period" rule is
chosen from the readiness measurement by the data-driven rule below; the market check is
context only and never changes the verdict tier; a possible candidate carries the same
cautions as a candidate; an unusual drop is unknown (`no_price_variation`) when the
historical moves show no variation at all; a company-confirmed report dated today whose
figures are in counts as a report in the period (only a scheduled date whose figures have
not appeared counts as "coming soon").

Settled by the orchestrator on 2026-11-11, overturning the session lead, and **confirmed
by the owner on 2026-11-12** (his answer relayed by the orchestrator): in the business
check a sub-test that is known to be negative (sales shrinking or earnings sliding) makes
the business `weakening` even when another sub-test is unknown, in line with "negative
evidence beats positive evidence". This holds only while the figures are current;
out-of-date figures, a passed expected report date and source-blocking reasons still give
`unknown`.

Decision-log entries consulted and honoured: 2026-10-15 Opportunities page contract and
latest-refresh window; 2026-11-04 company figures from free sources; 2026-11-08 company-figure
certainty rules and EPS consistency scaling; 2026-11-09 report dates preserve source
certainty, refresh with report timing; 2026-11-07 refresh publishes prices before company
figures; 2026-11-06 a refresh shows the latest price including intraday; 2026-11-08 and
2026-08-29 demo entries; 2026-07-10 persisted view settings; 2026-09-12 one clock and data
revision; 2026-07-15 and 2026-11-08 display precision; 2026-06-13 primary market data
sources; 2026-08-29 day change unavailable across sources, backfill restates aggregates;
2026-08-30 migration bytes; 2026-06-23 dividend transaction model.

## Design

### Checks

Every check has a state, reason codes, and the values needed for its sentence. The backend
computes them; the frontend maps codes to plain-language text in the view-model. Checks are
computed only for rows whose change in the window is available and negative ("fell"). Rows
that rose, or whose change is unavailable, carry no evidence and no verdict.

| Check | Answers | Data |
|---|---|---|
| Unusual drop | Is the fall large for this share's own same-length moves? | Single-source daily closes, look-back year |
| Off its high | How far below the 1-year high; near the 1-year low? | Single-source closes, 1 year |
| Report in the period | Did a quarterly report land in or just before the window? | Report schedules, observations, figures checks |
| Report coming soon | Is the next report within 7 days? | Next report date with its certainty label |
| Business intact | Profitable, sales not shrinking, earnings not sliding vs a year ago? | Company figures and quarter grid |
| Moved with or against its market | Did it fall more than its market? | Median of other held rows; later the market index |
| Stopped falling | Has the price turned up from the window's low? | Closes in the window |
| Went ex-dividend | Did a dividend explain part of the fall? | Dividend events from the price source |
| Room to buy | Below, at or without a conviction target? | Existing conviction target |

**Look-back series rules (shared by unusual drop, off its high, the history top-up and the
calibration replay):**

- The series is the instrument's stored closes from **one source**: the source of the
  latest close. Closes from another source, or in a currency other than the instrument's,
  are excluded and treated as missing, matching the page's same-source rule.
- **Gap rule:** a gap of more than 5 weekdays between consecutive closes in the look-back
  makes the long-history checks `unknown` with reason `price_history_gap` (and the gap's
  start date), never a computation across the hole. A gap of more than 5 weekdays between
  the look-back start and the first close (no full-year coverage) makes only the 1-year
  range `unknown`; the unusual drop needs 120 closes and no gap between them (owner
  decision 2026-11-11).
- **Split rule:** when a ledger split or a stored provider split event falls inside the
  look-back, the long-history checks are computed only if every close in the look-back was
  fetched after that split from Yahoo (whose chart closes are split-adjusted at fetch time).
  Otherwise `unknown` with `split_in_lookback`. Nasdaq Nordic series across a split are
  always `unknown`, because whether that source adjusts history is unverified.
- **Short history:** `unknown` with `price_history_too_short` below the minimum closes.

**Report in the period:**

- The check window starts **7 calendar days before the page's period start** and ends
  today. Seven days covers the start-close tolerance (at most 5 calendar days before the
  period start) plus the source's 1-2 day lag in posting new quarters (fundamentals spike,
  S2). Observed reports are judged by their observation interval `[absent_on, first_seen]`,
  not the displayed midpoint, so the midpoint's own error needs no further widening.
- `yes`: a confirmed (company-announced) last report date inside the window; an observed
  interval wholly inside the window; or a confirmed or Nasdaq-expected scheduled date
  before today inside the window whose figures have not appeared ("scheduled 4 Nov,
  figures not seen yet"). A date that is today counts as "report coming soon", not as in
  the period.
- `unknown`: an observed interval that only partly overlaps the window; figures first seen
  inside the window with no absence baseline (only an upper bound is known); an app
  estimate before today inside the window with figures not yet in; or no report can be
  ruled out after the next quarter's period end under the chosen rule below.
- `no`: the latest quarter's report is known to precede the window (its upper bound,
  `first_seen` or the confirmed date, is before the window start), and no next report can
  have landed in the window. The **strict rule** accepts this when the next quarter has not
  ended yet, or when a successful figures check on or after yesterday found no newer
  quarter. The **relaxed rule** additionally accepts it when the app's own estimate of the
  next report, based on the company's report history (`LastYearDate` basis), is at least 14
  days after today; the sentence then says "no report expected before ~<date> (est.)".
- **Which rule applies is resolved by the readiness measurement (Phase 1), not by a
  question:** the strict rule is used if it already answers (`yes` or `no`) at least two
  thirds of the held company rows in the 30-day window; otherwise the relaxed rule is used.
  Only if even the relaxed rule leaves most company rows (more than half) `unknown` in the
  30-day window does the plan go back to the owner, with one proposal: a fifth verdict
  "Possible candidate, report timing unknown" between candidate and Not enough data (see
  Open Questions). Adding a verdict changes what he agreed to; choosing between the two
  rules does not.
- A period-only last report is never presented as a date: the sentence says "last report
  for the quarter ending 30 Sep was before 24 Oct" (an upper bound) or "between 28 Oct and
  2 Nov" (an interval), per the 2026-11-09 certainty entry.
- `not_applicable` for funds, ETFs, certificates and other positively identified
  non-companies; `unknown` with the source reason for no source, ambiguous source or
  unknown security type.

**Business intact:**

- Sub-tests: profitable (trailing profit margin > 0); sales not shrinking (latest quarter
  vs the same quarter a year earlier ≥ 0); earnings not sliding (latest quarter diluted EPS
  not below the year-ago quarter by more than 10% of the year-ago magnitude; a move from
  profit to loss, or a loss that grows by more than that margin, counts as sliding, which is
  how "losses deepening" is detected).
- **EPS consistency reaches the earnings comparison.** Today the quarter grid marks a
  reported EPS complete, and the consistency check between reported EPS and net income ÷
  diluted shares only downgrades P/E and P/S. The company-figures domain gains a reusable
  consistency evaluation over a set of quarters (the existing trailing P/E and P/S use is
  refactored onto it with unchanged results) and returns its outcome as its own field
  instead of only through P/E and P/S, so unrelated P/E limitations (loss-making, share
  basis unverified, implausible ratio, split since report, stale price) never block the
  business check. The earnings comparison evaluates the two quarters it compares (latest
  and year-ago) with the same 0.02 per-share rounding allowance, the same 5% and 20%
  tolerances, and the summed absolute size of the derived per-quarter values as the scale,
  per the 2026-11-08 EPS consistency entries. Moderate disagreement (5-20%) keeps the
  sub-test usable and marks it "approx." with the gap; severe disagreement (over 20%) makes
  the earnings sub-test, and so the business check, `unknown` with `eps_inconsistent`. When
  either quarter lacks net income or diluted shares the pair cannot be cross-checked; the
  sub-test is used and its sentence says "EPS not cross-checked". An EPS already derived from
  net income keeps its existing approximate marker.
- `unknown` whenever the figures are flagged out of date (`newer_report_expected`), **or**
  the next expected report date (any basis) has passed without the newer quarter appearing,
  or any sub-test's figure is unavailable (too few quarters, latest quarter too old, report
  currency changed, source blocking reasons, severe EPS inconsistency). Approximate figures
  for other reasons are used and marked "approx." in the sentence.
- Results: `intact` (all three pass); `weakening` (sales shrinking or earnings sliding,
  whether or not the company is profitable); `loss_making` (not profitable, sales not
  shrinking and earnings not sliding); `not_applicable` for non-companies. Per the owner's
  decision, `loss_making` passes the company side with a "loss-making" caution.

**Went ex-dividend:** Yahoo's chart response already carries dividend events
(`events=div,splits`); they are parsed and stored with a coverage record per mapping.
`yes` when an ex-date falls after the start close and on or before the end close (amount
as a percentage of the start close); `no` when the stored coverage spans that interval and
holds no event; `unknown` for Nasdaq Nordic-only series, uncovered intervals, or intervals
whose last dividend capture failed validation (see Data prerequisites). Ledger dividends are
not used: they carry the payment date, not the ex-date, and arrive only when the owner
imports a broker export. When `yes`, the unusual-drop test uses the change with the
dividend added back; the displayed change stays close to close, as committed.

**Moved with or against its market (context only):** until the market index lands, the
reference is the median change of the other held rows with an available change in the same
window, needing at least 5. `fell_more_than_market` when the asset's change is below the
reference by more than the per-window margin; `moved_with_market` within the margin;
`fell_less_than_market` above it. The sentence names the source ("the median of your other
holdings"); its tooltip states the weaknesses (mixed markets and currencies, funds
included, self-referential in a concentrated portfolio). When an index exists for the
asset's market and has an available change, the index replaces the median and the check
says so. The verdict never reads this check, so swapping its source cannot change a
verdict.

**Stopped falling:** `yes` when the latest close is above the window's lowest close and
that low is at least 2 trading days (weekday approximation) old. Display only. During
market hours the latest price can be intraday (2026-11-06), so this check and "fell" itself
can change before the close; the footnote says so.

**Room to buy:** from the existing target status: `below` → room to buy; `on_target` or
`above` → "already at target"; `no_target` → "no target"; unavailable statuses → unknown.

### Initial parameters

All values live in one domain parameters module mirrored by the parameters table of
`docs/spikes/OpportunityChecksSpike.md`; a unit test keeps the two identical. The
calibration phase may change the price-check values under its pre-stated criteria.

| Parameter | Initial value |
|---|---|
| Look-back length | 365 calendar days |
| Unusual drop | change ≤ −1.5 × sample standard deviation of overlapping same-length moves ending on or before the start close |
| Unusual drop minimum history | 120 closes |
| 1-year range minimum history | 200 closes, full-year coverage |
| Gap limit | more than 5 weekdays between consecutive closes |
| Deep below 1-year high | at least 20% below the highest close |
| Near 1-year low | latest close within 5% above the lowest close |
| Report window widening | 7 calendar days before the period start |
| "No report" rule | strict, unless Phase 1 shows it answers under two thirds of company rows (30d); then relaxed |
| Relaxed "no report" horizon | estimate on report-history basis ≥ 14 days ahead |
| Report coming soon | next report 0-7 days ahead (today included) |
| Profitable | trailing profit margin > 0; otherwise loss-making (a caution, not a block) |
| Earnings sliding | latest quarter EPS more than 10% (of the year-ago magnitude) below the year-ago quarter |
| EPS pair consistency | 0.02 allowance; 5-20% gap → approx.; over 20% → unknown (existing tolerances) |
| Market reference minimum | 5 other held rows |
| Market margin | 3 / 4 / 6 percentage points for 7d / 14d / 30d |
| Stopped falling | latest close above the window low; low at least 2 trading days old |
| Calibration final close | a close counts as final only if its `fetched_at` UTC date is after its price date |
| Calibration sufficiency | per window: ≥ 100 dips and ≥ 30 flagged dips from ≥ 8 instruments over ≥ 26 distinct weeks |

Dispersion is computed deterministically (exact decimals, or `f64` only for the square
root with the result rounded before comparison); no new dependency is added.

### Verdict rules

Inputs for a row that fell, after the checks above:

- **Price evidence** is *present* when unusual drop = yes or off its high = deep; *absent*
  when unusual drop = no and off its high = not deep; otherwise *undetermined*.
- **Negative evidence**: report in the period = yes, or business = weakening (including a
  loss-making company whose sales shrink or whose losses deepen).
- **Company side passes**: report in the period = no and business = intact or
  `loss_making`; or both company checks are not applicable (fund, ETF, certificate, other
  non-company).

Rules, first match wins:

| # | Condition | Verdict |
|---|---|---|
| 1 | Price evidence absent | Normal movement |
| 2 | Negative evidence | Fell for a reason? |
| 3 | Price evidence present and company side passes | Sentiment-dip candidate |
| 4 | Price evidence present, business = intact or `loss_making`, and report in the period = unknown | Possible candidate, report timing unknown |
| 5 | Anything else | Not enough data |

Consequences of the table, all covered by an exhaustive test:

- A normal-sized fall is Normal movement even when a report landed: the report did not move
  the price unusually. Negative evidence beats positive evidence everywhere else.
- Undetermined price evidence with negative evidence gives Fell for a reason?; without it,
  Not enough data.
- Unknown report timing alone, with price evidence present and a business that is intact
  or loss-making, gives Possible candidate, report timing unknown (owner decision
  2026-11-11); it carries the same cautions as a candidate. Unknown business, or unknown
  report timing together with undetermined price evidence, gives Not enough data, which
  lists the missing checks ("Not enough data: business unknown, less than a year of
  prices").
- Unknown security type or no company-figures source makes both company checks unknown, so
  such rows can only be Normal movement or Not enough data: no negative evidence can be
  known for them, and no candidate can be confirmed.
- The market check, stopped falling, the average-price comparison and the target gap never
  change the verdict.
- Each verdict carries its **deciding checks** (the checks that matched its rule) so the
  row can say why in one line, and a candidate carries **cautions**: loss-making, report
  coming soon (with its certainty label), already at target, no target. Cautions never
  change the verdict or the order.

### Ordering

"Best candidates first": Sentiment-dip candidate, then Possible candidate, report timing
unknown, then Not enough data, then Fell for a reason?, then Normal movement;
within a tier by the displayed change, biggest fall first; ties by symbol then exchange.
Rows without a verdict follow: rows that did not fall in fallers order, then rows with an
unavailable change. The backend supplies the verdict code; the view-model owns the tier
order and sorts, as it does today.

**One-time switch for existing installs.** The order moves to a new storage key
`opportunities.ordering`. On load, when the new key is absent, the retired
`opportunities.order` is read once: a stored "Biggest risers" is carried over, because the
owner chose it deliberately; a stored "Biggest fallers" (indistinguishable from the old
default, which the page wrote on every visit) and anything else land on "Best candidates
first". The retired key is then removed. Afterwards the owner's choice persists under the
new key as usual.

### Presentation

The owner must see why without opening anything. The table gains one **Verdict** column
after Instrument: the verdict chip, a one-line "because" sentence built from the deciding
checks, and caution chips. Beneath each row that has a verdict, a full-width **evidence
line** spanning all columns lists every check as "Name: result" in a fixed order. Text
follows the visual guide's contrast rules, since this is the evidence the owner reads:
the "because" sentence and check results use Body-sm (13px) in `--text-secondary`, with
figures in mono; only the check names may use `--text-muted` as labels. Unknowns show
their reason in the same readable text; not applicable is italic "n/a"; approximate values
keep the amber "~" prefix and `--warning` text with their explanation in the title and
accessible label, matching the company-figure certainty markers. Rows without a verdict
stay single-line. Proposed chip styling, to be approved by eye and then recorded in
`docs/VisualDesign.DarkTheme.md`: candidate uses the reserved `--up-soft` alert chip with
`--up` text; Fell for a reason? uses `--warning-soft` with `--warning` text; Normal
movement and Not enough data use the neutral status chip; cautions are neutral compact
chips.

### Data prerequisites

- **A year of daily history.** The latest refresh tops up, automatically and once per need,
  the daily history of each held instrument (quantity above zero) whose look-back series
  is short, has a gap beyond the gap limit, or holds closes fetched before a split inside
  the look-back. It runs after company figures in latest-mode runs, one request per needing
  mapping covering the look-back year plus the longest window, at most once per mapping per
  local day, and is not repeated for an unchanged need once a successful attempt has
  covered it (for example a recently listed share whose provider has no older closes).
  Failures are logged with instrument, provider, symbol and range, reported in the refresh
  response as their own item kind, and never change the price/FX status or its counts. A
  provider rate limit stops further top-ups in that run. Writes use the refresh lease.
  The 37-day latest window and its decision-log entry are unchanged. Added history can
  restate portfolio aggregates as a correction under the 2026-08-29 backfill entry. The
  top-up fetches prices only; exchange-rate holes are not topped up.
- **Dividend events.** Parsed from the existing Yahoo chart response in every latest
  refresh, backfill and top-up; stored with a per-mapping coverage record in the same
  fenced write as the closes; events inside a re-fetched range are replaced, not merely
  added. Nasdaq Nordic returns none and records no coverage. **Failure isolation:** the
  provider response carries the closes and a separately validated dividend part. Malformed
  dividend data (missing or unparsable date or amount, non-positive amount, missing
  currency) never fails the closes: the price item still succeeds and its rows are written
  and counted as today. Invalid dividend data neither replaces stored events nor extends
  coverage, so the ex-dividend check stays explicitly `unknown` for that range. The problem
  is logged with instrument, provider, symbol, range and reason and reported as its own
  refresh item kind, never in the price/FX status or counts.
- **Market index series** (last phases). Stored in its own table, never as an instrument:
  latest refresh skips zero-quantity instruments without a conviction, backfill skips
  instruments without transactions, and a hidden instrument would leak into instrument
  lists. The listing market comes from the enabled price source: Yahoo symbol suffix
  (`.ST` Stockholm, `.OL`, `.CO`, `.HE`, other European suffixes, no suffix as US), then a
  Nasdaq Nordic mapping's currency, then the instrument currency. The instrument's exchange
  field is not used because broker imports can carry a broker tag there.

### API shape (additive)

Each row of `GET /api/opportunities` gains `evidence`, `null` unless the row fell:

```json
"evidence": {
  "verdict": { "code": "sentiment_dip_candidate", "deciding": ["unusual_drop", "report_in_period", "business"],
               "missing": [], "cautions": ["loss_making", "report_coming_soon"] },
  "checks": {
    "unusual_drop":    { "state": "yes", "reasons": [], "change_percent": "-9.12", "adjusted_change_percent": "-8.01", "usual_move_percent": "4.20", "closes": 251 },
    "off_high":        { "state": "deep", "...": "..." },
    "report_in_period":{ "state": "no", "basis": "...", "...": "..." },
    "report_coming_soon": { "...": "..." },
    "business":        { "state": "loss_making", "eps_consistency": "consistent", "...": "..." },
    "market":          { "state": "fell_more_than_market", "source": "other_holdings_median", "reference_percent": "-1.10", "count": 14 },
    "stopped_falling": { "...": "..." },
    "ex_dividend":     { "...": "..." },
    "room_to_buy":     { "...": "..." }
  }
}
```

Exact field names are the implementer's within this shape; percentages serialize with two
decimals like `change_percent`, and every unknown carries reason codes. Existing fields and
the `data_revision` contract are unchanged.

## Module layout

- `backend/src/domain/opportunity_evidence/` (pure, no I/O): `parameters.rs`,
  `lookback_series.rs` (single-source preparation, gap, split, short-history and top-up
  need), `price_checks.rs`, `report_timing.rs`, `business_check.rs`, `dividend_check.rs`,
  `market_reference.rs`, `verdict.rs`.
- `backend/src/domain/company_figures.rs`: the reusable EPS consistency evaluation and its
  separately reported outcome.
- `backend/src/api/opportunities.rs` stays the thin handler; tests move to
  `api/opportunities/tests.rs` and loading to `api/opportunities/evidence.rs`.
- `backend/src/market_data/history_topup.rs`, dividend parsing in the Yahoo provider and a
  dividend repository, later `backend/src/market_data/market_index_refresh.rs` and a
  market-index repository.
- Tools: `backend/examples/opportunity_data_readiness.rs`,
  `backend/examples/dip_check_replay.rs` (examples are tooling, precedent
  `sharesight_import_spike.rs`).
- Frontend: `opportunitiesViewModel.ts` grows (or splits into an evidence view-model
  module), `OpportunitiesPage.tsx` renders only.

## Phases

Verification commands run from the repository root unless noted; frontend commands run
from `frontend/`. "Backend checks" means `cargo build -p ticker-tape-tally-board-backend`,
`cargo test -p ticker-tape-tally-board-backend`, then
`cargo clippy -p ticker-tape-tally-board-backend --all-targets -- -D warnings` and
`cargo fmt`. "Frontend checks" means the relevant `npx vitest run <files>`, `npm run check`
and `npm run fmt`. `npm run test:browser` runs from Claude, not the Codex sandbox. No phase
touches `desktop/` or the workspace manifest except the version field; when the version
changes, widen clippy to `cargo clippy --workspace --all-targets -- -D warnings`.

**Ledger-copy procedure** (Phases 1 and 5): copy the newest launch backup snapshot into the
session scratch directory outside the repository, never open the production ledger, and
delete every copy afterwards. Measurement and replay tools open their input only through a
read-only SQLite connection; they refuse a path inside the repository working tree and
refuse to open without read-only mode. The one deliberate write is Phase 5's preparation
step, which runs a latest refresh against a second, scratch copy (never the original
snapshot or the production ledger) so the history top-up fills it before the replay reads
it read-only. Committed findings are aggregate counts and percentages only: no symbols,
names, ISINs, quantities, prices, amounts or trade dates; medians and shares are reported
only for groups of at least 5.

### Phase 1 — Measure how often the evidence is knowable (spike)

Tool written by the implementation pipeline; run by the session lead on a ledger copy.

1. Add `backend/examples/opportunity_data_readiness.rs`. It opens a given ledger read-only,
   reuses the existing repositories and `report_dates` functions so counts match the app,
   and prints aggregates only. A `--demo` mode seeds an in-memory demo ledger as a smoke
   test.
2. Measure, as of the run date:
   - R1 held rows by classification (company, fund/ETF, certificate, unknown or no source)
     and by listing market as derived above;
   - R2 last-report knowledge for companies: confirmed, observed ≤ 7 days, observed wider,
     figures without an absence baseline, unavailable; US and non-US separately;
   - R3 report-in-the-period outcomes for 7d, 14d and 30d under the strict and relaxed
     rules: yes / no / unknown with the unknown reason mix;
   - R4 next-report basis mix and days since the last successful figures fetch (0-1, 2-7,
     over 7);
   - R5 business check readiness: intact / weakening / loss-making only / unknown by reason,
     including how many companies the EPS consistency rule would mark approximate or
     unknown;
   - R6 price history readiness: single-source closes in the look-back year (≥200 with no
     gap, 120-199, under 120), gaps over the limit, splits in the look-back, source changes
     within the year, and how many rows the top-up would fix;
   - R7 Yahoo-priced versus Nasdaq-only rows (ex-dividend knowability);
   - R8 rows with an available change per window (the median needs 6 or more);
   - R9 buy-more trades (buys while already held) per year, and the share of stored closes
     that meet the final-close rule, to size the calibration sample.
3. Write the "Data readiness" section of `docs/spikes/OpportunityChecksSpike.md` (method,
   numbers, what they imply for the rules) and the initial parameters table.
4. Apply the data-driven "no report" rule from R3 and record which rule is used, in the
   spike and the parameters table. If even the relaxed rule leaves more than half of the
   company rows unknown in the 30-day window, put the fifth-verdict proposal (Open
   Question 1) to the owner before Phase 3.

Verification: backend checks; `cargo run -p ticker-tape-tally-board-backend --example
opportunity_data_readiness -- --demo` prints counts; the session lead reviews the spike
section against the sanitization rules. Human testing: none, unless the fifth-verdict
question is triggered.

### Phase 2 — Price evidence as pure domain functions

Implementation pipeline. No API or UI change.

1. Create `domain/opportunity_evidence/` with `parameters.rs`, `lookback_series.rs`,
   `price_checks.rs` (unusual drop, off its high and near low, stopped falling) and
   `market_reference.rs` (median of other rows with its minimum count; an index source
   variant is added later). Outcomes carry state, reason codes and values.
2. The parameters/spike-table consistency test (reads the spike document's table with
   `include_str!`).

Tests (domain unit tests): every state and reason of every check; boundaries at exactly
−1.5 standard deviations, 119/120 and 199/200 closes, 5 versus 6 weekday gaps (including
Easter- and Christmas-shaped gaps, which must pass), a split before and after the closes'
fetch time, a Nasdaq-only series across a split, a source change inside the look-back, a
currency mismatch, a low 1 versus 2 trading days old, 4 versus 5 other rows, odd and even
medians, and the sample excluding the current window.

Verification: backend checks. Human testing: none.

### Phase 3 — Company-side checks and the verdict rules as pure domain functions

Implementation pipeline. No API or UI change. Starts once Phase 1 has fixed the "no report"
rule (and, if triggered, the owner has answered Open Question 1).

1. Refactor the company-figures EPS consistency check into a reusable evaluation over a set
   of quarters and expose its outcome separately; the trailing P/E and P/S results stay
   identical (existing company-figures tests pass unchanged).
2. `report_timing.rs` (report in the period with the chosen rule, report coming soon),
   `business_check.rs` (sub-tests, EPS pair consistency, `loss_making`), `dividend_check.rs`
   (given events and coverage), room to buy from the target status, and `verdict.rs`
   (rules, deciding checks, missing checks, cautions including loss-making).
3. The verdict function takes check outcomes only and has no market-check input.

Tests: an exhaustive truth-table test enumerating every combination of price evidence
(present/absent/undetermined), report in the period (yes/no/unknown/n.a.), business
(intact/weakening/loss-making/unknown/n.a.) against a table written out in the test,
including a loss-making candidate carrying the caution and loss-making with shrinking sales
or deepening losses giving Fell for a reason?; report-timing cases for confirmed, observed
narrow, observed partial overlap, no absence baseline, scheduled confirmed passed without
figures, Nasdaq-expected passed, app estimate passed, next quarter not ended, fresh versus
stale figures check under both rules, a date that is today; business cases for each
sub-test, the out-of-date marker, a passed but not yet overdue expected date, approximate
figures, currency change; EPS consistency cases: consistent pair, moderate disagreement
(approx. with gap), severe disagreement (unknown), a pair without net income or shares
("not cross-checked"), and a company whose P/E is unavailable for an unrelated reason
(loss-making, share basis unverified) whose business check is still evaluated; ex-dividend
inside, at the edges, outside coverage, and in a range whose capture failed validation.

Verification: backend checks. Human testing: none.

### Phase 4 — The latest refresh secures a year of daily history

Implementation pipeline; a live check by the session lead is recommended. Placed before the
calibration so the replay can run on topped-up history.

1. Migration `0012` (new file, LF line endings): a per-mapping record of history top-up
   attempts (requested range, attempted date, outcome, earliest returned date).
2. `market_data/history_topup.rs`: need detection reuses `lookback_series` (short, gap,
   split-stale); runs after company figures in latest mode; one request per needing mapping;
   fenced writes; once per need and at most once per mapping per local day; rate limit stops
   the pass; response item kind for history; logging through `engine_logging`.
3. Update the refresh response type in `frontend/src/api/types.ts` for the new item kind.

Tests (refresh tests with fake providers): each need type triggers one request with the
expected range; an unchanged need after success is not retried; a new gap re-triggers; a
failure, rate limit or not-listed answer leaves the run status, `prices_written` and
`failed_items` exactly as without the top-up; lease loss stops it; backfill mode never
runs it; a zero-quantity instrument is never topped up.

Verification: backend checks; `npm run check` for the type change. Bump the backend
version. Human testing (session lead, recommended): run the development loop against a
scratch ledger copy with a live latest refresh; confirm in `engine.log` one top-up per
needing instrument, a second refresh making no top-up requests, and the refresh status
unchanged. This is also how Phase 5 prepares its replay copy.

### Phase 5 — Calibration replay (spike)

Method commit and tool by the implementation pipeline; run by the session lead. The
verdicts are not shown to the owner before this phase ends with the outcome "Calibrated".

1. **Pre-register** the method in the spike document's "Calibration" section and commit it
   before any run:
   - **Copy preparation:** a fresh snapshot copy is duplicated into a scratch working copy;
     the session lead runs one live latest refresh against the working copy in the
     development loop (filling it through the history top-up); the replay then opens the
     working copy read-only. Both copies are deleted afterwards. The owner may optionally
     run a backfill from the Import page beforehand, which deepens the sample.
   - **Final closes only.** Stored closes can keep intraday values indefinitely (the
     2026-11-06 entry; the price upsert overwrites close and `fetched_at` without a finality
     flag). A close counts as final only when the UTC calendar date of its `fetched_at` is
     after its price date. Every listing market the app holds ends its session before
     midnight UTC on its own date, and a later re-fetch only moves `fetched_at` later, so
     this needs no new stored field and is conservative (a final close fetched the same
     evening is excluded until a later refresh revisits it). Non-final closes are treated
     as missing, so the gap rule applies to them.
   - **Universe:** instrument-dates on which the ledger held the instrument and its
     look-back series, built from final closes only, passes the gap, split and same-source
     rules as of that date (closes after the date unseen).
   - **Forward horizon rules.** A forward return from the replay date's close to the close
     h trading days later (h = 5, 10, 20) is usable only if both closes are final, come
     from the same source as the look-back series, are in the instrument's currency, no
     gap over the gap limit lies inside the horizon, and no ledger or provider split falls
     after the start close and on or before the end close unless every horizon close was
     fetched after that split from Yahoo. A horizon that ends after the latest final close
     is **incomplete** and excluded. Each exclusion is counted by reason (non-final,
     source, currency, gap, split, incomplete).
   - **Benchmark rules.** The excess return subtracts the same-date median forward return
     of the other held instruments whose own forward return over the same horizon is
     usable under the same rules; at least 5 are needed, otherwise the event has no excess
     return and is counted as `benchmark_unavailable`.
   - Replay dates every fifth weekday; windows 7d, 14d and 30d separately.
   - Groups: unconditional (all held instrument-dates), all dips (fell), flagged dips
     (price evidence present), each price check alone, and flagged dips split by stopped
     falling.
   - Outcome: forward excess return over 5, 10 and 20 trading days, primarily 20, reported
     as count, distinct instruments, distinct weeks, median, share positive, 10th
     percentile.
   - Caveats stated up front: overlapping forward windows (weekly sampling with a 20-day
     horizon still overlaps four-fold), cross-sectional correlation, small samples, no
     significance claimed; survivor effects from instruments added later.
   - **Sufficiency and named outcome.** The replay ends in one of two outcomes. *Calibrated*:
     for each window, at least 100 usable dips and at least 30 usable flagged dips from at
     least 8 instruments over at least 26 distinct weeks. *Insufficient history*: anything
     less, including an empty replay. Insufficient history does **not** satisfy the
     calibration prerequisite: no threshold changes, the initial thresholds stay, the spike
     records the shortfall, and the verdict UI phases do not start. The session lead then
     asks the owner to run a backfill (a routine action on the Import page) and the replay
     is repeated on a fresh copy; if it is still insufficient, Open Question 2 goes to the
     owner.
   - Change criterion (only after Calibrated): a grid value replaces the initial one only if
     its flagged group has at least 30 events from at least 8 instruments, its median
     20-day excess return beats the initial value's by at least 1 percentage point, and the
     improvement has the same sign in both halves of the period. Grids: unusual drop
     1.0/1.5/2.0/2.5 standard deviations; deep below high 10/15/20/25/30%; stopped-falling
     age 1/2/3 days.
   - Drop criterion: if a price check's flagged group does no better than all dips at every
     grid value in both halves, propose removing it from the verdict (it stays displayed);
     a removal is put to the owner before it is applied.
   - Buy-more replay: for each buy made while already holding, the price-side verdict as of
     the trade date in each window: how many were flagged and their forward excess returns
     against unflagged buy-mores (aggregates only, groups of at least 5).
   - Not replayable, stated plainly: report in the period (no historical report dates;
     past Nasdaq calendar backfill was rejected on 2026-11-09); company checks, because
     stored figures are not point-in-time and history only starts with the app's first
     fetch in November 2026, so an as-of year-ago comparison exists for almost no replay
     date (the count is reported); ex-dividend before dividend events are captured;
     intraday prices.
2. Add `backend/examples/dip_check_replay.rs` using the Phase 2 and 3 domain functions,
   with a `--demo` smoke mode. Pure helpers it needs (final-close filter, forward-horizon
   eligibility, benchmark median, sufficiency outcome) live in a small tested module the
   example uses, so the rules above are unit-tested rather than only exercised by a run.
3. Session lead prepares the copy, runs the replay, writes results and the outcome, and,
   if Calibrated, applies any threshold change the criterion allows to `parameters.rs`, the
   spike table and affected tests in the same commit.
4. Finalize the wording of decision-log entry A with the calibrated parameters.

Tests: the final-close filter (fetched the same UTC day, the next day, re-fetched later);
each forward-horizon exclusion reason; incomplete horizon; benchmark with 4 versus 5 usable
others; the sufficiency outcome at and below each floor, and an empty replay giving
Insufficient history.

Verification: backend checks; `--demo` smoke run (demo history is long enough to reach the
Calibrated path in smoke mode; a truncated `--demo` variant reaches Insufficient history);
sanitization review of the spike document. Human testing: none required; the owner may read
the spike summary.

### Phase 6 — Dividend events from the price source

Implementation pipeline; the session lead records the fixture.

1. Session lead records a public Yahoo chart response with dividend events for a public
   dividend payer in the app's request shape (live network) as a test fixture.
2. Parse dividend events (ex-date via `gmtoffset`, amount, currency) in the Yahoo chart
   client as a separately validated part of the response; the price-provider boundary
   returns closes plus that dividend part (Nasdaq Nordic and fakes return none). A dividend
   parse problem is never a price-fetch error.
3. Migration `0013`: dividend events per instrument and provider, and per-mapping coverage;
   valid dividend data is written in the same fenced write as the closes, with events
   within the re-fetched range replaced; invalid dividend data writes nothing to events or
   coverage, is logged with context, and is reported as its own refresh item kind.

Tests: fixture parse; refresh stores events and coverage for latest, backfill and top-up;
a re-fetch that no longer lists an event removes it; a failed fetch changes neither events
nor coverage; Nasdaq-only instruments get no coverage; **regression: valid closes with a
malformed dividend event** (missing amount, unparsable date) still give a fetched price
item with its rows written and counted, an unchanged run status and counts, unchanged
stored events and coverage, a reported dividend item, and an ex-dividend check that stays
`unknown` for that range; the dividend check reads coverage correctly from stored data.

Verification: backend checks; `npm run check` if the refresh types change; bump the backend
version. Human testing: none.

### Phase 7 — The Opportunities API carries evidence and a verdict

Implementation pipeline.

1. Move-only commit: split `api/opportunities.rs` tests into `api/opportunities/tests.rs`.
2. `api/opportunities/evidence.rs` loads, per fell row, the single-source look-back rows
   with fetch times, ledger and provider splits, company-figures inputs and report
   observations and announcements (extend the shared fundamentals loading so this path
   gets figures, the separate EPS consistency outcome, next report, last-report inputs and
   source state in one pass without the stock-page source panel), dividend events and
   coverage, and the target status; computes the median reference from the rows' changes;
   calls the domain. Load failures are logged with instrument id, exchange, symbol, window
   and operation.
3. Serialize `evidence` per row as in the API shape.

Tests (public-contract API tests): one fixture ledger reaching each verdict, including a
loss-making candidate with its caution; representative unknown reasons (short history, gap,
split, no source, out-of-date figures, severe EPS inconsistency); a fund judged on price
checks alone; a riser and an unavailable change with `evidence: null`; the median excluding
the row itself and requiring 5 others; all existing opportunities tests pass unchanged; the
demo test still finds an available change for every held row in every window and now also
requires `evidence` for every row that fell.

Verification: backend checks; update `docs/Design.HighLevel.md` (API surface and the
evidence block); bump the backend version. Human testing: none (no UI yet).

### Phase 8 — Demo reaches every verdict and every check state

Implementation pipeline.

1. Reshape demo price paths, defined relative to today so they are stable from day to day,
   so that across the three windows every verdict and every state of every check appears:
   unusual drops, deep falls, near-low, turned-up and still-falling lows, a short or holed
   look-back, a split-stale series if practical; company cases for a report in the period
   (a passed confirmed date without figures and an observed interval), report coming soon
   with each certainty label, weakening sales, sliding EPS, **a loss-making
   Sentiment-dip candidate showing the "loss-making" caution** (the existing loss-making
   demo company can carry it), a loss-making company with shrinking sales giving Fell for a
   reason?, out-of-date figures, a moderate and a severe EPS disagreement; dividend events
   with and without coverage; targets below, at and without. Add a demo instrument where
   the seven existing ones cannot carry all cases (for example a recently listed share for
   short history).
2. Update every test that pins demo totals or history, deliberately and with the new values.

Tests: a demo coverage test asserting every verdict, every check state and every caution
(including loss-making on a candidate) appears for seven consecutive values of today
(weekday alignment changes the trading-day checks); the existing demo tests updated.

Verification: backend checks; bump the backend version. Human testing: none (seen in
Phase 9). Propose decision-log entry D.

### Phase 9 — The page shows the verdict and orders best candidates first

Implementation pipeline; by-eye approval by the owner. Starts only after Phase 5 ended
Calibrated.

1. Types for `evidence`; `OPPORTUNITY_ORDERS` gains `best_candidates_first` as default; new
   storage key `opportunities.ordering` with the one-time carry-over described under
   Ordering.
2. View-model: verdict label and tone, deciding-checks sentence, missing-checks sentence,
   caution labels (loss-making, report coming soon with its certainty label, already at
   target, no target), tier ordering and tie rules.
3. Component: Verdict column, chip, sentence and caution chips; order control with three
   options. Styles use tokens only and the contrast rules above.
4. The page footnote states, in readable text, that a verdict describes the dip (how
   unusual the fall is and what is known about the company) and does not predict the price,
   and that a replay of stored price history found no measurable difference in the
   following month between flagged and other dips. The wording is updated when a later
   replay changes that finding. It lands with the first slice that shows a verdict, never
   after it.

Tests: a component test by text that the note is present whenever a verdict is shown;
reducer and persistence tests: the default is best candidates first; with no new
key, a stored `"fallers_first"` under the old key loads best candidates first, and a stored
`"risers_first"` loads risers first, and in both cases the new key is written and the old
key removed; malformed or unknown old values land on best candidates first; once the new
key exists the old key is ignored; the new key round-trips each of the three orders;
malformed and unknown new-key values fall back to best candidates first. View-model tests
for tier order, change ordering inside a tier, ties, rows without a verdict, every verdict
label and caution. A component test by role and text for the three order options.
`npm run test:browser` extended with the verdict chip colours and the "because" sentence
colour against their tokens.

Verification: frontend checks; `npm run test:browser` from Claude; bump the frontend
version. **By-eye (owner, demo mode):** the page opens on "Best candidates first"; each
verdict appears in at least one window; the candidate and "Fell for a reason?" chips read
as intended and the neutral ones stay calm; the note that a verdict does not predict the
price is easy to find and reads as honest; the "because" sentence is easy to read; cautions
are visible on candidates, including "loss-making"; switching order and window and
reloading keeps the choice. Then on his own release app after updating: the page lands once
on "Best candidates first" if it had stored "Biggest fallers" (or stays on "Biggest risers"
if he had chosen that), and his next choice sticks.

### Phase 10 — Every check is readable on the row

Implementation pipeline; by-eye approval by the owner.

1. View-model sentences for every check state and reason code, with values formatted by
   the existing precision rules (percentages two decimals); approximation markers and their
   explanations (including "EPS not cross-checked" and EPS disagreement gaps); market-check
   tooltip naming the median's weaknesses; not-applicable as italic "n/a".
2. Component: the full-width evidence line beneath each row with a verdict, readable text
   per Presentation; footnote updated (ex-dividend days are detected where the price
   source reports dividends; the latest price can be intraday during market hours).
3. Record the approved Opportunities presentation in `docs/VisualDesign.DarkTheme.md`.

Tests: view-model tests for each check state's sentence and each reason code; a component
test that a verdict row exposes its evidence text; browser test extended for the evidence
line's result colour (`--text-secondary`), approximate markers (`--warning`) and the
not-applicable style.

Verification: frontend checks; `npm run test:browser`; bump the frontend version.
**By-eye (owner, demo mode):** read three rows of each verdict and confirm the "because"
sentence and evidence line explain the verdict without hovering; the evidence is easy to
read, not faint; unknown, approximate and n/a read as honest; the table stays scannable at
his usual window width and at a narrow width.

### Phase 11 — Market index symbols against the app's request shape (live-network spike)

Run by the session lead, not the implementation pipeline. No application code.

1. `scripts/probe-market-index.ps1` requests the Yahoo chart endpoint exactly as the app
   does (symbol as a percent-encoded path segment, `period1`/`period2`, `interval=1d`,
   `events=div,splits`) for candidate symbols per listing market present in R1: Stockholm
   (`^OMXSPI`, `^OMX`), US (`^GSPC`), broad Europe (`^STOXX`), and Oslo, Copenhagen or
   Helsinki indexes if held.
2. Record per symbol: HTTP status, whether closes and `currency` are present, `gmtoffset`
   and date alignment with the market's trading days, history depth, and whether the
   encoded `^` is accepted. Record public fixtures for the chosen symbols.
3. Write `docs/spikes/MarketIndexSpike.md` with the chosen symbol per market; markets
   without a working symbol keep the median.

Verification: the probe output and fixtures reviewed by the session lead. Human testing:
none.

### Phase 12 — Market index series are stored and refreshed

Implementation pipeline.

1. Migration `0014`: market index closes keyed by index code, provider and date, separate
   from instruments.
2. Listing-market derivation and the market-to-index mapping in the domain;
   `market_data/market_index_refresh.rs` fetches each index needed by currently held
   instruments over the latest window, after company figures and the top-up, in latest
   mode only; fenced writes; failures logged and reported as their own item kind, never in
   price/FX status or counts; availability for the page derives from the stored series.
3. Demo seeds offline synthetic index series for its markets.

Tests: mapping tests per suffix, Nasdaq currency and currency fallback; refresh stores
closes from the recorded fixtures; failure isolation; lease loss; backfill never fetches
indexes; demo seeds every needed index.

Verification: backend checks; `npm run check` if the refresh types change; bump the
backend version. Human testing (session lead, recommended): a live latest refresh in the
development loop on a scratch ledger copy stores index closes and leaves the refresh status
unchanged. Propose decision-log entry E.

### Phase 13 — The market check uses the index

Implementation pipeline; by-eye approval by the owner.

1. `market_reference` gains the index source: index closes paired on or before the asset's
   own start and end close dates within the start-close tolerance; otherwise fall back to
   the median with reason `market_index_unavailable`.
2. API carries the source and index name; view-model wording "fell 6.10% more than OMX
   Stockholm" versus "than the median of your other holdings".

Tests: domain pairing and fallback; API contract for both sources; a test that the verdict
is identical under either source; view-model wording; demo coverage now requires both
sources.

Verification: backend checks, frontend checks; bump both versions. **By-eye (owner, demo
mode):** market sentences name the index for indexed markets and the median elsewhere.

## Decision-log entries proposed

The plan proposes these; they are written into `docs/DecisionLog.md` when the owner
approves them (A after Phase 5 ends Calibrated, with calibrated values; B with Phase 4; C
with Phase 6; D with Phase 8; E with Phase 12).

**A. The Opportunities Page Judges Each Dip By Named Evidence Checks And A Rule-Based
Verdict — reverses part of 2026-10-15 The Opportunities Page Measures Recent Share-Price
Change Over Its Own Window**

Decision: For every held asset whose share price fell in the selected window, the
Opportunities page shows named evidence checks (unusual drop, off its high, report in the
period, report coming soon, business intact, moved with or against its market, stopped
falling, went ex-dividend, room to buy), each with a plain-language result and an explicit
unknown or not-applicable state, and one verdict derived from them by fixed rules: a fall
that is neither unusual nor deep below the 1-year high is Normal movement; otherwise a
report in the period or a weakening business (shrinking sales or sliding earnings, including
deepening losses) makes it Fell for a reason?; a Sentiment-dip candidate needs an unusual
drop or a deep fall from its high, no report in the period and a business that is intact or
loss-making without weakening; anything else is Not enough data, naming what is missing.
Funds, ETFs and certificates are judged on the price checks alone. Unknown evidence never
counts as a pass. Loss-making, report coming soon, already at target and no target are
cautions on a candidate and never change its verdict or position. The market check, stopped
falling, the average-price comparison and the target gap are context and never change a
verdict. There is no numeric score. The backend computes every check and the verdict; the
page renders and sorts. The default order is Best candidates first: candidates, then Not
enough data, then Fell for a reason?, then Normal movement, each by the size of the fall,
then the rows without a verdict; Biggest fallers and Biggest risers remain. Long-history
checks use one price source, refuse gaps of more than five weekdays and refuse series not
refetched since a split inside the look-back. A report is in the period when it falls within
the window widened by seven days, judged by confirmed dates and observation intervals; a
period-only report is never shown as a date; "no report" after a quarter's end rests on
<the strict or relaxed rule chosen by the readiness measurement>. The earnings comparison
carries the EPS consistency rule independently of P/E. The thresholds live in the
parameters table of the opportunity checks spike, set by its replay of stored final closes,
and change only through a new entry.
Context: The owner uses the page to buy a little more of holdings that fell on mood rather
than on business news, and ordering by change alone could not tell the two apart. The app
already holds company figures and report dates with certainty states. The owner chose on
2026-11-11 to let loss-making companies that are not weakening be candidates with a
caution, loosening "profitable" in his original description of an intact business.
Consequences: Reverses the 2026-10-15 entry's choice of ordering by change alone over a
two-tier ordering; its backend-resolved period, close-to-close native change, explicit
unavailability, same-source and same-currency rule, split rule, held-only scope, read-only
page and persisted settings stand. Refines its "does not detect" ex-dividend consequence:
the page detects ex-dividend days where the price source reports dividends and judges an
unusual drop with the dividend added back, while the displayed change stays close to
close. Existing installs switch once to the new default order, except where Biggest risers
had been chosen. Report timing cannot be replayed historically and the company checks were
not calibrated, because stored figures are not point-in-time. During market hours "fell"
and "stopped falling" can change before the close. The 2026-11-11 replay of stored price
history had enough data to judge the thresholds and kept them, but found no measurable
difference over the following 20 trading days between flagged and other dips; the owner
chose on 2026-11-12 to show the verdicts with a visible note that a verdict describes the
dip and does not predict the price, and the replay is repeated as history grows.

**B. The Latest Refresh Tops Up A Year Of Daily History For Held Instruments — refines
2026-10-15 The Latest Price Refresh Covers The Longest Recent-Change Window**

Decision: A latest-mode refresh, after company figures, fetches a year of daily closes plus
the longest recent-change window for each held instrument's price mapping whose stored
series is short, has a gap of more than five weekdays, or predates a split inside that year.
It is attempted at most once per mapping per local day and not repeated for an unchanged
need once a successful attempt covered it. Its failures are logged and reported as their own
refresh items and never change the price/FX status or its counts.
Context: The Opportunities checks need about a year of history, which only exists where the
owner hand-ran a backfill, and lengthening every latest refresh to thirteen months was
rejected.
Consequences: The 37-day latest window stays. The first launch after the change makes one
longer request per affected instrument; later launches rarely do. Added history can restate
portfolio aggregates as a correction under the 2026-08-29 backfill entry. Exchange-rate
history is not topped up.

**C. Dividend Events Come From The Price Source's Chart Response — refines 2026-10-15 The
Opportunities Page Measures Recent Share-Price Change Over Its Own Window**

Decision: The Yahoo chart response's dividend events are validated separately from its
closes and stored per instrument with a coverage record per mapping, written with the closes
they arrived with; a re-fetched range replaces its events. Invalid dividend data never fails
the closes, never replaces stored events and never extends coverage; the failed range is
remembered, so the ex-dividend check is unknown there until a later valid capture covers
it; it is reported as its own refresh item outside the price/FX status. An ex-dividend date
counts as known only inside covered ranges, which end at the last day a price was actually
received. Ledger dividends are not used as ex-dates.
Context: The page could not tell an ex-dividend drop from a fall. The request already asked
for dividend events; ledger dividends carry the payment date and arrive only with imports.
Consequences: Nasdaq Nordic-only instruments have no known ex-dividend dates. No extra
requests are made.

**D. Demo Data Exercises Opportunity Evidence States — refines 2026-11-08 Demo Data
Exercises Company-Figure Certainty States**

Decision: The demo seed reaches every Opportunities verdict, every state of every evidence
check and every caution across its three windows, using price paths defined relative to
today, synthetic dividend events and, once indexes exist, synthetic index series.
Context: The new states are rare in any one ledger, and the demo is where their display is
checked.
Consequences: Unlike the company-figure cases, these reshape demo prices (and may add a demo
instrument), so the demo portfolio's totals and history change. Demo stays ephemeral,
read-only and offline.

**E. Market Indexes Are Stored Apart From Instruments** (wording finalized after the index
spike)

Decision: Market index closes from the existing price source are stored in their own table,
fetched by latest refreshes for the markets of currently held instruments, and never
represented as instruments. An instrument's market comes from its price source's listing.
Failures never change the price/FX status. When an index is available for an asset's
market, it replaces the median of other holdings as the reference of the market check,
which remains context and never changes a verdict.
Context: The page should say how much more a holding fell than its market.
Consequences: Index symbols fall under the 2026-06-13 terms re-check before a distributed
installer. Markets without a working symbol keep the median.

## Documents to update

- `docs/DecisionLog.md`: entries A-E as above, when approved.
- `docs/Design.HighLevel.md`: Opportunities API and evidence block (Phase 7); history
  top-up, dividend events and index refresh in the market-data section (Phases 4, 6, 12);
  the separately reported EPS consistency outcome in the company-figures section (Phase 3);
  market check source (Phase 13).
- `docs/VisualDesign.DarkTheme.md`: Opportunities table section (Phases 9 and 10).
- `docs/spikes/OpportunityChecksSpike.md` (new, Phases 1 and 5) and
  `docs/spikes/MarketIndexSpike.md` (new, Phase 11).
- `docs/TODO.md`: the future candidates below, at completion.
- Versions: workspace `[workspace.package].version` and `frontend/package.json` as noted per
  phase.

## Out of scope

- The watchlist: assets followed but not held are not listed, not even behind a switch.
- Future candidates, listed for `docs/TODO.md` and not designed: trading volume,
  sector/industry, cheaper than its own usual valuation (historic P/E or P/S), analyst
  targets, short interest, insider buying, news headlines, any automated reading of news.
- Paid data services and new third-party components (crates or npm packages).
- Storing daily high, low or volume, or a price finality flag; intraday handling beyond the
  stated note.
- Changing how P/E and P/S use the EPS consistency check.
- Backfilling past Nasdaq calendar days for report history (rejected 2026-11-09).
- Topping up exchange-rate history; changing the 37-day latest window.
- Any numeric score, alerts or notifications.

## Proposal for the owner: longer horizons and repeat replays

Status: **approved by the owner on 2026-11-12, not built.** Written on 2026-11-12 at the
orchestrator's request after the owner said that 20 trading days may be too short and that
more evaluations of this kind may be needed. The owner chose option (b) of Open Question 3:
later replays also measure 60 and 120 trading days, and **60 trading days becomes the
deciding horizon once it passes its floors**, with 20 and 120 reported beside it; the
cadence below was put to him with the question. The "further evaluations" at the end are
listed only and not approved. The replay of 2026-11-11 and its recorded results are not
changed by anything here.

Work this creates, before the next replay and not before: extend the replay tool with the
two horizons and the deciding-horizon rule, with tests; write the change into the spike
document's pre-registered method and commit it before any run; until 60 days passes its
floors in a replay, no threshold changes at all (the 20-day result no longer decides).
Recorded in `docs/TODO.md` when this plan completes.

**What is proposed.**

- Later replays also measure what happened **60 and 120 trading days** after each dip
  (about three and six months), next to the existing 5, 10 and 20.
- The addition is written into the spike document's pre-registered method and committed
  **before** the next real run, with the same rules as today: the same universe, the same
  benchmark (the median of the other holdings over the same horizon, at least five), the
  same exclusion reasons, the same suppression floors, and the same sufficiency floors
  applied **per horizon**. A horizon that does not pass its floors is reported as counts
  only and can support no conclusion.
- No result for 60 or 120 days is looked at before that method is committed.

**How many dips each horizon leaves on today's history. These are estimates, not
measurements.** The replay tool has no 60- or 120-day horizon yet and the scratch copy is
deleted, so nothing was counted. The estimate rests on the recorded 20-day figures: about
30 weekly replay dates carry a usable 20-day outcome, each with roughly 11 to 12 usable
dips, and the usable history is about a year because only the automatic top-up year is
stored for most holdings. A longer horizon removes the most recent weekly dates: 8 more
at 60 days, 20 more at 120 days.

| Horizon | Distinct weeks behind the flagged dips | Usable dips per window | Usable flagged dips | Passes the floors (100 dips, 30 flagged, 26 weeks) |
|---|---|---|---|---|
| 20 days (measured) | 28 to 32 | 324 to 357 | 106 to 110 | Yes, the weeks floor narrowly |
| 60 days (estimate) | about 20 to 24 | about 230 to 270 | about 75 to 85 | No: short of the 26 weeks |
| 120 days (estimate) | about 8 to 12 | about 90 to 140 | about 30 to 45 | No: far short of the 26 weeks |

The exact counts come out of the next replay itself, since counts are always printed.
Without a backfill, 60 days reaches today's 20-day position around early January 2027 and
120 days around the end of March 2027, simply by time passing. A backfill from the Import
page adds older history, whose long horizons are already complete, and so helps at once.

**Limits that grow with the horizon, stated before any result.** With weekly sampling a
60-day outcome overlaps about twelve-fold and a 120-day outcome about twenty-four-fold, so
the number of independent observations is far smaller than the event counts suggest.
Holdings bought or sold inside the horizon, splits and source changes exclude more events.
No significance is claimed at any horizon.

**Which horizon decides a threshold change (owner question, see Open Question 3).** Today
only the 20-day outcome can change a threshold. Choosing another deciding horizon after a
replay showed nothing at 20 days is a second look at the same data; it is fair only if the
choice is fixed before any 60- or 120-day result exists.

**Re-run cadence.** Each replay is a new run on a fresh scratch copy under the same
conditions as the first (production ledger never opened, aggregates only, copies deleted),
recorded as a new dated replay, with at most one threshold change per replay. Proposed:
the next replay not before mid-January 2027 (when 60 days is expected to pass the floors),
then one per quarter. More frequent runs add little new data and raise the chance of acting
on noise. The page note about the replay finding is updated after each run.

**Further evaluations, listed and not designed.**

- **Record the verdicts going forward.** Report timing and the business check cannot be
  replayed historically, so the full verdict has never been evaluated. Storing each day's
  verdict per holding from now on would let a later replay judge the whole verdict, not
  only the price side. This needs new stored data and its own plan.
- **Stopped falling as evidence.** Flagged dips that had already stopped falling did
  better at 7 and 14 days in the first replay. Testing it as a pre-stated hypothesis in the
  next replay is fair; promoting it into the verdict on the first replay's numbers is not.
- **The company side once it becomes point-in-time**, from about November 2027, when a
  year of the app's own observed figures exists.

## Open Questions

Questions 1 and 3 are answered. Question 2 is conditional and goes to the owner only if
its trigger occurs.

1. **ANSWERED 2026-11-11: the owner chose (b), add the fifth verdict.** Original question:
   **Fifth verdict if report timing is mostly unknowable** (trigger: Phase 1 shows that even
   the relaxed "no report" rule leaves more than half of the held company rows unknown in
   the 30-day window). Options: (a) keep four verdicts, so most company dips show "Not
   enough data: report timing unknown"; (b) add "Possible candidate, report timing unknown"
   between Sentiment-dip candidate and Not enough data, for rows that would be candidates
   except that report timing is unknown. Under (a) the owner sees few candidates among
   companies; under (b) he sees a second, clearly labelled tier. Cheap to change (one rule
   row, one label, one ordering slot), but it alters the verdict set he agreed to.
   Recommendation: (b), because it keeps unknown evidence from counting as a pass while
   still surfacing the rows the price checks single out.
2. **Calibration still short of data after a backfill** (trigger: the replay ends
   Insufficient history even after the owner has run a backfill). Options: (a) hold the
   verdict UI until enough history accumulates and re-run the replay later; (b) ship the
   verdicts labelled "thresholds not yet calibrated" in the footnote and re-run the replay
   periodically. Under (a) the page keeps its current form for months; under (b) he sees
   verdicts with a visible caveat. Reversible either way. Recommendation: (a), because the
   owner made calibration a precondition for trusting the verdicts.
3. **ANSWERED 2026-11-12: the owner chose (b).** Original question: **Longer horizons in
   later replays, and which horizon decides** (see the proposal above). Options: (a) add 60 and 120 trading days as reported horizons and keep 20 days
   as the only one that can change a threshold; (b) add them and make 60 days the deciding
   horizon once it passes its floors, with 20 and 120 reported beside it; (c) leave the
   replay as it is. Under (a) the longer horizons inform but never act; under (b) thresholds
   follow the horizon closest to how long the owner holds what he buys on a dip; under (c)
   nothing is learned about longer holding periods. Cheap to change before the next replay
   is pre-registered, not after it has run. Recommendation: (b) if the owner typically holds
   such purchases for months rather than weeks, otherwise (a); in both cases with the
   quarterly cadence proposed above.
