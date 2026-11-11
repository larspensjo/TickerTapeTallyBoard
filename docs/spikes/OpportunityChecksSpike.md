# Opportunity checks spike

## Data readiness

Status: **measured on 2026-11-11** on a scratch copy of the newest launch backup
snapshot, deleted afterwards. The measurement date was the snapshot's last completed
refresh, one day before the run. The evaluation was complete: no row failed. The
relaxed "no report" rule is selected, and the owner decided to add a fifth verdict for
candidates whose report timing is unknown (see the rule-selection finding).

### Safe execution

The session lead copies the newest launch backup snapshot to a scratch directory
outside the repository, runs the tool on that completed standalone SQLite snapshot,
and deletes every copy afterwards. Never point the tool at a live ledger or an
original backup. A file copied from a running WAL database is not a substitute for
a completed backup snapshot: immutable reading deliberately ignores live journals.

From the repository root:

```text
cargo run -p ticker-tape-tally-board-backend --example opportunity_data_readiness -- --demo
cargo run -p ticker-tape-tally-board-backend --example opportunity_data_readiness -- --ledger <external-scratch-snapshot>
```

An optional `--as-of YYYY-MM-DD` overrides the measurement date. In ledger mode the
default is the application's local date of the snapshot's most recent completed
latest-mode refresh: the newest `finished_at` instant in `market_data_refresh_runs`
for a launch or manual run with a terminal succeeded, partial or failed status.
Backfills and running refreshes are excluded. This completed-run witness captures
the whole refresh, including figures work; individual figures' `last_success` values
can be older because figures fetches are skipped when not due. If no such run
exists, an explicit `--as-of` is required. Launch backups precede that launch's
refresh, so the clock's run date would otherwise age evidence since the previous
session. Demo mode retains the local run date unless explicitly overridden.

Every resolved run prints the signed number of days from the measurement date to
the clock's today (negative for a future override), without printing either date.
R3 freshness, R4 fetch age and all other windows use the measurement date; stored
instants still use the application's local-calendar conversion. This is a
present-state inventory, not a reconstruction of historical figures or source
links. An earlier explicit date does not restore the ledger's earlier knowledge.

File inputs are canonicalized and refused inside the build's repository or any
ancestor tree containing a `.git` entry (file or directory), when missing or not
regular files, or when they match a configured application ledger. The ancestor
check also covers other clones/worktrees, hard links named inside them, and Windows
UNC aliases that retain a different canonical spelling. Inputs inside a configured
production or development backup directory are also refused: only an external
scratch copy may be opened. Configuration resolution covers `TTTB_DATABASE_URL`,
the default production/development ledger locations and the existing backup-directory
resolver. An unresolved or uncanonicalizable backup directory cannot be guarded;
no filesystem search is attempted. These path guards do not prove that an external
hard link is a separate scratch copy, so the session lead must still make a copy.
Opening uses SQLite read-only, no-create, immutable and query-only options. Every migration's
version, success and checksum must exactly match this build. No migrations, refresh,
backup, log file or ledger writes occur. Demo alone creates and seeds an in-memory
database, then uses the same measurement routine.

Output contains only fixed category labels, counts, percentages and the date-offset
day count; years are the only transaction-date grouping. Identifying values and
raw errors are never printed.
Each table states its denominator population. A percentage is suppressed when that
population has fewer than five members, including an empty population. Counts are
always permitted. Unknown-reason tables use the unknown rows as their population;
business reasons can overlap. No medians are printed by this tool. The shared
analysis helpers also suppress medians below five observations for subsequent tools.

A row evaluation failure is counted in an `evaluation_failed` bucket under a fixed
measurement/operation label; identifying data and raw error text are suppressed.
Independent measurements and other rows continue. R6 counters are committed only
when the entire row evaluation succeeds. The summary explicitly marks any failed
evaluation as incomplete: unresolved held positions are absent from the held
population, and unresolved company inputs are absent from the company population;
other affected results can be missing within their stated populations. R9 close
finality includes failed rows in its population and counts them separately. A
failure preventing the whole inventory aborts with a fixed operation label. Do not
select a report rule from an incomplete run until these failures are understood.

### Measurement methods

R1–R8 use precisely the application's currently held rows: domain position
derivation over the stored ledger, quantity above zero. Watchlist rows are excluded.
Source selection and close pairing use the existing effective-price repositories.
Company inputs, source blockers, quarter grid and next-report selection reuse the
application's company-figure loader and domain calculations. Report history,
observations, confirmed dates, estimates and implied next quarters use the existing
`report_dates` functions. Rules that do not yet have a domain implementation are
private, pure measurement helpers beside the example.

