use super::availability::{
    merge_many_reasons, merge_reasons, unavailable_or_first, Availability, ValuationReason,
};
use super::market_snapshot::{data_freshness, fx_snapshot};
use super::market_snapshot::{FxCandidate, FxSnapshot, PriceCandidate, PriceSnapshot};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct DayChange {
    pub(super) previous_price: super::availability::Availability<PriceSnapshot>,
    pub(super) previous_fx: super::availability::Availability<FxSnapshot>,
    pub(super) previous_market_value_base: super::availability::Availability<Decimal>,
    pub(super) day_change_base: super::availability::Availability<Decimal>,
    pub(super) day_change_percent: super::availability::Availability<Decimal>,
}

pub(super) fn compute(
    quantity: i64,
    native_currency: &str,
    valuation_date: NaiveDate,
    latest_price: &super::availability::Availability<PriceSnapshot>,
    previous_price: Option<PriceCandidate>,
    latest_fx: &super::availability::Availability<FxSnapshot>,
    previous_fx: Option<FxCandidate>,
) -> DayChange {
    let previous_price = match previous_price {
        Some(candidate) => Availability::available(PriceSnapshot {
            date: candidate.date,
            close: candidate.close,
            currency: candidate.currency,
            source: candidate.source,
            freshness: data_freshness(valuation_date, candidate.date),
        }),
        None => Availability::unavailable(ValuationReason::MissingPreviousClose),
    };
    let previous_price = match (latest_price.as_ref(), previous_price.as_ref()) {
        (Some(latest), Some(previous)) if latest.source != previous.source => {
            Availability::unavailable(ValuationReason::PreviousCloseSourceMismatch)
        }
        _ => previous_price,
    };

    let previous_fx = fx_snapshot(native_currency, valuation_date, previous_fx, true);

    let previous_market_value_base = match (previous_price.as_ref(), previous_fx.as_ref()) {
        (Some(price), Some(fx)) => {
            Availability::available(price.close * fx.rate * Decimal::from(quantity))
        }
        _ => {
            let mut missing = Vec::new();
            missing.extend(previous_price.reasons());
            missing.extend(previous_fx.reasons());
            unavailable_or_first(missing, ValuationReason::MissingPreviousClose)
        }
    };

    let day_change_base = match (
        latest_price.as_ref(),
        previous_price.as_ref(),
        latest_fx.as_ref(),
        previous_fx.as_ref(),
    ) {
        (Some(latest), Some(previous), Some(latest_fx), Some(previous_fx)) => {
            let latest_value = latest.close * latest_fx.rate;
            let previous_value = previous.close * previous_fx.rate;
            Availability::available((latest_value - previous_value) * Decimal::from(quantity))
        }
        _ => Availability::Unavailable {
            reasons: merge_many_reasons(
                &[
                    latest_price.reasons(),
                    previous_price.reasons(),
                    latest_fx.reasons(),
                    previous_fx.reasons(),
                ],
                ValuationReason::MissingPreviousClose,
            ),
        },
    };

    let day_change_percent = match (
        day_change_base.as_ref(),
        previous_market_value_base.as_ref(),
    ) {
        (Some(day_change), Some(previous_market_value))
            if *previous_market_value != Decimal::ZERO =>
        {
            Availability::available((*day_change / *previous_market_value) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroPreviousMarketValue),
        _ => Availability::Unavailable {
            reasons: merge_reasons(
                &day_change_base.reasons(),
                &previous_market_value_base.reasons(),
                ValuationReason::MissingPreviousClose,
            ),
        },
    };

    DayChange {
        previous_price,
        previous_fx,
        previous_market_value_base,
        day_change_base,
        day_change_percent,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::super::valuation::{FxCandidate, PriceCandidate};
    use super::super::{
        derive_position, LedgerTransaction, Position, ProviderCode, TransactionKind,
    };
    use chrono::NaiveDate;
    use rust_decimal::Decimal;

    pub(crate) fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    pub(crate) fn buy(
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

    pub(crate) fn position(transactions: &[LedgerTransaction]) -> Position {
        derive_position(transactions).expect("derives")
    }

    pub(crate) fn price(date: NaiveDate, close: Decimal, currency: &str) -> PriceCandidate {
        PriceCandidate {
            date,
            close,
            currency: currency.to_owned(),
            source: ProviderCode::new("PRIMARY"),
        }
    }

    pub(crate) fn fx(date: NaiveDate, rate: Decimal, base: &str, quote: &str) -> FxCandidate {
        FxCandidate {
            date,
            rate,
            base: base.to_owned(),
            quote: quote.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{buy, d, fx, position, price};
    use crate::domain::valuation::value_position;
    use crate::domain::{Availability, ProviderCode, ValuationReason};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[test]
    fn missing_previous_close_blocks_day_change() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            None,
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(
            value.day_change_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPreviousClose]
            }
        );
        assert!(value
            .reasons
            .contains(&ValuationReason::MissingPreviousClose));
    }

    #[test]
    fn mixed_price_sources_keep_market_value_and_block_day_change() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);
        let latest = price(d(2026, 6, 16), dec!(110), "USD");
        let mut previous = price(d(2026, 6, 15), dec!(100), "USD");
        previous.source = ProviderCode::new("NASDAQ_NORDIC");

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(latest),
            Some(previous),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 15), dec!(10), "USD", "SEK")),
        );

        assert!(value.market_value_base.as_ref().is_some());
        assert_eq!(
            value.day_change_base,
            Availability::unavailable(ValuationReason::PreviousCloseSourceMismatch)
        );
    }

    #[test]
    fn same_price_source_keeps_day_change_available() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 15), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 15), dec!(10), "USD", "SEK")),
        );

        assert_eq!(value.day_change_base, Availability::available(dec!(500)));
        assert!(!value
            .reasons
            .contains(&ValuationReason::PreviousCloseSourceMismatch));
    }

    #[test]
    fn missing_previous_fx_is_preserved_as_day_change_reason() {
        let pos = position(&[buy(1, d(2026, 6, 1), 5, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            None,
        );

        assert_eq!(
            value.day_change_base,
            Availability::Unavailable {
                reasons: vec![ValuationReason::MissingPreviousFx]
            }
        );
        assert!(value.reasons.contains(&ValuationReason::MissingPreviousFx));
    }

    #[test]
    fn day_change_percent_uses_previous_market_value_denominator() {
        let pos = position(&[buy(1, d(2026, 6, 1), 2, dec!(100), Some(dec!(10)), "USD")]);

        let value = value_position(
            &pos,
            "USD",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(110), "USD")),
            Some(price(d(2026, 6, 13), dec!(100), "USD")),
            Some(fx(d(2026, 6, 16), dec!(10), "USD", "SEK")),
            Some(fx(d(2026, 6, 13), dec!(10), "USD", "SEK")),
        );

        assert_eq!(value.day_change_base, Availability::available(dec!(200)));
        assert_eq!(
            value.day_change_percent,
            Availability::available(dec!(10.0))
        );
    }

    #[test]
    fn zero_previous_market_value_makes_day_change_percent_unavailable() {
        let pos = position(&[buy(1, d(2026, 6, 1), 1, dec!(10), Some(dec!(1)), "SEK")]);

        let value = value_position(
            &pos,
            "SEK",
            d(2026, 6, 16),
            Some(price(d(2026, 6, 16), dec!(10), "SEK")),
            Some(price(d(2026, 6, 13), Decimal::ZERO, "SEK")),
            None,
            None,
        );

        assert_eq!(value.day_change_base, Availability::available(dec!(10)));
        assert_eq!(
            value.day_change_percent,
            Availability::Unavailable {
                reasons: vec![ValuationReason::ZeroPreviousMarketValue]
            }
        );
    }
}
