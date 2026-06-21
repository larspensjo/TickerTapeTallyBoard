# Money-Weighted (XIRR) Performance Returns Implementation Plan

> **For agentic workers:** implement task-by-task. Steps use checkbox (`- [ ]`) syntax.
> Plans are ephemeral; archive when implemented. Do not reference plan phases from durable docs.

**Goal:** Replace the single-shot Modified Dietz return percentage with a money-weighted
(XIRR) return for portfolio totals, and a stable current-position cost-basis percentage for
per-asset rows, so the headline "since start / YTD" number is cash-flow-neutral and stable.

XIRR is chosen because it provably satisfies the **hard requirement Lars stated**: a buy,
sell, or transfer at current value must not move the number until prices change (verified —
see the neutrality test in Task 1). Whether XIRR also *matches Sharesight* is a separate
**hypothesis**: the investigation found de-annualized IRR was ~688% for one holding while
Sharesight showed ~421%, and no single-pass method reproduced Sharesight exactly. So "follows
Sharesight" is not assumed — it is validated against portfolio-level Sharesight figures in
Task 4 before the DecisionLog reversal is recorded (see decision 4).

**Architecture:** Add a pure XIRR solver to `backend/src/domain/performance.rs`. Aggregate
the existing per-instrument period inputs (begin market value, dated cash flows, end market
value) across the portfolio and solve one XIRR for the whole portfolio per report period.
Display the *cumulative* period return `(1+xirr)^years − 1`, not the annualized rate. Per-asset
row percentages switch to a current-position simple return (`unrealized_gain ÷ cost_basis`),
keeping numerator and denominator in the same scope.

**Tech Stack:** Rust (axum, rust_decimal), React/TypeScript frontend.

## Global Constraints

- Build from `backend/`: `cargo build`; finish with `cargo clippy --all-targets -- -D warnings` then `cargo fmt`.
- Frontend from `frontend/`: `npm run check` then `npm run fmt`.
- Money/prices/FX stay exact `rust_decimal` round-tripped as TEXT (2026-06-14 Persistence Stack). XIRR's float solve is a derived display value only and must never feed stored money.
- Missing data is explicit `Availability::Unavailable { reasons }`, never zero.
- Bump `frontend/package.json` and `backend/Cargo.toml` versions; both surface in the UI.
- Income/dividends remain excluded (not persisted per 2026-06-16 Avanza decision); the XIRR is ex-dividend, matching the current gain amounts. State this in UI copy.

## Decisions to ratify before/while implementing

These reverse or refine committed DecisionLog entries. Confirm with Lars; record the outcome
in `docs/DecisionLog.md` as new entries (never edit committed ones).

1. **Reverses 2026-06-19** (Modified Dietz for totals AND rows): totals move to XIRR; rows move to cost-basis simple return.
2. **Refines 2026-06-20** (additive row subtotals): XIRR is **not** additive from row contributions, so the frontend can no longer compute a filtered-visible total percentage by summing rows. The portfolio total percentage comes from the backend only. The visible-rows sticky summary keeps the SEK sum but drops the client-computed percentage (or shows the backend whole-portfolio figure). **Open question for Lars: is dropping the filtered % acceptable?**
3. **Component split:** `capital_gain_percent` and `currency_gain_percent` are shown as a proportional split of the total return percentage (`total% × component_SEK / total_return_SEK`) so they stay additive and avoid a second denominator. **Open question for Lars: acceptable, or omit component percentages entirely?**
4. **Validation gate (not just a target):** Lars provides Sharesight's **portfolio-level** total return (since inception and YTD) and the configured Sharesight method. Task 4 compares our cumulative XIRR against those figures. Agree a tolerance up front (suggest **±3 percentage points absolute, or ±5% relative**, whichever is larger). If XIRR is inside tolerance, record the DecisionLog reversal as written. If it misses, **stop before recording the reversal** and either (a) revise the method, or (b) record explicitly that the app intentionally differs from Sharesight and uses XIRR for its neutrality property. XIRR is the implementation hypothesis until this gate passes.

---

## File Structure

