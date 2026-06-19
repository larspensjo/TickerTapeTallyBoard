# Sharesight-Style Performance Returns Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace cost-basis percentage totals in the Gains view with Modified Dietz money-weighted performance returns, add date-range semantics, and wire a frontend date-range control — matching Sharesight's Performance Report methodology.

**Architecture:** A new pure domain module `performance.rs` computes period reconstruction, amount components, Modified Dietz percentages, and annualisation. The `api/gains.rs` handler is extended with `start_date`/`end_date` query params; it loads start/end prices and FX from the existing DB helpers and passes them to the domain module. The frontend gains a date-range control and shows the performance method in the totals band. Phase 5 (income/dividends) is out of scope for this plan.

**Tech Stack:** Rust (`rust_decimal`, `chrono`), axum, sqlx SQLite (existing), React + TanStack Query (existing).

## Global Constraints

- Base currency is always `SEK`; FX stored as SEK per one unit of instrument currency.
- All money in domain layer uses `rust_decimal::Decimal`; stored as TEXT in DB; never coerced to zero when unavailable.
- Availability pattern: missing data → `Availability::Unavailable { reasons }` with distinct `ValuationReason` variants per the spec. **Never use `Vec<String>` for reasons; always use `Vec<ValuationReason>`.**
- `cargo clippy --all-targets -- -D warnings` and `cargo fmt` must pass after every backend task.
- `npm run check` and `npm run fmt` must pass after every frontend task.
- Run all commands from `backend/` for cargo commands, `frontend/` for npm commands.
- Do not commit — stage only for review.
- Spec reference: `docs/plans/Spec.sharesight-style-performance.md`

## Pre-Implementation Decisions

The following issues are resolved here before any code is written:

| Issue | Decision |
|---|---|
| `Availability<T>` reason type | Add performance-specific variants to the existing `ValuationReason` enum in `domain/valuation.rs`. Never use `String` reasons inside `Availability`. |
| Split-factor opening quantity | `split_factor(transactions, opening_qty)` — caller passes `start_position.quantity` for in-period and `end_position.quantity` for post-period slices. |
| Period amount formula | Market-value identity: `total_return = end_mv − begin_mv − net_flows`; capital gain = same formula at constant `end_fx`; currency gain = residual. No held-quantity tracking needed. |
| FX availability for non-SEK | Add `is_sek_instrument: bool` to `compute_period_amounts`. For non-SEK, missing `start_fx` (when `start_position.quantity > 0`) or `end_fx` returns `Unavailable`. |
| Modified Dietz signature | Final signature takes `start_date` + `end_date`; computes `period_days` and calendar-day weights internally. No intermediate `period_days: i64` version. |
| Inception `effective_start_date` | Derive `effective_start_date` from the earliest transaction date in the full ledger. Pass this concrete date to `reconstruct_period` and `compute_modified_dietz`. The API `report_period.start_date` remains `null` for "All". |
| Amount columns in TotalsResponse | When date-range is active, `capital_gain_base`, `currency_gain_base`, and `total_return_base` in `TotalsResponse` come from `PeriodAmounts` (period attribution), not from the existing cost-basis `TotalsAccumulator`. |
| Ledger truncation | Every calculation uses ledger transactions with `trade_date ≤ end_date`. Transactions after `end_date` must not affect any result. |
| API error helper | `ApiError::bad_request(code, message)` already exists in `api/error.rs:55`. No new helper needed. |
| FY date preset | FY means the current Swedish fiscal year to date: `Jan 1` of current year to today (same as YTD). Using Dec 31 of the current year is a future date; disallow it. |
| Frontend date formatting | Use `date.toLocaleDateString('sv-SE')` (yields `YYYY-MM-DD` in local time) instead of `toISOString().slice(0,10)` (UTC). |
| `AssetView.tsx` useGains caller | `AssetView.tsx` calls `useGains(true)` and must be updated alongside `GainsTable.tsx` and `BoardView.tsx`. |

## Open Questions Resolved In This Plan

| Question | Resolution |
|---|---|
| Start-date boundary | Transactions **strictly before** `start_date` form the start position; transactions on `start_date` through `end_date` inclusive are period flows. |
| Split adjustment | Convert ledger quantities to Yahoo's split-adjusted basis by applying cumulative split factors from in-period and post-period splits. |
| Cash-flow weights | Calendar days: `weight = (end_date - flow_date).num_days() / period_days` |
| Average Years Invested | `period_days as f64 / 365.25` (calibrate against Sharesight after first run) |
| Component annualisation | Only `total_return_percent` is annualised; component percentages share the same Modified Dietz denominator but are **not** individually annualised. |

---

## File Map

| Path | Status | Responsibility |
|---|---|---|
| `backend/src/domain/performance.rs` | **Create** | Pure period reconstruction, amount components, Modified Dietz, annualisation |
| `backend/src/domain/mod.rs` | **Modify** | Re-export performance types |
| `backend/src/domain/valuation.rs` | **Modify** | Add performance-specific `ValuationReason` variants |
| `backend/src/api/gains.rs` | **Modify** | Date-range query params, validation, performance wiring, extended response |
| `backend/src/api/valuation.rs` | **Modify** | New `load_period_inputs` helper loading start/end price+FX |
| `frontend/src/api/types.ts` | **Modify** | Add `report_period`, `percentage_method`, `display_percent_kind` to `GainsResponse` |
| `frontend/src/api/queries.ts` | **Modify** | `useGains` accepts `startDate`, `endDate` params |
| `frontend/src/components/GainsTable.tsx` | **Modify** | Date-range control; percentage kind label in totals |
| `frontend/src/components/AssetView.tsx` | **Modify** | Update `useGains(true)` → `useGains({ includeClosedPositions: true })` |
| `backend/Cargo.toml` | **Modify** | Version bump |
| `frontend/package.json` | **Modify** | Version bump |
| `docs/DecisionLog.md` | **Modify** | Record method and convention decisions |

---

## Task 1: Domain — Period Ledger Reconstruction

**Files:**
- Modify: `backend/src/domain/valuation.rs`
- Create: `backend/src/domain/performance.rs`
- Modify: `backend/src/domain/mod.rs`

**Interfaces produced:**
```rust
pub struct PeriodLedger {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    /// Position from transactions strictly before start_date.
    pub start_position: Position,
    /// Transactions with trade_date in [start_date, end_date].
    pub period_transactions: Vec<LedgerTransaction>,
    /// Position from transactions through end_date.
    pub end_position: Position,
    /// Product of ((qty_before + delta) / qty_before) for Split rows in period_transactions.
    /// Multiply start_position.quantity by this to get a Yahoo-split-adjusted quantity.
    pub in_period_split_factor: Decimal,
    /// Product of split factors for Split rows strictly after end_date.
    /// Multiply end_position.quantity by this to get a Yahoo-split-adjusted quantity.
    pub post_period_split_factor: Decimal,
}

pub fn reconstruct_period(
    all_transactions: &[LedgerTransaction],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<PeriodLedger, LedgerError>
```

- [ ] **Step 1: Add performance-specific variants to `ValuationReason`**

In `backend/src/domain/valuation.rs`, add to the `ValuationReason` enum:

```rust
pub enum ValuationReason {
    // existing variants unchanged...
    MissingPrice,
    MissingFx,
    MissingPreviousClose,
    MissingPreviousFx,
    StalePrice { trading_days: i64 },
    StaleFx { trading_days: i64 },
    ZeroCostBasis,
    ZeroPreviousMarketValue,
    BaseCostBasisUnavailable { reasons: Vec<UnavailableReason> },
    // new performance variants:
    MissingStartPrice,
    MissingEndPrice,
    MissingStartFx,
    MissingEndFx,
    MissingTransactionPrice { transaction_id: i64 },
    ZeroOrInvalidPerformanceDenominator,
}
```

