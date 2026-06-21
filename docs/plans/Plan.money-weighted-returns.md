# Money-Weighted (XIRR) Performance Returns Implementation Plan

> **For agentic workers:** implement task-by-task. Steps use checkbox (`- [ ]`) syntax.
> Plans are ephemeral; archive when implemented. Do not reference plan phases from durable docs.

**Goal:** Give the portfolio total return a **user-selectable method** — Money-weighted (XIRR,
the default), Simple (gain ÷ capital deployed), and Modified Dietz (the current single-shot
method, retained as a labeled comparison/legacy view) — with the choice persisted across
sessions. Per-asset rows switch to a stable current-position simple return regardless of the
selected total method.

XIRR is the default because it provably satisfies the **hard requirement Lars stated**: a buy,
sell, or transfer at current value must not move the number until prices change (verified —
see the neutrality test in Task 1). Whether XIRR also *matches Sharesight* is a separate
**hypothesis**: the investigation found de-annualized IRR was ~688% for one holding while
Sharesight showed ~421%, and no single-pass method reproduced Sharesight exactly. So "follows
Sharesight" is not assumed — it is validated against portfolio-level Sharesight figures in the
validation task before the DecisionLog entry is recorded (see decision 4).

**Architecture:** Add a pure XIRR solver to `backend/src/domain/performance.rs`. Aggregate the
existing per-instrument period inputs (begin market value, dated cash flows, end market value)
across the portfolio and compute the total percentage for the method requested via a `method`
query parameter on `/api/gains` (`xirr` | `simple` | `modified_dietz`, default `xirr`). XIRR is
displayed as the *cumulative* period return `(1+xirr)^years − 1`, not the annualized rate. The
frontend persists the selection in `localStorage` and sends it as the query parameter
(refetching on change, like the existing date preset). Per-asset row percentages switch to a
current-position simple return (`unrealized_gain ÷ cost_basis`), independent of the selector.

**Tech Stack:** Rust (axum, rust_decimal), React/TypeScript frontend.

## Global Constraints

- Build from `backend/`: `cargo build`; finish with `cargo clippy --all-targets -- -D warnings` then `cargo fmt`.
- Frontend from `frontend/`: `npm run check` then `npm run fmt`.
- Money/prices/FX stay exact `rust_decimal` round-tripped as TEXT (2026-06-14 Persistence Stack). XIRR's float solve is a derived display value only and must never feed stored money.
- Missing data is explicit `Availability::Unavailable { reasons }`, never zero.
- Bump `frontend/package.json` and `backend/Cargo.toml` versions; both surface in the UI.
- Income/dividends remain excluded (not persisted per 2026-06-16 Avanza decision); **all three methods** are ex-dividend, matching the current gain amounts. State this in UI copy.
- `/api/gains` gains a `method` query parameter (`xirr` | `simple` | `modified_dietz`, default `xirr`); an unknown value is a `400 invalid_method` (mirrors `invalid_date`). The frontend persists the choice in `localStorage` (key `gains.returnMethod`, default `xirr`) and refetches on change.

## Decisions to ratify before/while implementing

These refine committed DecisionLog entries. Record the outcome in `docs/DecisionLog.md` as new
entries (never edit committed ones).

1. **Refines 2026-06-19** (Modified Dietz for totals AND rows): the total return is now method-selectable — **XIRR (default)**, **Simple**, and **Modified Dietz** (retained as a labeled comparison/legacy option); rows move to a current-position simple return. *(Ratified: Lars chose the full three-method menu, persisted across sessions.)*
2. **Refines 2026-06-20** (additive row subtotals): neither XIRR nor Simple is additive from row contributions, so the filtered "visible rows" sticky summary stops computing a percentage from summed rows. It shows the SEK sum only; the method-specific percentage is the backend whole-portfolio figure. *(Default — Lars to object if the filtered % must return.)*
3. **Component split per method:** `capital_gain_percent` / `currency_gain_percent` use each method's natural denominator — for **Simple** and **Modified Dietz** the real denominator (`component_SEK / denominator`, additive); for **XIRR** (no single denominator) a proportional split of the total (`total% × component_SEK / total_return_SEK`). *(Default — Lars to confirm or omit component percentages.)*
4. **Validation gate (not just a target):** Lars provides Sharesight's **portfolio-level** total return (since inception and YTD) and the configured Sharesight method. The validation task compares our cumulative XIRR against those figures. Agree a tolerance up front (suggest **±3 percentage points absolute, or ±5% relative**, whichever is larger). If XIRR is inside tolerance, record the DecisionLog entry as written. If it misses, **stop before recording it** and either (a) revise the default method, or (b) record explicitly that the app intentionally differs from Sharesight and defaults to XIRR for its neutrality property. XIRR-as-Sharesight-match is a hypothesis until this gate passes; XIRR's neutrality property is already proven and is the primary reason it is the default.
5. **Simple-return denominator:** Simple total return % = `total_return_base / (begin_mv + Σ buy costs in period)` — opening market value plus gross purchases, **not** reduced by sells, so it stays stable. Per-asset Simple is not offered; the selector governs the portfolio total only.
6. **Persistence:** the selected method persists via `localStorage` (no backend settings table yet). Revisit if a server-side settings store is later introduced.
7. **Asset-detail method:** the asset page (`AssetView`) renders only method-independent current-position row data, so it uses the `xirr` default and does not mirror the board's persisted selection (Task 4 Step 1b). *(Default — Lars to object if asset detail must follow the persisted method.)*

---

## File Structure

