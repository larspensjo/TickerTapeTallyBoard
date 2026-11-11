# Opportunity checks spike

## Data readiness

Status: **NOT MEASURED ON A PRIVATE LEDGER**. Every result cell below is a
placeholder for the session lead. Demo output verifies execution only and must not
be substituted for private-ledger findings. No report rule has been selected.

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
| R1 classification | Count company (`Equity` or figures-bearing classification), fund/ETF, certificate, other positively identified non-company, and unknown/no-source rows. Preserve the application's certificate mapping override. Source availability remains visible in later reason counts. Denominator: held rows. | PENDING — establish the company and non-company evidence populations. |
| R1 listing market | Enabled Yahoo mapping first: suffix maps to its listing market; no suffix means US. Otherwise use Nasdaq Nordic mapping currency, then instrument currency. EUR from a Nordic mapping maps to Helsinki; instrument EUR alone maps to other Europe. Unknown suffixes remain other/unknown. Never use the broker/exchange tag. | PENDING — establish the held listing markets. |
| R2 last report | Companies, separately for US and non-US listing markets. Confirmed requires a resolved company-announced schedule. Otherwise use the latest quarter's full observation interval: at most seven days, wider, first fetch without baseline, other missing absence baseline, or unavailable. `first_fetch_without_baseline` means the latest quarter has no `absent_on` and its `first_seen` equals the instrument's earliest active observation (quarterly or annual) `first_seen`; the other no-baseline cases remain `figures_without_absence_baseline`. A wide observation remains an interval even though the application cannot use its midpoint as report history. | PENDING — assess report-history certainty and regional differences. |
| R3 report in period | All held company rows, independently for 7d, 14d and 30d, each under strict and relaxed absence rules. Widen the period start by seven calendar days. A confirmed report before today, wholly contained observed interval, or passed company-announced/Nasdaq-expected schedule without new figures yields yes. A partly overlapping interval, first-seen bound without baseline inside the window, or passed app estimate yields unknown. A date today is not a past scheduled report. No requires a latest-report upper bound before the widened start and either an unended next quarter or a successful figures check today/yesterday. Relaxed additionally permits the app's own LastYearDate estimate at least fourteen days ahead. Split no-baseline unknowns by the same first-fetch definition as R2. Print yes/no/unknown, unknown reasons, and answered (yes + no) counts and shares. For 30d under each rule print answered shares twice: all held company rows, and that population excluding rows whose unknown reason is `first_fetch_without_baseline`; suppress each share independently below five denominator rows. | PENDING — report both bases before the session lead settles which real-number basis decides the two-thirds strict answered threshold and the relaxed more-than-half-unknown trigger. This document makes neither decision. |
| R4 next report and fetch age | Held companies: selected next basis (company-announced, Nasdaq expected, report-history estimate, typical-gap estimate, unavailable); age of last successful figures fetch in the application's local calendar: 0–1, 2–7, over 7 days, never successful. A future timestamp is counted separately. | PENDING — distinguish missing history from stale checks. |
| R5 business | Held companies: existing trailing margin and latest-quarter year-on-year sales growth plus latest/year-ago diluted EPS. Any missing sub-test, source blocker, currency change, too-old quarter, newer-report marker, or passed next expected report makes the result unknown. A sales decline or EPS below the prior EPS minus 10% of its magnitude (including profit to loss) is weakening; otherwise positive margin is intact, non-positive margin is loss-making only. Other approximations remain usable. | PENDING — loss-making only is distinct from weakening and intact. Unknown reasons can overlap. |
| R5 EPS consistency | Cross-check only the compared two quarters. Sum reported EPS and income/share-derived EPS, subtract a single 0.02 rounding allowance from the absolute aggregate gap, divide by the sum of the absolute derived quarter values. At least 5% through 20% is approximate; over 20% makes business unknown. Missing income or positive diluted shares means not cross-checked, with reported EPS still usable. Existing derived-EPS approximation is retained on the grid. A zero derived scale follows the current company-figures evaluation (no disagreement downgrade). | PENDING — size the additional approximation and unknown buckets independently of P/E availability. |
| R6 history | Held rows, trailing 365 calendar days, only the latest close's source and instrument currency. Count ≥200 closes without a gap, ≥200 with a gap, 120–199, under 120; also count >5 missing weekdays between closes and between look-back start and first close. Count ledger/provider splits, blocked split history, currency exclusions and changes in the effective enabled source. Yahoo closes must all have been fetched on a UTC date strictly after each split's effective date; Nordic history across a split is blocked. Unusual-drop history needs 120 closes; range history needs 200 with year-start coverage; both reject gaps and blocked splits. | PENDING — size history top-up demand. Readiness buckets overlap with gap/split/source counters. |
| R6 top-up effect | Count rows needing top-up (under 200 closes, gap, or blocked split), those with no mapping, Nasdaq Nordic series across a split that a top-up cannot repair even when a Yahoo mapping exists, and other-source split cases, and those a top-up would repair **if the provider supplies a full valid year**. Repairability follows the latest close's series provider and requires an enabled mapping for that provider. This last count is a conditional upper bound, not verified provider availability: a read-only inventory cannot establish an IPO's missing past or a provider's response without fetching it. | PENDING — report conditional repairability and retain this limitation; do not claim a guaranteed repaired count. |
| R7 dividend source | Held rows by actual latest pricing source: Yahoo-priced, Nasdaq-only, Nasdaq-priced with an enabled Yahoo mapping, other source (including manual and Twelve Data), no latest price. Yahoo capability is not evidence of captured dividend coverage; events and coverage are not yet stored. | PENDING — size potential dividend knowability. |
| R8 change | Reuse `recent_price_change`, the effective latest/start closes and ledger split dates exactly as Opportunities does, for each window. Count available/unavailable; with six available held rows, every such row has at least five other rows and can use a median reference. No median value is printed. | PENDING — establish each window's usable peer population. |
| R9 calibration sample | Historical buys up to the measurement date, including now-closed holdings: domain-derived position immediately before each `(trade_date, id)` buy determines buy-more status. Count by calendar year and in total, with buys as the share denominator. For instruments ever bought by that date, count stored provider closes up to that date whose `fetched_at` UTC date is later than the price date. Includes all stored sources so later replay can apply its own eligibility filters; excludes watchlist-only instruments. | PENDING — estimate sample size only; neither final-close share nor buy-more count establishes calibration sufficiency. |

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