Stage: `git add backend/src/domain/valuation.rs`

- [ ] **Step 2: Create `performance.rs` with imports and failing test module**

```rust
// backend/src/domain/performance.rs
use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::position::{derive_position, Position};
use super::transaction::{LedgerError, LedgerTransaction, TransactionKind};
use super::valuation::{Availability, ValuationReason};

pub struct PeriodLedger {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub start_position: Position,
    pub period_transactions: Vec<LedgerTransaction>,
    pub end_position: Position,
    pub in_period_split_factor: Decimal,
    pub post_period_split_factor: Decimal,
}

pub fn reconstruct_period(
    all_transactions: &[LedgerTransaction],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<PeriodLedger, LedgerError> {
    todo!()
}

/// Computes the cumulative split factor for a slice of transactions,
/// given the quantity already held before the first transaction in the slice.
/// For each Split with delta d when running quantity is q: factor *= (q + d) / q.
fn split_factor(transactions: &[LedgerTransaction], opening_qty: i64) -> Result<Decimal, LedgerError> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn buy(id: i64, d: &str, qty: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(dec!(10)),
            fx_rate_to_base: Some(dec!(10)),
            brokerage_base: Decimal::ZERO,
        }
    }

    fn split_tx(id: i64, d: &str, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    #[test]
    fn buy_before_start_is_in_start_position() {
        let txs = vec![buy(1, "2026-01-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 100);
        assert_eq!(p.period_transactions.len(), 0);
        assert_eq!(p.end_position.quantity, 100);
    }

    #[test]
    fn buy_on_start_date_is_period_flow_not_start_position() {
        let txs = vec![buy(1, "2026-06-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 0);
        assert_eq!(p.period_transactions.len(), 1);
        assert_eq!(p.end_position.quantity, 100);
    }

    #[test]
    fn buy_after_end_excluded_from_end_position() {
        let txs = vec![buy(1, "2026-07-01", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 0);
        assert_eq!(p.period_transactions.len(), 0);
        assert_eq!(p.end_position.quantity, 0);
    }

    #[test]
    fn in_period_split_factor_for_2_to_1_split() {
        // 100 shares held before start, 2:1 split mid-period:
        // factor = (100 + 100) / 100 = 2
        let txs = vec![
            buy(1, "2026-01-01", 100),
            split_tx(2, "2026-06-15", 100),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.start_position.quantity, 100);
        assert_eq!(p.end_position.quantity, 200);
        assert_eq!(p.in_period_split_factor, dec!(2));
        assert_eq!(p.post_period_split_factor, dec!(1));
    }

    #[test]
    fn post_period_split_factor_for_split_after_end() {
        // 100 shares held, 2:1 split after end_date:
        // post factor = (100 + 100) / 100 = 2
        let txs = vec![
            buy(1, "2026-01-01", 100),
            split_tx(2, "2026-08-01", 100),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.end_position.quantity, 100);
        assert_eq!(p.in_period_split_factor, dec!(1));
        assert_eq!(p.post_period_split_factor, dec!(2));
    }

    #[test]
    fn in_period_split_with_zero_start_position_is_factor_one() {
        // No pre-period shares; in-period buy then split.
        // split_factor(&period, opening_qty=0): split at running_qty=100 → factor = 200/100 = 2.
        // But in_period_split_factor is called with opening_qty = start_position.quantity = 0.
        // The split happens after the buy, so running_qty at split = 100.
        let txs = vec![
            buy(1, "2026-06-05", 100),
            split_tx(2, "2026-06-15", 100),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.in_period_split_factor, dec!(2));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```
cd backend && cargo test domain::performance -- --nocapture 2>&1 | head -30
```
Expected: compilation error or panics on `todo!()`

- [ ] **Step 4: Implement `split_factor` and `reconstruct_period`**

```rust
fn split_factor(transactions: &[LedgerTransaction], opening_qty: i64) -> Result<Decimal, LedgerError> {
    let mut factor = Decimal::ONE;
    let mut running_qty: i64 = opening_qty;
    for tx in transactions {
        match tx.kind {
            TransactionKind::Buy => running_qty += tx.quantity,
            TransactionKind::Sell => running_qty += tx.quantity, // tx.quantity is negative for sells
            TransactionKind::Split => {
                if running_qty > 0 {
                    let after = running_qty + tx.quantity;
                    factor *= Decimal::from(after) / Decimal::from(running_qty);
                    running_qty = after;
                }
            }
            TransactionKind::Dividend => {}
        }
    }
    Ok(factor)
}

pub fn reconstruct_period(
    all_transactions: &[LedgerTransaction],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<PeriodLedger, LedgerError> {
    let pre: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date < start_date)
        .cloned()
        .collect();
    let period: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date >= start_date && t.trade_date <= end_date)
        .cloned()
        .collect();
    let through_end: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date <= end_date)
        .cloned()
        .collect();
    let post: Vec<_> = all_transactions
        .iter()
        .filter(|t| t.trade_date > end_date)
        .cloned()
        .collect();

    let start_position = derive_position(&pre)?;
    let end_position = derive_position(&through_end)?;
    // Pass opening quantity so split_factor sees the pre-existing shares.
    let in_period_split_factor = split_factor(&period, start_position.quantity)?;
    let post_period_split_factor = split_factor(&post, end_position.quantity)?;

    Ok(PeriodLedger {
        start_date,
        end_date,
        start_position,
        period_transactions: period,
        end_position,
        in_period_split_factor,
        post_period_split_factor,
    })
}
```

- [ ] **Step 5: Add re-export to `domain/mod.rs`**

```rust
mod performance;

pub use performance::{reconstruct_period, PeriodLedger};
```

- [ ] **Step 6: Run tests**

```
cd backend && cargo test domain::performance -- --nocapture
```
Expected: all tests pass.

- [ ] **Step 7: Clippy + fmt**

```
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
```

- [ ] **Step 8: Stage**

```
git add backend/src/domain/performance.rs backend/src/domain/mod.rs backend/src/domain/valuation.rs
```

---

## Task 2: Domain — Period Amount Components

**Files:**
- Modify: `backend/src/domain/performance.rs`

**Interfaces produced:**
```rust
pub struct PeriodAmounts {
    pub begin_market_value_base: Availability<Decimal>,
    pub end_market_value_base: Availability<Decimal>,
    pub capital_gain_base: Availability<Decimal>,
    pub currency_gain_base: Availability<Decimal>,
    pub total_return_base: Availability<Decimal>,
}

pub fn compute_period_amounts(
    period: &PeriodLedger,
    start_price_native: Option<Decimal>,
    end_price_native: Option<Decimal>,
    /// None when instrument currency == SEK.
    start_fx: Option<Decimal>,
    /// None when instrument currency == SEK.
    end_fx: Option<Decimal>,
    /// True when instrument currency == SEK; skips FX availability checks.
    is_sek_instrument: bool,
) -> PeriodAmounts
```

**Attribution formulas (market-value identity — avoids held-quantity tracking):**

```
adj_start_qty = start_position.quantity × in_period_split_factor × post_period_split_factor
adj_end_qty   = end_position.quantity × post_period_split_factor

begin_mv = adj_start_qty × start_price × start_fx
end_mv   = adj_end_qty × end_price × end_fx

net_flows         = Σ_buys(qty × price × buy_fx + brokerage_base)
                  − Σ_sells(|qty| × price × sell_fx − brokerage_base)

total_return_base = end_mv − begin_mv − net_flows

capital_gain_base = (adj_end_qty × end_price − adj_start_qty × start_price) × end_fx
                  − Σ_buys(qty × price × end_fx + brokerage_base)
                  + Σ_sells(|qty| × price × end_fx − brokerage_base)

