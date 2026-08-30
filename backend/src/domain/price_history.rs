use super::availability::{Availability, ValuationReason};
use super::valuation::{FxCandidate, PriceCandidate};
use super::ProviderCode;
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Clone, Debug, PartialEq)]
pub struct FxApplied {
    pub rate: Decimal,
    pub date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PricePoint {
    pub date: NaiveDate,
    pub close: Decimal,
    pub source: ProviderCode,
    pub close_base: Availability<Decimal>,
    pub fx: Option<FxApplied>,
}

/// Build a per-instrument daily series converted to SEK.
///
/// `prices` and `fx_rates` must both be sorted by date ascending. A single
/// forward pass advances an FX index while `fx.date <= point.date`, retaining
/// the last seen rate as the carry-forward value. Rows whose currency differs
/// from `native_currency` are dropped (internal data error). SEK instruments
/// take the identity path with `fx` omitted.
pub fn build_price_history(
    native_currency: &str,
    prices: &[PriceCandidate],
    fx_rates: &[FxCandidate],
) -> Vec<PricePoint> {
    let is_base = native_currency.eq_ignore_ascii_case("SEK");
    let mut points = Vec::new();
    let mut fx_idx = 0usize;
    let mut current_fx: Option<&FxCandidate> = None;

    for price in prices {
        if !price.currency.eq_ignore_ascii_case(native_currency) {
            continue;
        }

        if is_base {
            points.push(PricePoint {
                date: price.date,
                close: price.close,
                source: price.source.clone(),
                close_base: Availability::available(price.close),
                fx: None,
            });
            continue;
        }

        while fx_idx < fx_rates.len() && fx_rates[fx_idx].date <= price.date {
            current_fx = Some(&fx_rates[fx_idx]);
            fx_idx += 1;
        }

        match current_fx {
            Some(fx) => points.push(PricePoint {
                date: price.date,
                close: price.close,
                source: price.source.clone(),
                close_base: Availability::available(price.close * fx.rate),
                fx: Some(FxApplied {
                    rate: fx.rate,
                    date: fx.date,
                }),
            }),
            None => points.push(PricePoint {
                date: price.date,
                close: price.close,
                source: price.source.clone(),
                close_base: Availability::unavailable(ValuationReason::MissingFx),
                fx: None,
            }),
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use super::{
        build_price_history, Availability, FxApplied, FxCandidate, PriceCandidate, ProviderCode,
        ValuationReason,
    };
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
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
    fn build_price_history_carries_fx_forward() {
        // FX only on the 10th; the 11th (no same-day rate) carries the 10th forward.
        let mut prices = vec![
            price(d(2026, 6, 10), dec!(100), "USD"),
            price(d(2026, 6, 11), dec!(110), "USD"),
        ];
        prices[1].source = ProviderCode::new("NASDAQ_NORDIC");
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].source.as_str(), "PRIMARY");
        assert_eq!(points[1].source.as_str(), "NASDAQ_NORDIC");
        assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
        assert_eq!(
            points[0].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 10)
            })
        );
        // Carry-forward: the 11th still applies the 10th's rate and reports the 10th's date.
        assert_eq!(points[1].close_base, Availability::available(dec!(1100)));
        assert_eq!(
            points[1].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 10)
            })
        );
    }

    #[test]
    fn build_price_history_marks_missing_fx_before_any_rate() {
        // Price predates every FX rate: close_base unavailable, native close retained, fx omitted.
        let prices = vec![price(d(2026, 6, 9), dec!(100), "USD")];
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close, dec!(100));
        assert_eq!(
            points[0].close_base,
            Availability::unavailable(ValuationReason::MissingFx)
        );
        assert_eq!(points[0].fx, None);
    }

    #[test]
    fn build_price_history_carries_fx_dated_before_the_first_price() {
        // The only applicable rate predates the first price: prove the full FX set was used.
        let prices = vec![price(d(2026, 6, 20), dec!(100), "USD")];
        let fx_rates = vec![fx(d(2026, 6, 1), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close_base, Availability::available(dec!(1000)));
        assert_eq!(
            points[0].fx,
            Some(FxApplied {
                rate: dec!(10),
                date: d(2026, 6, 1)
            })
        );
    }

    #[test]
    fn build_price_history_sek_uses_identity_and_omits_fx() {
        let prices = vec![price(d(2026, 6, 10), dec!(42), "SEK")];

        let points = build_price_history("SEK", &prices, &[]);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].close_base, Availability::available(dec!(42)));
        assert_eq!(points[0].fx, None);
    }

    #[test]
    fn build_price_history_drops_wrong_currency_rows() {
        // Instrument is USD; a stray EUR row is an internal data error and is excluded.
        let prices = vec![
            price(d(2026, 6, 10), dec!(100), "USD"),
            price(d(2026, 6, 11), dec!(200), "EUR"),
        ];
        let fx_rates = vec![fx(d(2026, 6, 10), dec!(10), "USD", "SEK")];

        let points = build_price_history("USD", &prices, &fx_rates);

        assert_eq!(points.len(), 1);
        assert_eq!(points[0].date, d(2026, 6, 10));
    }
}