| Measurement | Method and denominator | Interpretation after measurement |
|---|---|---|
| R1 classification | Count company (`Equity` or figures-bearing classification), fund/ETF, certificate, other positively identified non-company, and unknown/no-source rows. Preserve the application's certificate mapping override. Source availability remains visible in later reason counts. Denominator: held rows. | 20 of 21 held rows are companies and one is a certificate. No fund, ETF or unknown row is held, so the company checks apply to nearly every row. |
| R1 listing market | Enabled Yahoo mapping first: suffix maps to its listing market; no suffix means US. Otherwise use Nasdaq Nordic mapping currency, then instrument currency. EUR from a Nordic mapping maps to Helsinki; instrument EUR alone maps to other Europe. Unknown suffixes remain other/unknown. Never use the broker/exchange tag. | 19 rows are US-listed; Stockholm and the Netherlands have one row each. A market index matters first for the US. |
| R2 last report | Companies, separately for US and non-US listing markets. Confirmed requires a resolved company-announced schedule. Otherwise use the latest quarter's full observation interval: at most seven days, wider, first fetch without baseline, other missing absence baseline, or unavailable. `first_fetch_without_baseline` means the latest quarter has no `absent_on` and its `first_seen` equals the instrument's earliest active observation (quarterly or annual) `first_seen`; the other no-baseline cases remain `figures_without_absence_baseline`. A wide observation remains an interval even though the application cannot use its midpoint as report history. | No last report is confirmed. 18 of 20 companies have only the first fetch's upper bound; two have an observed window of at most seven days. There are too few non-US rows to compare regions. |
| R3 report in period | All held company rows, independently for 7d, 14d and 30d, each under strict and relaxed absence rules. Widen the period start by seven calendar days. A confirmed report before today, wholly contained observed interval, or passed company-announced/Nasdaq-expected schedule without new figures yields yes. A partly overlapping interval, first-seen bound without baseline inside the window, or passed app estimate yields unknown. A date today is not a past scheduled report. No requires a latest-report upper bound before the widened start and either an unended next quarter or a successful figures check today/yesterday. Relaxed additionally permits the app's own LastYearDate estimate at least fourteen days ahead. Split no-baseline unknowns by the same first-fetch definition as R2. Print yes/no/unknown, unknown reasons, and answered (yes + no) counts and shares. For 30d under each rule print answered shares twice: all held company rows, and that population excluding rows whose unknown reason is `first_fetch_without_baseline`; suppress each share independently below five denominator rows. | Both rules answer 2 of 20 companies in every window, both with yes; no row gets a no. 16 of the 18 unknowns are the start-up effect described below. See the rule-selection finding. |
| R4 next report and fetch age | Held companies: selected next basis (company-announced, Nasdaq expected, report-history estimate, typical-gap estimate, unavailable); age of last successful figures fetch in the application's local calendar: 0–1, 2–7, over 7 days, never successful. A future timestamp is counted separately. | 15 of 20 next-report dates are typical-gap estimates and none rests on report history, so the relaxed rule cannot yet add an answer. 14 of 20 figures checks are 2–7 days old, so the strict rule's fresh-check condition holds for a minority of rows on a given day. |
| R5 business | Held companies: existing trailing margin and latest-quarter year-on-year sales growth plus latest/year-ago diluted EPS. Any missing sub-test, source blocker, currency change, too-old quarter, newer-report marker, or passed next expected report makes the result unknown. A sales decline or EPS below the prior EPS minus 10% of its magnitude (including profit to loss) is weakening; otherwise positive margin is intact, non-positive margin is loss-making only. Other approximations remain usable. | 16 of 20 companies get a business result: 10 intact, 5 weakening, 1 loss-making only. 4 are unknown; their reasons can overlap. |
| R5 EPS consistency | Cross-check only the compared two quarters. Sum reported EPS and income/share-derived EPS, subtract a single 0.02 rounding allowance from the absolute aggregate gap, divide by the sum of the absolute derived quarter values. At least 5% through 20% is approximate; over 20% makes business unknown. Missing income or positive diluted shares means not cross-checked, with reported EPS still usable. Existing derived-EPS approximation is retained on the grid. A zero derived scale follows the current company-figures evaluation (no disagreement downgrade). | The pair agrees for 18 of 20 companies. One disagrees severely, which makes its business check unknown, and one pair cannot be evaluated. No row falls in the approximate band. |
| R6 history | Held rows, trailing 365 calendar days, only the latest close's source and instrument currency. Count ≥200 closes without a gap, ≥200 with a gap, 120–199, under 120; also count >5 missing weekdays between closes and between look-back start and first close. Count ledger/provider splits, blocked split history, currency exclusions and changes in the effective enabled source. Yahoo closes must all have been fetched on a UTC date strictly after each split's effective date; Nordic history across a split is blocked. Unusual-drop history needs 120 closes; range history needs 200 with year-start coverage; both reject gaps and blocked splits. | 18 of 21 rows already have a full, gap-free year. 3 rows need a top-up: each is short and has a gap over the limit. No split, source change or currency exclusion lies in any look-back. Readiness buckets overlap with the gap counter. |
| R6 top-up effect | Count rows needing top-up (under 200 closes, gap, or blocked split), those with no mapping, Nasdaq Nordic series across a split that a top-up cannot repair even when a Yahoo mapping exists, and other-source split cases, and those a top-up would repair **if the provider supplies a full valid year**. Repairability follows the latest close's series provider and requires an enabled mapping for that provider. This last count is a conditional upper bound, not verified provider availability: a read-only inventory cannot establish an IPO's missing past or a provider's response without fetching it. | All 3 rows needing a top-up are repairable if the provider supplies a full valid year. This remains a conditional upper bound, not a guaranteed repaired count. |
| R7 dividend source | Held rows by actual latest pricing source: Yahoo-priced, Nasdaq-only, Nasdaq-priced with an enabled Yahoo mapping, other source (including manual and Twelve Data), no latest price. Yahoo capability is not evidence of captured dividend coverage; events and coverage are not yet stored. | 20 of 21 rows are Yahoo-priced, so ex-dividend days can become knowable for all but one row once dividend events are captured. |
| R8 change | Reuse `recent_price_change`, the effective latest/start closes and ledger split dates exactly as Opportunities does, for each window. Count available/unavailable; with six available held rows, every such row has at least five other rows and can use a median reference. No median value is printed. | Every held row has an available change in all three windows, and every row has at least five peers for the median reference. |
| R9 calibration sample | Historical buys up to the measurement date, including now-closed holdings: domain-derived position immediately before each `(trade_date, id)` buy determines buy-more status. Count by calendar year and in total, with buys as the share denominator. For instruments ever bought by that date, count stored provider closes up to that date whose `fetched_at` UTC date is later than the price date. Includes all stored sources so later replay can apply its own eligibility filters; excludes watchlist-only instruments. | 123 of 162 buys were buy-more trades, and 99.78% of 10,707 stored closes are final. This sizes the sample only; neither number establishes calibration sufficiency. |