`PENDING` means **not measured**, never zero. Expand market, year and unknown-reason
rows to match the aggregate output. Retain suppressed shares as `suppressed`; do not
derive small-population shares for the committed findings.

| R1 classification | Count | Share of held rows |
|---|---|---|
| Company | PENDING | PENDING |
| Fund/ETF | PENDING | PENDING |
| Certificate | PENDING | PENDING |
| Other non-company | PENDING | PENDING |
| Unknown or no source | PENDING | PENDING |
| Total held rows | PENDING | PENDING |

| R1 listing market | Count | Share of held rows |
|---|---|---|
| Each fixed market category from output | PENDING | PENDING |

| R2 last-report knowledge | US count / share | Non-US count / share |
|---|---|---|
| Confirmed | PENDING | PENDING |
| Observed ≤7 days | PENDING | PENDING |
| Observed wider | PENDING | PENDING |
| First fetch without baseline | PENDING | PENDING |
| Other figures without absence baseline | PENDING | PENDING |
| Unavailable | PENDING | PENDING |
| Company population | PENDING | PENDING |

| R3 window / rule | Yes count | No count | Unknown count | Answered count / share of companies |
|---|---|---|---|---|
| 7d strict | PENDING | PENDING | PENDING | PENDING |
| 7d relaxed | PENDING | PENDING | PENDING | PENDING |
| 14d strict | PENDING | PENDING | PENDING | PENDING |
| 14d relaxed | PENDING | PENDING | PENDING | PENDING |
| 30d strict | PENDING | PENDING | PENDING | PENDING |
| 30d relaxed | PENDING | PENDING | PENDING | PENDING |

| R3 30d answered basis / rule | Company population | Answered count | Answered share |
|---|---|---|---|
| All held companies / strict | PENDING | PENDING | PENDING |
| Excluding first-fetch unknowns / strict | PENDING | PENDING | PENDING |
| All held companies / relaxed | PENDING | PENDING | PENDING |
| Excluding first-fetch unknowns / relaxed | PENDING | PENDING | PENDING |

| R3 window / rule / unknown reason | Count | Share of unknown companies |
|---|---|---|
| First fetch without baseline, for each window/rule | PENDING | PENDING |
| Other figures without absence baseline, for each window/rule | PENDING | PENDING |
| Each remaining unknown reason for each of the six combinations | PENDING | PENDING |

