use chrono::NaiveDate;
use rust_decimal::Decimal;

use super::performance::PeriodLedger;
use super::transaction::TransactionKind;
use super::valuation::{Availability, ValuationReason};

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
            TransactionKind::Dividend => {
                let p = match tx.dividend_per_share {
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
                // Dividend is cash the investor receives: negative flow (out of the investment).
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: -income,
                });
            }
            TransactionKind::Split => {}
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
            TransactionKind::Dividend => {
                let p = match tx.dividend_per_share {
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
                // Dividend is cash the investor receives: negative flow (out of the investment).
                flows.push(CashFlow {
                    date: tx.trade_date,
                    amount_base: -income,
                });
            }
            TransactionKind::Split => {}
        }
    }
    Availability::Available(flows)
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::super::transaction::LedgerTransaction;
    use super::*;

    pub(crate) fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    pub(crate) fn split_tx(id: i64, d: &str, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    pub(crate) fn buy_with_fx(
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
            dividend_per_share: None,
            fx_rate_to_base: Some(fx),
            brokerage_base: brokerage,
        }
    }

    pub(crate) fn dividend_tx(
        id: i64,
        d: &str,
        qty: i64,
        price: Decimal,
        fx: Option<Decimal>,
    ) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Dividend,
            quantity: qty,
            price: None,
            dividend_per_share: Some(price),
            fx_rate_to_base: fx,
            brokerage_base: Decimal::ZERO,
        }
    }

    pub(crate) fn avail(a: &Availability<Decimal>) -> Decimal {
        match a {
            Availability::Available(v) => *v,
            Availability::Unavailable { reasons } => {
                panic!("expected Available, got {:?}", reasons)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::performance::{compute_period_amounts, reconstruct_period};
    use super::test_support::{avail, buy_with_fx, date, dividend_tx, split_tx};
    use super::*;
    use rust_decimal_macros::dec;

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
    fn period_cash_flows_include_dividend_as_negative_flow() {
        // Dividend = cash out of investment -> negative amount_base
        let txs = vec![
            buy_with_fx(1, "2026-06-05", 100, dec!(10), dec!(10), Decimal::ZERO),
            dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let flows = match period_cash_flows(&p, false) {
            Availability::Available(v) => v,
            _ => panic!("expected available"),
        };
        // buy: +10_000 (post-period split factor = 1); dividend: -250
        assert_eq!(flows.len(), 2);
        let buy_flow = flows
            .iter()
            .find(|f| f.amount_base > Decimal::ZERO)
            .expect("buy flow");
        let div_flow = flows
            .iter()
            .find(|f| f.amount_base < Decimal::ZERO)
            .expect("dividend flow");
        assert_eq!(buy_flow.amount_base, dec!(10000));
        assert_eq!(div_flow.amount_base, dec!(-250));
    }
}
