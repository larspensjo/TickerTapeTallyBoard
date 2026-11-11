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

When the historical moves show no variation at all, the unusual-drop check is unknown
(`no_price_variation`) rather than yes: a threshold of zero built from a price that
never moved would count any fall as unusual.
