# Fundamentals Source Spike

Date: 2026-11-04 · Status: complete · Probe: `scripts/probe-fundamentals.ps1`

## Summary

The free sources are usable for company figures computed by the app.

- Yahoo's fundamentals-timeseries endpoint answers the backend's plain request shape
  (no cookie or crumb) and returns quarterly revenue, diluted EPS, net income, diluted
  average shares and ordinary shares for every equity probed, plus annual and trailing
  series and a dated company market value.
- The share-basis evidence check (S7) works on the data. Yahoo's latest trailing market
  value, divided by the listing close on its date times the latest diluted average
  share count, lands within 0.895-1.181 for every 1:1 listing. Ordinary-unit share
  counts against receipt-unit market values show up as clean ratios. It does **not**
  meet the plan's literal expectation that TSM shows as a 5:1 mismatch: Yahoo already
  reports TSM per US receipt. See S7 and **Findings that change plan expectations**.
- Nasdaq's US earnings data **has a confirmation signal**: a scheduled session time
  (before market open or after market close) marks a company-announced date, while
  "time not supplied" dates are explicitly described as algorithmic. The per-symbol
  endpoint drops the date on the report day itself, so a missing answer must never
  supersede a stored date.
- Under the planned rules, of the 21 current refresh targets, P/E would be complete for
  12, unavailable as loss-making for 5, and unavailable for 3 more (share basis
  unverified for 2, FX outside the ECB set for 1); one target is a positively
  identified certificate (not applicable). P/S would be complete for 15 and
  approximate for 2.

Three findings change expectations written into the plan; see **Findings that change
plan expectations**.

## Method

- The newest launch snapshot was copied outside the repository; the instrument list was
  read from the copy with a read-only SQLite connection and written to
  `docs/spikes/private/` (gitignored). The copy was deleted afterwards. The production
  ledger was not opened.
- The probe captured, per instrument (44 ledger instruments plus 10 public reference
  listings): one timeseries request carrying 51 series types, one Yahoo search (by ISIN
  when known), two years of daily chart closes with split events, and, for suffix-free
  symbols, the Nasdaq per-symbol earnings-date endpoint. It also scanned 35 weekdays of
  the Nasdaq US calendar (2026-10-19 to 2026-12-04).
- Ratios were computed by hand from the captured responses with the planned rules, FX
  from Frankfurter's 2026-11-04 reference rates (TWD, which the ECB does not publish,
  from Yahoo's `TWDSEK=X` chart for reference only), and compared with Yahoo's own ratio
  series only as a cross-check.
- Fixtures were recorded separately with `curl` using the reduced request described
  under S1 and **Fixtures**; the probe's `-Types` and `-ChartRange` options reproduce
  them.
- Findings below are aggregate counts. Only the public fixture companies are named.

## S1 — Quarterly series types

All 47 equity listings with figures carried these quarterly types (`3M` period type):
`quarterlyTotalRevenue`, `quarterlyOperatingRevenue`, `quarterlyDilutedEPS`,
`quarterlyBasicEPS`, `quarterlyNetIncome`, `quarterlyNetIncomeCommonStockholders`,
`quarterlyDilutedAverageShares`, `quarterlyBasicAverageShares`,
`quarterlyOrdinarySharesNumber` and `quarterlyFreeCashFlow`.
`quarterlyDilutedNIAvailtoComStockholders` was present for 46 of 47.

Value shape, per series entry:

```json
{"dataId": 20091, "asOfDate": "2026-09-30", "periodType": "3M",
 "currencyCode": "USD", "reportedValue": {"raw": 3.6629E10, "fmt": "36.63B"}}
```

- `asOfDate` is the period end. `currencyCode` is the report currency, and it is also
  attached to share counts, where it carries no meaning.
- Each series covers at most five quarters. Each series drops quarters independently,
  so the union of period ends can reach six.
