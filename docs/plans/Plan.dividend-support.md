# Dividend Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enable recording dividend transactions and surface income as a distinct, computable component in performance calculations and the gains waterfall.

**Architecture:** Dividends reuse the existing `LedgerTransaction` field layout (`quantity` = shares eligible, `price` = per-share amount, `currency`, `fx_rate_to_base`). Performance calculations add `income_base` to `PeriodAmounts`; `currency_gain = total_return − capital_gain − income`. The gains API exposes `income_base` per row and in totals; the waterfall replaces its placeholder with a real effect bar.

**Tech Stack:** Rust/axum backend (`rust_decimal`), React/TypeScript frontend (no new dependencies).

## Global Constraints

- Base currency is SEK; SEK instruments use `fx = 1`. Non-SEK dividends may omit `fx_rate_to_base` (same as buy/sell behavior): the transaction is stored, but `income_base` becomes `Unavailable` until FX is supplied. Write-time validation only rejects an FX value that is explicitly provided but ≤ 0.
- Money stored as TEXT strings; exact `rust_decimal` arithmetic throughout the backend.
- No brokerage on dividends (ISK account; no tax withheld). The `brokerage_base` field is rejected if present and non-zero.
- Import adapters (Avanza, Sharesight) that currently skip dividends are **out of scope for this plan** — they remain skipped.
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` must pass from `backend/` after each backend task.
- `npm run check` and `npm run fmt` must pass from `frontend/` after each frontend task.

---

## File Map

| File | Change |
|------|--------|
| `backend/src/domain/transaction.rs` | Validate `Dividend`; remove `DividendNotSupported` |
| `backend/src/api/transactions.rs` | Accept `Dividend`; apply currency-match + ledger-valid guards |
| `backend/src/domain/performance.rs` | Add `income_base` to `PeriodAmounts`; update `compute_period_amounts`, `period_cash_flows`, `actual_period_cash_flows` |
| `backend/src/api/gains.rs` | Add `income_base` to `GainRow` and `TotalsResponse`; aggregate income |
| `frontend/src/api/types.ts` | Add `income_base: MoneyValue` to `GainsRow`; `income_base/income_percent` already exist in `GainsTotals` |
| `frontend/src/components/AddTransactionForm.tsx` | Add "Dividend" to type selector; show appropriate fields |
| `frontend/src/components/waterfallViewModel.ts` | Replace `placeholderRow("dividends")` with real `pushEffect` |
| `frontend/src/components/waterfallViewModel.test.ts` | Add test for dividend income row |
| `docs/DecisionLog.md` | Record dividend model decision |

---

## Task 1: Domain Validation For Dividends

**Files:**
- Modify: `backend/src/domain/transaction.rs`

**Interfaces:**
- Removes: `ValidationError::DividendNotSupported`
- Produces: `validate(&ProposedTransaction { kind: Dividend, quantity: N, price: Some(P), currency: Some(C), brokerage_base: None/Some(0), .. })` returns `Ok(0)` (zero position effect)

- [ ] **Step 1: Write failing tests for valid and invalid dividend proposals**

In `backend/src/domain/transaction.rs`, inside the `#[cfg(test)] mod tests` block, add after the existing `dividend_is_not_supported` test (which will be replaced):

```rust
#[test]
fn dividend_with_valid_fields_succeeds() {
    let d = ProposedTransaction {
        kind: TransactionKind::Dividend,
        trade_date: date(),
        quantity: 10,
        price: Some(dec!(0.50)),
        currency: Some("USD".to_owned()),
        fx_rate_to_base: Some(dec!(10.5)),
        brokerage_base: None,
    };
    assert_eq!(validate(&d), Ok(0));
}

#[test]
fn dividend_without_quantity_is_rejected() {
    let d = ProposedTransaction {
        kind: TransactionKind::Dividend,
        quantity: 0,
        price: Some(dec!(0.50)),
        currency: Some("USD".to_owned()),
        fx_rate_to_base: None,
        brokerage_base: None,
        trade_date: date(),
    };
    assert_eq!(validate(&d), Err(ValidationError::QuantityMustBePositive));
}

#[test]
fn dividend_without_price_is_rejected() {
    let d = ProposedTransaction {
        kind: TransactionKind::Dividend,
        quantity: 10,
        price: None,
        currency: Some("USD".to_owned()),
        fx_rate_to_base: None,
        brokerage_base: None,
        trade_date: date(),
    };
    assert_eq!(validate(&d), Err(ValidationError::PriceRequired));
}

#[test]
fn dividend_without_currency_is_rejected() {
    let d = ProposedTransaction {
        kind: TransactionKind::Dividend,
        quantity: 10,
        price: Some(dec!(0.50)),
        currency: None,
        fx_rate_to_base: None,
        brokerage_base: None,
        trade_date: date(),
    };
    assert_eq!(validate(&d), Err(ValidationError::CurrencyRequired));
}

#[test]
fn dividend_with_brokerage_is_rejected() {
    let d = ProposedTransaction {
        kind: TransactionKind::Dividend,
        quantity: 10,
        price: Some(dec!(0.50)),
        currency: Some("USD".to_owned()),
        fx_rate_to_base: None,
        brokerage_base: Some(dec!(5.00)),
        trade_date: date(),
    };
    assert_eq!(validate(&d), Err(ValidationError::DividendMustNotCarryBrokerage));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd backend && cargo test domain::transaction::tests 2>&1 | head -40
```
Expected: compilation errors (unknown variant, unknown error variant).