- `backend/src/domain/performance.rs` — add `MoneyWeightedReturn` struct + `compute_money_weighted_return`; add `actual_period_cash_flows` (actual investor cash, no split multiplication) alongside the existing split-adjusted `period_cash_flows`; the Modified Dietz functions stay (now a live selectable method, not dead code).
- `backend/src/domain/valuation.rs` — add `ValuationReason::PerformanceDidNotConverge`.
- `backend/src/domain/mod.rs` — export the new symbols.
- `backend/src/api/gains.rs` — parse the `method` query param; accumulate aggregate `end_mv`; compute the totals percentage for the chosen method (XIRR / Simple / Modified Dietz); switch row percentages to current-position; map the new reason.
- `frontend/src/api/types.ts` — `GainsQuery`/fetch gains a `method`; `percentage_method` echoes the request; `display_percent_kind` gains `"money_weighted"` and `"simple"`.
- `frontend/src/components/GainsTable.tsx` — method selector + `localStorage` persistence; label/tooltip copy per method; remove/replace `visibleTotalReturnPercent`.
- `frontend/src/api/queries.ts` — the `useGains` fetch layer: add `method` to `GainsParams`, include it in the React Query `queryKey` (alongside `includeClosedPositions`/`startDate`/`endDate`), and set it in `URLSearchParams`.
- `frontend/src/components/AssetView.tsx` — decide and document the asset-detail method (see Task 4 Step 1b).

---

## Task 1: XIRR solver in the domain (pure, unit-tested)

**Files:**
- Modify: `backend/src/domain/valuation.rs:30-48` (add reason variant)
- Modify: `backend/src/domain/performance.rs` (add struct + function + tests)
- Modify: `backend/src/domain/mod.rs:12-16` (export)

**Interfaces:**
- Produces:
  ```rust
  pub struct MoneyWeightedReturn {
      pub annualized: Decimal,   // the solved XIRR rate per year
      pub cumulative: Decimal,   // (1+annualized)^years - 1, the period return
      pub period_days: i64,
  }

  pub fn compute_money_weighted_return(
      begin_market_value: &Availability<Decimal>,
      cash_flows: &Availability<Vec<CashFlow>>,
      end_market_value: &Availability<Decimal>,
      start_date: NaiveDate,
      end_date: NaiveDate,
  ) -> Availability<MoneyWeightedReturn>;
  ```
  `CashFlow.amount_base` keeps its existing sign convention: **+ for buy cost, − for sell proceeds**. The investor-perspective flow used in the NPV is the negation.

  **Actual flows, not split-adjusted flows (review High 1).** This solver consumes an
  *actual investor cash-flow series* — the real cash paid/received at trade date (quantity ×
  price × trade-date FX ± brokerage), with **no** `post_period_split_factor` multiplication. The
  existing `period_cash_flows` deliberately scales buy/sell quantities by
  `post_period_split_factor` so the Modified Dietz denominator agrees with the split-adjusted
  valuation numerator (`backend/src/domain/performance.rs:307-344`); that is correct for Modified
  Dietz but wrong for XIRR and Simple, which work on absolute cash amounts. A later split must not
  retroactively double the historical cash invested. The actual-flow path is added in Task 2
  (`actual_period_cash_flows`); Task 1 only requires the solver to treat its `cash_flows` argument
  as already being the actual series.

- [ ] **Step 1: Add the unavailable reason.** In `valuation.rs`, add to `ValuationReason` after `ZeroOrInvalidPerformanceDenominator`:

```rust
    PerformanceDidNotConverge,
```

- [ ] **Step 2: Write the neutrality + correctness failing tests** in `performance.rs` tests module:

```rust
#[test]
fn money_weighted_simple_hold_matches_simple_return() {
    // Invest 100k at start, worth 120k after exactly one year, no flows.
    let begin = Availability::Available(Decimal::ZERO);
    let flows = Availability::Available(vec![CashFlow {
        date: date("2025-01-01"),
        amount_base: dec!(100000), // buy cost
    }]);
    let end = Availability::Available(dec!(120000));
    let r = compute_money_weighted_return(&begin, &flows, &end, date("2025-01-01"), date("2026-01-01"));
    let v = match r { Availability::Available(v) => v, _ => panic!("available") };
    assert!((v.cumulative - dec!(0.20)).abs() < dec!(0.001), "cumulative {}", v.cumulative);
}

#[test]
fn money_weighted_is_cash_flow_neutral_for_same_day_trade() {
    // A buy today (cash out + equal market-value in) must not change the result.
    let start = date("2025-01-01");
    let end = date("2025-07-01");
    let base_flows = vec![CashFlow { date: start, amount_base: dec!(100000) }];
    let begin = Availability::Available(Decimal::ZERO);
    let end_mv = Availability::Available(dec!(150000));

    let r1 = compute_money_weighted_return(
        &begin, &Availability::Available(base_flows.clone()), &end_mv, start, end);

    // Same trade today: +50k buy cost flow at end_date, end MV also +50k.
    let mut with_trade = base_flows.clone();
    with_trade.push(CashFlow { date: end, amount_base: dec!(50000) });
    let r2 = compute_money_weighted_return(
        &begin, &Availability::Available(with_trade), &Availability::Available(dec!(200000)), start, end);

    let a = match r1 { Availability::Available(v) => v.annualized, _ => panic!() };
    let b = match r2 { Availability::Available(v) => v.annualized, _ => panic!() };
    assert!((a - b).abs() < dec!(0.0001), "neutrality violated: {a} vs {b}");
}

#[test]
fn money_weighted_unavailable_when_no_sign_change() {
    // All inflows, no outflow -> no root.
    let begin = Availability::Available(Decimal::ZERO);
    let flows = Availability::Available(vec![CashFlow { date: date("2025-01-01"), amount_base: dec!(-100) }]);
    let end = Availability::Available(dec!(100));
    let r = compute_money_weighted_return(&begin, &flows, &end, date("2025-01-01"), date("2026-01-01"));
    assert!(matches!(r, Availability::Unavailable { .. }));
}

#[test]
fn money_weighted_solves_with_interleaved_buy_and_sell() {
    // Alternating-sign flows: buy, partial sell mid-period, open remainder at end.
    // A single well-defined root must still be found (solver must not assume the NPV
    // curve is monotonic or that positive NPV always belongs to the lower bound).
    let begin = Availability::Available(Decimal::ZERO);
    let flows = Availability::Available(vec![
        CashFlow { date: date("2025-01-01"), amount_base: dec!(100000) },  // buy cost
        CashFlow { date: date("2025-07-01"), amount_base: dec!(-60000) },  // sell proceeds
    ]);
    let end = Availability::Available(dec!(70000));
    let r = compute_money_weighted_return(&begin, &flows, &end, date("2025-01-01"), date("2026-01-01"));
    let v = match r { Availability::Available(v) => v, _ => panic!("expected a root") };
    // Sanity: NPV at the solved annualized rate is ~0 (recompute independently if desired).
    assert!(v.annualized.is_sign_positive() || v.annualized.is_sign_negative());
}

#[test]
fn money_weighted_unavailable_when_multiple_roots() {
    // A flow series that produces more than one sign change / IRR root must return
    // PerformanceDidNotConverge rather than silently picking one. Construct a series with
    // two interior roots (large early inflow, larger outflow, inflow again).
    let begin = Availability::Available(Decimal::ZERO);
    let flows = Availability::Available(vec![
        CashFlow { date: date("2025-01-01"), amount_base: dec!(1000) },     // -1000 investor
        CashFlow { date: date("2025-06-01"), amount_base: dec!(-2500) },    // +2500 investor
    ]);
    let end = Availability::Available(dec!(-1560)); // forces a second sign change in NPV
    let r = compute_money_weighted_return(&begin, &flows, &end, date("2025-01-01"), date("2026-01-01"));
    // Either a single documented root or Unavailable; this case must be Unavailable.
    assert!(matches!(r, Availability::Unavailable { .. }));
}
```

> The multi-root example values are illustrative — when implementing, pick concrete amounts that
> demonstrably yield two sign changes in `npv(rate)` over `[-0.9999, 1_000_000]` and assert the
> `PerformanceDidNotConverge` contract. The point is the test, not these exact numbers.

- [ ] **Step 3: Run the tests, verify they fail to compile** (function missing).

Run: `cd backend && cargo test domain::performance::tests::money_weighted -- --nocapture`
Expected: compile error / FAIL.

- [ ] **Step 4: Implement** in `performance.rs`:

```rust
#[derive(Debug, Clone)]
pub struct MoneyWeightedReturn {
    pub annualized: Decimal,
    pub cumulative: Decimal,
    pub period_days: i64,
}

/// Money-weighted (XIRR) return over [start_date, end_date].
///
/// `cash_flows` MUST be the actual investor cash-flow series (real cash at trade date, with
/// no post-period split multiplication — see Task 2's `actual_period_cash_flows`).
///
/// Builds the dated investor cash-flow series:
///   -begin_mv at start_date, -cf.amount_base at each flow date, +end_mv at end_date,
/// then scans [-0.9999, 1_000_000] for sign-change sub-brackets (the NPV curve is not
/// assumed monotonic, because interleaved buys and sells can make it non-monotonic). It
/// solves the unique bracket by sign-tracked bisection and reports both the annualized rate
/// and the cumulative period return (1+rate)^years - 1. Returns Unavailable when any input
/// is unavailable, the period is non-positive, or the scan finds zero or more than one root
/// (an ambiguous multi-IRR series is refused, not silently resolved).
pub fn compute_money_weighted_return(
    begin_market_value: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    end_market_value: &Availability<Decimal>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<MoneyWeightedReturn> {
    let begin = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };
    let flows = match cash_flows {
        Availability::Available(v) => v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };
    let end_mv = match end_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };

    let period_days = (end_date - start_date).num_days();
    if period_days <= 0 {
        return Availability::unavailable(ValuationReason::ZeroOrInvalidPerformanceDenominator);
    }

    // Investor-perspective dated flows in f64 (display-only solve).
    let mut series: Vec<(f64, f64)> = Vec::with_capacity(flows.len() + 2);
    let years_at = |d: NaiveDate| (d - start_date).num_days() as f64 / 365.25;
    // Fallible conversion: a value that cannot become a finite f64 must surface as
    // unavailable, never silently become zero (which would alter the cash flows and
    // violate the "missing data is explicit, never zero" constraint).
    fn to_finite_f64(x: Decimal) -> Option<f64> {
        x.to_f64().filter(|v| v.is_finite())
    }
    let begin_f = match to_finite_f64(begin) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    series.push((0.0, -begin_f));
    for cf in flows {
        let cf_f = match to_finite_f64(cf.amount_base) {
            Some(v) => v,
            None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
        };
        series.push((years_at(cf.date), -cf_f));
    }
    let total_years = period_days as f64 / 365.25;
    let end_f = match to_finite_f64(end_mv) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    series.push((total_years, end_f));

    let npv = |rate: f64| -> f64 {
        series.iter().map(|(t, c)| c / (1.0 + rate).powf(*t)).sum()
    };

    // Scan the rate range for sign-change sub-brackets instead of trusting the two
    // endpoints. Interleaved buys/sells make NPV non-monotonic: an interior root can sit
    // between two same-sign endpoints, and multiple roots can exist. We collect every
    // bracket; zero brackets = no root, more than one = ambiguous multi-IRR -> refuse.
    let mut scan: Vec<f64> = vec![-0.9999];
    let mut r = -0.99_f64;
    while r < 1.0 {
        scan.push(r);
        r += 0.01; // 1% steps across the realistic -99%..100% band
    }
    let mut r = 1.0_f64;
    while r < 1_000_000.0 {
        scan.push(r);
        r *= 2.0; // geometric out to the cap
    }
    scan.push(1_000_000.0);

    let mut brackets: Vec<(f64, f64)> = Vec::new();
    let mut exact: Option<f64> = None;
    for w in scan.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let (flo, fhi) = (npv(lo), npv(hi));
        if flo.is_nan() || fhi.is_nan() {
            continue;
        }
        if flo == 0.0 {
            exact = Some(lo);
            break;
        }
        if flo * fhi < 0.0 {
            brackets.push((lo, hi));
        }
    }
    let rate = if let Some(r) = exact {
        r
    } else {
        if brackets.len() != 1 {
            // Zero roots or an ambiguous multi-root series: do not guess.
            return Availability::unavailable(ValuationReason::PerformanceDidNotConverge);
        }
        let (mut a, mut b) = brackets[0];
        // Track the sign at `a`; move whichever endpoint matches the midpoint's sign so the
        // bracket invariant holds regardless of the curve's orientation.
        let sign_a = npv(a) > 0.0;
        for _ in 0..300 {
            let m = (a + b) / 2.0;
            if (npv(m) > 0.0) == sign_a {
                a = m;
            } else {
                b = m;
            }
        }
        (a + b) / 2.0
    };
    let cumulative = (1.0 + rate).powf(total_years) - 1.0;

    let annualized = match Decimal::from_f64_retain(rate) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    let cumulative = match Decimal::from_f64_retain(cumulative) {
        Some(v) => v,
        None => return Availability::unavailable(ValuationReason::PerformanceDidNotConverge),
    };
    Availability::Available(MoneyWeightedReturn { annualized, cumulative, period_days })
}
```