- Results arrive one object per type: `timeseries.result[].meta.type[0]` names the
  type, and the values sit under the same key. Requested types without data return an
  entry without values.

Recommended request (the fixture request): quarterly `TotalRevenue`, `DilutedEPS`,
`NetIncome`, `NetIncomeCommonStockholders`, `DilutedNIAvailtoComStockholders`,
`DilutedAverageShares`, `OrdinarySharesNumber`; annual `TotalRevenue`, `NetIncome`,
`NetIncomeCommonStockholders`, `DilutedEPS`; trailing `TotalRevenue`, `NetIncome`,
`DilutedEPS`, `MarketCap`. Use `period1` 2015-01-01 and `period2` now. A production
request should also add `annualDilutedNIAvailtoComStockholders`, so that exact
subtraction (S3) can fill the same net income measure the app uses.

**Net income measure.** EPS reconciles with `DilutedNIAvailtoComStockholders` (falling
back to `NetIncomeCommonStockholders`) divided by diluted average shares. Plain
`NetIncome` includes minority interests and gave a 7.8% gap for one holding. Use the
common-stockholder measure for EPS derivation, the consistency check and profit margin.

## S2 — Missing quarters and freshness

- Inner gaps (a quarter missing between present ones) across the probe: EPS 13, diluted
  average shares 13, net income 2, revenue 1. EPS and diluted average shares go missing
  **together**, mostly in the September (8) and December (5) quarters, which is the
  US fiscal-year-end Q4 pattern. So the "derive EPS from net income and shares" path
  rarely applies; the three-of-four rule does.
- A whole-quarter gap across every series (EVO.ST's September 2025 quarter, period ends
  184 days apart) appeared for 2 of 47 listings. One recently listed company had only
  two quarters in its trailing window.