- [ ] **Step 3: Update `ValidationError`, remove `DividendNotSupported`, add `DividendMustNotCarryBrokerage`**

In `backend/src/domain/transaction.rs`:

Remove `DividendNotSupported` from the enum, and add `DividendMustNotCarryBrokerage`:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationError {
    QuantityMustBePositive,
    PriceRequired,
    PriceMustBePositive,
    CurrencyRequired,
    FxRateMustBePositive,
    BrokerageMustNotBeNegative,
    SplitQuantityMustBeNonZero,
    SplitMustNotCarryCostInputs,
    DividendMustNotCarryBrokerage,
}
```

Update `code()`:
```rust
Self::DividendMustNotCarryBrokerage => "dividend_must_not_carry_brokerage",
```

Update `message()`:
```rust
Self::DividendMustNotCarryBrokerage => {
    "Dividend must not carry brokerage."
}
```

- [ ] **Step 4: Implement dividend validation in `validate()`**

Replace the `TransactionKind::Dividend => Err(ValidationError::DividendNotSupported)` arm with:

```rust
TransactionKind::Dividend => {
    if proposed.quantity <= 0 {
        return Err(ValidationError::QuantityMustBePositive);
    }
    match proposed.price {
        None => return Err(ValidationError::PriceRequired),
        Some(price) if price <= Decimal::ZERO => {
            return Err(ValidationError::PriceMustBePositive);
        }
        Some(_) => {}
    }
    if proposed
        .currency
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        return Err(ValidationError::CurrencyRequired);
    }
    if proposed
        .fx_rate_to_base
        .is_some_and(|fx| fx <= Decimal::ZERO)
    {
        return Err(ValidationError::FxRateMustBePositive);
    }
    if proposed
        .brokerage_base
        .is_some_and(|b| b != Decimal::ZERO)
    {
        return Err(ValidationError::DividendMustNotCarryBrokerage);
    }
    Ok(0) // dividend has no position quantity effect
}
```

Also delete the old test `fn dividend_is_not_supported()`.

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cd backend && cargo test domain::transaction::tests
```
Expected: all green, no compilation errors.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
```

```bash
git add backend/src/domain/transaction.rs
git commit -m "$(cat <<'EOF'
feat(domain): validate dividend transactions

Removes DividendNotSupported; dividends accept quantity, price, currency,
and optional FX; brokerage is rejected. validate() returns Ok(0) (zero
position effect). Adds DividendMustNotCarryBrokerage validation error.
EOF
)"
```

---

## Task 2: API Acceptance Of Dividend Transactions

**Files:**
- Modify: `backend/src/api/transactions.rs`

**Interfaces:**
- Consumes: `ValidationError::DividendMustNotCarryBrokerage` from Task 1
- Produces: `POST /api/transactions` with `type: "Dividend"` creates a stored row and returns 201

- [ ] **Step 1: Write a failing integration test**

In `backend/src/api/transactions.rs`, inside `#[cfg(test)] mod tests`, replace the existing `async fn dividend_is_rejected()` test with:

```rust
#[tokio::test]
async fn dividend_round_trips_through_list() {
    let state = AppState::for_tests().await;
    let instrument_id = create_instrument(&state).await;
    // First add some shares so the dividend makes sense (not required by API but realistic)
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":100,"price":"12.50","currency":"USD","fx_rate_to_base":"10.5"}),
    )
    .await;

    let (status, created) = send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
               "quantity":100,"price":"0.25","currency":"USD","fx_rate_to_base":"10.5"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["type"], "Dividend");
    assert_eq!(created["quantity"], 100);
    assert_eq!(created["price"], "0.25");
    assert_eq!(created["currency"], "USD");

    let (status, list) = send(&state, "GET", "/api/transactions", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list.as_array().expect("array").len(), 2);
}

#[tokio::test]
async fn dividend_with_brokerage_is_rejected() {
    let state = AppState::for_tests().await;
    let instrument_id = create_instrument(&state).await;
    let (status, error) = send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
               "quantity":100,"price":"0.25","currency":"USD","brokerage":"5.00"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["error"]["code"], "dividend_must_not_carry_brokerage");
}
```