Company figures and observations only began being recorded around the initial
company-figures migration. Quarters already present at first fetch share that
fetch's `first_seen` and lack an absence baseline. Until that date precedes the
30d widened start, this one-off start-up effect can dominate unknowns under both
rules. The excluded-first-fetch share shows its impact without assuming those
reports were absent. A first-fetch row already answered by another source or by an
upper bound before the window remains in that denominator. The relaxed rule can
differ from the strict rule only once the app holds a year of its own report
history; provider historical calendar dates cannot supply that history.

### Result tables

Counts are aggregate. A share shown as `suppressed` has a denominator population
under five rows and is deliberately not derived here.

| R1 classification | Count | Share of held rows |
|---|---|---|
| Company | 20 | 95.24% |
| Fund/ETF | 0 | 0.00% |
| Certificate | 1 | 4.76% |
| Other non-company | 0 | 0.00% |
| Unknown or no source | 0 | 0.00% |
| Total held rows | 21 | 100.00% |

| R1 listing market | Count | Share of held rows |
|---|---|---|
| US | 19 | 90.48% |
| Stockholm | 1 | 4.76% |
| Netherlands | 1 | 4.76% |
| Every other market category | 0 | 0.00% |

| R2 last-report knowledge | US count / share | Non-US count / share |
|---|---|---|
| Confirmed | 0 / 0.00% | 0 / suppressed |
| Observed ≤7 days | 2 / 10.53% | 0 / suppressed |
| Observed wider | 0 / 0.00% | 0 / suppressed |
| First fetch without baseline | 17 / 89.47% | 1 / suppressed |
| Other figures without absence baseline | 0 / 0.00% | 0 / suppressed |
| Unavailable | 0 / 0.00% | 0 / suppressed |
| Company population | 19 | 1 |

| R3 window / rule | Yes count | No count | Unknown count | Answered count / share of companies |
|---|---|---|---|---|
| 7d strict | 2 | 0 | 18 | 2 / 10.00% |
| 7d relaxed | 2 | 0 | 18 | 2 / 10.00% |
| 14d strict | 2 | 0 | 18 | 2 / 10.00% |
| 14d relaxed | 2 | 0 | 18 | 2 / 10.00% |
| 30d strict | 2 | 0 | 18 | 2 / 10.00% |
| 30d relaxed | 2 | 0 | 18 | 2 / 10.00% |

| R3 30d answered basis / rule | Company population | Answered count | Answered share |
|---|---|---|---|
| All held companies / strict | 20 | 2 | 10.00% |
| Excluding first-fetch unknowns / strict | 4 | 2 | suppressed |
| All held companies / relaxed | 20 | 2 | 10.00% |
| Excluding first-fetch unknowns / relaxed | 4 | 2 | suppressed |

The unknown reasons are identical for all six window and rule combinations.

