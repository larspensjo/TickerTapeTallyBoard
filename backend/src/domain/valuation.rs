use super::attribution::{compute as compute_attribution, Attribution};
use super::availability::{append_amount_reasons, dedup_reasons, merge_reasons};
pub use super::availability::{Availability, ValuationReason};
use super::day_change::compute as compute_day_change;
use super::market_snapshot::{
    append_fx_reasons, append_snapshot_reasons, data_freshness, fx_snapshot,
};
pub use super::market_snapshot::{
    DataFreshness, FxCandidate, FxSnapshot, PriceCandidate, PriceSnapshot,
};
use super::Position;
use chrono::NaiveDate;
use rust_decimal::Decimal;

pub use super::price_history::{build_price_history, FxApplied, PricePoint};
pub use super::valuation_summary::{summarize_holdings, ValuationSummary};

#[derive(Clone, Debug, PartialEq)]
pub struct ValuedHolding {
    pub quantity: i64,
    pub cost_basis_native: Decimal,
    pub cost_basis_base: Availability<Decimal>,
    pub fee_component_base: Availability<Decimal>,
    pub price_effect_base: Availability<Decimal>,
    pub fx_effect_base: Availability<Decimal>,
    pub latest_price: Availability<PriceSnapshot>,
    pub previous_price: Availability<PriceSnapshot>,
    pub latest_fx: Availability<FxSnapshot>,
    pub previous_fx: Availability<FxSnapshot>,
    pub market_value_native: Availability<Decimal>,
    pub market_value_base: Availability<Decimal>,
    pub unrealized_gain_base: Availability<Decimal>,
    pub unrealized_gain_percent: Availability<Decimal>,
    pub day_change_base: Availability<Decimal>,
    pub day_change_percent: Availability<Decimal>,
    pub reasons: Vec<ValuationReason>,
}

pub fn value_position(
    position: &Position,
    native_currency: &str,
    valuation_date: NaiveDate,
    latest_price: Option<PriceCandidate>,
    previous_price: Option<PriceCandidate>,
    latest_fx: Option<FxCandidate>,
    previous_fx: Option<FxCandidate>,
) -> ValuedHolding {
    debug_assert!(
        position.quantity >= 0,
        "valuation expects a derived position"
    );

    let latest_price = match latest_price {
        Some(candidate) => {
            let freshness = data_freshness(valuation_date, candidate.date);
            let snapshot = PriceSnapshot {
                date: candidate.date,
                close: candidate.close,
                currency: candidate.currency,
                source: candidate.source,
                freshness,
            };
            Availability::available(snapshot)
        }
        None => Availability::unavailable(ValuationReason::MissingPrice),
    };

    let latest_fx = fx_snapshot(native_currency, valuation_date, latest_fx, false);
    let super::day_change::DayChange {
        previous_price,
        previous_fx,
        previous_market_value_base,
        day_change_base,
        day_change_percent,
    } = compute_day_change(
        position.quantity,
        native_currency,
        valuation_date,
        &latest_price,
        previous_price,
        &latest_fx,
        previous_fx,
    );

    let market_value_native = match latest_price.as_ref() {
        Some(price) => Availability::available(price.close * Decimal::from(position.quantity)),
        None => Availability::unavailable(ValuationReason::MissingPrice),
    };

    let Attribution {
        cost_basis_base,
        fee_component_base,
        price_effect_base,
        fx_effect_base,
    } = compute_attribution(position, &market_value_native, &latest_fx);

    let market_value_base = match (market_value_native.as_ref(), latest_fx.as_ref()) {
        (Some(native_value), Some(fx)) => Availability::available(*native_value * fx.rate),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &market_value_native.reasons(),
                &latest_fx.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let unrealized_gain_base = match (market_value_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(market_value), Some(cost_basis)) => {
            Availability::available(*market_value - *cost_basis)
        }
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &market_value_base.reasons(),
                &cost_basis_base.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let unrealized_gain_percent = match (unrealized_gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &unrealized_gain_base.reasons(),
                &cost_basis_base.reasons(),
                ValuationReason::MissingPrice,
            ),
        },
    };

    let mut reasons = Vec::new();
    append_snapshot_reasons(&mut reasons, &latest_price);
    append_fx_reasons(&mut reasons, &latest_fx);
    append_amount_reasons(&mut reasons, &market_value_native);
    append_amount_reasons(&mut reasons, &market_value_base);
    append_amount_reasons(&mut reasons, &previous_market_value_base);
    append_amount_reasons(&mut reasons, &price_effect_base);
    append_amount_reasons(&mut reasons, &fx_effect_base);
    append_amount_reasons(&mut reasons, &unrealized_gain_base);
    append_amount_reasons(&mut reasons, &unrealized_gain_percent);
    append_amount_reasons(&mut reasons, &day_change_base);
    append_amount_reasons(&mut reasons, &day_change_percent);
    append_amount_reasons(&mut reasons, &cost_basis_base);
    dedup_reasons(&mut reasons);

    ValuedHolding {
        quantity: position.quantity,
        cost_basis_native: position.cost_basis_native,
        cost_basis_base,
        fee_component_base,
        price_effect_base,
        fx_effect_base,
        latest_price,
        previous_price,
        latest_fx,
        previous_fx,
        market_value_native,
        market_value_base,
        unrealized_gain_base,
        unrealized_gain_percent,
        day_change_base,
        day_change_percent,
        reasons,
    }
}