- [ ] **Step 2: Run the tests to confirm they fail**

```bash
cd backend && cargo test api::transactions::tests::dividend 2>&1 | head -30
```
Expected: `dividend_round_trips_through_list` fails with 422 (dividend rejected), `dividend_with_brokerage_is_rejected` fails with wrong code.

- [ ] **Step 3: Confirm `DividendMustNotCarryBrokerage` is covered by the generic validation-error conversion**

In `backend/src/api/error.rs`, the existing `impl From<ValidationError> for ApiError` already maps every variant through `error.code()` and `error.message()` — no exhaustive match arm needs to be added. Verify the generic conversion is present and that `DividendMustNotCarryBrokerage` therefore needs no change to `error.rs`.

The `dividend_with_brokerage_is_rejected` API test (Step 1) asserting `422` and `"dividend_must_not_carry_brokerage"` is the verification for this step.

- [ ] **Step 4: Extend `assert_currency_matches` and `transaction_currency` for Dividend**

In `backend/src/api/transactions.rs`:

Update `assert_currency_matches` — include `Dividend` alongside `Buy | Sell`:

```rust
fn assert_currency_matches(
    proposed: &ProposedTransaction,
    instrument: &InstrumentRow,
) -> Result<(), ApiError> {
    if !matches!(proposed.kind, TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend) {
        return Ok(());
    }
    // ... rest unchanged
```

Update `transaction_currency` — include `Dividend` alongside `Buy | Sell`:

```rust
fn transaction_currency(
    proposed: &ProposedTransaction,
    instrument: &InstrumentRow,
) -> Option<String> {
    if matches!(proposed.kind, TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend) {
        Some(instrument.currency.clone())
    } else {
        proposed.currency.clone()
    }
}
```

- [ ] **Step 5: Run tests to confirm they pass**

```bash
cd backend && cargo test api::transactions::tests
```
Expected: all green including both new dividend tests.

- [ ] **Step 6: Clippy + fmt + commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
```

```bash
git add backend/src/api/transactions.rs backend/src/api/error.rs
git commit -m "$(cat <<'EOF'
feat(api): accept Dividend transactions via POST /api/transactions

Dividend passes currency-match and ledger-valid guards. Brokerage on a
dividend returns 422 dividend_must_not_carry_brokerage. position effect
is zero (no ledger quantity change).
EOF
)"
```

---

## Task 3: Performance Calculations Include Dividend Income

**Files:**
- Modify: `backend/src/domain/performance.rs`

**Interfaces:**
- Consumes: `TransactionKind::Dividend` already exists in the enum
- Produces:
  - `PeriodAmounts.income_base: Availability<Decimal>` (new field, total dividend income in base for the period)
  - `compute_period_amounts(...)` updated return
  - `period_cash_flows(...)` includes dividends as negative flows
  - `actual_period_cash_flows(...)` includes dividends as negative flows
  - `currency_gain_base = total_return_base − capital_gain_base − income_base`

**Key formula:** Dividend income in base = `quantity × price × fx_rate_to_base` (or `× 1` for SEK instruments). Missing FX on a dividend for a non-SEK instrument makes the whole `PeriodAmounts` unavailable (consistent with buy/sell missing-FX behavior).

- [ ] **Step 1: Write failing tests for dividend income in period amounts**

In `backend/src/domain/performance.rs`, inside `#[cfg(test)] mod tests`, add a helper and tests:

```rust
fn dividend_tx(id: i64, d: &str, qty: i64, price: Decimal, fx: Option<Decimal>) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: date(d),
        kind: TransactionKind::Dividend,
        quantity: qty,
        price: Some(price),
        fx_rate_to_base: fx,
        brokerage_base: Decimal::ZERO,
    }
}

#[test]
fn period_amounts_dividend_adds_to_total_return_and_income() {
    // 100 shares held from before start; price flat at 10 USD / FX 10; dividend $0.25/share on Jun 15
    let txs = vec![
        buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO),
        dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
    ];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    // price flat, FX flat → capital_gain = 0, currency_gain = 0
    // income = 100 × 0.25 × 10 = 250
    // total_return = 250
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(10)), Some(dec!(10)), Some(dec!(10)), false);
    assert_eq!(avail(&a.income_base), dec!(250));
    assert_eq!(avail(&a.total_return_base), dec!(250));
    assert_eq!(avail(&a.capital_gain_base), dec!(0));
    assert_eq!(avail(&a.currency_gain_base), dec!(0));
}

#[test]
fn period_amounts_dividend_components_sum_to_total_return() {
    // 100 shares; price 10→12 USD; FX 10→11; dividend $0.25/share at FX 10
    let txs = vec![
        buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO),
        dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
    ];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(12)), Some(dec!(10)), Some(dec!(11)), false);
    // capital: (12-10)*100*11 = 2200; currency: 10*100*(11-10)=1000; income: 100*0.25*10=250
    // total = 2200+1000+250 = 3450
    let capital = avail(&a.capital_gain_base);
    let currency = avail(&a.currency_gain_base);
    let income = avail(&a.income_base);
    let total = avail(&a.total_return_base);
    assert_eq!(capital + currency + income, total);
}

#[test]
fn period_cash_flows_include_dividend_as_negative_flow() {
    // Dividend = cash out of investment → negative amount_base
    let txs = vec![
        buy_with_fx(1, "2026-06-05", 100, dec!(10), dec!(10), Decimal::ZERO),
        dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
    ];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let flows = match period_cash_flows(&p, false) {
        Availability::Available(v) => v,
        _ => panic!("expected available"),
    };
    // buy: +10_000 (post-period split factor = 1 here); dividend: -250
    assert_eq!(flows.len(), 2);
    let buy_flow = flows.iter().find(|f| f.amount_base > Decimal::ZERO).expect("buy flow");
    let div_flow = flows.iter().find(|f| f.amount_base < Decimal::ZERO).expect("dividend flow");
    assert_eq!(buy_flow.amount_base, dec!(10000));
    assert_eq!(div_flow.amount_base, dec!(-250));
}
```

- [ ] **Step 2: Run tests to confirm they fail**

```bash
cd backend && cargo test domain::performance::tests::period_amounts_dividend 2>&1 | head -30
```
Expected: compiler errors because `income_base` field doesn't exist.

- [ ] **Step 3: Add `income_base` field to `PeriodAmounts`**

In `backend/src/domain/performance.rs`, update the struct and its `unavailable` constructor:

```rust
pub struct PeriodAmounts {
    pub begin_market_value_base: Availability<Decimal>,
    pub end_market_value_base: Availability<Decimal>,
    pub capital_gain_base: Availability<Decimal>,
    pub currency_gain_base: Availability<Decimal>,
    pub income_base: Availability<Decimal>,
    pub total_return_base: Availability<Decimal>,
}

impl PeriodAmounts {
    fn unavailable(reasons: Vec<ValuationReason>) -> Self {
        Self {
            begin_market_value_base: Availability::Unavailable { reasons: reasons.clone() },
            end_market_value_base: Availability::Unavailable { reasons: reasons.clone() },
            capital_gain_base: Availability::Unavailable { reasons: reasons.clone() },
            currency_gain_base: Availability::Unavailable { reasons: reasons.clone() },
            income_base: Availability::Unavailable { reasons: reasons.clone() },
            total_return_base: Availability::Unavailable { reasons },
        }
    }
}
```

- [ ] **Step 4: Update `compute_period_amounts` to handle dividends**

In the main accumulation loop, add a `TransactionKind::Dividend` arm. The arm computes `qty × price × fx` and subtracts from `net_flows` (investor receives this cash, reducing the net invested amount, which increases total return):

```rust
TransactionKind::Dividend => {
    let p = match tx.price {
        Some(p) => p,
        None => {
            return PeriodAmounts::unavailable(vec![
                ValuationReason::MissingTransactionPrice { transaction_id: tx.id },
            ]);
        }
    };
    let f = if is_sek_instrument {
        Decimal::ONE
    } else {
        match tx.fx_rate_to_base {
            Some(f) => f,
            None => {
                return PeriodAmounts::unavailable(vec![
                    ValuationReason::MissingTransactionFx { transaction_id: tx.id },
                ]);
            }
        }
    };
    let income = Decimal::from(tx.quantity) * p * f;
    net_flows -= income;
    // capital_flows_at_end_fx deliberately excludes dividend income so
    // capital_gain stays a pure price-movement metric at constant FX.
}
```

After computing `total_return` and `capital_gain`, compute `income_base` and adjust `currency_gain`:

```rust
// Compute income_base: sum of dividend amounts in base within the period.
// (Already subtracted from net_flows above, so total_return already includes it.)
let mut income_total = Decimal::ZERO;
for tx in &period.period_transactions {
    if tx.kind == TransactionKind::Dividend {
        let p = match tx.price {
            Some(p) => p,
            None => unreachable!("missing price already handled above"),
        };
        let f = if is_sek_instrument {
            Decimal::ONE
        } else {
            match tx.fx_rate_to_base {
                Some(f) => f,
                None => unreachable!("missing FX already handled above"),
            }
        };
        income_total += Decimal::from(tx.quantity) * p * f;
    }
}
let income = income_total;

let total_return = end_mv - begin_mv - net_flows;
let capital_gain =
    (adj_end_qty * end_price - adj_start_qty * start_price) * end_fx - capital_flows_at_end_fx;
let currency_gain = total_return - capital_gain - income;

PeriodAmounts {
    begin_market_value_base: Availability::Available(begin_mv),
    end_market_value_base: Availability::Available(end_mv),
    capital_gain_base: Availability::Available(capital_gain),
    currency_gain_base: Availability::Available(currency_gain),
    income_base: Availability::Available(income),
    total_return_base: Availability::Available(total_return),
}
```