- `backend/src/domain/performance.rs` — add `MoneyWeightedReturn` struct + `compute_money_weighted_return`; keep Modified Dietz functions for now (remove dead ones in cleanup).
- `backend/src/domain/valuation.rs` — add `ValuationReason::PerformanceDidNotConverge`.
- `backend/src/domain/mod.rs` — export the new symbols.
- `backend/src/api/gains.rs` — accumulate aggregate `end_mv`; compute totals percentage via XIRR; switch row percentages to cost-basis; map the new reason.
- `frontend/src/api/types.ts` — no shape change (string percentages); `display_percent_kind` gains `"money_weighted"`.
- `frontend/src/components/GainsTable.tsx` — label/tooltip copy; remove/replace `visibleTotalReturnPercent`.

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
```

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
/// Builds the dated investor cash-flow series:
///   -begin_mv at start_date, -cf.amount_base at each flow date, +end_mv at end_date,
/// solves NPV(rate)=0 by bisection, and reports both the annualized rate and the
/// cumulative period return (1+rate)^years - 1. Returns Unavailable when any input is
/// unavailable, the period is non-positive, or no root brackets in [-0.9999, 1_000_000].
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

    let (lo, hi) = (-0.9999_f64, 1_000_000.0_f64);
    let (flo, fhi) = (npv(lo), npv(hi));
    if flo.is_nan() || fhi.is_nan() || flo * fhi > 0.0 {
        return Availability::unavailable(ValuationReason::PerformanceDidNotConverge);
    }
    let (mut a, mut b) = (lo, hi);
    for _ in 0..300 {
        let m = (a + b) / 2.0;
        if npv(m) > 0.0 { a = m; } else { b = m; }
    }
    let rate = (a + b) / 2.0;
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
Expected: PASS (including the three new tests).

- [ ] **Step 7: Stage** (commit only when Lars asks — repo planning workflow keeps changes staged for review).

```bash
git add backend/src/domain/performance.rs backend/src/domain/valuation.rs backend/src/domain/mod.rs
```

---

## Task 2: Portfolio totals use XIRR (cumulative)

**Files:**
- Modify: `backend/src/api/gains.rs` (`PerformanceAccumulator`, `into_percents`, reason serialization)
- Modify: `backend/src/api/valuation.rs` (`serialize_valuation_reason` for the new variant)

**Interfaces:**
- Consumes: `compute_money_weighted_return`, `MoneyWeightedReturn` from Task 1.

- [ ] **Step 1: Accumulate aggregate end market value.** Add `end_mv: Decimal` to `PerformanceAccumulator` and, in `add(...)`, when the row is fully available, `self.end_mv += end;` reading `amounts.end_market_value_base`. (Add `end_market_value_base` to the match tuple.)

- [ ] **Step 2: Update the failing test** `gains_with_date_range_returns_modified_dietz_percent` (rename to `..._returns_money_weighted_percent`) to assert the cumulative XIRR. For the 100 sh @ $10→$12, FX flat 10, full June (29 days) inception case, begin_mv=0, one buy of 10_000 at start, end_mv=12_000 → cumulative ≈ 20% but XIRR annualizes over 29 days then de-annualizes back, so cumulative is exactly the period return 20.00%. Assert:

```rust
assert_available(&body["totals"]["total_return_percent"], "20.00");
assert_eq!(body["percentage_method"], "money_weighted");
```

Also add a backend test (unit-level on `into_percents`, or an API case) where capital gain and
currency gain offset to a **zero total return** with non-zero components, asserting both
component percentages are `unavailable` (not `0.00`) and `total_return_percent` is `0.00`.

- [ ] **Step 3: Run, verify it fails** (still Modified Dietz).

Run: `cd backend && cargo test gains_with_date_range`
Expected: FAIL.

- [ ] **Step 4: Rewrite `into_percents`** to use XIRR for the total and a proportional split for components:

```rust
let mw = compute_money_weighted_return(
    &Availability::Available(self.begin_mv),
    &Availability::Available(self.cash_flows.clone()),
    &Availability::Available(self.end_mv),
    effective_start_date,
    end_date,
);
let (total_pct, kind) = match &mw {
    Availability::Available(v) => (v.cumulative, "money_weighted".to_string()),
    Availability::Unavailable { reasons } => {
        let u = AvailabilityResponse::Unavailable { reasons: serialize_reasons(reasons) };
        return (u.clone(), u.clone(), u, "money_weighted".to_string());
    }
};
// Proportional, additive component split (avoids a second denominator).
// When total_return == 0 the split is undefined. Only report 0.00 if BOTH
// components are also zero; otherwise mark unavailable rather than hiding
// offsetting capital/currency attribution (e.g. +X capital, −X currency)
// behind a fake 0.00.
let comp = |part: Decimal| -> AvailabilityResponse {
    if self.total_return.is_zero() {
        if self.capital_gain.is_zero() && self.currency_gain.is_zero() {
            return AvailabilityResponse::Available { value: "0.00".to_string() };
        }
        return AvailabilityResponse::Unavailable {
            reasons: serialize_reasons(&[ValuationReason::ZeroOrInvalidPerformanceDenominator]),
        };
    }
    let pct = total_pct * (part / self.total_return) * Decimal::from(100);
    AvailabilityResponse::Available { value: format!("{:.2}", pct) }
};
(
    comp(self.capital_gain),
    comp(self.currency_gain),
    AvailabilityResponse::Available { value: format!("{:.2}", total_pct * Decimal::from(100)) },
    kind,
)
```

Update `percentage_method` in the response from `"modified_dietz"` to `"money_weighted"` ([gains.rs:267](backend/src/api/gains.rs#L267)).

- [ ] **Step 5: Map the new reason** in `api/valuation.rs::serialize_valuation_reason`: `ValuationReason::PerformanceDidNotConverge => "performance_did_not_converge"`.

- [ ] **Step 6: Run the gains tests, verify pass.**

Run: `cd backend && cargo test --test '*' gains; cargo test api::gains`
Expected: PASS. Fix any other tests asserting `modified_dietz` or specific MD percentages (e.g. `gains_populated_portfolio_*`, `gains_totals_include_closed_*`) by recomputing the expected cumulative value or switching to `assert_available_status`.

- [ ] **Step 7: Stage** (commit only when Lars asks).

```bash
git add backend/src/api/gains.rs backend/src/api/valuation.rs
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