- **The newest quarter can be partly populated.** For a few days after a report, Yahoo
  can carry EPS and diluted shares for the new quarter but not yet revenue or net income
  (the O'Reilly fixture shows this). Companies that had reported 1-2 days earlier still
  showed the previous quarter as latest.
- Latest-quarter age on 2026-11-04: 35 days for September reporters that had published,
  96 days for July fiscal quarters, and 127 days for June quarters awaiting their
  November report. The maximum was 127, so the 200-day `latest_quarter_too_old`
  default holds.
- European equity coverage gap: one Paris-listed equity returned no revenue, EPS or net
  income at all; it had only share counts, at half-year intervals. It may be a
  half-year reporter that Yahoo does not cover quarterly. It ends as unavailable, not
  as not applicable.

## S3 — Annual and trailing series

- `annual*` and `trailing*` types exist for revenue, net income and EPS. Annual series
  carry four fiscal years; `annual*` `asOfDate`s are fiscal year ends (month-ends: 151
  December, plus January, March, April, June, July, August, September and October
  year ends).
- Annual `TotalRevenue` and annual `NetIncome` equalled the sum of their four
  quarterly values **exactly** in every case where all four were present (only three
  companies had a fully covered fiscal year inside the five-quarter window). Exact
  subtraction is allowed for revenue and for a net income measure whose annual and
  quarterly types are the same measure, when the annual period ends on a stored slot
  and its other three quarters are present.
- Annual EPS differed from the sum of quarterly EPS (MSFT 17.95 versus 17.96; another
  holding -1.2%). This confirms the plan's rule that EPS is never subtracted.
- Trailing series are Yahoo's own TTM values at quarter ends and are used only as
  cross-checks.

## S4 — Announcement dates in Yahoo

There are none. Timeseries entries carry only `dataId`, `asOfDate` (period end),
`periodType`, `currencyCode` and `reportedValue`.

## S5 — Nasdaq US earnings dates

Two endpoints, both answering a request with the backend user agent and
`Accept: application/json`, about 1-3 s each. Whether the `Accept` header is required
was not tested; the Nasdaq client should send it.

- **Per symbol:** `GET https://api.nasdaq.com/api/analyst/{SYMBOL}/earnings-date`.
  `data.reportText` takes one of three forms:
  - *Company-announced:* "… is expected* to report earnings on  12/02/2026 after market
    close.  The report will be for the fiscal Quarter ending Oct 2026. …" It always
    carries a session (before market open / after market close) and the fiscal
    quarter.
  - *Algorithmic:* "… is estimated to report earnings on  12/02/2026. The upcoming
    earnings date is derived from an algorithm based on a company's historical
    reporting dates. Our vendor, Zacks Investment Research, might revise this date in
    the future, once the company announces the actual earnings date. …"
  - *None:* "Our vendor, Zacks Investment Research, hasn't provided us with the upcoming
    earnings report date." This answered 26 of 39 suffix-free symbols on 2026-11-04.
    It is typical in the weeks right after a report, **and also on the report day
    itself**: four symbols with a session time in that day's calendar all answered
    "none" here.
  - Unknown symbols return `status.rCode` 400 with `bCodeMessage[0].errorMessage`
    "Symbol not exists.".
- **By day:** `GET https://api.nasdaq.com/api/calendar/earnings?date=YYYY-MM-DD`.
  Upcoming rows carry `symbol`, `name`, `time`, `fiscalQuarterEnding` ("Sep/2026"),
  `lastYearRptDt`, `marketCap` and analyst fields. `time` is `time-pre-market`,
  `time-after-hours` or `time-not-supplied`. For future dates, the calendar's session
  time agreed with the per-symbol form for every symbol checked (10 company-announced
  forms with a session time, 1 algorithmic form with `time-not-supplied`). The
  exception is report-day symbols, where the per-symbol endpoint answers "none". A
  market holiday returns `rows: null`.
- **How far ahead.** Company-announced rows thin out with distance. They were about
  85% of next-day rows and about 5-10% of rows four weeks out, which fits companies
  announcing two to four weeks ahead. Algorithmic dates appeared up to about six weeks
  ahead.
- **Confirmation signal (proposed).** A date is `confirmed` when the per-symbol text
  uses the *company-announced* form (equivalently, a calendar row with a session time).
  An *algorithmic* date is `nasdaq_expected`. Neither the asterisk nor any field says
  "confirmed" (the asterisk also appears in the algorithmic `announcement` field), so
  this rests on Nasdaq's own wording of the algorithmic form and the timing pattern.
  The human check against investor-relations pages, planned for the US report-date
  work, verifies it.
- **A "none" answer never supersedes a stored date.** Only a different dated answer
  (another date or form) or a fresh calendar scan of that day without the entry
  counts as a move or withdrawal. Otherwise the report-day due fetch would discard the
  confirmed date before the quarter appears, which can take a day or two (S2).
- **Past days remain retrievable, but the mark does not survive.** Once a day has
  passed, its rows switch to a results layout (`eps`, `surprise`; no `lastYearRptDt`),
  and most report `time-not-supplied`. The app must therefore record the mark at
  scheduling time, as the plan requires. Backfilled past days can only feed
  `nasdaq_expected` history.
- Movement between scans (postponement or withdrawal) could not be observed in a
  one-day spike. The reconciliation rules stay as planned.
- **Cost.** A calendar day takes about 2 s (up to 612 rows). The 25 weekdays in a
  five-week horizon cost about 50 s. The per-symbol endpoint costs about 2 s per US
  target, and there are 19 US targets. **Use the per-symbol endpoint**, queried only
  for US targets that need a date, as the plan's alternative allows. The response
  contains analyst figures; only the date, session, fiscal quarter and form are parsed.
- Nasdaq lags from quarter end to report date in the scanned window: past reports had a
  median of 29 days (p90 35); upcoming company-announced dates 37; upcoming algorithmic
  dates 43. The scan began 19 days after the September quarter end, so the early tail
  is cut off. No fiscal-year-end quarters fell in the window.

## S6 — Report currencies and the ECB set

Report currencies seen across all probed listings: USD, EUR, SEK, NOK, DKK, KRW, TWD,
CNY and GBP. A quarter's currency can differ from the listing currency. EVO.ST reports
in EUR and lists in SEK, and US-listed depositary receipts report in their home
currency (TSM in TWD).

Frankfurter's (ECB) currency list on 2026-11-04 is: AUD, BRL, CAD, CHF, CNY, CZK, DKK,
EUR, GBP, HKD, HUF, IDR, ILS, INR, ISK, JPY, KRW, MXN, MYR, NOK, NZD, PHP, PLN, RON,
SEK, SGD, THB, TRY, USD, ZAR. TWD is outside it, so TSM's P/E and P/S end as
`fx_unavailable` under the plan's rule, while its margin and growth still show.

## S7 — Share basis (blocking criterion: evidence source found; TSM reinterpretation accepted by Lars)

**Evidence source.** `trailingMarketCap` from the same timeseries request: take its
latest entry. Its `asOfDate` was 1-2 days before the probe date for every listing (the
earlier entries are sparse snapshots, weeks apart). Its currency was the listing
currency of the queried symbol in every case where one was given. 12 of 48 latest
entries had a null `currencyCode`, and a few earlier entries did too (for example
ORLY's and ASML.AS's 2024-06-10). Every non-null entry equalled the listing currency.

**Why it is independent of the figures it checks.** The plan warns that price cancels
out of any check built only from price, EPS, net income and shares. Yahoo's market
value is a company-level value, not the listing's price times Yahoo's diluted count:

- Volvo A and Volvo B carry the identical value (628,600,779,821 SEK) although their
  prices differ.
- Receipts deviate from 1 by their premium or discount to the home-market price (TSM
  0.895).

So the ratio compares the listing's own price with a home-market valuation, which is
what reveals a listing-to-share unit difference. Remaining blind spot: if Yahoo made
the same unit error in both its market value and its EPS for a listing, the check
would pass.

**Share series.** `quarterlyDilutedAverageShares`, which is the EPS denominator. Per
quarter, EPS matched common-stockholder net income ÷ diluted average shares within a
median of 0.2% (p90 3.2%). The larger deviations were all EPS reported to cents near
zero, or IPO quarters.

**Interpretation.** `units = market value / (listing close on or before the evidence
date × diluted average shares of the latest window quarter that has them ×
FX(listing → evidence currency))`. Only the instrument's own listing price is used.

| units | state |
|---|---|
| 0.80 ≤ units ≤ 1.25 | `established` |
| otherwise, take the clean ratio r nearest to units by log distance, from the integers 2-20 and their reciprocals; if units ÷ r is within 0.90-1.10 | `listing_unit_mismatch`, r in `detail` |
| anything else; no entry; evidence older than 30 days; a null currency when the series has no non-null currency, or has non-null currencies other than the listing currency; or no close on or before the evidence date | `share_basis_unverified` |

The nearest-ratio rule matters because neighbouring ratios' bands overlap at the wider
established tolerance. The mismatch band is narrower because a misidentified ratio
would put a wrong figure in the hover text. The 30-day age limit keeps last-good
figures through outages and rate-limit skips: the share basis changes only through
splits and receipt-ratio changes, and the split rules below cover those.

Observed values:

- 1:1 listings: 0.895-1.181. Examples are MSFT 0.997, ASML.AS 1.005, EVO.ST 0.956,
  VOLV-B.ST 1.001, VOLV-A.ST 0.999 and GOOG 0.997.
  - The spread comes from diluted versus outstanding counts and buybacks.
  - The 1.181 maximum is a fast-issuing company whose newest quarter lacked diluted
    shares, so the older count lagged. That is 6% from the band edge, and such cases
    fail safe (unverified).
- TSM: 0.895. **Yahoo reports TSM's EPS and diluted average shares per US receipt**
  (5 ordinary shares each, about 5.19 billion units against 25.9 billion ordinary
  shares). So with the EPS-basis share series, the listing is 1:1 and P/E agrees with
  the per-receipt figures. The remaining 10% is the receipt's premium over the Taipei
  price.
  - Using `OrdinarySharesNumber` instead gives 0.179. Its nearest clean ratio is 1/6
    (0.167, off by 7%), not the true 1/5 (off by 10.5%). The receipt premium
    multiplies into the ratio, so a ratio stated in `detail` is approximate.
- A US-listed Chinese receipt gave 0.978 with diluted shares and 0.124 with ordinary
  shares (nearest 1/8).
- Two current targets end as `share_basis_unverified`. One listing gave 0.739, which is
  outside both bands. A newly listed company gave 1.574, because its IPO quarter's
  weighted-average count lags its outstanding count. This is the honest outcome, and
  margin and growth still show.
- With the chosen share series, no real listing in the probe produced
  `listing_unit_mismatch`. That branch is exercised only through the ordinary-share
  series, so its tests need a constructed case.

**Dual-class.** Volvo A and B returned identical total-company share counts and
quarterly figures. Both evidence values were 1:1, and both P/E values were 16.3.
GOOGL and GOOG likewise (15.3 and 15.2).

**Splits.**

- The chart endpoint returns split events (`events.splits.{ts}.splitRatio`, for
  example `"15:1"`). This is a provider split source in addition to ledger `SPLIT`
  rows.
- **Yahoo restates earlier periods' per-share values after a split.** O'Reilly split
  15:1 in June 2025, and its fixture shows annual diluted EPS for 2022-2024 as 2.23,
  2.56 and 2.71, already divided by 15. The same restatement was seen on a quarterly
  series straddling a December 2025 split.
- An ADS ratio change appears as a split event, for example AstraZeneca's US listing
  shows "1:2" on 2026-02-02.
- Consequence for the figures computation: per-share values (EPS, share counts) stored
  from a fetch before a split event and not re-fetched afterwards are in pre-split
  units. They must not be combined with post-split values in a TTM window. Exclude such
  a slot (it becomes missing) unless a later fetch restated it. The split-since-report
  rule still covers the case where every stored quarter predates the split.
- How a split behaves between the report date and later prices could not be observed:
  no probed company had a split after its latest quarter end. The split-since-report
  rule needs a constructed test case.

## S8 — Classification

- Yahoo search `quoteType` over all ISIN queries: 46 `EQUITY` and 1 `ETF`. No absent or
  unrecognised values were observed.
- The chart endpoint's `meta.instrumentType` is present for every mapped symbol (49
  `EQUITY`, 2 `ETF`). This classifies a symbol that already has a Yahoo price mapping
  without a search call.