| R3 unknown reason (each window and rule) | Count | Share of unknown companies |
|---|---|---|
| First fetch without baseline | 16 | 88.89% |
| Other figures without absence baseline | 0 | 0.00% |
| App estimate passed inside the window, figures not seen | 2 | 11.11% |

**Rule-selection finding.**

- The strict rule answers at least two thirds at 30d: **no**. It answers 2 of 20
  companies (10.00%). Excluding first-fetch unknowns it answers 2 of 4, a population
  too small for a share, and still under two thirds. Both bases agree, so the
  **relaxed rule is selected**.
- The relaxed rule leaves more than half unknown at 30d: **yes on all companies**
  (18 of 20, 90.00%). Excluding first-fetch unknowns it leaves 2 of 4 unknown, which
  is not more than half, but four rows cannot carry the decision.
- Owner question required: **yes**. The two bases disagree on the trigger, so the
  question of a fifth verdict for rows that would be candidates except for unknown
  report timing was put to the owner on 2026-11-11. **Answer (2026-11-11):
  add the verdict "Possible candidate, report timing unknown" between Sentiment-dip
  candidate and Not enough data.**

Two effects lie behind the 90%:

- **Start-up effect (temporary).** 16 of 18 unknowns exist only because the app first
  fetched company figures inside the widened window. They clear once that first fetch
  precedes the widened 30-day window, around mid-December 2026.
- **Structural effect (about one year).** After a quarter ends and before its report
  lands, a "no" needs either a figures check from today or yesterday (6 of 20 rows
  had one) or a report-history estimate (0 of 20 rows have one; the app needs a year
  of its own report history). In those weeks of each quarter, most company rows are
  expected to have unknown report timing under either rule until that history exists.

| R4 next-report basis | Count | Share of companies |
|---|---|---|
| Company-announced | 3 | 15.00% |
| Nasdaq expected | 2 | 10.00% |
| LastYearDate | 0 | 0.00% |
| TypicalGap | 15 | 75.00% |
| Unavailable | 0 | 0.00% |

| R4 successful figures fetch age | Count | Share of companies |
|---|---|---|
| 0–1 days | 6 | 30.00% |
| 2–7 days | 14 | 70.00% |
| Over 7 days | 0 | 0.00% |
| Never successful | 0 | 0.00% |
| Future timestamp | 0 | 0.00% |

| R5 business readiness | Count | Share of companies |
|---|---|---|
| Intact | 10 | 50.00% |
| Weakening | 5 | 25.00% |
| Loss-making only | 1 | 5.00% |
| Unknown | 4 | 20.00% |

| R5 unknown reason (may overlap) | Count | Share of unknown companies |
|---|---|---|
| Expected report date passed | 2 | suppressed |
| EPS inconsistent | 1 | suppressed |
| Quarter missing | 1 | suppressed |
| Too few quarters | 1 | suppressed |

**2026-11-11 — business rule changed after measurement.** A sub-test known to be
negative now gives weakening while the figures are current, even if another
sub-test is missing or inconsistent. Up to 2 of the 4 unknown companies could
therefore count as weakening under the current rule. The recorded counts above
are unchanged. Measured again the same day under the current rule, after one more
refresh on a fresh scratch copy: 10 intact, 6 weakening, 1 loss-making only and
3 unknown of 20 companies.

| R5 EPS pair consistency | Count | Share of companies |
|---|---|---|
| Consistent | 18 | 90.00% |
| Approximate | 0 | 0.00% |
| Unknown: inconsistent | 1 | 5.00% |
| Not cross-checked | 0 | 0.00% |
| Pair unavailable | 1 | 5.00% |

| R6 history readiness | Count | Share of held rows |
|---|---|---|
| ≥200 closes, no gap | 18 | 85.71% |
| ≥200 closes, gap | 0 | 0.00% |
| 120–199 closes | 1 | 4.76% |
| Under 120 closes | 2 | 9.52% |
| Gap over limit | 3 | 14.29% |
| Split in look-back | 0 | 0.00% |
| Split blocks checks | 0 | 0.00% |
| Effective source changes | 0 | 0.00% |
| Currency exclusions | 0 | 0.00% |
| No latest price | 0 | 0.00% |
| Unusual-drop history ready | 18 | 85.71% |
| Range history ready | 18 | 85.71% |
| Top-up needed | 3 | 14.29% |
| Conditional repairable upper bound | 3 | 14.29% |
| Top-up cannot fix Nasdaq series split (even with Yahoo mapping) | 0 | 0.00% |
| Top-up cannot fix other-source split | 0 | 0.00% |
| Top-up has no mapping | 0 | 0.00% |

| R7 ex-dividend source | Count | Share of held rows |
|---|---|---|
| Yahoo-priced | 20 | 95.24% |
| Nasdaq-only | 1 | 4.76% |
| Nasdaq-priced with Yahoo mapping | 0 | 0.00% |
| Other source | 0 | 0.00% |
| No latest price | 0 | 0.00% |