currency_gain_base = total_return_base − capital_gain_base
```

For SEK instruments: `start_fx = end_fx = Decimal::ONE` (never missing).
For non-SEK: `start_fx` is required when `start_position.quantity > 0`; `end_fx` is always required when `end_position.quantity > 0` or period has sells.

**Availability rules:**
- Missing `end_price_native` → `Unavailable { reasons: [ValuationReason::MissingEndPrice] }`
- Missing `end_fx` for non-SEK (when needed) → `Unavailable { reasons: [ValuationReason::MissingEndFx] }`
- Missing `start_price_native` with `start_position.quantity > 0` → `Unavailable { reasons: [ValuationReason::MissingStartPrice] }`
- Missing `start_fx` for non-SEK with `start_position.quantity > 0` → `Unavailable { reasons: [ValuationReason::MissingStartFx] }`
- Missing transaction price → `Unavailable { reasons: [ValuationReason::MissingTransactionPrice { transaction_id }] }`

- [ ] **Step 1: Write failing tests**

Add to `performance.rs` tests module:

```rust
fn buy_with_fx(id: i64, d: &str, qty: i64, price: Decimal, fx: Decimal, brokerage: Decimal) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: date(d),
        kind: TransactionKind::Buy,
        quantity: qty,
        price: Some(price),
        fx_rate_to_base: Some(fx),
        brokerage_base: brokerage,
    }
}

fn sell_with_fx(id: i64, d: &str, qty: i64, price: Decimal, fx: Decimal) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: date(d),
        kind: TransactionKind::Sell,
        quantity: -(qty as i64),
        price: Some(price),
        fx_rate_to_base: Some(fx),
        brokerage_base: Decimal::ZERO,
    }
}

fn avail(a: &Availability<Decimal>) -> Decimal {
    match a {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => panic!("expected Available, got {:?}", reasons),
    }
}

#[test]
fn period_amounts_simple_hold_price_gain() {
    // 100 shares held through period; price 10 → 12 USD; FX constant at 10
    let txs = vec![buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(12)), Some(dec!(10)), Some(dec!(10)), false);
    assert_eq!(avail(&a.capital_gain_base), dec!(2000));   // (12-10)*100*10
    assert_eq!(avail(&a.currency_gain_base), dec!(0));
    assert_eq!(avail(&a.total_return_base), dec!(2000));
}

#[test]
fn period_amounts_simple_hold_fx_gain() {
    // 100 shares; price flat at 10 USD; FX 10 → 11
    let txs = vec![buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(10)), Some(dec!(10)), Some(dec!(11)), false);
    assert_eq!(avail(&a.capital_gain_base), dec!(0));
    assert_eq!(avail(&a.currency_gain_base), dec!(1000));  // 10*100*(11-10)
    assert_eq!(avail(&a.total_return_base), dec!(1000));
}

#[test]
fn period_amounts_inception_mode_no_start_price_needed() {
    // Inception: buy 100 shares at price 10, FX 10 during the period; end price 12, FX 10
    let txs = vec![buy_with_fx(1, "2026-06-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    // start_price = None (inception, start_position.quantity == 0)
    let a = compute_period_amounts(&p, None, Some(dec!(12)), None, Some(dec!(10)), false);
    assert_eq!(avail(&a.begin_market_value_base), dec!(0));
    // total_return = end_mv - begin_mv - net_flows = 12000 - 0 - 10000 = 2000
    assert_eq!(avail(&a.total_return_base), dec!(2000));
}

#[test]
fn period_amounts_missing_end_price_returns_unavailable() {
    let txs = vec![buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), None, Some(dec!(10)), Some(dec!(10)), false);
    assert!(matches!(a.total_return_base, Availability::Unavailable { .. }));
}

#[test]
fn period_amounts_capital_plus_currency_equals_total_return() {
    // Both price and FX move; verify decomposition adds up
    let txs = vec![buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(12)), Some(dec!(10)), Some(dec!(11)), false);
    assert_eq!(avail(&a.capital_gain_base) + avail(&a.currency_gain_base), avail(&a.total_return_base));
}

#[test]
fn period_amounts_buy_and_sell_all_within_period_no_double_count() {
    // Buy 100 shares at $10, FX 10 on Jun 5; sell all at $11, FX 10 on Jun 20
    // No pre-period position; end_mv = 0
    // total_return = 0 - 0 - (10000 - 11000) = 1000
    let txs = vec![
        buy_with_fx(1, "2026-06-05", 100, dec!(10), dec!(10), Decimal::ZERO),
        sell_with_fx(2, "2026-06-20", 100, dec!(11), dec!(10)),
    ];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    // end_position.quantity = 0, so end_price is not strictly needed but pass it anyway
    let a = compute_period_amounts(&p, None, Some(dec!(11)), None, Some(dec!(10)), false);
    assert_eq!(avail(&a.begin_market_value_base), dec!(0));
    assert_eq!(avail(&a.end_market_value_base), dec!(0));
    assert_eq!(avail(&a.total_return_base), dec!(1000));
    assert_eq!(avail(&a.capital_gain_base), dec!(1000));
    assert_eq!(avail(&a.currency_gain_base), dec!(0));
}

#[test]
fn period_amounts_missing_start_fx_for_non_sek_returns_unavailable() {
    // Non-SEK instrument with pre-period position; missing start_fx should be unavailable
    let txs = vec![buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO)];
    let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
    let a = compute_period_amounts(&p, Some(dec!(10)), Some(dec!(12)), None, Some(dec!(10)), false);
    assert!(matches!(a.total_return_base, Availability::Unavailable { .. }));
}
```

- [ ] **Step 2: Run to verify they fail**

```
cd backend && cargo test domain::performance::tests::period_amounts -- --nocapture 2>&1 | head -20
```

- [ ] **Step 3: Implement `PeriodAmounts` and `compute_period_amounts`**

```rust
#[derive(Debug, Clone)]
pub struct PeriodAmounts {
    pub begin_market_value_base: Availability<Decimal>,
    pub end_market_value_base: Availability<Decimal>,
    pub capital_gain_base: Availability<Decimal>,
    pub currency_gain_base: Availability<Decimal>,
    pub total_return_base: Availability<Decimal>,
}

impl PeriodAmounts {
    fn unavailable(reasons: Vec<ValuationReason>) -> Self {
        Self {
            begin_market_value_base: Availability::Unavailable { reasons: reasons.clone() },
            end_market_value_base: Availability::Unavailable { reasons: reasons.clone() },
            capital_gain_base: Availability::Unavailable { reasons: reasons.clone() },
            currency_gain_base: Availability::Unavailable { reasons: reasons.clone() },
            total_return_base: Availability::Unavailable { reasons },
        }
    }
}