> **Note:** The double-pass over period_transactions for income is slightly redundant, but it keeps the control flow clear. An alternative is to accumulate `income_total` inside the existing loop; either approach is fine.

- [ ] **Step 5: Update `period_cash_flows` to include dividends**

In `period_cash_flows`, add a `TransactionKind::Dividend` arm (before or after the `Split` arm):

```rust
TransactionKind::Dividend => {
    let p = match tx.price {
        Some(p) => p,
        None => {
            return Availability::Unavailable {
                reasons: vec![ValuationReason::MissingTransactionPrice {
                    transaction_id: tx.id,
                }],
            }
        }
    };
    let f = if is_sek_instrument {
        Decimal::ONE
    } else {
        match tx.fx_rate_to_base {
            Some(f) => f,
            None => {
                return Availability::Unavailable {
                    reasons: vec![ValuationReason::MissingTransactionFx {
                        transaction_id: tx.id,
                    }],
                }
            }
        }
    };
    let income = Decimal::from(tx.quantity) * p * f;
    // Dividend is cash the investor receives: negative amount (out of the investment).
    flows.push(CashFlow {
        date: tx.trade_date,
        amount_base: -income,
    });
}
```

Remove `TransactionKind::Dividend` from the `Split | Dividend` no-op arm so it becomes just `TransactionKind::Split`.

- [ ] **Step 6: Update `actual_period_cash_flows` identically for dividends**

Apply the same change to `actual_period_cash_flows` — add a `TransactionKind::Dividend` arm (identical logic, no post-period split factor to apply for dividends).

Also remove `Dividend` from the `Split | Dividend` no-op arm.

- [ ] **Step 7: Run tests**

```bash
cd backend && cargo test domain::performance::tests
```
Expected: all pass, including the three new dividend tests.

- [ ] **Step 8: Clippy + fmt + commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
```

```bash
git add backend/src/domain/performance.rs
git commit -m "$(cat <<'EOF'
feat(domain): include dividend income in period performance calculations