| R8 window | Available count / share | Unavailable count / share | Rows with five usable peers count / share |
|---|---|---|---|
| 7d | 21 / 100.00% | 0 / 0.00% | 21 / 100.00% |
| 14d | 21 / 100.00% | 0 / 0.00% | 21 / 100.00% |
| 30d | 21 / 100.00% | 0 / 0.00% | 21 / 100.00% |

| R9 buy-more sample | Buy count | Buy-more count | Buy-more share of buys |
|---|---|---|---|
| 2025 | 69 | 45 | 65.22% |
| 2026 | 93 | 78 | 83.87% |
| All years | 162 | 123 | 75.93% |

| R9 stored-close finality | Count | Share of eligible stored closes |
|---|---|---|
| Final | 10,683 | 99.78% |
| Non-final | 24 | 0.22% |
| Evaluation failed | 0 | 0.00% |
| Total | 10,707 | 100.00% |

| Evaluation completeness | Count / status |
|---|---|
| Clock today minus measurement date (days; dates omitted) | 1 |
| Overall evaluation status | complete; no evaluation failed |
| Measurement/operation labels with an evaluation_failed bucket | none |

**Overall implications and remaining gaps.**

- **Price evidence is nearly ready.** 18 of 21 rows have a usable year of history
  today, the other 3 are within reach of a history top-up, and every row has a change
  and a median reference in every window.
- **The business check is knowable for most companies** (16 of 20). The two-quarter
  EPS consistency rule costs one row and marks none approximate.
- **Report timing is the bottleneck.** It is unknown for 18 of 20 companies today.
  Most of that is the temporary start-up effect, but the weeks between a quarter's
  end and its report stay largely unknown for about a year.
- **Ex-dividend days** can become knowable for 20 of 21 rows once dividend events are
  captured.
- **Calibration sample.** 123 buy-more trades and 10,683 final closes are available.
  Whether that is enough is decided by the replay's own sufficiency rule, not here.
- **Market index.** US listings dominate, so the US index is the one that matters.

## Calibration

### Pre-registered method

**Caveats, before any results.** Weekly sampling with a 20-trading-day forward horizon
still overlaps approximately four-fold. Holdings are correlated across instruments,
samples can be small, and no statistical significance is claimed. Instruments added
later introduce survivor effects. Holdings on different markets can end the same horizon
on calendar dates a day or two apart. Stored closes may have been restated since the
replay date; this is a price-history replay, not a reconstruction of everything the
application knew then. Company information is not point-in-time. No thresholds are
selected from demo output.

**Copy preparation and execution.** Before the first real replay, commit this method.
The session lead copies a fresh launch snapshot outside all repository trees and
configured ledger/backup directories, then duplicates it into a scratch working copy. In
the development loop the session lead runs one live latest refresh against the working
copy, filling history through the history top-up. After the development loop closes
cleanly and its WAL is checkpointed, the replay opens that completed working copy
read-only, through the existing scratch-ledger guards, with the exact schema of this
build. Both copies are deleted afterwards. The owner may optionally backfill from the
Import page beforehand to deepen the sample. The implementation task performs only
offline demo and fixture runs (`--demo` and `--demo --truncated`); preparation and the
real run belong to the session lead. No file-backed ledger is created or migrated by the
replay. The real run requires `--as-of`, used only to fix the run date to the date of
the preparation refresh, never to explore alternative cutoffs. Record exactly one run
per prepared copy. A repeat is permitted only on a fresh copy after more history exists,
and is recorded as a new replay. Demo runs cannot authorize either criterion.

**Final closes only.** A stored close is final only if the UTC calendar date of its
`fetched_at` is after its price date, using the policy in `parameters.rs`. All held
listing markets end their session before midnight UTC on their own date. A later
re-fetch only moves `fetched_at` later. This conservatively excludes even final closes
fetched the same evening until a later refresh revisits them. Non-final closes are
missing when building the check history, so the normal gap rule applies. Only closes
whose price date is on or before a replay date influence checks; subsequent closes are
used only for forward returns. Later re-fetch timestamps establish finality/restatement,
not a historical knowledge claim.

**Universe and per-check eligibility.** An instrument-date enters the universe when the
ledger held the instrument after that date's transactions, its window change is
available under the existing Opportunities rules computed from final closes as of that
date, and at least one long-history price check is computable from those closes. Unusual
drop needs 120 closes without an internal gap; only the 1-year range needs full-year
coverage and 200 closes. Each single-check group uses only instrument-dates on which
that check is computable. All dips are universe instrument-dates whose change is
negative. Flagged dips are dips whose verdict price evidence is present: unusual drop =
yes or off its high = deep. Unknown never counts as a pass. The unconditional group
deliberately consists of universe instrument-dates, not all held instrument-dates, to
keep populations comparable. Universe exclusions `change_unavailable` and
`no_computable_long_history_check` mean, respectively, no available window change and
both long-history checks unknown. For each group, `selected` counts its universe events
before forward-outcome exclusions (buy-more groups count selected buy events).