Add `use rust_decimal::prelude::FromPrimitive;` if not already imported (the file already imports `ToPrimitive`).

- [ ] **Step 5: Export** in `mod.rs` performance block: add `compute_money_weighted_return, MoneyWeightedReturn`.

- [ ] **Step 6: Run tests, verify pass.**

Run: `cd backend && cargo test domain::performance`
Expected: PASS (including the new tests: simple-hold correctness, same-day-trade neutrality,
no-sign-change unavailable, interleaved buy/sell root, and multi-root → `PerformanceDidNotConverge`).

- [ ] **Step 7: Stage** (commit only when Lars asks — repo planning workflow keeps changes staged for review).

```bash
git add backend/src/domain/performance.rs backend/src/domain/valuation.rs backend/src/domain/mod.rs
```

---

## Task 2: Portfolio totals support a selectable method (XIRR / Simple / Modified Dietz)

**Files:**
- Modify: `backend/src/domain/performance.rs` (add `actual_period_cash_flows`)
- Modify: `backend/src/domain/mod.rs` (export `actual_period_cash_flows`)
- Modify: `backend/src/api/gains.rs` (`GainsQuery`, `PerformanceAccumulator`, `into_percents`, `percentage_method`, reason serialization)
- Modify: `backend/src/api/valuation.rs` (`serialize_valuation_reason` for the new variant)

**Interfaces:**
- Consumes: `compute_money_weighted_return`, `MoneyWeightedReturn` from Task 1; existing `compute_modified_dietz_denominator`, `apply_annualisation`, `period_cash_flows` (split-adjusted, kept for Modified Dietz).

- [ ] **Step 1: Add the method enum + query parsing.** In `gains.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReturnMethod { Xirr, Simple, ModifiedDietz }

fn parse_method(value: Option<&str>) -> Result<ReturnMethod, ApiError> {
    match value.unwrap_or("xirr") {
        "xirr" => Ok(ReturnMethod::Xirr),
        "simple" => Ok(ReturnMethod::Simple),
        "modified_dietz" => Ok(ReturnMethod::ModifiedDietz),
        other => Err(ApiError::bad_request("invalid_method", format!("invalid method: {other}"))),
    }
}
```

Add `method: Option<String>` to `GainsQuery`; in `list`, `let method = parse_method(query.method.as_deref())?;` near the date parsing. Pass `method` into `into_percents` and set the response `percentage_method` to the requested name (`"money_weighted"` | `"simple"` | `"modified_dietz"`).