Dividends reduce net_flows (total_return increases by income). Adds
income_base to PeriodAmounts. currency_gain = total_return - capital -
income so the three components sum to total_return. period_cash_flows
and actual_period_cash_flows emit dividends as negative flows.
EOF
)"
```

---

## Task 4: Gains API Exposes Income Per Row And In Totals

**Files:**
- Modify: `backend/src/api/gains.rs`

**Interfaces:**
- Consumes: `PeriodAmounts.income_base` from Task 3
- Produces:
  - `GainRow.income_base: AvailabilityResponse` (new field)
  - `TotalsResponse.income_base / income_percent` now computed (previously hard-coded to `income_not_tracked` unavailable)

- [ ] **Step 1: Identify the two places to change in gains.rs**

Read `backend/src/api/gains.rs` around line 89 (`TotalsResponse`) and around the GainRow construction. The income fields in `TotalsResponse` are currently:

```rust
income_base: AvailabilityResponse::Unavailable {
    reasons: vec!["income_not_tracked".to_string()],
},
income_percent: AvailabilityResponse::Unavailable {
    reasons: vec!["income_not_tracked".to_string()],
},
```

Find these and the `GainRow` struct (around line 98).

- [ ] **Step 2: Write a failing test**

In `backend/src/api/gains.rs` tests block, find the assertions on `income_base` and add a new scenario:

```rust
#[tokio::test]
async fn dividend_income_appears_in_gain_row_and_totals() {
    let state = AppState::for_tests().await;
    let instrument_id = create_sek_instrument(&state).await; // helper for a SEK instrument
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10.00","currency":"SEK"}),
    ).await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
               "quantity":100,"price":"0.50","currency":"SEK"}),
    ).await;
    // income = 100 × 0.50 = 50 SEK
    let (status, body) = send(&state, "GET", "/api/gains", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    let row = &body["rows"][0];
    assert_eq!(row["income_base"]["status"], "available");
    assert_eq!(row["income_base"]["value"], "50.00");
    assert_eq!(body["totals"]["income_base"]["status"], "available");
    assert_eq!(body["totals"]["income_base"]["value"], "50.00");
}
```

> `create_sek_instrument` is a test helper to create an instrument with `currency: "SEK"`. Add it near `create_instrument`.

- [ ] **Step 3: Run the test to confirm it fails**

```bash
cd backend && cargo test api::gains::tests::dividend_income 2>&1 | head -30
```
Expected: compile error — `income_base` unknown on `GainRow`.

- [ ] **Step 4: Add `income_base` to `GainRow` struct and extend row builders**

In `backend/src/api/gains.rs`, add to `GainRow`:

```rust
pub income_base: AvailabilityResponse,
```

The existing `open_gain_row` and `closed_gain_row` builder functions receive only valuation/current-position data and realized-gain data — they do not currently receive `PeriodAmounts`. You must call `compute_period_amounts` before calling each builder (or compute it inside the builder), then pass the resulting `income_base` availability into the builder as an additional parameter:

```rust
fn open_gain_row(
    // ... existing params ...
    income_base: &Availability<Decimal>,
) -> GainRow { ... }
```

Decide where `period_amounts` is computed relative to the builder call, and thread `income_base` through. For rows where the period is unavailable or outside the report window, use `Availability::Available(Decimal::ZERO)` for `income_base` (no dividends = zero income, not unknown).

- [ ] **Step 5: Populate `income_base` when building each `GainRow`**

Inside each builder, serialize `income_base` using the same pattern as `capital_gain_base`, `currency_gain_base`, and `total_return_base`:

```rust
income_base: serialize_availability(income_base, money_string),
```

- [ ] **Step 6: Add `income_gain` to `PerformanceAccumulator` and update `into_percents()`**

The totals path uses `PerformanceAccumulator::into_percents()` with method-specific logic for XIRR, Simple, and Modified Dietz — there is no single `total_denominator` variable to reuse. Do not add a standalone `income_total` outside the accumulator; instead:

1. Add `income_gain: Availability<Decimal>` (or `income_base`) to `PerformanceAccumulator` and accumulate `amounts.income_base` there in the per-instrument loop.

2. Update `into_percents()` to return `income_percent` alongside the existing components:
   - **Simple / Modified Dietz:** compute `income_percent` with the same denominator used for `capital_percent` and `currency_percent`.
   - **XIRR:** allocate the money-weighted total return across capital, income, and currency proportionally; the zero-total guard must consider all three components so `capital + income + currency = total_return` holds.

3. Replace the two hard-coded `income_not_tracked` unavailabilities in `TotalsResponse` with the values returned from `into_percents()`.

Verification: Add one totals test per method (`method=xirr`, `method=simple`, `method=modified_dietz`) where `capital + income + currency = total_return` and all three component percentages are present in the response.

- [ ] **Step 7: Run all tests**

```bash
cd backend && cargo test api::gains::tests
```
Expected: all green including the new dividend income test; existing `income_not_tracked` assertions may now fail — remove or update them to assert `"available"` values of `"0.00"` for portfolios with no dividends.

- [ ] **Step 8: Clippy + fmt + commit**

```bash
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
```

```bash
git add backend/src/api/gains.rs
git commit -m "$(cat <<'EOF'
feat(api): expose income_base in GainRow and GainsTotals

income_base is now computed from period dividend transactions. Totals
aggregate across instruments. Previously hard-coded income_not_tracked
unavailability is now computed; portfolios without dividends show 0.00.
EOF
)"
```

---

## Task 5: Frontend Types And Waterfall Show Dividend Income

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/components/waterfallViewModel.ts`
- Modify: `frontend/src/components/waterfallViewModel.test.ts`

**Interfaces:**
- Consumes: `GainRow.income_base: MoneyValue` (from Task 4 backend)
- Produces: `waterfallView(gain)` includes a real `"income"` effect row using `gain.income_base`

- [ ] **Step 1: Add `income_base` to the `GainsRow` TypeScript type**

In `frontend/src/api/types.ts`, add to `GainsRow`:

```ts
income_base: MoneyValue;
```

Place it after `currency_gain_percent` and before `total_return_base`.

- [ ] **Step 2: Write failing waterfall tests for dividend income**

In `frontend/src/components/waterfallViewModel.test.ts`, add tests for the open and closed waterfall income rows. Find the existing test structure and add:

```ts
function makeGain(overrides: Partial<GainsRow> = {}): GainsRow {
  // ... copy the existing test factory or build from existing patterns in the file
  return {
    ...existingTestGain,
    income_base: { status: "available", value: "250.00" },
    ...overrides,
  };
}

it("open waterfall includes income row with real value", () => {
  const gain = makeGain({ position_status: "open" });
  const view = waterfallView(gain);
  const incomeRow = view.rows.find((r) => r.key === "income");
  expect(incomeRow).toBeDefined();
  expect(incomeRow!.kind).toBe("effect");
  expect(incomeRow!.value).toEqual({ status: "available", value: "250.00" });
});

it("open waterfall income row is placeholder when unavailable", () => {
  const gain = makeGain({
    position_status: "open",
    income_base: { status: "unavailable", reasons: ["income_not_tracked"] },
  });
  const view = waterfallView(gain);
  const incomeRow = view.rows.find((r) => r.key === "income");
  expect(incomeRow).toBeDefined();
  expect(incomeRow!.kind).toBe("placeholder");
});

it("open waterfall realized and income rows are sequential when both non-zero", () => {
  const gain = makeGain({
    position_status: "open",
    // realized_gain_base non-zero so overlap would be visible if running is not updated
    income_base: { status: "available", value: "250.00" },
  });
  const view = waterfallView(gain);
  const rows = view.rows;
  const realizedIdx = rows.findIndex((r) => r.key === "realized");
  const incomeIdx = rows.findIndex((r) => r.key === "income");
  expect(realizedIdx).toBeGreaterThanOrEqual(0);
  expect(incomeIdx).toBe(realizedIdx + 1);
  // income bar must start where realized bar ends (no gap, no overlap)
  const realizedRow = rows[realizedIdx];
  const incomeRow = rows[incomeIdx];
  expect(incomeRow.start).toBeCloseTo(realizedRow.start + (realizedRow.value ?? 0));
});
```

- [ ] **Step 3: Run the tests to confirm they fail**

```bash
cd frontend && npm run check 2>&1 | head -30
```
Expected: TypeScript error — `income_base` missing on `GainsRow` usages, and test failures.

- [ ] **Step 4: Update `waterfallViewModel.ts` — replace placeholder with real income row**

In `frontend/src/components/waterfallViewModel.ts`, update both `openWaterfall` and `closedWaterfall`:

Replace:
```ts
rows.push(placeholderRow("dividends", "Dividends"));
```

With a conditional: show a real effect row when `income_base` is available or unavailable-but-not-placeholder, and show a placeholder only when the reason is `"income_not_tracked"`:

```ts
function incomeRow(
  gain: GainsRow,
  costBasis: MoneyValue,
  running: number,
  rows: WaterfallRow[],
): number {
  if (
    gain.income_base.status === "unavailable" &&
    gain.income_base.reasons.includes("income_not_tracked")
  ) {
    rows.push(placeholderRow("income", "Dividend income"));
    return running;
  }
  return pushEffect(rows, "income", "Dividend income", gain.income_base, costBasis, running);
}
```

Call `incomeRow(gain, costBasis, running, rows)` and capture the returned `running` in both `openWaterfall` and `closedWaterfall`, replacing the previous `placeholderRow` call.

**Important for `openWaterfall`:** The current code calls `pushEffect()` for the `"realized"` row but deliberately does not assign the returned `running`. Before calling `incomeRow`, you must assign `running = pushEffect(...)` for the realized row first — otherwise the income bar starts from the wrong baseline and overlaps the realized step when realized gain is non-zero.

Verification: Assert that the realized row span and income row span are sequential, and that the total-return value equals `unrealized + realized + income`.

- [ ] **Step 5: Update total-return computation to include income in open waterfall**

In `openWaterfall`, the existing `displaySum(gain.unrealized_gain_base, gain.realized_gain_base)` now excludes income. Update:

```ts
const totalReturn = displaySum(
  displaySum(gain.unrealized_gain_base, gain.realized_gain_base),
  gain.income_base,
);
```

This keeps the total-return terminus correct when income is available.

- [ ] **Step 6: Run tests and type-check**

```bash
cd frontend && npm run check && npx vitest run 2>&1 | tail -20
```
Expected: all green.

- [ ] **Step 7: fmt + commit**

```bash
cd frontend && npm run fmt
```

```bash
git add frontend/src/api/types.ts frontend/src/components/waterfallViewModel.ts frontend/src/components/waterfallViewModel.test.ts
git commit -m "$(cat <<'EOF'
feat(frontend): show dividend income in gains waterfall

Adds income_base to GainsRow type. Waterfall replaces the dividends
placeholder with a real effect row when income is tracked; falls back
to placeholder when reason is income_not_tracked.
EOF
)"
```

---

## Task 6: Transaction Form Supports Dividend Entry

**Files:**
- Modify: `frontend/src/components/AddTransactionForm.tsx`

**Interfaces:**
- Consumes: existing `TransactionType` (already includes `"Dividend"`)
- Produces: "Dividend" option in the type dropdown; shows quantity/price/currency/fx fields (no brokerage); quantity label reads "Shares eligible"

- [ ] **Step 1: Add "Dividend" to the type dropdown**

In `frontend/src/components/AddTransactionForm.tsx`, find the `<select>` for transaction type (around line 281). Add the option:

```tsx
<option value="Dividend">Dividend</option>
```

- [ ] **Step 2: Update the `isSplit` conditional to `isSplit | isDividend` where brokerage is hidden**

```tsx
const isSplit = state.type === "Split";
const isDividend = state.type === "Dividend";
```