**Dates and close selection.** Replay 7d, 14d and 30d separately, on every fifth
weekday, anchored at the first weekday on or after the earliest eligible ledger buy
date, through the preparation-refresh date fixed by `--as-of` (demo defaults to the
clock's run date). The replay date maps to the latest effective final close on or before
that date, using existing provider precedence for ties. The start close and change use
the existing recent-price-change computation and its tolerance and split rules. Provider
mappings are the scratch copy's enabled mappings; historical mapping revisions are not
available. Each replay date has equal weight per instrument. The period halves split
only the scheduled replay dates having at least one universe instrument-date in any
window into the first floor(n/2) such dates and the remaining such dates. The midpoint
is the first date in the second half. This ignores forward outcomes and their
exclusions, and is shared across all windows and grids; empty early history cannot
consume the first half. If no date has a universe member, the run date is the fallback
midpoint. Distinct weeks are ISO week-year pairs of replay dates.

**Forward horizons.** From the replay date's selected close, the close h trading days
later is the h-th stored session date after that close in the instrument's own stored
closes, counted **before eligibility filtering**, for h = 5, 10 and 20. One effective
bar per date is selected with the existing provider precedence; missing sessions are not
invented. Both endpoints must be final, from the same source as the as-of look-back
series, and in the instrument currency. Interior non-final, wrong-source or
wrong-currency bars are missing for gap evaluation. No gap over five missing weekdays
may lie inside the horizon. No ledger or provider split may fall after the start and on
or before the end, unless every horizon close was fetched on a UTC date after that split
from the same source whose provider capability verifies split adjustment at fetch. That
capability is supplied per source by the loader, not inferred in domain rules. A horizon
ending after the latest final close, or without an h-th stored session, is incomplete. A
missing h-th stored session returns only `incomplete` and suppresses other reasons for
that event/horizon. Count exclusions by reason: `non_final`, `source`, `currency`,
`gap`, `split`, `incomplete`; exclusion reasons can overlap, so their counts do not sum
to the number of excluded events. Non-positive endpoint closes and overflowing return
arithmetic are additionally counted as `invalid_close_or_arithmetic`, never silently
zeroed. Negative and zero returns remain valid.

**Benchmark.** Subtract the same-replay-date median forward return of the other
instruments held on that date, each using its own start close and h stored sessions
under the same eligibility rules. Benchmark peers need not pass a long-history check.
Exclude the subject; require at least five usable others. Otherwise count
`benchmark_unavailable` and omit the excess return. Benchmark availability is counted
only after the subject's own horizon is usable; failed own horizons retain their own
exclusion reasons. Different markets may reach their h-th sessions on slightly different
calendar dates.

**Groups and statistics.** For each window and each horizon report unconditional (all
universe instrument-dates, including rises), all dips, flagged dips, unusual drop alone,
deep below high alone, and flagged dips split by stopped falling (yes/no/unknown).
Single-check flagged groups contain negative changes only. Report usable count, distinct
instruments, distinct weeks, median forward excess return in percentage points, share
strictly positive, and tenth percentile. Twenty trading days is primary. The median
sorts exact decimals and averages the middle two for even counts; the tenth percentile
is nearest rank, observation ceil(n/10) in ascending order. Ties retain their full
weight; zero is not positive. No float statistics are used. Round only displayed
statistics to two decimals, midpoint to even. Every median, percentile, share and
improvement is suppressed when its group has fewer than five usable events or fewer than
three distinct instruments. An improvement requires both compared groups to pass these
floors, including each half separately. Counts are always printable. Output contains no
identities, individual dates, quantities, prices or amounts. Errors and panics must
likewise disclose no ledger values.

**Sufficiency and named outcomes.** Apply the floors only to events with a usable
**20-day excess return**: per window at least 100 dips and at least 30 flagged dips,
with those flagged dips spanning at least 8 instruments and 26 distinct ISO weeks. The
5-day and 10-day results do not determine sufficiency. Print the counts and shortfalls
for every floor per window. Overall **Calibrated** requires all three windows to meet
every floor. Anything less, including an empty replay, is **Insufficient history**. That
outcome does not satisfy the calibration prerequisite: keep initial thresholds, make no
threshold changes, record the shortfall, and do not start verdict UI implementation. The
session lead asks the owner to run a backfill and repeats the replay on a fresh copy. If
still insufficient, ask the owner whether to keep waiting or ship visibly uncalibrated
verdicts; the existing recommendation is to wait. Demo sufficiency proves only the
runner's smoke paths.

**Change criterion and grids.** Only after a real Calibrated outcome may a grid value
qualify to replace the initial value in a window: its flagged group must contain at
least 30 events from at least 8 instruments, its median 20-day excess return must exceed
the initial group's median by at least one percentage point, and the median improvement
must be positive in both period halves. Test unusual-drop deviations 1.0/1.5/2.0/2.5,
deep-below-high thresholds 10/15/20/25/30%, and stopped-falling minimum ages 1/2/3
trading days. Vary one input at a time, holding the others at their defaults; print the
combined flagged group for the two price-evidence grids, each check's own flagged group
for the drop criterion, and the stopped-yes subset of default flagged dips for the age
grid. Print full-period and both-half statistics and counts, including all-dip
references in both halves. Suppressed half medians cannot establish improvement. A value
qualifies for a check only if that same value meets the criterion in all three windows.
If multiple values qualify, choose the largest whole-period improvement of median 20-day
excess return in the 30d window; break ties by distance to the initial value, then by
the smaller value. Apply at most ONE threshold change per replay overall: among
qualifying checks' chosen values, select the check with the largest such 30d
improvement. Equal cross-check gains use the fixed grid order unusual drop, deep below
high, stopped-falling age. Other qualifying checks are candidates for a later replay and
are not applied. The tool prints per-window/value comparisons, one overall decision per
grid, and one replay decision; it never edits parameters. A failed window makes a value
not met; otherwise a suppressed comparison makes it undetermined. With no qualifying
value a grid is undetermined if any value is undetermined, otherwise not met.

**Drop criterion.** Only after a real Calibrated outcome, if a price check's own flagged
group does no better than the dips on which THAT check is computable at every grid value
in both halves, in every window, propose removing it from the verdict while keeping it
displayed. This per-check baseline matches populations; it is not all universe dips. The
all-windows criterion is met only when every window meets it; a failed window makes it
not met, otherwise missing comparisons make it undetermined. Print per-window findings
and one overall proposal per check. Compare primary median excess returns; missing or
suppressed medians cannot establish this criterion. Any removal goes to the owner before
application. Stopped falling remains display-only and its grid does not introduce it
into verdict evidence.

**Buy-more replay.** For each buy while already held, determine the pre-buy position by
the domain ledger fold immediately before its `(trade_date, id)`. Include instruments
later sold out. Evaluate the price side as of its trade date in each window, without
weekly subsampling, and compare flagged and unflagged buy-mores' forward excess returns
under the same horizons, benchmark and suppression rules. Report rises/non-negative
changes separately as `buy_more_rose`; unflagged means fell with price evidence absent.
If both long-history checks are unknown, the buy-more belongs to undetermined, not
unavailable; unavailable means the window change itself cannot be computed. Forward
returns start at the trade date's selected final close (on or before that date), never
the trade price. Repeated buys remain individual events. Trade dates are never printed.
For halves, buys before the weekly period's midpoint belong to the first half and the
rest to the second.

**Not replayable.** Report in the period lacks historical report dates; past Nasdaq
calendar backfill was rejected. Company checks cannot be replayed: stored figures are
not point-in-time and observations begin only with the application's first fetch in
November 2026. Report the count of replay instrument-dates with a latest and year-ago
quarterly EPS observation already seen by that date as a descriptive count. The latest
quarter must be within 200 calendar days, and the year-ago quarter must end in the same
month one year earlier. This is never a point-in-time company result. Ex-dividend
evidence before event capture and intraday prices are not replayable. The replay's
unusual-drop check never adds dividends back: no historical dividend evidence is
supplied. This tool does not infer any of these checks or full candidate verdicts from
price evidence.

### Results

**Replay of 2026-11-11.** One replay was run, on one prepared scratch copy of the
newest launch backup snapshot after one live latest refresh, with the run date fixed
to that day. Both copies were deleted afterwards. All figures below are aggregates;
every statistic rests on at least five events from at least three instruments.

**Outcome: Calibrated.** Every window met every sufficiency floor on the 20-day
outcome.

| Window | Usable dips (floor 100) | Usable flagged dips (floor 30) | Instruments (floor 8) | Distinct weeks (floor 26) | Outcome |
|---|---|---|---|---|---|
| 7d | 357 | 106 | 22 | 28 | Calibrated |
| 14d | 342 | 106 | 19 | 30 | Calibrated |
| 30d | 324 | 110 | 21 | 32 | Calibrated |

"Calibrated" means there was enough history to judge the thresholds. It does not mean
the checks were shown to predict a recovery. The weeks floor was met narrowly.

**Median 20-day excess return, whole period (percentage points; share positive).**

| Group | 7d | 14d | 30d |
|---|---|---|---|
| Unconditional (universe) | 0.12 (50.79%) | 0.13 (50.85%) | 0.14 (50.99%) |
| All dips | −0.13 (48.74%) | −0.29 (48.54%) | 0.08 (50.93%) |
| Flagged dips | −0.38 (46.23%) | −1.71 (46.23%) | −0.04 (49.09%) |
| Unusual drop alone | −0.82 (41.86%) | −2.74 (39.47%) | 0.02 (51.28%) |
| Deep below high alone | −0.15 (48.61%) | 0.09 (50.00%) | 0.23 (50.00%) |
| Flagged, stopped falling | 4.15 (58.82%) | 2.54 (58.54%) | −0.18 (48.44%) |
| Flagged, still falling | −0.82 (43.82%) | −3.19 (38.46%) | 0.00 (50.00%) |

Event counts for these rows range from 17 to 762; the smallest is "Flagged, stopped
falling" at 7d. The tenth percentile of every group lies between about −12 and −26
points, so the spread dwarfs every median difference.

**What the numbers say.**

- **Flagged dips did no better than other dips.** In all three windows the flagged
  group's median 20-day excess return is at or below that of all dips. On about a
  year of usable history, with overlapping horizons and correlated holdings, this
  shows no measurable edge for the price evidence; it does not show harm either.
- **The one pattern with some support is descriptive.** Flagged dips that had
  already stopped falling did better than those still falling in the 7d and 14d
  windows, not in the 30d window. Stopped falling stays display-only.
- **Halves.** Unusual drop alone was at or below zero in both halves at 7d and 14d.
  The deep-below-high check has no usable first half yet, because it needs a full
  year of history before each replay date.

**Change criterion: not met; initial thresholds are kept.** No grid value met the
criterion in all three windows. One value (unusual drop at 2.5 standard deviations)
met it in the 14d window only, which the all-windows rule rejects. Replay decision:
keep initial thresholds. No candidate is carried to a later replay.

**Drop criterion: undetermined for both price checks.** Some half comparisons are
suppressed or missing, so the criterion cannot be established in either direction.
No removal is proposed.

**Exclusions (20-day horizon, each window).** 81 weekly events had an incomplete
horizon (too recent) and 1 a non-final endpoint. No event was excluded for source,
currency, gap, split or a missing benchmark. Before that, 4 / 19 / 68 held
instrument-dates (7d / 14d / 30d) had no available change, and 462 / 448 / 401 had
no computable long-history check.

**Buy-more replay (median 20-day excess return, whole period).**

| Group | 7d | 14d | 30d |
|---|---|---|---|
| Buy-more on a flagged dip | 1.92 (25 events) | 0.16 (18) | 4.22 (16) |
| Buy-more on an unflagged dip | −1.63 (11) | 3.75 (16) | 2.95 (13) |
| Buy-more on a rise | 2.89 (34) | −0.19 (47) | −2.22 (48) |
| Buy-more, price evidence undetermined | −0.95 (41) | −0.64 (22) | 0.20 (19) |

These groups are small and point in different directions across windows; they
support no conclusion.

**Company checks.** No replay instrument-date had both a latest and a year-ago
quarter already observed by its date, so the company side contributes nothing to
this replay, as stated under "Not replayable".

**Follow-up.** The thresholds in the parameters table stand. A later replay on a
fresh copy, once more history exists, is recorded as a new replay. Whether the
verdicts are presented as planned, given that the price evidence showed no
measurable edge, is the owner's decision.

## Initial parameters

Values and meaning are preserved; procedural references name the relevant
measurement or replay behaviour. This stable two-column table is the reference for
the later domain-parameter consistency test.

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
| "No report" rule | relaxed, because the readiness measurement showed the strict rule answers under two thirds of company rows (30d) |
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

The displayed unusual-drop change is anchored on today, while the historical moves
it is compared with are anchored on each stored close. Before the day's close, the
displayed move can therefore span one trading day fewer than the moves in the sample.

The owner decided on 2026-11-11 that the unusual-drop check works from 120 closes with no
gap between consecutive closes, and that only the 1-year range needs full-year coverage.
The readiness counts above were measured before that decision, when both checks required
full-year coverage, so "Unusual-drop history ready" can be understated by up to the three
short-history rows.

**History top-up trial (2026-11-11).** A latest refresh with the history top-up was run
once against a scratch copy of the newest launch backup snapshot, deleted afterwards.
Aggregate outcome:

- 3 of 21 held rows needed a top-up; each got exactly one request and all 3 succeeded.
- The price source had no older closes for any of the 3 (they are recent listings), so
  their history stays short. The remaining needs were recorded, and a second refresh the
  same day made no top-up request.
- Both refreshes ended with the normal succeeded status and no failed items; the top-up
  added nothing to the price counts or failures.
- Measured afterwards under the owner's rule: unusual-drop history is ready for 19 of 21
  rows and 1-year range history for 18 of 21. The other rows become ready as their
  listings age, not through a top-up.

When the historical moves show no variation at all, the unusual-drop check is unknown
(`no_price_variation`) rather than yes: a threshold of zero built from a price that
never moved would count any fall as unusual.