- ETF listings (IQQK.DE and a second ETF listing) return no quarterly values.
- Treat as positively fund or ETF: `ETF`, `MUTUALFUND`, `MONEYMARKET`. Only `ETF` was
  observed; the other two are Yahoo's documented fund types.
- Nasdaq Nordic asset classes that are positively non-share: `TRACKER_CERTIFICATES`
  (observed). `SHARES` is a share.

## S9 — Discovery

- ISIN search returned the instrument's own mapped Yahoo symbol for 38 of 44 ledger
  instruments.
- **No match: 3.** Two large US listings (TSM among them) returned no quotes for
  their ISIN, and the Nasdaq-priced certificate returned none either.
- **Other candidates only: 3.** Two instruments without a Yahoo price mapping returned
  secondary German or Mexican listings, or only the London primary listing. One ETF
  ISIN returned another listing of the same ETF.
- No ambiguous or classification-unknown results occurred.
- Recommended order (refines plan step 1):
  1. If an enabled Yahoo price mapping exists, verify that symbol directly with the
     timeseries call, classifying it by the chart `instrumentType`.
  2. Search only when there is no mapping or that check fails: by ISIN first, then by
     symbol.
- Nasdaq-priced Nordic shares: the ISIN search can return only the home primary
  listing (for example `AZN.L`), which quotes in GBp while its market value is in GBP.
  Prefer a Yahoo listing on the same exchange as the Nasdaq price listing (`.ST`, `.CO`,
  `.HE`), found by searching the Nasdaq listing's ticker. Fall back to the ISIN result,
  with the evidence check converting via FX at the instrument's own price.