Update the check that shows/hides the price/currency/fx/brokerage row. For dividends, show price/currency/fx but **hide brokerage**:

```tsx
{!isSplit ? (
  <div className="form-row">
    <label className="form-field">
      <span>
        {isDividend ? "Dividend per share (native)" : "Price (native)"}
      </span>
      <input value={state.price} onChange={setField("price")} />
    </label>
    <label className="form-field">
      <span>Currency</span>
      <input value={state.currency} onChange={setField("currency")} />
    </label>
    <label className="form-field">
      <span>FX to SEK (optional)</span>
      <input value={state.fxRate} onChange={setField("fxRate")} />
    </label>
    {!isDividend ? (
      <label className="form-field">
        <span>Brokerage (SEK)</span>
        <input value={state.brokerage} onChange={setField("brokerage")} />
      </label>
    ) : null}
  </div>
) : null}
```

- [ ] **Step 3: Update quantity label for Dividend**

Find the quantity field label (around line 304):

```tsx
<span>{isSplit ? "Quantity delta" : isDividend ? "Shares eligible" : "Quantity"}</span>
```

- [ ] **Step 4: Ensure brokerage is not sent for dividend submissions**

In `handleSubmit`, update the `input` construction:

```tsx
brokerage: (isSplit || isDividend) ? undefined : trimmedOrUndefined(state.brokerage),
```

- [ ] **Step 5: Type-check**

```bash
cd frontend && npm run check
```
Expected: clean.

- [ ] **Step 6: Manual smoke test**

Start the backend (`cargo run` from `backend/`) and frontend (`npm run dev` from `frontend/`). Open the app, click "Add transaction", select "Dividend", fill in: instrument = any existing, date = today, shares eligible = 100, dividend per share = 0.50, currency = USD (or SEK). Confirm it saves and appears in the Transactions list. Check that no brokerage field appears.

- [ ] **Step 7: fmt + commit**

```bash
cd frontend && npm run fmt
```

```bash
git add frontend/src/components/AddTransactionForm.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): add Dividend option to transaction form

Shows quantity (shares eligible), price (dividend per share), currency,
and FX fields. Hides brokerage. Labels adapt per transaction type.
EOF
)"
```

---

## Task 7: Decision Log

**Files:**
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Append a decision entry**

Add to the end of `docs/DecisionLog.md`:

```markdown
## 2026-06-23 - Dividend Transaction Model

Decision: Dividends use the existing `LedgerTransaction` field layout: `quantity` = shares eligible (positive integer), `price` = dividend per share in the instrument's native currency (positive), `currency` = instrument currency (required), `fx_rate_to_base` = optional SEK conversion (missing FX is stored but makes `income_base` unavailable, consistent with buy/sell behavior). Brokerage is rejected. Position quantity effect is zero (dividends do not change share count). `income_base` is added to `PeriodAmounts` as the sum of all period dividend cash flows in base currency; `currency_gain = total_return − capital_gain − income` so all three components remain additive. Dividend cash flows are negative in Modified Dietz and XIRR series (investor receives the cash). Import adapters (Avanza, Sharesight) continue to skip dividends until a future plan enables import.
Context: ISK account; no per-dividend tax calculation needed. Reusing existing fields avoids a schema migration. Missing FX is accepted at write time (lazy — user can supply later) rather than rejected eagerly, matching the existing buy/sell contract. The import-skip path is unchanged to keep this plan self-contained.
Consequences: Manually-entered dividends are immediately surfaced in Gains income column and the waterfall. Import adapters need a separate follow-on plan to map parsed dividend rows to the new validation path.
```

- [ ] **Step 2: Commit**

```bash
git add docs/DecisionLog.md
git commit -m "$(cat <<'EOF'
docs: record dividend transaction model decision
EOF
)"
```

---

## Self-Review

**Spec coverage:**
- [x] Domain validation for dividends (Task 1)
- [x] API acceptance and currency guard (Task 2)
- [x] Performance income component in PeriodAmounts (Task 3)
- [x] capital + income + currency = total_return (Task 3)
- [x] Gains API income_base per row and in totals (Task 4)
- [x] Frontend type updated (Task 5)
- [x] Waterfall replaces placeholder with real income (Task 5)
- [x] Transaction form (Task 6)
- [x] Decision log (Task 7)
- [x] Import adapters explicitly deferred (Global Constraints)

**Placeholder scan:** None found — all code blocks are concrete.

**Type consistency:**
- `income_base: Availability<Decimal>` in Rust / `income_base: MoneyValue` in TypeScript — consistent.
- `PeriodAmounts.income_base` consumed in `gains.rs` as `period_amounts.income_base` — consistent.
- `waterfallViewModel.ts` reads `gain.income_base` — typed on `GainsRow` in Task 5 Step 1.