## Task 4: Frontend wiring, copy, versions, and Sharesight validation

**Files:**
- Modify: `frontend/src/components/GainsTable.tsx`
- Modify: `frontend/package.json`, `backend/Cargo.toml`
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Update label/tooltip copy.** In `GainsTotalsBand`, map `displayPercentKind === "money_weighted"` to label `"Total return (money-weighted)"` and a tooltip noting it is the cumulative period return, ex-dividends. Keep the `"annualised"`/`"absolute"` branches for safety. (The filtered sticky-summary subtotal was already handled in Task 3.)

- [ ] **Step 2: Bump versions.** `frontend/package.json` and `backend/Cargo.toml` (patch or minor as appropriate); confirm both render via `/api/health` and the frontend footer.

- [ ] **Step 3: Verify frontend and backend.**

Run: `cd frontend && npm run check && npm run fmt`
Run: `cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt` (re-run because `Cargo.toml` changed)
Expected: no type, lint, or clippy errors.

- [ ] **Step 4: HUMAN TESTING — Sharesight validation GATE.** Run the app (`run` skill or manual), open Gains. Compare the portfolio total return % (since inception, and YTD via the date preset) against the Sharesight portfolio figures Lars provides (decision 4). Confirm a buy/sell in the app does not shift the total % until prices update. **Record the comparison.** Then branch:
  - **Within tolerance** (decision 4): proceed to Step 5.
  - **Outside tolerance:** STOP. Do not record the reversal as "Sharesight-style". Report the gap to Lars and decide whether to revise the method or record that the app intentionally differs (using XIRR for its neutrality property). Adjust the Step 5 wording to match the outcome.

- [ ] **Step 5: Record DecisionLog entries** (only after the Step 4 gate; new entries at end of `docs/DecisionLog.md`), e.g.:

```
## 2026-06-21: Money-Weighted (XIRR) Performance Returns — reverses 2026-06-19
Decision: Gains portfolio totals use a money-weighted (XIRR) return, displayed as the
cumulative period return (1+xirr)^years−1, computed once over the whole portfolio's dated
cash flows plus begin/end market value. Per-asset row percentages use a current-position
simple return (unrealized gain ÷ cost basis; capital = price effect, currency = FX effect,
each over cost basis). This reverses the 2026-06-19 Modified Dietz method.
Context: Investigation (docs/Investigation.PerformancePercentageMethods.md) showed single-shot
Modified Dietz produced ~1,130% vs Sharesight ~421% for a churned holding. XIRR was chosen
because it is cash-flow neutral (a trade at fair value does not move the number until prices
change) — Lars's stated hard requirement. Whether XIRR also matches Sharesight was validated
against portfolio-level Sharesight figures: <record result vs the agreed tolerance here>.
Consequences: Returns are ex-dividend until income is tracked. Component percentages are a
proportional split of the total (unavailable when total return is zero but components are not).
Row subtotals are no longer additive from row contributions (refines 2026-06-20 Canonical
Performance Period). XIRR float solve is display-only and never feeds the money pipeline.
```

- [ ] **Step 6: Stage** (commit only when Lars asks).

```bash
git add frontend/ backend/Cargo.toml docs/DecisionLog.md
```

---

## Verification summary per phase

- **Task 1:** `cargo test domain::performance` — solver correctness + neutrality + non-convergence.
- **Task 2:** `cargo test api::gains` — totals percentage is cumulative XIRR; `percentage_method == "money_weighted"`.
- **Task 3:** `cargo test api::gains` — row percentages equal the current-position simple return (`total_return_percent == unrealized_gain_percent`); frontend subtotal updated in the same task.
- **Task 4:** `npm run check`, `cargo clippy`/`cargo fmt` after the version bump; Sharesight comparison is a **gate** (decision 4) that precedes the DecisionLog entry.

## Self-review notes

- Spec coverage: XIRR engine (T1), totals (T2), rows (T3), UI/versions/validation (T4), DecisionLog reversal (T4). Open decisions 1–4 are flagged inline, not silently resolved.
- The float solve is isolated to a display value; exact money pipeline untouched.
- Income exclusion and the currency-sign difference vs Sharesight are explicitly out of scope here.