## S10 — Period-end regularity

Yahoo normalises 52/53-week fiscal calendars to month-end `asOfDate`s. For example, a
late-January fiscal year end appears as 2026-01-31. Adjacent quarters were 89-92 days
apart, and a whole missing quarter showed as 184 days (two missing as 274). The plan's
80-100 day adjacency and the ~91-day multiple hold.

**The expected next period end is the latest month-end plus three months.**

## S11 — Latency and volume

Average latency per call:

| Call | Average |
|---|---|
| Yahoo timeseries | 0.48 s |
| Yahoo search | 0.25 s |
| Yahoo chart | 0.14 s |
| Nasdaq per-symbol | 2.4 s |
| Nasdaq calendar day | 2.0 s |

No throttling or 429 response appeared over about 230 Yahoo calls and 80 Nasdaq calls
at 0.3 s spacing.

- **Today's 21 targets, first refresh:** 21 timeseries calls, plus up to 21 searches (0
  when mappings verify directly). That is about 10-15 s sequentially for Yahoo. Adding
  19 Nasdaq per-symbol calls for US report dates adds about 45 s.
- **Typical refresh:** only due instruments, usually a handful, a few seconds. A weekly
  backstop day refetches all 21 (about 10 s). Sequential Yahoo fetching is acceptable
  (under the plan's 15 s threshold). Nasdaq calls should use bounded concurrency
  (about 4) or be limited to instruments needing a date.

## S12 — Terms

- **Yahoo:** the general Terms of Service prohibit collecting data from the services by
  automated means, and no terms specific to these endpoints are published.
- **Nasdaq:** nasdaq.com grants a personal, non-commercial, revocable licence.
- Automated use of Yahoo is therefore an **accepted risk** for personal use, exactly
  as for the existing Yahoo price source (2026-06-13). It is not permitted by Yahoo's
  terms. Nasdaq use is personal and non-commercial. The terms re-check before any
  distributed installer (2026-11-02 direction) applies to both.

## S13 — Calibration (from the committed fixtures)

The price is the last bar of each chart fixture: a 2026-11-04 **intraday** value
recorded around 16:00 UTC, before the markets closed. MSFT was 553.57 against the
previous close of 520.99. A spot-check against a reference closing price will
therefore differ by that day's move. FX comes from Frankfurter 2026-11-04
(EUR→SEK 10.9718); TWD→USD 0.03041 (from Yahoo's TWD chart) is shown for reference
only.

| Company | TTM EPS | P/E | P/S | Margin | Evidence units | Consistency gap |
|---|---|---|---|---|---|---|
| MSFT | 19.16 USD | 28.9 | 11.90 | 41.2% | 0.997 | 0.06% |
| ASML.AS | 35.00 EUR | 38.0 | 12.92 | 34.1% | 1.005 | 0.00% |
| EVO.ST | 5.36 EUR | 14.6 | **6.96** | 49.6% | 0.956 | 0.03% |
| VOLV-B.ST | 19.21 SEK | 16.3 | 1.33 | 8.1% | 1.001 | 0.03% |
| VOLV-A.ST | 19.21 SEK | 16.3 | 1.33 | 8.1% | 0.999 | 0.03% |
| TSM (reference) | 491.28 TWD | 26.5 | 13.65 | 51.1% | 0.895 | 0.80% |

- EVO.ST's P/S lands at 6.96. Reading EUR revenue as SEK gives 6.96 × 10.97 ≈ 76.
  That is the same order as the old figure of about 74, whose price date differed.
- Yahoo's own ratio series agreed within about 5% for these companies (MSFT 27.2/11.2
  on the previous close). They are not used as figures.
- **Consistency check.** Compare TTM EPS with the sum of per-quarter
  `DilutedNIAvailtoComStockholders ÷ DilutedAverageShares`, not with latest shares ×
  TTM net income. One holding's share count rose 15% in the window after an
  acquisition, which makes the latest-share form disagree by 36% on correct data.
  Among positive-EPS holdings the per-quarter form's largest gap was 2.3% (EPS in cents
  near zero). Tolerances: under 5% no effect; 5-20% `figures_disagree`; over 20%
  `figures_inconsistent`. Allow an absolute 0.02 per-share rounding before applying the
  percentage.