pub fn compute_period_amounts(
    period: &PeriodLedger,
    start_price_native: Option<Decimal>,
    end_price_native: Option<Decimal>,
    start_fx_in: Option<Decimal>,
    end_fx_in: Option<Decimal>,
    is_sek_instrument: bool,
) -> PeriodAmounts {
    // Resolve FX — SEK instruments always use 1; non-SEK require explicit values.
    let (start_fx, end_fx) = if is_sek_instrument {
        (Decimal::ONE, Decimal::ONE)
    } else {
        let efx = match end_fx_in {
            Some(f) => f,
            None => return PeriodAmounts::unavailable(vec![ValuationReason::MissingEndFx]),
        };
        let sfx = if period.start_position.quantity > 0 {
            match start_fx_in {
                Some(f) => f,
                None => return PeriodAmounts::unavailable(vec![ValuationReason::MissingStartFx]),
            }
        } else {
            // No pre-period position; start FX only needed for pre-period sells (none possible).
            end_fx_in.unwrap_or(efx)
        };
        (sfx, efx)
    };

    // end_price required unless end_position.quantity == 0 AND no in-period sells
    // For simplicity: always require end_price.
    let end_price = match end_price_native {
        Some(p) => p,
        None => return PeriodAmounts::unavailable(vec![ValuationReason::MissingEndPrice]),
    };

    // start_price required when start_position has shares.
    let start_price = if period.start_position.quantity > 0 {
        match start_price_native {
            Some(p) => p,
            None => return PeriodAmounts::unavailable(vec![ValuationReason::MissingStartPrice]),
        }
    } else {
        Decimal::ZERO
    };

    let adj_start_qty = Decimal::from(period.start_position.quantity)
        * period.in_period_split_factor
        * period.post_period_split_factor;
    let adj_end_qty = Decimal::from(period.end_position.quantity)
        * period.post_period_split_factor;

    let begin_mv = adj_start_qty * start_price * start_fx;
    let end_mv = adj_end_qty * end_price * end_fx;

    // Accumulate flows and capital-flow-at-constant-end-fx in one pass.
    let mut net_flows = Decimal::ZERO;
    let mut capital_flows_at_end_fx = Decimal::ZERO;

    for tx in &period.period_transactions {
        match tx.kind {
            TransactionKind::Buy => {
                let p = match tx.price {
                    Some(p) => p,
                    None => return PeriodAmounts::unavailable(vec![
                        ValuationReason::MissingTransactionPrice { transaction_id: tx.id }
                    ]),
                };
                let f = if is_sek_instrument {
                    Decimal::ONE
                } else {
                    match tx.fx_rate_to_base {
                        Some(f) => f,
                        None => return PeriodAmounts::unavailable(vec![
                            ValuationReason::MissingTransactionPrice { transaction_id: tx.id }
                        ]),
                    }
                };
                let qty = Decimal::from(tx.quantity) * period.post_period_split_factor;
                net_flows += qty * p * f + tx.brokerage_base;
                capital_flows_at_end_fx += qty * p * end_fx + tx.brokerage_base;
            }
            TransactionKind::Sell => {
                let p = match tx.price {
                    Some(p) => p,
                    None => return PeriodAmounts::unavailable(vec![
                        ValuationReason::MissingTransactionPrice { transaction_id: tx.id }
                    ]),
                };
                let f = if is_sek_instrument {
                    Decimal::ONE
                } else {
                    match tx.fx_rate_to_base {
                        Some(f) => f,
                        None => return PeriodAmounts::unavailable(vec![
                            ValuationReason::MissingTransactionPrice { transaction_id: tx.id }
                        ]),
                    }
                };
                let qty = Decimal::from(-tx.quantity); // positive
                net_flows -= qty * p * f - tx.brokerage_base;
                capital_flows_at_end_fx -= qty * p * end_fx - tx.brokerage_base;
            }
            _ => {}
        }
    }

    let total_return = end_mv - begin_mv - net_flows;
    let capital_gain = (adj_end_qty * end_price - adj_start_qty * start_price) * end_fx
        - capital_flows_at_end_fx;
    let currency_gain = total_return - capital_gain;

    PeriodAmounts {
        begin_market_value_base: Availability::Available(begin_mv),
        end_market_value_base: Availability::Available(end_mv),
        capital_gain_base: Availability::Available(capital_gain),
        currency_gain_base: Availability::Available(currency_gain),
        total_return_base: Availability::Available(total_return),
    }
}
```

- [ ] **Step 4: Run tests**

```
cd backend && cargo test domain::performance -- --nocapture
```
Expected: all tests pass.

- [ ] **Step 5: Clippy + fmt + stage**

```
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/domain/performance.rs
```

---

## Task 3: Domain — Modified Dietz And Annualisation

**Files:**
- Modify: `backend/src/domain/performance.rs`

**Interfaces produced:**
```rust
pub struct CashFlow {
    pub date: NaiveDate,
    /// Positive = money into the instrument (buy cost). Negative = money out (sell proceeds).
    pub amount_base: Decimal,
}

pub fn period_cash_flows(period: &PeriodLedger, is_sek_instrument: bool) -> Availability<Vec<CashFlow>>

