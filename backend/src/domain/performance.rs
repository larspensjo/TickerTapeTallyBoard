use chrono::NaiveDate;
use rust_decimal::prelude::ToPrimitive;
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

/// Computes the cumulative split factor for a slice of transactions,
/// given the quantity already held before the first transaction in the slice.
/// For each Split with delta d when running quantity is q: factor *= (q + d) / q.
fn split_factor(
    transactions: &[LedgerTransaction],
    opening_qty: i64,
) -> Result<Decimal, LedgerError> {
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
            begin_market_value_base: Availability::Unavailable {
                reasons: reasons.clone(),
            },
            end_market_value_base: Availability::Unavailable {
                reasons: reasons.clone(),
            },
            capital_gain_base: Availability::Unavailable {
                reasons: reasons.clone(),
            },
            currency_gain_base: Availability::Unavailable {
                reasons: reasons.clone(),
            },
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
            None => {
                return PeriodAmounts::unavailable(vec![ValuationReason::MissingEndFx]);
            }
        };
        let sfx = if period.start_position.quantity > 0 {
            match start_fx_in {
                Some(f) => f,
                None => {
                    return PeriodAmounts::unavailable(vec![ValuationReason::MissingStartFx]);
                }
            }
        } else {
            // No pre-period position; start FX only needed for pre-period sells (none possible).
            end_fx_in.unwrap_or(efx)
        };
        (sfx, efx)
    };

    // End price is only needed when shares remain at the end of the period.
    let end_price = if period.end_position.quantity > 0 {
        match end_price_native {
            Some(p) => p,
            None => {
                return PeriodAmounts::unavailable(vec![ValuationReason::MissingEndPrice]);
            }
        }
    } else {
        Decimal::ZERO
    };

    // start_price required when start_position has shares.
    let start_price = if period.start_position.quantity > 0 {
        match start_price_native {
            Some(p) => p,
            None => {
                return PeriodAmounts::unavailable(vec![ValuationReason::MissingStartPrice]);
            }
        }
    } else {
        Decimal::ZERO
    };

    let adj_start_qty = Decimal::from(period.start_position.quantity)
        * period.in_period_split_factor
        * period.post_period_split_factor;
    let adj_end_qty = Decimal::from(period.end_position.quantity) * period.post_period_split_factor;

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
                    None => {
                        return PeriodAmounts::unavailable(vec![
                            ValuationReason::MissingTransactionPrice {
                                transaction_id: tx.id,
                            },
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
                                ValuationReason::MissingTransactionFx {
                                    transaction_id: tx.id,
                                },
                            ]);
                        }
                    }
                };
                let qty = Decimal::from(tx.quantity) * period.post_period_split_factor;
                net_flows += qty * p * f + tx.brokerage_base;
                capital_flows_at_end_fx += qty * p * end_fx + tx.brokerage_base;
            }
            TransactionKind::Sell => {
                let p = match tx.price {
                    Some(p) => p,
                    None => {
                        return PeriodAmounts::unavailable(vec![
                            ValuationReason::MissingTransactionPrice {
                                transaction_id: tx.id,
                            },
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
                                ValuationReason::MissingTransactionFx {
                                    transaction_id: tx.id,
                                },
                            ]);
                        }
                    }
                };
                let qty = Decimal::from(-tx.quantity); // positive (tx.quantity is negative for sells)
                net_flows -= qty * p * f - tx.brokerage_base;
                capital_flows_at_end_fx -= qty * p * end_fx - tx.brokerage_base;
            }
            TransactionKind::Split | TransactionKind::Dividend => {}
        }
    }

    let total_return = end_mv - begin_mv - net_flows;
    let capital_gain =
        (adj_end_qty * end_price - adj_start_qty * start_price) * end_fx - capital_flows_at_end_fx;
    let currency_gain = total_return - capital_gain;

    PeriodAmounts {
        begin_market_value_base: Availability::Available(begin_mv),
        end_market_value_base: Availability::Available(end_mv),
        capital_gain_base: Availability::Available(capital_gain),
        currency_gain_base: Availability::Available(currency_gain),
        total_return_base: Availability::Available(total_return),
    }
}