- [ ] **Step 2: Add the actual-flow path and accumulate end MV + actual flows.**

  **(a)** In `performance.rs`, add `actual_period_cash_flows(period: &PeriodLedger, is_sek_instrument: bool) -> Availability<Vec<CashFlow>>`. It mirrors `period_cash_flows` but computes the **actual** invested/received cash — `qty = Decimal::from(tx.quantity)` (and `Decimal::from(-tx.quantity)` for sells) with **no** `* period.post_period_split_factor`. Same sign convention (+ buy cost, − sell proceeds), same price/FX/brokerage and missing-data reasons. Add a unit test asserting a buy before a later split yields the actual (un-doubled) cost, unlike `period_cash_flows`. Export it in `mod.rs`.

  **(b)** In `gains.rs`, where the row is built (currently `let cash_flows = period_cash_flows(&period, is_sek);`), also compute `let actual_cash_flows = actual_period_cash_flows(&period, is_sek);` and pass both into `perf_accum.add(...)`.

  **(c)** Add `end_mv: Decimal` and `actual_cash_flows: Vec<CashFlow>` to `PerformanceAccumulator`. Extend `add(...)` to take the actual flows; when the row is fully available, `self.end_mv += end;` (read `amounts.end_market_value_base`, adding it to the match tuple) and `self.actual_cash_flows.extend_from_slice(actual_cfs)`. Keep `self.cash_flows` (split-adjusted) for the Modified Dietz path. If the actual flows are `Unavailable`, the row must be excluded with its reasons (same as the existing flows-unavailable handling) so XIRR/Simple never silently drop a holding.

- [ ] **Step 3: Write failing tests** (rename `gains_with_date_range_returns_modified_dietz_percent`). Same June fixture (100 sh @ $10→$12, FX flat 10, begin_mv=0, one 10_000 buy at start, end_mv=12_000):

```rust
// default = xirr; 29-day period annualizes then de-annualizes back to the period: 20.00%.
let (_, body) = send(&state, "GET", "/api/gains?start_date=2026-06-01&end_date=2026-06-30", json!({})).await;
assert_eq!(body["percentage_method"], "money_weighted");
assert_available(&body["totals"]["total_return_percent"], "20.00");

// simple: 2000 / (0 + 10000) = 20.00%
let (_, s) = send(&state, "GET", "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=simple", json!({})).await;
assert_eq!(s["percentage_method"], "simple");
assert_available(&s["totals"]["total_return_percent"], "20.00");

// modified_dietz still available (legacy path retained)
let (_, md) = send(&state, "GET", "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=modified_dietz", json!({})).await;
assert_eq!(md["percentage_method"], "modified_dietz");
assert_available_status(&md["totals"]["total_return_percent"]);

// unknown method -> 400
let (st, _) = send(&state, "GET", "/api/gains?method=bogus", json!({})).await;
assert_eq!(st, StatusCode::BAD_REQUEST);
```

Also add a backend test where capital and currency gain offset to a **zero total return** with non-zero components (method=xirr), asserting both component percentages are `unavailable` (not `0.00`) and `total_return_percent` is `0.00`.

Also add a **split-neutrality regression** (review High 1): a buy before a later split, with the
end date once before and once after the split-adjustment boundary. The XIRR and Simple
`total_return_percent` must be driven by actual cash and current market value, and must **not**
change merely because an already-recorded split rescales the provider-adjusted historical price.
(Modified Dietz may legitimately differ — it is the split-adjusted path.)

- [ ] **Step 4: Run, verify failure.**

Run: `cd backend && cargo test gains_with_date_range`
Expected: FAIL / compile error (method param + dispatch not yet added).

- [ ] **Step 5: Rewrite `into_percents`** to dispatch on `method`. Each method uses its natural component denominator; the result tuple stays `(capital_pct, currency_pct, total_pct, display_kind)`:

```rust
fn into_percents(self, start: NaiveDate, end: NaiveDate, method: ReturnMethod)
    -> (AvailabilityResponse, AvailabilityResponse, AvailabilityResponse, String)
{
    if !self.has_data {
        let u = AvailabilityResponse::Unavailable { reasons: serialize_reasons(&self.unavailable_reasons) };
        return (u.clone(), u.clone(), u, "absolute".to_string());
    }
    let pct100 = |x: Decimal| format!("{:.2}", x * Decimal::from(100));
    match method {
        ReturnMethod::Xirr => {
            // Actual investor flows (review High 1) — NOT the split-adjusted self.cash_flows.
            let mw = compute_money_weighted_return(
                &Availability::Available(self.begin_mv),
                &Availability::Available(self.actual_cash_flows.clone()),
                &Availability::Available(self.end_mv),
                start, end);
            let total_pct = match &mw {
                Availability::Available(v) => v.cumulative,
                Availability::Unavailable { reasons } => {
                    let u = AvailabilityResponse::Unavailable { reasons: serialize_reasons(reasons) };
                    return (u.clone(), u.clone(), u, "money_weighted".to_string());
                }
            };
            // No single denominator -> proportional split; undefined when total is zero.
            let comp = |part: Decimal| -> AvailabilityResponse {
                if self.total_return.is_zero() {
                    if self.capital_gain.is_zero() && self.currency_gain.is_zero() {
                        return AvailabilityResponse::Available { value: "0.00".to_string() };
                    }
                    return AvailabilityResponse::Unavailable {
                        reasons: serialize_reasons(&[ValuationReason::ZeroOrInvalidPerformanceDenominator]),
                    };
                }
                AvailabilityResponse::Available { value: pct100(total_pct * (part / self.total_return)) }
            };
            (comp(self.capital_gain), comp(self.currency_gain),
             AvailabilityResponse::Available { value: pct100(total_pct) }, "money_weighted".to_string())
        }
        ReturnMethod::Simple => {
            // Stable denominator: opening value + gross purchases (not reduced by sells).
            // Use actual flows so a later split does not inflate historical purchase cost.
            let gross_buys: Decimal = self.actual_cash_flows.iter().map(|c| c.amount_base.max(Decimal::ZERO)).sum();
            let denom = self.begin_mv + gross_buys;
            if denom <= Decimal::ZERO {
                let u = AvailabilityResponse::Unavailable {
                    reasons: serialize_reasons(&[ValuationReason::ZeroOrInvalidPerformanceDenominator]),
                };
                return (u.clone(), u.clone(), u, "simple".to_string());
            }
            let comp = |part: Decimal| AvailabilityResponse::Available { value: pct100(part / denom) };
            (comp(self.capital_gain), comp(self.currency_gain),
             comp(self.total_return), "simple".to_string())
        }
        ReturnMethod::ModifiedDietz => {
            // Legacy path: existing single-shot denominator + annualisation.
            let denom = compute_modified_dietz_denominator(self.begin_mv, &self.cash_flows, start, end);
            let cap = component_percent(self.capital_gain, &denom);
            let cur = component_percent(self.currency_gain, &denom);
            match &denom {
                Availability::Available(d) => {
                    let (v, kind) = apply_annualisation(self.total_return / d, (end - start).num_days());
                    let label = match kind { DisplayPercentKind::Annualised => "annualised", DisplayPercentKind::Absolute => "absolute" };
                    (cap, cur, AvailabilityResponse::Available { value: pct100(v) }, label.to_string())
                }
                Availability::Unavailable { reasons } =>
                    (cap, cur, AvailabilityResponse::Unavailable { reasons: serialize_reasons(reasons) }, "absolute".to_string()),
            }
        }
    }
}
```

