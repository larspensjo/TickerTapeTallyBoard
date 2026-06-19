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

    // end_price always required for simplicity.
    let end_price = match end_price_native {
        Some(p) => p,
        None => {
            return PeriodAmounts::unavailable(vec![ValuationReason::MissingEndPrice]);
        }
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
}
