use super::availability::{Availability, ValuationReason};
use super::valuation::ValuedHolding;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub struct ValuationSummary {
    pub market_value_base: Availability<Decimal>,
    pub cost_basis_base: Availability<Decimal>,
    pub price_effect_base: Availability<Decimal>,
    pub fx_effect_base: Availability<Decimal>,
    pub unrealized_gain_base: Availability<Decimal>,
    pub unrealized_gain_percent: Availability<Decimal>,
    pub day_change_base: Availability<Decimal>,
    pub day_change_percent: Availability<Decimal>,
    pub excluded_rows: usize,
}

pub fn summarize_holdings(rows: &[ValuedHolding]) -> ValuationSummary {
    let mut market_value_base = Decimal::ZERO;
    let mut cost_basis_base = Decimal::ZERO;
    let mut price_effect_base = Decimal::ZERO;
    let mut fx_effect_base = Decimal::ZERO;
    let mut day_change_base = Decimal::ZERO;
    let mut previous_market_value_base = Decimal::ZERO;
    let mut included_rows = 0usize;
    let mut effect_rows = 0usize;
    let mut day_change_rows = 0usize;
    let mut excluded_rows = 0usize;

    for row in rows {
        match (row.market_value_base.as_ref(), row.cost_basis_base.as_ref()) {
            (Some(market_value), Some(cost_basis)) => {
                included_rows += 1;
                market_value_base += *market_value;
                cost_basis_base += *cost_basis;
                if let (Some(price_effect), Some(fx_effect)) =
                    (row.price_effect_base.as_ref(), row.fx_effect_base.as_ref())
                {
                    price_effect_base += *price_effect;
                    fx_effect_base += *fx_effect;
                    effect_rows += 1;
                }
                if let Some(day_change) = row.day_change_base.as_ref() {
                    day_change_base += *day_change;
                    previous_market_value_base += *market_value - *day_change;
                    day_change_rows += 1;
                }
            }
            _ => excluded_rows += 1,
        }
    }

    let market_value_base = if included_rows > 0 {
        Availability::available(market_value_base)
    } else {
        Availability::unavailable_empty()
    };
    let cost_basis_base = if included_rows > 0 {
        Availability::available(cost_basis_base)
    } else {
        Availability::unavailable_empty()
    };
    let price_effect_base = if effect_rows > 0 {
        Availability::available(price_effect_base)
    } else {
        Availability::unavailable_empty()
    };
    let fx_effect_base = if effect_rows > 0 {
        Availability::available(fx_effect_base)
    } else {
        Availability::unavailable_empty()
    };
    let unrealized_gain_base = match (market_value_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(market_value), Some(cost_basis)) => {
            Availability::available(*market_value - *cost_basis)
        }
        _ => Availability::unavailable_empty(),
    };
    let unrealized_gain_percent = match (unrealized_gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::unavailable_empty(),
    };
    let day_change_base = if day_change_rows > 0 {
        Availability::available(day_change_base)
    } else {
        Availability::unavailable_empty()
    };
    let day_change_percent = match day_change_base.as_ref() {
        Some(day_change) if previous_market_value_base != Decimal::ZERO => {
            Availability::available((*day_change / previous_market_value_base) * Decimal::from(100))
        }
        Some(_) => Availability::unavailable(ValuationReason::ZeroPreviousMarketValue),
        None => Availability::unavailable_empty(),
    };

    ValuationSummary {
        market_value_base,
        cost_basis_base,
        price_effect_base,
        fx_effect_base,
        unrealized_gain_base,
        unrealized_gain_percent,
        day_change_base,
        day_change_percent,
        excluded_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::{summarize_holdings, Availability, ValuationReason};
    use crate::domain::{
        derive_position, value_position, FxCandidate, PriceCandidate, ProviderCode,
    };
    use crate::domain::{LedgerTransaction, TransactionKind};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    fn buy(
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: Decimal,
        fx: Option<Decimal>,
        _currency: &str,
    ) -> LedgerTransaction {
        LedgerTransaction {
            id,
            trade_date: date,
            kind: TransactionKind::Buy,
            quantity: qty,
            price: Some(price),
            dividend_per_share: None,
            fx_rate_to_base: fx,
            brokerage_base: Decimal::ZERO,
        }
    }

    fn position(transactions: &[LedgerTransaction]) -> crate::domain::Position {
        derive_position(transactions).expect("derives")
    }

    fn price(date: NaiveDate, close: Decimal, currency: &str) -> PriceCandidate {
        PriceCandidate {
            date,
            close,
            currency: currency.to_owned(),
            source: ProviderCode::new("PRIMARY"),
        }
    }

    fn fx(date: NaiveDate, rate: Decimal, base: &str, quote: &str) -> FxCandidate {
        FxCandidate {
            date,
            rate,
            base: base.to_owned(),
            quote: quote.to_owned(),
        }
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

        let summary = summarize_holdings(&[usd, sek]);

        assert_eq!(
            summary.market_value_base,
            Availability::available(dec!(12250))
        );
        assert_eq!(
            summary.cost_basis_base,
            Availability::available(dec!(10100))
        );
        assert_eq!(
            summary.price_effect_base,
            Availability::available(dec!(1150))
        );
        assert_eq!(summary.fx_effect_base, Availability::available(dec!(1000)));
        assert_eq!(
            summary.unrealized_gain_base,
            Availability::available(dec!(2150))
        );
        assert_eq!(
            *summary.price_effect_base.as_ref().expect("price effect")
                + *summary.fx_effect_base.as_ref().expect("fx effect"),
            *summary.unrealized_gain_base.as_ref().expect("gain")
        );
    }

    #[test]
    fn zero_previous_market_value_makes_day_change_percent_unavailable() {
        let value = value_position(
            &position(&[buy(1, d(2026, 6, 1), 1, dec!(10), Some(dec!(1)), "SEK")]),
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(10), "SEK")),
            Some(price(d(2026, 6, 13), Decimal::ZERO, "SEK")),
            None,
            None,
        );

        let summary = summarize_holdings(&[value]);
        assert_eq!(
            summary.day_change_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroPreviousMarketValue]
            }
        );
    }

    #[test]
    fn summary_excludes_incomplete_rows() {
        let included = value_position(
            &position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );
        let excluded = value_position(
            &position(&[buy(2, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]),
            "USD",
            d(2026, 6, 16),
            None,
            None,
            None,
            None,
        );

        let summary = summarize_holdings(&[included, excluded]);

        assert_eq!(summary.excluded_rows, 1);
        assert_eq!(
            summary.market_value_base,
            Availability::available(dec!(5500))
        );
        assert_eq!(summary.cost_basis_base, Availability::available(dec!(5000)));
        assert_eq!(
            summary.unrealized_gain_base,
            Availability::available(dec!(500))
        );
        assert_eq!(
            summary.price_effect_base,
            Availability::available(dec!(500))
        );
        assert_eq!(summary.fx_effect_base, Availability::available(dec!(0)));
    }
}
