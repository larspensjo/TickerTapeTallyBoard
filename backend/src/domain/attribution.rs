use super::availability::{dedup_reasons, Availability, ValuationReason};
use super::market_snapshot::FxSnapshot;
use super::{BaseCostBasis, Position};
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Attribution {
    pub(super) cost_basis_base: Availability<Decimal>,
    pub(super) fee_component_base: Availability<Decimal>,
    pub(super) price_effect_base: Availability<Decimal>,
    pub(super) fx_effect_base: Availability<Decimal>,
}

pub(super) fn compute(
    position: &Position,
    market_value_native: &Availability<Decimal>,
    latest_fx: &Availability<FxSnapshot>,
) -> Attribution {
    let cost_basis_base = match &position.base {
        BaseCostBasis::Available {
            cost_basis_base, ..
        } => Availability::available(*cost_basis_base),
        BaseCostBasis::Unavailable { reasons } => Availability::Unavailable {
            reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                reasons: reasons.clone(),
            }],
        },
    };

    let fee_component_base = match &position.base {
        BaseCostBasis::Available {
            fee_component_base, ..
        } => Availability::available(*fee_component_base),
        BaseCostBasis::Unavailable { reasons } => Availability::Unavailable {
            reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                reasons: reasons.clone(),
            }],
        },
    };

    let effect_reasons = effect_reasons(
        market_value_native,
        &cost_basis_base,
        latest_fx,
        position.cost_basis_native,
    );

    let price_effect_base = match (
        market_value_native.as_ref(),
        cost_basis_base.as_ref(),
        latest_fx.as_ref(),
        fee_component_base.as_ref(),
    ) {
        (Some(native_value), Some(_), Some(fx), Some(fee_component_base))
            if position.cost_basis_native != Decimal::ZERO =>
        {
            Availability::available(
                (*native_value - position.cost_basis_native) * fx.rate - fee_component_base,
            )
        }
        _ => Availability::Unavailable {
            reasons: effect_reasons.clone(),
        },
    };

    let fx_effect_base = match (
        market_value_native.as_ref(),
        cost_basis_base.as_ref(),
        latest_fx.as_ref(),
        fee_component_base.as_ref(),
    ) {
        (Some(_native_value), Some(cost_basis_base), Some(fx), Some(fee_component_base))
            if position.cost_basis_native != Decimal::ZERO =>
        {
            let gross_base = *cost_basis_base - fee_component_base;
            Availability::available(position.cost_basis_native * fx.rate - gross_base)
        }
        _ => Availability::Unavailable {
            reasons: effect_reasons,
        },
    };

    Attribution {
        cost_basis_base,
        fee_component_base,
        price_effect_base,
        fx_effect_base,
    }
}

fn effect_reasons(
    market_value_native: &Availability<Decimal>,
    cost_basis_base: &Availability<Decimal>,
    latest_fx: &Availability<FxSnapshot>,
    cost_basis_native: Decimal,
) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    reasons.extend(market_value_native.reasons());
    reasons.extend(cost_basis_base.reasons());
    reasons.extend(latest_fx.reasons());
    if cost_basis_native == Decimal::ZERO {
        reasons.push(ValuationReason::ZeroCostBasis);
    }
    dedup_reasons(&mut reasons);
    reasons
}

#[cfg(test)]
mod tests {
    use super::super::day_change::test_support::{buy, d, fx, position, price};
    use super::super::valuation::{value_position, Availability, ValuationReason};
    use crate::domain::{BaseCostBasis, Position, UnavailableReason};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[test]
    fn base_cost_unavailability_is_propagated() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD"),
            buy(2, d(2026, 6, 2), 10, dec!(200), None, "USD"),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(
            value.cost_basis_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
        assert!(matches!(
            value.unrealized_gain_base,
            Availability::Unavailable { .. }
        ));
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::BaseCostBasisUnavailable {
                    reasons: vec![UnavailableReason::MissingFx { transaction_id: 2 }],
                }]
            }
        );
    }

    #[test]
    fn zero_cost_basis_makes_gain_percent_unavailable() {
        let pos = Position {
            quantity: 1,
            cost_basis_native: Decimal::ZERO,
            base: BaseCostBasis::Available {
                cost_basis_base: Decimal::ZERO,
                fee_component_base: Decimal::ZERO,
            },
        };
        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(10), "SEK")),
            Some(price(d(2026, 6, 13), dec!(9), "SEK")),
            None,
            None,
        );

        assert_eq!(
            value.unrealized_gain_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
        assert_eq!(
            value.price_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
        assert_eq!(
            value.fx_effect_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroCostBasis]
            }
        );
    }

    #[test]
    fn brokerage_lands_in_price_effect_when_price_and_fx_are_unchanged() {
        let mut tx = buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD");
        tx.brokerage_base = dec!(25);
        let pos = position(&[tx]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(100), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(value.cost_basis_base, Availability::available(dec!(10025)));
        assert_eq!(value.price_effect_base, Availability::available(dec!(-25)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(0)));
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(-25))
        );
    }

    #[test]
    fn multi_lot_buys_at_different_fx_rates_split_gain_across_price_and_fx() {
        let pos = position(&[
            buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD"),
            buy(2, d(2026, 6, 2), 10, dec!(100), Some(dec!(12)), "USD"),
        ]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(120), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(13), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(12), "USD", "SEK")),
        );

        assert_eq!(value.cost_basis_base, Availability::available(dec!(22000)));
        assert_eq!(
            value.market_value_base,
            Availability::available(dec!(31200))
        );
        assert_eq!(
            value.unrealized_gain_base,
            Availability::available(dec!(9200))
        );
        assert_eq!(value.price_effect_base, Availability::available(dec!(5200)));
        assert_eq!(value.fx_effect_base, Availability::available(dec!(4000)));
    }

    #[test]
    fn summary_attribution_sums_match_row_effects_and_total_gain() {
        let usd = value_position(
            &position(&[buy(1, d(2026, 6, 1), 10, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(11), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );
        let sek = value_position(
            &position(&[buy(2, d(2026, 6, 1), 5, dec!(20), Some(dec!(1)), "SEK")]),
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(30), "SEK")),
            Some(price(d(2026, 6, 13), dec!(25), "SEK")),
            None,
            None,
        );

        assert_eq!(
            *usd.price_effect_base.as_ref().expect("usd price effect")
                + *usd.fx_effect_base.as_ref().expect("usd fx effect"),
            *usd.unrealized_gain_base.as_ref().expect("usd gain")
        );
        assert_eq!(
            *sek.price_effect_base.as_ref().expect("sek price effect")
                + *sek.fx_effect_base.as_ref().expect("sek fx effect"),
            *sek.unrealized_gain_base.as_ref().expect("sek gain")
        );
    }
}