pub fn compute_modified_dietz(
    begin_market_value: &Availability<Decimal>,
    total_return: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal>

pub enum DisplayPercentKind { Absolute, Annualised }

pub fn apply_annualisation(
    holding_period_return: Decimal,
    period_days: i64,
) -> (Decimal, DisplayPercentKind)
```

**Modified Dietz formula:**
```
period_days = (end_date − start_date).num_days()
weight_i    = (end_date − cf_date).num_days() / period_days
denominator = begin_mv + Σ(weight_i × cf_i)
return_pct  = total_return / denominator
```

Zero, negative, or zero-day period → `Unavailable { reasons: [ValuationReason::ZeroOrInvalidPerformanceDenominator] }`.

**Annualisation:**
```
years = period_days as f64 / 365.25
if years >= 1.0:
    annualised = (1 + hpr)^(1/years) - 1
    kind = Annualised
else:
    kind = Absolute, value = hpr
Guard: if 1 + hpr <= 0 or years <= 0 → kind = Absolute
```

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn modified_dietz_simple_hold_no_cash_flows() {
    // 10,000 begin MV, 2,000 total return, no flows → 20%
    let begin = Availability::Available(dec!(10000));
    let total = Availability::Available(dec!(2000));
    let flows = Availability::Available(vec![]);
    let sd = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
    let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(); // 181 days; no flows so period_days irrelevant
    let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
    assert_eq!(avail(&result), dec!(0.2));
}

#[test]
fn modified_dietz_buy_at_start_full_weight() {
    // Begin MV 0, buy 10,000 at day 0 of 30-day period, end MV 12,000
    // weight = (end - start).num_days() / period_days = 30/30 = 1
    // denominator = 0 + 1*10000 = 10000; total_return = 2000; pct = 20%
    let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(); // 30 days
    let begin = Availability::Available(dec!(0));
    let total = Availability::Available(dec!(2000));
    let flows = Availability::Available(vec![
        CashFlow { date: sd, amount_base: dec!(10000) },
    ]);
    let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
    assert_eq!(avail(&result), dec!(0.2));
}

#[test]
fn modified_dietz_mid_period_flow_partial_weight() {
    // Begin MV 10,000; buy 10,000 on day 15 of 30-day period; total_return = 3,000
    // weight = 15/30 = 0.5; denominator = 10000 + 0.5*10000 = 15000; pct = 20%
    let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let mid = NaiveDate::from_ymd_opt(2026, 6, 16).unwrap(); // 15 days before end
    let begin = Availability::Available(dec!(10000));
    let total = Availability::Available(dec!(3000));
    let flows = Availability::Available(vec![
        CashFlow { date: mid, amount_base: dec!(10000) },
    ]);
    let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
    assert_eq!(avail(&result), dec!(0.2));
}

#[test]
fn modified_dietz_zero_denominator_is_unavailable() {
    let sd = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
    let ed = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let begin = Availability::Available(dec!(0));
    let total = Availability::Available(dec!(0));
    let flows = Availability::Available(vec![]);
    let result = compute_modified_dietz(&begin, &total, &flows, sd, ed);
    assert!(matches!(result, Availability::Unavailable { .. }));
}

#[test]
fn annualise_over_one_year_returns_annualised() {
    // 20% over 730 days ≈ 9.54%
    let (result, kind) = apply_annualisation(dec!(0.20), 730);
    assert!(matches!(kind, DisplayPercentKind::Annualised));
    let expected = rust_decimal::Decimal::from_f64_retain(1.2f64.powf(0.5) - 1.0).unwrap();
    let diff = (result - expected).abs();
    assert!(diff < dec!(0.0001), "diff too large: {diff}");
}

#[test]
fn annualise_under_one_year_returns_absolute() {
    let (result, kind) = apply_annualisation(dec!(0.20), 180);
    assert!(matches!(kind, DisplayPercentKind::Absolute));
    assert_eq!(result, dec!(0.20));
}

#[test]
fn annualise_negative_one_plus_return_returns_absolute_guard() {
    let (result, kind) = apply_annualisation(dec!(-1.5), 730);
    assert!(matches!(kind, DisplayPercentKind::Absolute));
    assert_eq!(result, dec!(-1.5));
}
```

- [ ] **Step 2: Run to verify they fail**

```
cd backend && cargo test domain::performance::tests::modified_dietz -- --nocapture 2>&1 | head -20
```

- [ ] **Step 3: Implement `CashFlow`, `period_cash_flows`, `compute_modified_dietz`, `apply_annualisation`**

```rust
#[derive(Debug, Clone)]
pub struct CashFlow {
    pub date: NaiveDate,
    pub amount_base: Decimal,
}

pub fn period_cash_flows(period: &PeriodLedger, is_sek_instrument: bool) -> Availability<Vec<CashFlow>> {
    let mut flows = Vec::new();
    for tx in &period.period_transactions {
        match tx.kind {
            TransactionKind::Buy => {
                let p = match tx.price {
                    Some(p) => p,
                    None => return Availability::Unavailable {
                        reasons: vec![ValuationReason::MissingTransactionPrice { transaction_id: tx.id }]
                    },
                };
                let f = if is_sek_instrument {
                    Decimal::ONE
                } else {
                    match tx.fx_rate_to_base {
                        Some(f) => f,
                        None => return Availability::Unavailable {
                            reasons: vec![ValuationReason::MissingTransactionPrice { transaction_id: tx.id }]
                        },
                    }
                };
                let cost = Decimal::from(tx.quantity) * p * f + tx.brokerage_base;
                flows.push(CashFlow { date: tx.trade_date, amount_base: cost });
            }
            TransactionKind::Sell => {
                let p = match tx.price {
                    Some(p) => p,
                    None => return Availability::Unavailable {
                        reasons: vec![ValuationReason::MissingTransactionPrice { transaction_id: tx.id }]
                    },
                };
                let f = if is_sek_instrument {
                    Decimal::ONE
                } else {
                    match tx.fx_rate_to_base {
                        Some(f) => f,
                        None => return Availability::Unavailable {
                            reasons: vec![ValuationReason::MissingTransactionPrice { transaction_id: tx.id }]
                        },
                    }
                };
                let proceeds = Decimal::from(-tx.quantity) * p * f - tx.brokerage_base;
                flows.push(CashFlow { date: tx.trade_date, amount_base: -proceeds });
            }
            _ => {}
        }
    }
    Availability::Available(flows)
}

pub fn compute_modified_dietz(
    begin_market_value: &Availability<Decimal>,
    total_return: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal> {
    let begin_mv = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };
    let total = match total_return {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };
    let flows = match cash_flows {
        Availability::Available(v) => v,
        Availability::Unavailable { reasons } => return Availability::Unavailable { reasons: reasons.clone() },
    };

    let period_days = (end_date - start_date).num_days();
    if period_days <= 0 {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    let period_days_dec = Decimal::from(period_days);

    let weighted_flows: Decimal = flows.iter().map(|cf| {
        let remaining = (end_date - cf.date).num_days().max(0);
        let weight = Decimal::from(remaining) / period_days_dec;
        weight * cf.amount_base
    }).sum();

    let denominator = begin_mv + weighted_flows;
    if denominator <= Decimal::ZERO {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }

    Availability::Available(total / denominator)
}

pub enum DisplayPercentKind {
    Absolute,
    Annualised,
}

pub fn apply_annualisation(
    holding_period_return: Decimal,
    period_days: i64,
) -> (Decimal, DisplayPercentKind) {
    let years = period_days as f64 / 365.25;
    if years < 1.0 || period_days <= 0 {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let one_plus_r = holding_period_return + Decimal::ONE;
    if one_plus_r <= Decimal::ZERO {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let base: f64 = one_plus_r.try_into().unwrap_or(f64::NAN);
    let annualised = base.powf(1.0 / years) - 1.0;
    match rust_decimal::Decimal::from_f64_retain(annualised) {
        Some(d) => (d, DisplayPercentKind::Annualised),
        None => (holding_period_return, DisplayPercentKind::Absolute),
    }
}
```

- [ ] **Step 4: Run all performance tests**

```
cd backend && cargo test domain::performance -- --nocapture
```

- [ ] **Step 5: Clippy + fmt + stage**

```
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/domain/performance.rs backend/src/domain/mod.rs
```

---

## Task 4: API — Date Range Query Params And Period Price Loading

**Files:**
- Modify: `backend/src/api/gains.rs`
- Modify: `backend/src/api/valuation.rs`

**Interfaces produced:**
```rust
// In gains.rs
pub struct GainsQuery {
    pub include_closed: bool,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

// In valuation.rs
pub(super) struct PeriodInputs {
    pub start_price: Option<PriceCandidate>,
    pub end_price: Option<PriceCandidate>,
    pub start_fx: Option<FxCandidate>,
    pub end_fx: Option<FxCandidate>,
}

pub(super) async fn load_period_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    start_date: Option<NaiveDate>,  // None = inception
    end_date: NaiveDate,
) -> Result<PeriodInputs, ApiError>
```

**Important:** The `gains.rs` handler must filter the instrument's full transaction ledger to `trade_date ≤ end_date` before any derivation. Transactions after `end_date` must not affect any result (positions, cash flows, split factors, or accumulated totals).

- [ ] **Step 1: Write failing integration tests**

Add to `backend/src/api/gains.rs` tests module:

```rust
#[tokio::test]
async fn gains_rejects_malformed_start_date() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains?start_date=not-a-date", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_date");
}

#[tokio::test]
async fn gains_rejects_start_after_end() {
    let state = AppState::for_tests().await;
    let (status, body) = send(
        &state, "GET",
        "/api/gains?start_date=2026-06-30&end_date=2026-06-01",
        json!({}),
    ).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "start_date_after_end_date");
}

#[tokio::test]
async fn gains_with_end_date_uses_that_date_as_valuation_date() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-01-15", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["as_of_date"], "2026-01-15");
    assert_eq!(body["report_period"]["end_date"], "2026-01-15");
}

#[tokio::test]
async fn gains_with_no_dates_returns_inception_period() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["report_period"]["start_date"].is_null());
}

#[tokio::test]
async fn gains_post_end_date_transaction_excluded() {
    // Buy before range, buy after range; only first buy should affect end position.
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "TSLA", "NASDAQ", "USD").await;
    send(&state, "POST", "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    ).await;
    send(&state, "POST", "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-09-01",
               "quantity":100,"price":"15","currency":"USD","fx_rate_to_base":"10"}),
    ).await;
    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    // Row for TSLA should show quantity 100, not 200
    let row = body["rows"].as_array().unwrap()
        .iter().find(|r| r["ticker"] == "TSLA").unwrap();
    assert_eq!(row["quantity"], 100);
}
```

- [ ] **Step 2: Run to verify they fail**

```
cd backend && cargo test api::gains::tests::gains_rejects -- --nocapture 2>&1 | head -20
```

- [ ] **Step 3: Extend `GainsQuery` and add date parsing**

In `gains.rs`:

```rust
#[derive(Debug, Deserialize)]
pub struct GainsQuery {
    #[serde(default)]
    include_closed: bool,
    start_date: Option<String>,
    end_date: Option<String>,
}

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
        ApiError::bad_request("invalid_date", format!("invalid {field}: {s}"))
    })
}
```

In the `list()` handler, before the main loop:

```rust
let end_date = match &query.end_date {
    Some(s) => parse_date(s, "end_date")?,
    None => Local::now().naive_local().date(),
};
let start_date = match &query.start_date {
    Some(s) => {
        let d = parse_date(s, "start_date")?;
        if d > end_date {
            return Err(ApiError::bad_request(
                "start_date_after_end_date",
                "start_date must not be after end_date",
            ));
        }
        Some(d)
    }
    None => None,
};
```

Then filter the transaction ledger before derivation:
```rust
// Truncate ledger at end_date — transactions after end_date must not affect any result.
let ledger: Vec<LedgerTransaction> = all_transactions
    .into_iter()
    .filter(|t| t.trade_date <= end_date)
    .collect();
```

- [ ] **Step 4: Add `ReportPeriodResponse` and new fields to `GainsResponse`**

```rust
#[derive(Debug, Serialize)]
pub struct ReportPeriodResponse {
    pub start_date: Option<String>,
    pub end_date: String,
}

// In GainsResponse, add:
pub report_period: ReportPeriodResponse,
pub percentage_method: String,
pub display_percent_kind: String,
```

Populate in the handler:
```rust
report_period: ReportPeriodResponse {
    start_date: start_date.map(|d| d.format("%Y-%m-%d").to_string()),
    end_date: end_date.format("%Y-%m-%d").to_string(),
},
percentage_method: "modified_dietz".to_string(),
display_percent_kind: "absolute".to_string(), // updated in Task 5
```

- [ ] **Step 5: Add `load_period_inputs` to `valuation.rs`**

```rust
pub(super) struct PeriodInputs {
    pub start_price: Option<PriceCandidate>,
    pub end_price: Option<PriceCandidate>,
    pub start_fx: Option<FxCandidate>,
    pub end_fx: Option<FxCandidate>,
}

pub(super) async fn load_period_inputs(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    start_date: Option<NaiveDate>,
    end_date: NaiveDate,
) -> Result<PeriodInputs, ApiError> {
    let price_mapping =
        provider_symbols::find_by_instrument_provider(pool, instrument.id, PRICE_PROVIDER).await?;
    let mapping_enabled = price_mapping.as_ref().is_some_and(|r| r.enabled);

    let (start_price, end_price) = if mapping_enabled {
        let end = prices::find_latest_on_or_before(pool, instrument.id, PRICE_PROVIDER, end_date)
            .await?
            .and_then(price_candidate);
        let start = if let Some(sd) = start_date {
            prices::find_latest_on_or_before(pool, instrument.id, PRICE_PROVIDER, sd)
                .await?
                .and_then(price_candidate)
        } else {
            None
        };
        (start, end)
    } else {
        (None, None)
    };

    let is_base_currency = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
    let (start_fx, end_fx) = if is_base_currency {
        (None, None)
    } else {
        let end = fx_rates::find_latest_on_or_before(
            pool, &instrument.currency, BASE_CURRENCY, FX_PROVIDER, end_date,
        ).await?.and_then(fx_candidate);
        let start = if let Some(sd) = start_date {
            fx_rates::find_latest_on_or_before(
                pool, &instrument.currency, BASE_CURRENCY, FX_PROVIDER, sd,
            ).await?.and_then(fx_candidate)
        } else {
            None
        };
        (start, end)
    };

    Ok(PeriodInputs { start_price, end_price, start_fx, end_fx })
}
```

(`price_candidate` and `fx_candidate` are the existing private helpers in `valuation.rs`.)

- [ ] **Step 6: Run tests**

```
cd backend && cargo test api::gains::tests -- --nocapture
```
Expected: new validation tests pass; existing gains tests continue to pass.

- [ ] **Step 7: Clippy + fmt + stage**

```
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/api/gains.rs backend/src/api/valuation.rs
```

---

## Task 5: API — Wire Performance Engine Into Gains Handler

**Files:**
- Modify: `backend/src/api/gains.rs`

Replace the cost-basis percentage calculation with Modified Dietz aggregation. When `start_date` or `end_date` is provided, `capital_gain_base`, `currency_gain_base`, and `total_return_base` in `TotalsResponse` come from `PeriodAmounts`, not from the existing cost-basis accumulator. The existing cost-basis amounts remain available on individual rows.

**Inception mode:** When `start_date` is `None`, derive `effective_start_date` from the earliest `trade_date` in the (truncated) ledger. Pass `effective_start_date` to `reconstruct_period` and `compute_modified_dietz`. Pass `None` for `start_price_native` to `compute_period_amounts` (begin MV = 0).

**Denominator computation:** Extract the raw denominator from `compute_modified_dietz` by adding a helper that returns `(pct, denominator)`, or recompute it inline. Component percentages must share the same denominator as `total_return_percent`.

- [ ] **Step 1: Write failing integration test for date-range performance**

```rust
#[tokio::test]
async fn gains_with_date_range_returns_modified_dietz_percent() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

    send(&state, "POST", "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    ).await;

    let fetched_at = crate::import::now_iso8601();
    prices::upsert(&state.pool, &prices::NewPrice {
        instrument_id,
        provider: PRICE_PROVIDER.to_owned(),
        provider_symbol: "MSFT".to_owned(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        close: dec!(12),
        currency: "USD".to_owned(),
        fetched_at: fetched_at.clone(),
    }).await.unwrap();
    fx_rates::upsert(&state.pool, &fx_rates::NewFxRate {
        base: "USD".to_owned(),
        quote: BASE_CURRENCY.to_owned(),
        date: chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
        rate: dec!(10),
        provider: FX_PROVIDER.to_owned(),
        fetched_at: fetched_at.clone(),
    }).await.unwrap();
    provider_symbols::upsert(&state.pool, &provider_symbols::NewProviderSymbol {
        instrument_id,
        provider: PRICE_PROVIDER.to_owned(),
        provider_symbol: "MSFT".to_owned(),
        currency: Some("USD".to_owned()),
        enabled: true,
        created_at: fetched_at.clone(),
        updated_at: fetched_at,
    }).await.unwrap();

    let (status, body) = send(
        &state, "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
        json!({}),
    ).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["percentage_method"], "modified_dietz");
    // start price for 2026-06-01 is not seeded → performance unavailable
    assert_eq!(body["totals"]["total_return_percent"]["status"], "unavailable");
}
```

- [ ] **Step 2: Run to verify it fails**

```
cd backend && cargo test api::gains::tests::gains_with_date_range -- --nocapture 2>&1 | head -30
```

- [ ] **Step 3: Add `PerformanceAccumulator` and wire into handler**

```rust
#[derive(Default)]
struct PerformanceAccumulator {
    begin_mv: Decimal,
    total_return: Decimal,
    capital_gain: Decimal,
    currency_gain: Decimal,
    cash_flows: Vec<CashFlow>,
    unavailable_reasons: Vec<ValuationReason>,
}

impl PerformanceAccumulator {
    fn add(&mut self, amounts: &PeriodAmounts, flows: &Availability<Vec<CashFlow>>) {
        match (
            &amounts.begin_market_value_base,
            &amounts.total_return_base,
            &amounts.capital_gain_base,
            &amounts.currency_gain_base,
        ) {
            (
                Availability::Available(begin),
                Availability::Available(total),
                Availability::Available(cap),
                Availability::Available(cur),
            ) => {
                self.begin_mv += begin;
                self.total_return += total;
                self.capital_gain += cap;
                self.currency_gain += cur;
                if let Availability::Available(cfs) = flows {
                    self.cash_flows.extend_from_slice(cfs);
                }
            }
            _ => {
                // Collect all reasons for transparency
                for field in [
                    &amounts.begin_market_value_base,
                    &amounts.total_return_base,
                ] {
                    if let Availability::Unavailable { reasons } = field {
                        self.unavailable_reasons.extend_from_slice(reasons);
                    }
                }
            }
        }
    }

    fn into_percents(
        self,
        effective_start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> (
        AvailabilityResponse, // capital_gain_percent
        AvailabilityResponse, // currency_gain_percent
        AvailabilityResponse, // total_return_percent
        String,               // display_percent_kind
    ) {
        if !self.unavailable_reasons.is_empty() {
            let u = AvailabilityResponse::Unavailable { reasons: serialize_reasons(&self.unavailable_reasons) };
            return (u.clone(), u.clone(), u, "absolute".to_string());
        }

        let begin = Availability::Available(self.begin_mv);
        let total = Availability::Available(self.total_return);
        let flows = Availability::Available(self.cash_flows);
        let period_days = (end_date - effective_start_date).num_days();

        let pct = compute_modified_dietz(&begin, &total, &flows, effective_start_date, end_date);

        // Compute denominator for component percentages by re-deriving it.
        let denominator_avail = compute_modified_dietz_denominator(
            self.begin_mv, &flows_ref, effective_start_date, end_date,
        );

        let cap_pct = component_percent(self.capital_gain, &denominator_avail);
        let cur_pct = component_percent(self.currency_gain, &denominator_avail);

        let (total_pct_value, kind) = match &pct {
            Availability::Available(r) => apply_annualisation(*r, period_days),
            Availability::Unavailable { reasons } => {
                return (
                    cap_pct,
                    cur_pct,
                    AvailabilityResponse::Unavailable { reasons: serialize_reasons(reasons) },
                    "absolute".to_string(),
                );
            }
        };

        (
            cap_pct,
            cur_pct,
            serialize_availability(&Availability::Available(total_pct_value), |v| format!("{:.4}", v)),
            match kind { DisplayPercentKind::Annualised => "annualised", DisplayPercentKind::Absolute => "absolute" }.to_string(),
        )
    }
}
```

Add a denominator helper to `performance.rs` (or expose it inline):
```rust
pub fn compute_modified_dietz_denominator(
    begin_mv: Decimal,
    cash_flows: &[CashFlow],
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal> {
    let period_days = (end_date - start_date).num_days();
    if period_days <= 0 {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    let pd = Decimal::from(period_days);
    let weighted: Decimal = cash_flows.iter().map(|cf| {
        let remaining = (end_date - cf.date).num_days().max(0);
        Decimal::from(remaining) / pd * cf.amount_base
    }).sum();
    let d = begin_mv + weighted;
    if d <= Decimal::ZERO {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    Availability::Available(d)
}
```

In the handler's main loop, for each instrument:
1. Load `all_transactions` from DB; filter to `trade_date ≤ end_date` (ledger truncation)
2. Derive `effective_start_date`: if `start_date.is_none()`, use the earliest `trade_date` in ledger, else `start_date.unwrap()`
3. Call `reconstruct_period(&ledger, effective_start_date, end_date)`
4. Call `load_period_inputs(pool, instrument, start_date, end_date)` — note: `start_date` here is `None` for inception so no start price is loaded
5. Determine `is_sek = instrument.currency.eq_ignore_ascii_case("SEK")`
6. Call `compute_period_amounts(&period, start_price, end_price, start_fx, end_fx, is_sek)` — pass `None` for `start_price_native` in inception mode (start_position.quantity will be 0 for instruments with all transactions in-period)
7. Call `period_cash_flows(&period, is_sek)`
8. Feed into `PerformanceAccumulator::add()`
9. Continue to accumulate instrument rows as before

After the loop, replace `TotalsResponse` amount fields and all percent fields:
- `capital_gain_base`, `currency_gain_base`, `total_return_base` come from the `PerformanceAccumulator` accumulated `PeriodAmounts` values
- Percent fields come from `PerformanceAccumulator::into_percents(effective_start_date, end_date)`
- Set `display_percent_kind` from the returned kind string

- [ ] **Step 4: Run all gains tests**

```
cd backend && cargo test api::gains -- --nocapture
```
Expected: all existing tests continue to pass; new date-range test passes.

- [ ] **Step 5: Clippy + fmt + stage**

```
cd backend && cargo clippy --all-targets -- -D warnings && cargo fmt
git add backend/src/api/gains.rs backend/src/api/valuation.rs backend/src/domain/performance.rs backend/src/domain/mod.rs
```

---

## Task 6: Frontend — Types, Query Hook, And Date-Range Control

**Files:**
- Modify: `frontend/src/api/types.ts`
- Modify: `frontend/src/api/queries.ts`
- Modify: `frontend/src/components/GainsTable.tsx`
- Modify: `frontend/src/components/AssetView.tsx`

**Human testing required:** After implementation, verify:
- Date range control renders in toolbar above the Gains table.
- Selecting "12M" updates the table and totals with non-cost-basis percentages.
- Selecting "All" passes no `start_date` (inception mode).
- Totals band shows "Performance return" or "Annualised return" label depending on `display_percent_kind`.
- Custom date inputs accept `YYYY-MM-DD` and validate client-side before sending.
- Existing "Include closed positions" checkbox still works.

- [ ] **Step 1: Extend `types.ts`**

```typescript
export interface ReportPeriod {
  start_date: string | null;
  end_date: string;
}

export interface GainsResponse {
  as_of_date: string;
  base_currency: string;
  include_closed_positions: boolean;
  summary: GainsSummary;
  totals: GainsTotals;
  rows: GainsRow[];
  // New fields:
  report_period: ReportPeriod;
  percentage_method: string;        // "modified_dietz"
  display_percent_kind: string;     // "absolute" | "annualised"
}
```

- [ ] **Step 2: Update `queries.ts`**

```typescript
export interface GainsParams {
  includeClosedPositions?: boolean;
  startDate?: string | null;
  endDate?: string | null;
}

export function useGains(params: GainsParams = {}) {
  const { includeClosedPositions = false, startDate, endDate } = params;
  return useQuery({
    queryKey: ["gains", includeClosedPositions, startDate ?? null, endDate ?? null],
    queryFn: () => {
      const search = new URLSearchParams();
      if (includeClosedPositions) search.set("include_closed", "true");
      if (startDate) search.set("start_date", startDate);
      if (endDate) search.set("end_date", endDate);
      const qs = search.toString();
      return apiGet<GainsResponse>(`/api/gains${qs ? `?${qs}` : ""}`);
    },
  });
}
```

- [ ] **Step 3: Update all callers of `useGains`**

Find all usages:
```
cd frontend && grep -r "useGains" src/
```

Update each caller to the object-parameter form:
- `useGains(false)` → `useGains()`
- `useGains(true)` → `useGains({ includeClosedPositions: true })`

This covers at minimum: `BoardView.tsx`, `GainsTable.tsx`, `AssetView.tsx`.

- [ ] **Step 4: Add date-range control to `GainsTable.tsx`**

Use `toLocaleDateString('sv-SE')` for local-timezone date formatting (yields `YYYY-MM-DD`):

```typescript
type DatePreset = "today" | "7d" | "12m" | "ytd" | "fy" | "all" | "custom";

interface DateRange {
  startDate: string | null;
  endDate: string | null;
}

function localDateString(d: Date): string {
  return d.toLocaleDateString("sv-SE"); // yields YYYY-MM-DD in local time
}

function presetToRange(preset: DatePreset, customStart: string, customEnd: string): DateRange {
  const today = new Date();
  const fmt = localDateString;

  switch (preset) {
    case "today":
      return { startDate: fmt(today), endDate: fmt(today) };
    case "7d": {
      const start = new Date(today);
      start.setDate(start.getDate() - 7);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "12m": {
      const start = new Date(today);
      start.setFullYear(start.getFullYear() - 1);
      return { startDate: fmt(start), endDate: fmt(today) };
    }
    case "ytd":
      return { startDate: `${today.getFullYear()}-01-01`, endDate: fmt(today) };
    case "fy":
      // Swedish fiscal year = calendar year; "FY to date" = Jan 1 to today
      return { startDate: `${today.getFullYear()}-01-01`, endDate: fmt(today) };
    case "all":
      return { startDate: null, endDate: fmt(today) };
    case "custom":
      return { startDate: customStart || null, endDate: customEnd || fmt(today) };
  }
}
```

Add preset buttons and custom date inputs in the toolbar:

```tsx
<div className="table-toolbar">
  <div className="date-range-presets">
    {(["today", "7d", "12m", "ytd", "fy", "all", "custom"] as DatePreset[]).map((p) => (
      <button
        key={p}
        type="button"
        className={`preset-btn${selectedPreset === p ? " active" : ""}`}
        onClick={() => {
          setSelectedPreset(p);
          onDateRangeChange(presetToRange(p, customStart, customEnd));
        }}
      >
        {p === "today" ? "Today" : p === "7d" ? "7D" : p === "12m" ? "12M"
          : p === "ytd" ? "YTD" : p === "fy" ? "FY" : p === "all" ? "All" : "Custom"}
      </button>
    ))}
    {selectedPreset === "custom" && (
      <>
        <input
          type="date"
          value={customStart}
          onChange={(e) => {
            setCustomStart(e.target.value);
            onDateRangeChange(presetToRange("custom", e.target.value, customEnd));
          }}
        />
        <input
          type="date"
          value={customEnd}
          onChange={(e) => {
            setCustomEnd(e.target.value);
            onDateRangeChange(presetToRange("custom", customStart, e.target.value));
          }}
        />
      </>
    )}
  </div>
  {/* existing filter + closed-positions controls */}
</div>
```

- [ ] **Step 5: Show percentage kind label in totals band**

```tsx
function GainsTotalsBand({
  totals,
  displayPercentKind,
}: {
  totals: GainsTotals;
  displayPercentKind: string;
}) {
  const label =
    displayPercentKind === "annualised" ? "Annualised return" : "Performance return";
  return (
    <section className="gains-totals" aria-label="Gains totals">
      <span className="gains-totals-method">{label}</span>
      {/* existing metric cells */}
    </section>
  );
}
```

- [ ] **Step 6: Wire date state into `BoardView.tsx`**

Move `dateRange` state to `BoardView`. Pass `onDateRangeChange` and `dateRange` down to `GainsTable`. Pass `dateRange.startDate` and `dateRange.endDate` into `useGains`.

Default state: `{ startDate: null, endDate: null }` (inception / today, matching "All" preset).

- [ ] **Step 7: Run frontend checks**

```
cd frontend && npm run check && npm run fmt
```

Expected: no TypeScript errors, no lint errors.

- [ ] **Step 8: Stage**

```
git add frontend/src/api/types.ts frontend/src/api/queries.ts frontend/src/components/GainsTable.tsx frontend/src/components/BoardView.tsx frontend/src/components/AssetView.tsx
```

---

## Task 7: Version Bumps And Decision Log

**Files:**
- Modify: `backend/Cargo.toml`
- Modify: `frontend/package.json`
- Modify: `docs/DecisionLog.md`

- [ ] **Step 1: Bump backend version**

In `backend/Cargo.toml`, increment minor version (e.g. `0.4.6` → `0.5.0`):
```toml
version = "0.5.0"
```

- [ ] **Step 2: Bump frontend version**

In `frontend/package.json`, increment minor version to match:
```json
"version": "0.5.0"
```

- [ ] **Step 3: Add decision log entries**

Append to `docs/DecisionLog.md`:

```markdown
## 2026-06-19: Sharesight-Style Performance Returns Method

Decision: The Gains view uses Modified Dietz money-weighted performance returns for all percentage totals, replacing cost-basis percentages. The denominator uses calendar-day weights. Component percentages (capital, currency) share the Modified Dietz denominator but are not individually annualised; only `total_return_percent` is annualised when average years invested ≥ 1. Period years are approximated as `period_days / 365.25`.

Context: Sharesight documents its Performance Report as dollar-weighted / money-weighted using a Modified Dietz variation. Cost-basis ratios were misleading when compared against Sharesight exports.

Consequences: The Gains API accepts `start_date` and `end_date` query params; missing prices/FX surface as explicit unavailable reasons, never zero. Income/dividends remain unavailable until Phase 5. Row-level percentages retain cost-basis semantics; only totals are Modified Dietz.

## 2026-06-19: Period Reconstruction Boundary Convention

Decision: For period performance, transactions strictly before `start_date` form the opening position; transactions with `trade_date ∈ [start_date, end_date]` are period cash flows. A buy on the first day of the report is a cash inflow, not an opening balance. Transactions after `end_date` are excluded entirely (ledger truncation).

Context: This matches the spec's initial recommendation and the intuition that a purchase on the start date is a decision made within the reporting window.

Consequences: When `start_date` matches a buy date, the begin market value is zero for those shares. Calibrate this boundary against a Sharesight export before claiming full compatibility.

## 2026-06-19: Split-Adjusted Quantities For Period Valuation

Decision: `split_factor(transactions, opening_qty)` computes the cumulative factor given the quantity already held before the first transaction in the slice. For in-period splits, opening_qty = start_position.quantity; for post-period splits, opening_qty = end_position.quantity. Split factor for a split with delta d when quantity is q: factor = (q+d)/q.

Context: Yahoo historical prices are split-adjusted. A pre-split ledger quantity without adjustment would understate market value when multiplied by a split-adjusted price. Starting running_qty at zero would silently miscalculate factors for pre-period holders.

Consequences: Instruments with splits in or after the report range will have adjusted quantities in the performance computation. Instruments without splits are unaffected (factor = 1).

## 2026-06-19: Performance Amount Formula (Market-Value Identity)

Decision: total_return = end_mv − begin_mv − net_flows. Capital gain = same formula evaluated at constant end_fx. Currency gain = total_return − capital_gain. No held-quantity tracking or FIFO matching required.

Context: A stateful "held vs sold" tracking approach risks negative held quantities when in-period sells exceed the start position. The market-value identity is equivalent and avoids this class of errors.

Consequences: Decomposition into capital and currency gain is correct for all cases including buy-then-sell within the period, mixed pre/in-period sells, and inception mode.
```

- [ ] **Step 4: Build and run full test suite**

```
cd backend && cargo build && cargo test -- --nocapture 2>&1 | tail -20
```

Expected: all tests pass, no compilation errors.

- [ ] **Step 5: Stage**

```
git add backend/Cargo.toml frontend/package.json docs/DecisionLog.md
```

---

## Verification Summary

| Phase | Automated | Human |
|---|---|---|
| Period reconstruction | Unit tests in `domain::performance` | Compare start/end quantities for one known instrument |
| Amount components | Unit tests; buy-then-sell no-double-count test; decomposition identity | Compare monetary values against Sharesight for major holdings |
| Modified Dietz percent | Unit tests; mid-period flow weight; zero-denominator guard | Compare total return % for current portfolio against Sharesight |
| Date-range UI | API integration tests for validation and ledger truncation | Test Today, 12M, All, and one custom Sharesight range; confirm label shows "Annualised" when applicable |
| Missing data handling | Unit tests for `Unavailable` reasons; non-SEK FX tests | Confirm UI shows no false zeros when start price is missing |

**Note on price/FX coverage:** This implementation uses cached price and FX data only. For custom historical ranges, `end_price` or `start_price` may be unavailable if that date was never fetched. The first implementation is cache-only and will return `Unavailable` until a backfill task lands; do not claim reconciliation with Sharesight until a known range has been verified with both prices cached.

**Expected residual at first calibration:** Income component will show "Not tracked" until Phase 5 (dividends). Total return percentage will be lower than Sharesight when dividends are material. Do not tune the Modified Dietz method to absorb this gap.