- Implausible bounds: P/E below 1 or P/S below 0.05 never occurred; keep them.

**Human check recommended.** Lars spot-checks the P/E and P/S of three or four of the
companies above against a reference he trusts.

**Human check result (2026-11-07): passed.** Lars compared four companies with Stock
Analysis's published 2026-11-06 statistics. Each is within about 6%, which matches the
price move since the intraday fixture price:

| Company | Fixture P/E / P/S | Published P/E / P/S |
|---|---|---|
| MSFT | 28.9 / 11.90 | 27.71 / 11.37 |
| ASML | 38.0 / 12.92 | 36.55 / 12.17 |
| EVO | 14.6 / 6.96 | 13.93 / 6.58 |
| VOLV B | 16.3 / 1.33 | 16.29 / 1.33 |

EVO's P/S shows no sign of the roughly 11× currency error.

## Findings that change plan expectations

1. **TSM is not a unit-mismatch case.** Yahoo already reports its EPS and diluted shares
   per US receipt.
   - With the EPS-basis share series it is `established`, and in the app it ends as
     `fx_unavailable` (TWD) instead.
   - The planned "TSM ends as `listing_unit_mismatch` with ratio 5" check, its test,
     and the demo share-basis case need a constructed mismatch: a receipt-unit market
     value against ordinary-unit diluted shares, as the ordinary-share series shows.
   - This misses an acceptance expectation of the share-basis gate. Lars accepted the
     reinterpretation on 2026-11-04: the share-basis rule stands unchanged, and the
     mismatch branch is tested with constructed data.
