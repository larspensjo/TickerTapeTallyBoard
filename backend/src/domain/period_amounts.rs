use rust_decimal::Decimal;

use super::performance::PeriodLedger;
use super::transaction::TransactionKind;
use super::valuation::{Availability, ValuationReason};

#[derive(Debug, Clone)]
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
            income_base: Availability::Unavailable {
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
    let mut income_total = Decimal::ZERO;

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
            TransactionKind::Dividend => {
                let p = match tx.dividend_per_share {
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
                let income = Decimal::from(tx.quantity) * p * f;
                income_total += income;
                // Dividend reduces net_flows (investor received cash), increasing total_return.
                // capital_flows_at_end_fx is deliberately excluded so capital_gain stays
                // a pure price-movement metric at constant FX.
                net_flows -= income;
            }
            TransactionKind::Split => {}
        }
    }

    let total_return = end_mv - begin_mv - net_flows;
    let capital_gain =
        (adj_end_qty * end_price - adj_start_qty * start_price) * end_fx - capital_flows_at_end_fx;
    let income = income_total;
    let currency_gain = total_return - capital_gain - income;

    PeriodAmounts {
        begin_market_value_base: Availability::Available(begin_mv),
        end_market_value_base: Availability::Available(end_mv),
        capital_gain_base: Availability::Available(capital_gain),
        currency_gain_base: Availability::Available(currency_gain),
        income_base: Availability::Available(income),
        total_return_base: Availability::Available(total_return),
    }
}

#[cfg(test)]
mod tests {
    use super::super::cash_flow::test_support::{buy_with_fx, date, dividend_tx};
    use super::super::performance::reconstruct_period;
    use super::super::transaction::LedgerTransaction;
    use super::*;
    use rust_decimal_macros::dec;

    fn sell_with_fx(id: i64, d: &str, qty: i64, price: Decimal, fx: Decimal) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date(d),
            kind: TransactionKind::Sell,
            quantity: -qty,
            price: Some(price),
            dividend_per_share: None,
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

    #[test]
    fn period_amounts_dividend_adds_to_total_return_and_income() {
        // 100 shares held from before start; price flat at 10 USD / FX 10;
        // dividend $0.25/share on Jun 15 at FX 10.
        let txs = vec![
            buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO),
            dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        // price flat, FX flat -> capital_gain = 0, currency_gain = 0
        // income = 100 * 0.25 * 10 = 250; total_return = 250
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(10)),
            Some(dec!(10)),
            false,
        );
        assert_eq!(avail(&a.income_base), dec!(250));
        assert_eq!(avail(&a.total_return_base), dec!(250));
        assert_eq!(avail(&a.capital_gain_base), dec!(0));
        assert_eq!(avail(&a.currency_gain_base), dec!(0));
    }

    #[test]
    fn period_amounts_dividend_components_sum_to_total_return() {
        // 100 shares; price 10->12 USD; FX 10->11; dividend $0.25/share at FX 10
        let txs = vec![
            buy_with_fx(1, "2026-01-01", 100, dec!(10), dec!(10), Decimal::ZERO),
            dividend_tx(2, "2026-06-15", 100, dec!(0.25), Some(dec!(10))),
        ];
        let p = reconstruct_period(&txs, date("2026-06-01"), date("2026-06-30")).unwrap();
        let a = compute_period_amounts(
            &p,
            Some(dec!(10)),
            Some(dec!(12)),
            Some(dec!(10)),
            Some(dec!(11)),
            false,
        );
        // capital: (12-10)*100*11 = 2200; currency: 10*100*(11-10)=1000; income: 100*0.25*10=250
        // total = 2200+1000+250 = 3450
        let capital = avail(&a.capital_gain_base);
        let currency = avail(&a.currency_gain_base);
        let income = avail(&a.income_base);
        let total = avail(&a.total_return_base);
        assert_eq!(capital, dec!(2200));
        assert_eq!(currency, dec!(1000));
        assert_eq!(income, dec!(250));
        assert_eq!(capital + currency + income, total);
    }
}