/// A cash flow event within a period: positive means money into the instrument (buy cost),
/// negative means money out (sell proceeds).
#[derive(Debug, Clone)]
pub struct CashFlow {
    pub date: NaiveDate,
    /// Positive = money into the instrument (buy cost). Negative = money out (sell proceeds).
    pub amount_base: Decimal,
}

/// Collect all buy/sell cash flows from a period ledger in base currency.
///
/// Returns `Unavailable` if any transaction is missing a price or (for non-SEK) an FX rate.
pub fn period_cash_flows(
    period: &PeriodLedger,
    is_sek_instrument: bool,
) -> Availability<Vec<CashFlow>> {
    let mut flows = Vec::new();
    for tx in &period.period_transactions {
        match tx.kind {
            TransactionKind::Buy => {
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
                // Apply post_period_split_factor so denominator flows agree with
                // compute_period_amounts numerator flows (both use split-adjusted quantities).
                let qty = Decimal::from(tx.quantity) * period.post_period_split_factor;
                let cost = qty * p * f + tx.brokerage_base;
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: cost,
                });
            }
            TransactionKind::Sell => {
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
                // tx.quantity is negative for sells; (-tx.quantity) is the positive share count.
                // Apply post_period_split_factor to match compute_period_amounts.
                let qty = Decimal::from(-tx.quantity) * period.post_period_split_factor;
                let proceeds = qty * p * f - tx.brokerage_base;
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: -proceeds,
                });
            }
            TransactionKind::Split | TransactionKind::Dividend => {}
        }
    }
    Availability::Available(flows)
}

/// Collect all buy/sell cash flows from a period ledger in base currency using the actual
/// quantities traded (no post-period split adjustment).
///
/// Sign convention: + for buy cost (qty × price × fx + brokerage), − for sell proceeds.
/// Returns `Unavailable` if any transaction is missing a price or (for non-SEK) an FX rate.
pub fn actual_period_cash_flows(
    period: &PeriodLedger,
    is_sek_instrument: bool,
) -> Availability<Vec<CashFlow>> {
    let mut flows = Vec::new();
    for tx in &period.period_transactions {
        match tx.kind {
            TransactionKind::Buy => {
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
                let qty = Decimal::from(tx.quantity);
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: qty * p * f + tx.brokerage_base,
                });
            }
            TransactionKind::Sell => {
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
                let qty = Decimal::from(-tx.quantity);
                let proceeds = qty * p * f - tx.brokerage_base;
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: -proceeds,
                });
            }
            TransactionKind::Split | TransactionKind::Dividend => {}
        }
    }
    Availability::Available(flows)
}

/// Compute the Modified Dietz weighted denominator (begin_mv + Σ weight_i × cf_i).
///
/// Returns `Unavailable` when the denominator is zero or negative, or `period_days ≤ 0`.
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
    let period_days_dec = Decimal::from(period_days);
    let weighted_flows: Decimal = cash_flows
        .iter()
        .map(|cf| {
            let remaining = (end_date - cf.date).num_days().max(0);
            let weight = Decimal::from(remaining) / period_days_dec;
            weight * cf.amount_base
        })
        .sum();
    let denominator = begin_mv + weighted_flows;
    if denominator <= Decimal::ZERO {
        return Availability::Unavailable {
            reasons: vec![ValuationReason::ZeroOrInvalidPerformanceDenominator],
        };
    }
    Availability::Available(denominator)
}