#[cfg(test)]
mod tests {
    use super::super::day_change::test_support::{buy, d, fx, position, price};
    use super::{value_position, Availability, DataFreshness, ValuationReason};
    use crate::domain::{LedgerTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn split(id: i64, date: NaiveDate, delta: i64) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Split,
            quantity: delta,
            price: None,
            dividend_per_share: None,
            fx_rate_to_base: None,
            brokerage_base: Decimal::ZERO,
        }
    }

    #[test]
    fn usd_market_value_uses_fx_and_previous_points() {
        let pos = position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10.5), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10.0), "USD", "SEK")),
        );

        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(1100))
        );
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(11550))
        );
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(1550))
        );
        assert_eq!(value.price_effect_base, Availability::available(dec!(1050)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(500)));
        assert_eq!(
            value
                .unrealized_gain_percent
                .as_ref()
                .expect("gain percent")
                .round_dp(2),
            dec!(15.50)
        );
        assert_eq!(value.day_change_base, Availability::available(dec!(1550)));
        assert_eq!(
            value
                .day_change_percent
                .as_ref()
                .expect("day change")
                .round_dp(2),
            dec!(15.50)
        );
    }

    #[test]
    fn older_previous_points_do_not_emit_stale_reasons() {
        let pos = position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 10), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 10), dec!(9), "USD", "SEK")),
        );

        assert!(!value
            .reasons
            .iter()
            .any(|reason| matches!(reason, ValuationReason::StalePrice { .. })));
        assert!(!value
            .reasons
            .iter()
            .any(|reason| matches!(reason, ValuationReason::StaleFx { .. })));
    }

    #[test]
    fn eur_holdings_mark_stale_prices_but_keep_values_visible() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(600), Some(dec!(11)), "EUR")]);

        let value = value_position(
            &pos,
            "EUR",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 10), dec!(650), "EUR")),
            Some(price(d(2026, 6, 9), dec!(640), "EUR")),
            Some(fx(d(2026, 6, 10), dec!(11.2), "EUR", "SEK")),
            Some(fx(d(2026, 6, 9), dec!(11.1), "EUR", "SEK")),
        );

        match value.latest_price {
            Availability::Available(snapshot) => {
                assert_eq!(
                    snapshot.freshness,
                    DataFreshness::WarningStale { trading_days: 4 }
                );
            }
            Availability::Unavailable { .. } => panic!("latest price should be available"),
        }
        assert!(value
            .reasons
            .contains(&ValuationReason::StalePrice { trading_days: 4 }));
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(36400))
        );
    }

    #[test]
    fn sek_positions_use_identity_fx() {
        let pos = position(&[buy(1, d(2026, 6, 1), 4, dec!(20), Some(dec!(1)), "SEK")]);

        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(21), "SEK")),
            Some(price(d(2026, 6, 13), dec!(19), "SEK")),
            None,
            None,
        );

        assert_eq!(value.market_value_base, Availability::available(dec!(84)));
        assert_eq!(value.day_change_base, Availability::available(dec!(8)));
        assert_eq!(value.price_effect_base, Availability::available(dec!(4)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(0)));
        match value.latest_fx {
            Availability::Available(snapshot) => {
                assert_eq!(snapshot.rate, Decimal::ONE);
                assert_eq!(snapshot.base, "SEK");
                assert_eq!(snapshot.quote, "SEK");
            }
            Availability::Unavailable { .. } => panic!("identity fx should be available"),
        }
        match value.previous_fx {
            Availability::Available(snapshot) => {
                assert_eq!(snapshot.rate, Decimal::ONE);
            }
            Availability::Unavailable { .. } => panic!("identity fx should be available"),
        }
    }

    #[test]
    fn missing_price_keeps_cost_basis_but_blocks_market_value() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(&pos, "USD", d(2026, 6, 16), None, None, None, None);

        assert_eq!(
            value.market_value_native,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPrice]
            }
        );
        assert_eq!(
            value.market_value_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPrice, ValuationReason::MissingFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingPrice));
        assert_eq!(value.cost_basis_base, Availability::available(dec!(5000)));
    }

    #[test]
    fn missing_fx_blocks_base_value_for_non_sek_positions() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            None,
            None,
        );

        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(550))
        );
        assert_eq!(
            value.market_value_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingFx));
    }

    #[test]
    fn split_adjusted_quantity_combines_with_current_price() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(120), Some(dec!(1)), "USD"),
            split(2, d(2026, 6, 2), 10),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(60), "USD")),
            Some(price(d(2026, 6, 13), dec!(55), "USD")),
            Some(fx(d(2026, 6, 16), dec!(1), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(1), "USD", "SEK")),
        );

        assert_eq!(value.quantity, 20);
        assert_eq!(value.cost_basis_native, dec!(1200));
        assert_eq!(
            value.market_value_native,
            Availability::available(dec!(1200))
        );
    }
}