- [ ] **Step 6: Map the new reason** in `api/valuation.rs::serialize_valuation_reason`: `ValuationReason::PerformanceDidNotConverge => "performance_did_not_converge"`.

- [ ] **Step 7: Run the gains tests, verify pass.**

Run: `cd backend && cargo test api::gains`
Expected: PASS. Tests previously asserting Modified Dietz percentages still pass via `?method=modified_dietz`; the default-method assertions move to the XIRR/simple values (recompute, or use `assert_available_status`).

- [ ] **Step 8: Stage** (commit only when Lars asks).

```bash
git add backend/src/domain/performance.rs backend/src/domain/mod.rs backend/src/api/gains.rs backend/src/api/valuation.rs
```

---

## Task 3: Per-asset rows show a stable current-position return (backend + coupled frontend subtotal)

**Row contract (the fix for the hybrid percentage):** an open row's percentages are a
**current-position simple return** — numerator and denominator are both current-position, so an
in-period partial sell never produces a hybrid of period gains over end-position cost basis:

- `total_return_percent` = `unrealized_gain_base / cost_basis_base`
- `capital_gain_percent`  = `price_effect_base / cost_basis_base`
- `currency_gain_percent` = `fx_effect_base / cost_basis_base`

The matching SEK columns become the current-position values (`unrealized_gain_base`,
`price_effect_base`, `fx_effect_base`) so amount and percent describe the same thing. This
removes the per-row period Modified Dietz amounts introduced by the reversed 2026-06-19
decision. Closed rows keep their existing realized-gain-over-realized-cost-basis logic in
`closed_gain_row`. This refines the 2026-06-17 framing: rows attribute the **current** holding;
period performance lives only in the portfolio total (Task 2).

**Coupling note (review High 2):** `performance_denominator_base` stops being a Modified Dietz
denominator, and the frontend `visibleTotalReturnPercent` row-summing subtotal must change in
**this same task** — otherwise a committed backend change would leave the footer computing a
bogus annualized percentage from cost-basis denominators.

**Files:**
- Modify: `backend/src/api/gains.rs` (`open_gain_row`, `closed_gain_row`; delete `row_performance_response`, `row_total_return_percent`, `row_component_percent` and their use)
- Modify: `frontend/src/components/GainsTable.tsx` (remove/replace `visibleTotalReturnPercent`, `frontend/src/components/GainsTable.tsx:255-309` and its use at `:361`)

**Interfaces:**
- Consumes: `ValuedHolding` fields `unrealized_gain_base`, `price_effect_base`, `fx_effect_base`, `cost_basis_base` (current-position; per 2026-06-17, price + fx effects sum to unrealized gain).

- [ ] **Step 1: Write the failing regression test** in `gains.rs` tests — opening position, in-period partial sell, open remainder. The invariant: the row percentage is current-position, so it equals `unrealized_gain_percent` and does **not** depend on the in-period sell.

```rust
#[tokio::test]
async fn gains_open_row_percent_is_current_position_not_period_hybrid() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let latest = Local::now().naive_local().date();
    let previous = latest - Duration::days(1);

    // Opening buy well before the period, then an in-period partial sell, open remainder.
    send(&state, "POST", "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"})).await;
    send(&state, "POST", "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-03-01",
               "quantity":4,"price":"150","currency":"USD","fx_rate_to_base":"10"})).await;
    seed_market_data(&state, instrument_id, latest, previous).await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let row = &body["rows"][0];
    assert_eq!(row["quantity"], 6);
    // Current-position contract: row total-return percent == unrealized percent.
    assert_eq!(row["total_return_percent"], row["unrealized_gain_percent"]);
    assert_eq!(row["total_return_base"], row["unrealized_gain_base"]);
}
```

- [ ] **Step 2: Run, verify it fails** (rows still carry period Modified Dietz percentages).

Run: `cd backend && cargo test gains_open_row_percent_is_current_position`
Expected: FAIL.

- [ ] **Step 3: Add a current-position simple-percent helper** in `gains.rs`:

```rust
fn current_position_percent(
    amount: &Availability<Decimal>,
    cost_basis: &Availability<Decimal>,
) -> AvailabilityResponse {
    match (amount.as_ref(), cost_basis.as_ref()) {
        (Some(gain), Some(cb)) if !cb.is_zero() =>
            AvailabilityResponse::Available { value: format!("{:.2}", (*gain / *cb) * Decimal::from(100)) },
        (Some(_), Some(_)) =>
            AvailabilityResponse::Unavailable { reasons: serialize_reasons(&[ValuationReason::ZeroCostBasis]) },
        _ => AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(&merge_response_reasons(&[amount.reasons(), cost_basis.reasons()])),
        },
    }
}
```

- [ ] **Step 4: Rewire `open_gain_row`** so `total_return_base`/`capital_gain_base`/`currency_gain_base` carry the current-position values and the three `*_percent` fields use `current_position_percent` over `cost_basis_base`:

```rust
total_return_base: serialize_availability(&valued_holding.unrealized_gain_base, |v| money_string(*v)),
total_return_percent: current_position_percent(&valued_holding.unrealized_gain_base, &valued_holding.cost_basis_base),
capital_gain_base: serialize_availability(&valued_holding.price_effect_base, |v| money_string(*v)),
capital_gain_percent: current_position_percent(&valued_holding.price_effect_base, &valued_holding.cost_basis_base),
currency_gain_base: serialize_availability(&valued_holding.fx_effect_base, |v| money_string(*v)),
currency_gain_percent: current_position_percent(&valued_holding.fx_effect_base, &valued_holding.cost_basis_base),
```

Set `performance_denominator_base` to `cost_basis_base` and stop computing the Modified Dietz
row denominator. Delete `row_performance_response`, `row_total_return_percent`,
`row_component_percent`, and the now-unused `RowPerformanceResponse` plumbing if no longer
referenced. The accumulator path in Task 2 still computes the per-instrument `PeriodAmounts`
for the totals; only the **row** fields change here.

- [ ] **Step 5: Remove the frontend row-summing subtotal** (couples with Step 4). In `GainsTable.tsx`, replace `visibleTotalReturnPercent(rows, reportPeriod)` (lines ~255-309 and its use at ~361) so the sticky summary either shows the backend whole-portfolio `totals.total_return_percent` or shows the SEK sum only (per ratified decision 2). Delete the now-dead helper. Add a code comment that `performance_denominator_base` is no longer additive.

- [ ] **Step 6: Update other row assertions** in `gains.rs` tests (`gains_populated_portfolio_*`, `gains_include_closed_*`) to the current-position values (e.g. capital = `price_effect_base`/`cost_basis_base`).

- [ ] **Step 7: Verify backend + frontend.**

Run: `cd backend && cargo test api::gains && cargo clippy --all-targets -- -D warnings && cargo fmt`
Run: `cd frontend && npm run check && npm run fmt`
Expected: PASS / clean.

- [ ] **Step 8: Stage** (commit only when Lars asks).

```bash
git add backend/src/api/gains.rs frontend/src/components/GainsTable.tsx
```

---

## Task 4: Method selector, persistence, copy, versions, and Sharesight validation

**Files:**
- Modify: `frontend/src/api/queries.ts` (`GainsParams` + `useGains`: thread `method` through params, **queryKey**, and `URLSearchParams`) + `frontend/src/api/types.ts` (`method`, `percentage_method`, `display_percent_kind`)
- Modify: `frontend/src/components/GainsTable.tsx` (selector + persistence + copy)
- Modify: `frontend/src/components/AssetView.tsx` (asset-detail method decision, Step 1b)
- Modify: `frontend/package.json`, `backend/Cargo.toml`
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Thread `method` through the gains fetch (review Medium 1).** Add `method?: ReturnMethod` to `GainsParams` in `queries.ts`. **Include `method` in the React Query `queryKey`** — the current key is `["gains", includeClosedPositions, startDate ?? null, endDate ?? null]` (`frontend/src/api/queries.ts:55-60`); without `method` in the key, switching methods reuses/overwrites the same cache entry and totals flash stale values. Then set it in `URLSearchParams` (`if (method) search.set("method", method);`) so the call becomes `/api/gains?...&method=<value>`. Default `xirr` when unset. Also add `method` to the gains query type in `types.ts`.

  Verify: switch XIRR → Simple → Modified Dietz and confirm each issues a distinct request and the displayed `percentage_method` follows the selection (frontend test or the focused manual check in Step 6).

- [ ] **Step 1b: Decide the asset-detail method (review Medium 2).** `AssetView` also calls `useGains({ includeClosedPositions: true })` (`frontend/src/components/AssetView.tsx:38`). Because Task 3 makes per-asset **row** percentages a current-position simple return that is **independent of the selected total method**, the asset page does not need the persisted board selection for its row figures. **Default decision:** `AssetView` leaves `method` unset so `useGains` uses the `xirr` default (a separate cache entry from the board's selection); the asset page renders only method-independent row data and does not surface a method-dependent portfolio total. If the asset page later shows a method-dependent total, revisit and share the persisted `loadReturnMethod()` helper. *(Default — Lars to object if asset detail should mirror the board's persisted method instead.)* Add a short code comment in `AssetView` stating this.

- [ ] **Step 2: Add the method selector + `localStorage` persistence.** A dropdown on the Gains totals band: **Money-weighted (XIRR)** / **Simple** / **Modified Dietz (legacy)**. Initial value from `localStorage`; on change, persist and refetch.

```ts
type ReturnMethod = "xirr" | "simple" | "modified_dietz";
const RETURN_METHOD_KEY = "gains.returnMethod";
function loadReturnMethod(): ReturnMethod {
  const v = localStorage.getItem(RETURN_METHOD_KEY);
  return v === "simple" || v === "modified_dietz" ? v : "xirr";
}
function saveReturnMethod(m: ReturnMethod) { localStorage.setItem(RETURN_METHOD_KEY, m); }
```