2. **Per-share history is restated after splits.** This adds the S7 exclusion rule for
   pre-split per-share values (cheap to change).
3. **Discovery verifies an existing Yahoo price mapping before searching.** ISIN search
   misses some large US listings. Use the S9 order (cheap to change).
4. **The per-symbol Nasdaq endpoint goes silent on report day.** A "none" answer must
   not supersede a stored date (S5).

None of these reverses a decision listed in the plan. Item 1 changes an expected test
outcome of the share-basis rule, not the rule itself.

## Parameters table

| Ref | Parameter | Value |
|---|---|---|
| S1 | Timeseries request types | the 15 types listed under S1 plus `annualDilutedNIAvailtoComStockholders`; `period1` 2015-01-01 |
| S1 | Net income measure for EPS derivation, consistency and margin | `DilutedNIAvailtoComStockholders`, fallback `NetIncomeCommonStockholders` |
| S1/S7 | Share series for P/S and evidence | `quarterlyDilutedAverageShares` (latest window quarter that has it) |
| S2 | Latest-quarter maximum age | 200 days |
| S3 | Exact subtraction | revenue, and the chosen net income measure where its annual type is the same measure; annual period ending on a stored slot with the other three quarters present; never EPS |
| S3 | Trailing series | cross-check only |
| S5 | US date source | Nasdaq per-symbol `earnings-date`; calendar only as fallback |
| S5 | Confirmation signal | company-announced text form, or calendar session time present |
| S5 | Supersession | a per-symbol "none" never supersedes a stored date; only a different dated answer or a fresh calendar scan of that day without the entry |
| S5 | Default report lag | 30 days; fiscal-year-end quarter 45 days (assumption, refined by each instrument's own history) |
| S6 | ECB currency constant | AUD BRL CAD CHF CNY CZK DKK EUR GBP HKD HUF IDR ILS INR ISK JPY KRW MXN MYR NOK NZD PHP PLN RON SEK SGD THB TRY USD ZAR |
| S7 | Evidence | latest `trailingMarketCap`; maximum age 30 days; a null currency is read as the listing currency only when every non-null entry equals it; price = close on or before the evidence date |
| S7 | Established band | 0.80-1.25 |
| S7 | Clean ratios | nearest of the integers 2-20 and their reciprocals by log distance, accepted within 0.90-1.10 |
| S7 | Split source | ledger `SPLIT` rows and chart `events.splits` |
| S7 | Split restatement | per-share values last fetched before a split event are excluded from TTM until restated by a later fetch |
| S9 | Discovery order | verify an enabled Yahoo price mapping directly (chart `instrumentType` classifies it); otherwise search by ISIN, then symbol; for Nasdaq-priced Nordic shares prefer the same-exchange Yahoo listing |
| S8 | Fund/ETF types | `ETF`, `MUTUALFUND`, `MONEYMARKET` (search `quoteType` or chart `instrumentType`) |
| S8 | Nasdaq non-share classes | `TRACKER_CERTIFICATES` |
| S10 | Grid | adjacency 80-100 days; missing quarters = round(gap ÷ 91.3) − 1; next period end = latest month-end + 3 months |
| S11 | Fetching | sequential Yahoo; Nasdaq bounded concurrency about 4 |
| S13 | Consistency | per-quarter form; 0.02 absolute allowance; under 5% none, 5-20% approximate, over 20% unavailable |
| S13 | Implausible bounds | P/E < 1, P/S < 0.05 |

## Fixtures

Recorded 2026-11-04 under `backend/tests/fixtures/fundamentals/` with the backend's user
agent. Reproduce them with
`scripts/probe-fundamentals.ps1 -Symbols <list> -Types <the S1 list> -ChartRange 1mo`
and, for the split chart, `-ChartRange 2y -ChartInterval 1mo`:

- **`yahoo_timeseries_{msft,asml_as,evo_st,volv_b_st,volv_a_st,tsm,orly,iqqk_de}.json`:**
  the S1 request.
  - EVO.ST has the whole skipped quarter.
  - TSM reports in TWD per receipt.
  - O'Reilly (ORLY) had a 15:1 split in June 2025 and a partly populated newest
    quarter.
  - IQQK.DE is an ETF with no values.
- **`yahoo_chart_*.json`:** one month of daily closes, for the evidence-date price.
  `yahoo_chart_orly_split.json` adds two years of monthly data with the split event.
- **`yahoo_search_*.json`:**
  - EVO.ST by ISIN;
  - TSM by ISIN (empty) and by symbol;
  - the ETF ISIN `IE00B0M63391`.
- **`nasdaq_earnings_date_*.json`:** the company-announced, algorithmic, not-provided
  and unknown-symbol forms.
- **`nasdaq_calendar_upcoming_day_trimmed.json`** and
  **`nasdaq_calendar_past_day_trimmed.json`:** real responses with `rows` trimmed to
  three and two public companies, showing both marks and the past-day results layout.
  **`nasdaq_calendar_empty_day.json`** is a market holiday.

## Sources Checked

- Yahoo Terms of Service: `https://legal.yahoo.com/xw/en/yahoo/terms/otos/index.html`
- Nasdaq legal and terms of use: `https://www.nasdaq.com/legal`
- Frankfurter currency list: `https://api.frankfurter.dev/v1/currencies`