Rule-selection finding: **PENDING session-lead run and decision**. Denominator basis
(all companies or excluding first-fetch unknowns): **PENDING real numbers**. The strict rule
answers at least two thirds at 30d: **PENDING**. Relaxed leaves more than half unknown
at 30d: **PENDING**. Owner question required: **PENDING**.

| R4 next-report basis | Count | Share of companies |
|---|---|---|
| Company-announced | PENDING | PENDING |
| Nasdaq expected | PENDING | PENDING |
| LastYearDate | PENDING | PENDING |
| TypicalGap | PENDING | PENDING |
| Unavailable | PENDING | PENDING |

| R4 successful figures fetch age | Count | Share of companies |
|---|---|---|
| 0–1 days | PENDING | PENDING |
| 2–7 days | PENDING | PENDING |
| Over 7 days | PENDING | PENDING |
| Never successful | PENDING | PENDING |
| Future timestamp | PENDING | PENDING |

| R5 business readiness | Count | Share of companies |
|---|---|---|
| Intact | PENDING | PENDING |
| Weakening | PENDING | PENDING |
| Loss-making only | PENDING | PENDING |
| Unknown | PENDING | PENDING |

| R5 unknown reason | Count | Share of unknown companies |
|---|---|---|
| Each reason from output (may overlap) | PENDING | PENDING |

| R5 EPS pair consistency | Count | Share of companies |
|---|---|---|
| Consistent | PENDING | PENDING |
| Approximate | PENDING | PENDING |
| Unknown: inconsistent | PENDING | PENDING |
| Not cross-checked | PENDING | PENDING |
| Pair unavailable | PENDING | PENDING |

| R6 history readiness | Count | Share of held rows |
|---|---|---|
| ≥200 closes, no gap | PENDING | PENDING |
| ≥200 closes, gap | PENDING | PENDING |
| 120–199 closes | PENDING | PENDING |
| Under 120 closes | PENDING | PENDING |
| Gap over limit | PENDING | PENDING |
| Split in look-back | PENDING | PENDING |
| Split blocks checks | PENDING | PENDING |
| Effective source changes | PENDING | PENDING |
| Currency exclusions | PENDING | PENDING |
| No latest price | PENDING | PENDING |
| Unusual-drop history ready | PENDING | PENDING |
| Range history ready | PENDING | PENDING |
| Top-up needed | PENDING | PENDING |
| Conditional repairable upper bound | PENDING | PENDING |
| Top-up cannot fix Nasdaq series split (even with Yahoo mapping) | PENDING | PENDING |
| Top-up cannot fix other-source split | PENDING | PENDING |
| Top-up has no mapping | PENDING | PENDING |

| R7 ex-dividend source | Count | Share of held rows |
|---|---|---|
| Yahoo-priced | PENDING | PENDING |
| Nasdaq-only | PENDING | PENDING |
| Nasdaq-priced with Yahoo mapping | PENDING | PENDING |
| Other source | PENDING | PENDING |
| No latest price | PENDING | PENDING |

| R8 window | Available count / share | Unavailable count / share | Rows with five usable peers count / share |
|---|---|---|---|
| 7d | PENDING | PENDING | PENDING |
| 14d | PENDING | PENDING | PENDING |
| 30d | PENDING | PENDING | PENDING |

| R9 buy-more sample | Buy count | Buy-more count | Buy-more share of buys |
|---|---|---|---|
| Each year from output | PENDING | PENDING | PENDING |
| All years | PENDING | PENDING | PENDING |

| R9 stored-close finality | Count | Share of eligible stored closes |
|---|---|---|
| Final | PENDING | PENDING |
| Non-final | PENDING | PENDING |
| Evaluation failed | PENDING | PENDING |
| Total | PENDING | PENDING |

| Evaluation completeness | Count / status |
|---|---|
| Clock today minus measurement date (days; dates omitted) | PENDING |
| Overall evaluation status | PENDING |
| Each fixed measurement/operation label with an evaluation_failed bucket | PENDING |

Overall implications and remaining gaps: **PENDING session-lead findings**.

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
| "No report" rule | strict, unless the readiness measurement shows it answers under two thirds of company rows (30d); then relaxed |
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