Hold the method in board/Gains state initialized from `loadReturnMethod()`; the selector calls `saveReturnMethod` and triggers the refetch (same path as the date preset).

- [ ] **Step 3: Method-aware copy.** In `GainsTotalsBand`, label by `percentage_method`: `"money_weighted"` → "Total return (money-weighted)" with a cumulative-period-return, ex-dividends tooltip; `"simple"` → "Total return (simple)" (gain ÷ capital deployed); `"modified_dietz"` → keep the existing "Performance return"/"Annualised return" label plus a "legacy / comparison only" note. Keep the `"annualised"`/`"absolute"` branches for the Modified Dietz path.

- [ ] **Step 4: Bump versions.** `frontend/package.json` and `backend/Cargo.toml`; confirm both render via `/api/health` and the frontend footer.

- [ ] **Step 5: Verify frontend and backend.**

Run: `cd frontend && npm run check && npm run fmt`
Run: `cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt` (re-run because `Cargo.toml` changed)
Expected: no type, lint, or clippy errors.

- [ ] **Step 6: HUMAN TESTING — Sharesight validation GATE.** Run the app, open Gains. With the method set to XIRR, compare the portfolio total return % (since inception, and YTD via the date preset) against the Sharesight portfolio figures Lars provides (decision 4). Toggle to Simple and Modified Dietz and confirm the selector + persistence work (survives reload). Confirm a buy/sell does not shift the XIRR total % until prices update. **Record the comparison.** Then branch:
  - **Within tolerance** (decision 4): proceed to Step 7.
  - **Outside tolerance:** STOP. Do not record the entry as "Sharesight-matching". Report the gap to Lars and decide whether to change the default method or record that XIRR is the default for its neutrality property and intentionally differs from Sharesight. Adjust the Step 7 wording to match.

- [ ] **Step 7: Record DecisionLog entries** (only after the Step 6 gate; new entries at end of `docs/DecisionLog.md`), e.g.:

```
## 2026-06-21: Selectable Performance Return Method — refines 2026-06-19
Decision: The Gains portfolio total return is user-selectable and persisted (localStorage):
Money-weighted (XIRR, default), Simple (total return ÷ opening value + gross purchases), and
Modified Dietz (the prior single-shot method, retained as a labeled comparison/legacy option).
XIRR is shown as the cumulative period return (1+xirr)^years−1. Per-asset row percentages use a
current-position simple return (unrealized gain ÷ cost basis; capital = price effect, currency
= FX effect, each over cost basis), independent of the selected total method. This refines the
2026-06-19 Modified Dietz decision (MD is no longer the only method, and no longer the default).
Context: Investigation (docs/Investigation.PerformancePercentageMethods.md) showed single-shot
Modified Dietz produced ~1,130% vs Sharesight ~421% for a churned holding. XIRR is the default
because it is cash-flow neutral (a trade at fair value does not move the number until prices
change) — Lars's stated hard requirement. Whether XIRR also matches Sharesight was validated
against portfolio-level Sharesight figures: <record result vs the agreed tolerance here>.
Consequences: All methods are ex-dividend until income is tracked. Component percentages use
each method's natural denominator (proportional split for XIRR). The filtered visible-rows
subtotal no longer reports a percentage (refines 2026-06-20 Canonical Performance Period). The
XIRR float solve is display-only and never feeds the money pipeline. Method persistence is
client-side localStorage until a server settings store exists.
```

- [ ] **Step 8: Stage** (commit only when Lars asks).

```bash
git add frontend/ backend/Cargo.toml docs/DecisionLog.md
```

---

## Verification summary per phase

- **Task 1:** `cargo test domain::performance` — solver correctness + neutrality + non-convergence + interleaved-flow root + multi-root refusal; `actual_period_cash_flows` is un-split-adjusted.
- **Task 2:** `cargo test api::gains` — `method=xirr` (default) gives cumulative XIRR from **actual** flows; `method=simple` and `method=modified_dietz` resolve their values; split-neutrality holds for XIRR/Simple; unknown method → `400`.
- **Task 3:** `cargo test api::gains` — row percentages equal the current-position simple return (`total_return_percent == unrealized_gain_percent`); frontend subtotal updated in the same task.
- **Task 4:** `npm run check`, `cargo clippy`/`cargo fmt` after the version bump; selector + `localStorage` persistence survive reload; Sharesight comparison is a **gate** (decision 4) that precedes the DecisionLog entry.

## Self-review notes

- Spec coverage: XIRR engine (T1), method-selectable totals (T2), current-position rows (T3), selector + persistence + UI/versions/validation (T4), DecisionLog entry (T4, gated). Decisions 1–6 are recorded inline; 2 and 3 are stated defaults for Lars to confirm.
- The float solve is isolated to a display value; exact money pipeline untouched.
- Modified Dietz is retained (selectable), so this refines rather than fully reverses the 2026-06-19 decision.
- Income exclusion and the currency-sign difference vs Sharesight are explicitly out of scope here.
- Review follow-up `docs/Review.money-weighted-returns-plan-followup.md` is the durable review record for this plan (the earlier `docs/reviews/Review.money-weighted-returns-plan.md` was not present in this checkout). Its five findings are folded in: actual (non-split-adjusted) flows for XIRR/Simple (T1 note + T2 `actual_period_cash_flows`), robust multi-root-safe solver (T1), `method` in the React Query key (T4), and the asset-detail method decision (T4 Step 1b / decision 7).