/// Compute the Modified Dietz holding-period return:
///
/// ```text
/// return = total_return / (begin_mv + Σ weight_i × cf_i)
/// ```
///
/// Returns `Unavailable` if any input is unavailable, if `period_days ≤ 0`, or if the
/// denominator is zero or negative.
pub fn compute_modified_dietz(
    begin_market_value: &Availability<Decimal>,
    total_return: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<Decimal> {
    let begin_mv = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let total = match total_return {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let flows = match cash_flows {
        Availability::Available(v) => v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };

    let denominator =
        match compute_modified_dietz_denominator(begin_mv, flows, start_date, end_date) {
            Availability::Available(d) => d,
            unavail => return unavail,
        };

    Availability::Available(total / denominator)
}

/// Whether a performance percentage is displayed as-is or annualised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayPercentKind {
    Absolute,
    Annualised,
}

/// Convert a holding-period return to an annualised figure when the period exceeds one year.
///
/// Returns `(value, Absolute)` when:
/// - `period_days ≤ 0`
/// - `years < 1.0`
/// - `1 + hpr ≤ 0` (cannot take a root of a non-positive base)
/// - The float conversion fails
pub fn apply_annualisation(
    holding_period_return: Decimal,
    period_days: i64,
) -> (Decimal, DisplayPercentKind) {
    if period_days <= 0 {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let years = period_days as f64 / 365.25;
    if years < 1.0 {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let one_plus_r = holding_period_return + Decimal::ONE;
    if one_plus_r <= Decimal::ZERO {
        return (holding_period_return, DisplayPercentKind::Absolute);
    }
    let base: f64 = match one_plus_r.to_f64() {
        Some(v) => v,
        None => return (holding_period_return, DisplayPercentKind::Absolute),
    };
    let annualised_f64 = base.powf(1.0 / years) - 1.0;
    match rust_decimal::Decimal::from_f64_retain(annualised_f64) {
        Some(d) => (d, DisplayPercentKind::Annualised),
        None => (holding_period_return, DisplayPercentKind::Absolute),
    }
}

#[derive(Debug, Clone)]
pub struct MoneyWeightedReturn {
    pub annualized: Decimal,
    pub cumulative: Decimal,
    pub period_days: i64,
}

/// `cash_flows` MUST be the actual investor cash-flow series (real cash at trade date, with
/// no post-period split multiplication).
pub fn compute_money_weighted_return(
    begin_market_value: &Availability<Decimal>,
    cash_flows: &Availability<Vec<CashFlow>>,
    end_market_value: &Availability<Decimal>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Availability<MoneyWeightedReturn> {
    let begin = match begin_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let flows = match cash_flows {
        Availability::Available(v) => v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
    };
    let end_mv = match end_market_value {
        Availability::Available(v) => *v,
        Availability::Unavailable { reasons } => {
            return Availability::Unavailable {
                reasons: reasons.clone(),
            }
        }
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

    let npv = |rate: f64| -> f64 { series.iter().map(|(t, c)| c / (1.0 + rate).powf(*t)).sum() };

    // Scan the rate range for sign-change sub-brackets instead of trusting the two
    // endpoints. Interleaved buys/sells make NPV non-monotonic: an interior root can sit
    // between two same-sign endpoints, and multiple roots can exist. We collect every
    // bracket; zero brackets = no root, more than one = ambiguous multi-IRR -> refuse.
    let mut scan: Vec<f64> = vec![-0.9999];
    let mut r = -0.99_f64;
    while r < 1.0 {
        scan.push(r);
        r += 0.01;
    }
    let mut r = 1.0_f64;
    while r < 1_000_000.0 {
        scan.push(r);
        r *= 2.0;
    }
    scan.push(1_000_000.0);

    let mut brackets: Vec<(f64, f64)> = Vec::new();
    let mut exact: Option<f64> = None;
    let mut exact_count: usize = 0;
    for w in scan.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let (flo, fhi) = (npv(lo), npv(hi));
        if flo.is_nan() || fhi.is_nan() {
            continue;
        }
        if flo == 0.0 {
            exact = Some(lo);
            exact_count += 1;
        } else if flo * fhi < 0.0 {
            brackets.push((lo, hi));
        }
    }
    let rate = if let Some(r) = exact {
        if !brackets.is_empty() || exact_count > 1 {
            // Exact root(s) plus additional crossings or multiple exact roots = ambiguous.
            return Availability::unavailable(ValuationReason::PerformanceDidNotConverge);
        }
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
    Availability::Available(MoneyWeightedReturn {
        annualized,
        cumulative,
        period_days,
    })
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

    fn buy_with_fx(
        id: i64,
        d: &str,
        qty: i64,
        price: Decimal,
        fx: Decimal,
        brokerage: Decimal,
    ) -> LedgerTransaction {
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
            quantity: -qty,
            price: Some(price),
            fx_rate_to_base: Some(fx),
            brokerage_base: Decimal::ZERO,
        }
    }

    fn avail(a: &Availability<Decimal>) -> Decimal {
        match a {
            Availability::Available(v) => *v,
            Availability::Unavailable { reasons } => {
                panic!("expected Available, got {:?}", reasons)
            }
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
        let txs = vec![buy(1, "2026-01-01", 100), split_tx(2, "2026-06-15", 100)];
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
        let txs = vec![buy(1, "2026-01-01", 100), split_tx(2, "2026-08-01", 100)];
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
        let txs = vec![buy(1, "2026-06-05", 100), split_tx(2, "2026-06-15", 100)];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.in_period_split_factor, dec!(2));
    }

    #[test]
    fn period_amounts_simple_hold_price_gain() {
        // 100 shares held through period; price 10 → 12 USD; FX constant at 10
        let txs = vec![buy_with_fx(
            1,
            "2026-01-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(12)),
            Some(dec!(10)),
            Some(dec!(10)),
            false,
        );
        assert_eq!(avail(&a.capital_gain_base), dec!(2000)); // (12-10)*100*10
        assert_eq!(avail(&a.currency_gain_base), dec!(0));
        assert_eq!(avail(&a.total_return_base), dec!(2000));
    }

    #[test]
    fn period_amounts_simple_hold_fx_gain() {
        // 100 shares; price flat at 10 USD; FX 10 → 11
        let txs = vec![buy_with_fx(
            1,
            "2026-01-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(11)),
            false,
        );
        assert_eq!(avail(&a.capital_gain_base), dec!(0));
        assert_eq!(avail(&a.currency_gain_base), dec!(1000)); // 10*100*(11-10)
        assert_eq!(avail(&a.total_return_base), dec!(1000));
    }

    #[test]
    fn period_amounts_inception_mode_no_start_price_needed() {
        // Inception: buy 100 shares at price 10, FX 10 during the period; end price 12, FX 10
        let txs = vec![buy_with_fx(
            1,
            "2026-06-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        // start_price = None (inception, start_position.quantity == 0)
        let a = compute_period_amounts(&p, None, Some(dec!(12)), None, Some(dec!(10)), false);
        assert_eq!(avail(&a.begin_market_value_base), dec!(0));
        // total_return = end_mv - begin_mv - net_flows = 12000 - 0 - 10000 = 2000
        assert_eq!(avail(&a.total_return_base), dec!(2000));
    }

    #[test]
    fn period_amounts_missing_end_price_returns_unavailable() {
        let txs = vec![buy_with_fx(
            1,
            "2026-01-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            None,
            Some(dec!(10)),
            Some(dec!(10)),
            false,
        );
        assert!(matches!(
            a.total_return_base,
            Availability::Unavailable { .. }
        ));
    }

    #[test]
    fn period_amounts_capital_plus_currency_equals_total_return() {
        // Both price and FX move; verify decomposition adds up
        let txs = vec![buy_with_fx(
            1,
            "2026-01-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(12)),
            Some(dec!(10)),
            Some(dec!(11)),
            false,
        );
        assert_eq!(
            avail(&a.capital_gain_base) + avail(&a.currency_gain_base),
            avail(&a.total_return_base)
        );
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
        let a = compute_period_amounts(&p, None, None, None, Some(dec!(10)), false);
        assert_eq!(avail(&a.begin_market_value_base), dec!(0));
        assert_eq!(avail(&a.end_market_value_base), dec!(0));
        assert_eq!(avail(&a.total_return_base), dec!(1000));
        assert_eq!(avail(&a.capital_gain_base), dec!(1000));
        assert_eq!(avail(&a.currency_gain_base), dec!(0));
    }

    #[test]
    fn period_amounts_missing_start_fx_for_non_sek_returns_unavailable() {
        // Non-SEK instrument with pre-period position; missing start_fx should be unavailable
        let txs = vec![buy_with_fx(
            1,
            "2026-01-01",
            100,
            dec!(10),
            dec!(10),
            Decimal::ZERO,
        )];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(12)),
            None,
            Some(dec!(10)),
            false,
        );
        assert!(matches!(
            a.total_return_base,
            Availability::Unavailable { .. }
        ));
    }

    // ── Modified Dietz tests ──────────────────────────────────────────────────

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
        let flows = Availability::Available(vec![CashFlow {
            date: sd,
            amount_base: dec!(10000),
        }]);
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
        let flows = Availability::Available(vec![CashFlow {
            date: mid,
            amount_base: dec!(10000),
        }]);
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

    // ── Annualisation tests ───────────────────────────────────────────────────

    #[test]
    fn annualise_over_one_year_returns_annualised() {
        // 20% over 730 days ≈ sqrt(1.2) - 1
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

    #[test]
    fn period_cash_flows_apply_post_period_split_factor() {
        // 100 shares bought at $10, FX 10; 2:1 split happens after end_date.
        // post_period_split_factor = 2.
        // The cash flow (denominator) should be 200 (split-adjusted) * 10 * 10 = 20_000,
        // matching compute_period_amounts's net_flows for this buy.
        let txs = vec![
            buy_with_fx(1, "2026-06-05", 100, dec!(10), dec!(10), Decimal::ZERO),
            split_tx(2, "2026-08-01", 100), // after end_date → post-period
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.post_period_split_factor, dec!(2));

        let flows = match period_cash_flows(&p, false) {
            Availability::Available(v) => v,
            _ => panic!("expected available flows"),
        };
        assert_eq!(flows.len(), 1);
        // 100 shares × 2 (split factor) × $10 price × 10 FX = 20_000
        assert_eq!(flows[0].amount_base, dec!(20000));

        // compute_period_amounts must produce the same net_flows for consistency.
        let amounts = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(10)),
            false,
        );
        // end_mv = 200 shares × $10 × 10 FX = 20_000; begin_mv = 0; net_flows = 20_000
        // total_return = 20_000 - 0 - 20_000 = 0
        assert_eq!(avail(&amounts.total_return_base), dec!(0));
    }

    // ── Money-weighted return (XIRR) tests ───────────────────────────────────

    #[test]
    fn money_weighted_simple_hold_matches_simple_return() {
        // Invest 100k at start, worth 120k after exactly one year, no flows.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![CashFlow {
            date: date("2025-01-01"),
            amount_base: dec!(100000), // buy cost
        }]);
        let end = Availability::Available(dec!(120000));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        let v = match r {
            Availability::Available(v) => v,
            _ => panic!("available"),
        };
        assert!(
            (v.cumulative - dec!(0.20)).abs() < dec!(0.001),
            "cumulative {}",
            v.cumulative
        );
    }

    #[test]
    fn money_weighted_is_cash_flow_neutral_for_same_day_trade() {
        // A buy today (cash out + equal market-value in) must not change the result.
        let start = date("2025-01-01");
        let end = date("2025-07-01");
        let base_flows = vec![CashFlow {
            date: start,
            amount_base: dec!(100000),
        }];
        let begin = Availability::Available(Decimal::ZERO);
        let end_mv = Availability::Available(dec!(150000));

        let r1 = compute_money_weighted_return(
            &begin,
            &Availability::Available(base_flows.clone()),
            &end_mv,
            start,
            end,
        );

        // Same trade today: +50k buy cost flow at end_date, end MV also +50k.
        let mut with_trade = base_flows.clone();
        with_trade.push(CashFlow {
            date: end,
            amount_base: dec!(50000),
        });
        let r2 = compute_money_weighted_return(
            &begin,
            &Availability::Available(with_trade),
            &Availability::Available(dec!(200000)),
            start,
            end,
        );

        let a = match r1 {
            Availability::Available(v) => v.annualized,
            _ => panic!(),
        };
        let b = match r2 {
            Availability::Available(v) => v.annualized,
            _ => panic!(),
        };
        assert!(
            (a - b).abs() < dec!(0.0001),
            "neutrality violated: {a} vs {b}"
        );
    }

    #[test]
    fn money_weighted_unavailable_when_no_sign_change() {
        // All inflows, no outflow -> no root.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![CashFlow {
            date: date("2025-01-01"),
            amount_base: dec!(-100),
        }]);
        let end = Availability::Available(dec!(100));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        assert!(matches!(r, Availability::Unavailable { .. }));
    }

    #[test]
    fn money_weighted_solves_with_interleaved_buy_and_sell() {
        // Alternating-sign flows: buy, partial sell mid-period, open remainder at end.
        // A single well-defined root must still be found (solver must not assume the NPV
        // curve is monotonic or that positive NPV always belongs to the lower bound).
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(100000),
            }, // buy cost
            CashFlow {
                date: date("2025-07-01"),
                amount_base: dec!(-60000),
            }, // sell proceeds
        ]);
        let end = Availability::Available(dec!(70000));
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        let v = match r {
            Availability::Available(v) => v,
            _ => panic!("expected a root"),
        };
        // Verify: NPV at the solved rate is negligible (< 1.0 in SEK absolute terms).
        let rate_f64 = v.annualized.to_f64().expect("finite");
        let t_sell = 181.0_f64 / 365.25;
        let t_end = 365.0_f64 / 365.25;
        let npv_residual = -100000.0_f64
            + 60000.0 / (1.0 + rate_f64).powf(t_sell)
            + 70000.0 / (1.0 + rate_f64).powf(t_end);
        assert!(
            npv_residual.abs() < 1.0,
            "NPV residual at solved rate: {npv_residual}"
        );
    }

    #[test]
    fn actual_period_cash_flows_unaffected_by_post_period_split() {
        // 100 shares bought at $10, FX 10; 2:1 split happens after end_date.
        // post_period_split_factor = 2, but actual_period_cash_flows must NOT apply it.
        // Expected: 100 × $10 × 10 = 10_000 (vs period_cash_flows which would yield 20_000).
        let txs = vec![
            buy_with_fx(1, "2026-06-05", 100, dec!(10), dec!(10), Decimal::ZERO),
            split_tx(2, "2026-08-01", 100),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        assert_eq!(p.post_period_split_factor, dec!(2));

        let actual_flows = match actual_period_cash_flows(&p, false) {
            Availability::Available(v) => v,
            _ => panic!("expected available flows"),
        };
        assert_eq!(actual_flows.len(), 1);
        assert_eq!(actual_flows[0].amount_base, dec!(10000));

        let split_adjusted_flows = match period_cash_flows(&p, false) {
            Availability::Available(v) => v,
            _ => panic!("expected available flows"),
        };
        assert_eq!(split_adjusted_flows[0].amount_base, dec!(20000));
    }

    #[test]
    fn money_weighted_unavailable_when_multiple_roots() {
        // A flow series that produces more than one sign change / IRR root must return
        // PerformanceDidNotConverge rather than silently picking one. Construct a series with
        // two interior roots (large early inflow, larger outflow, inflow again).
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(1000),
            }, // -1000 investor
            CashFlow {
                date: date("2025-06-01"),
                amount_base: dec!(-2500),
            }, // +2500 investor
        ]);
        let end = Availability::Available(dec!(-1560)); // forces a second sign change in NPV
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        // Either a single documented root or Unavailable; this case must be Unavailable.
        assert!(matches!(r, Availability::Unavailable { .. }));
    }

    #[test]
    fn money_weighted_unavailable_when_all_flows_cancel_at_every_rate() {
        // Same-day buy and sell of equal amount with zero begin/end MV:
        // investor flows are (-amount at t=0) + (+amount at t=0) = 0 for every rate.
        // NPV is identically zero across all rates — degenerate, should refuse.
        let begin = Availability::Available(Decimal::ZERO);
        let flows = Availability::Available(vec![
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(50000),
            }, // buy cost
            CashFlow {
                date: date("2025-01-01"),
                amount_base: dec!(-50000),
            }, // sell proceeds same day
        ]);
        let end = Availability::Available(Decimal::ZERO);
        let r = compute_money_weighted_return(
            &begin,
            &flows,
            &end,
            date("2025-01-01"),
            date("2026-01-01"),
        );
        assert!(
            matches!(r, Availability::Unavailable { .. }),
            "degenerate all-zero NPV must be unavailable"
        );
    }
}
