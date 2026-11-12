//! Relative price paths: the same market picture for every supplied application date.
use super::{DemoInstrument, DemoPrice};
use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

pub(super) const SPLIT_AGE_DAYS: i64 = 200;

/// Deterministic market inputs; scales and exceptional history belong to the case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PricePath {
    pub scale: Decimal,
    pub end: Decimal,
    pub wave_percent: Decimal,
    pub recovery: bool,
    pub rising_week: bool,
    pub history_days: Option<i64>,
    pub gap: Option<(i64, i64)>,
    pub split_stale: bool,
}

impl Default for PricePath {
    fn default() -> Self {
        Self {
            scale: Decimal::ONE,
            end: dec!(85),
            wave_percent: dec!(0.7),
            recovery: false,
            rising_week: false,
            history_days: None,
            gap: None,
            split_stale: false,
        }
    }
}

pub(super) fn prices(
    start: NaiveDate,
    today: NaiveDate,
    instruments: &[DemoInstrument],
) -> Vec<DemoPrice> {
    instruments
        .iter()
        .enumerate()
        .flat_map(|(instrument_index, instrument)| {
            let path = &instrument.scenario.prices;
            (0..=(today - start).num_days()).filter_map(move |age| {
                // Keep every endpoint while leaving a hole of more than five weekdays.
                if path.history_days.is_some_and(|limit| age > limit)
                    || path
                        .gap
                        .is_some_and(|(start, end)| (start..=end).contains(&age))
                {
                    return None;
                }
                Some(DemoPrice {
                    instrument_index,
                    provider: instrument.provider,
                    provider_symbol: instrument.provider_symbol,
                    date: today - Duration::days(age),
                    close: close(path, age),
                    currency: instrument.currency,
                })
            })
        })
        .collect()
}

/// Integer triangle waves avoid floating-point or clock-dependent seed changes.
fn wave(age: i64, period: i64) -> Decimal {
    Decimal::from((age % period - period / 2).abs() * 4) / Decimal::from(period) - Decimal::ONE
}

fn movement(age: i64, amplitude: Decimal) -> Decimal {
    amplitude * (wave(age, 46) + wave(age, 18) * dec!(0.4) + wave(age, 8) * dec!(0.2))
}

pub(super) fn close(path: &PricePath, age: i64) -> Decimal {
    let value = if age > 30 {
        // Mild long-run appreciation with overlapping waves of a few percent.
        dec!(100) - Decimal::from(age - 30) * dec!(0.018) + movement(age, path.wave_percent)
            - movement(30, path.wave_percent)
    } else if path.recovery {
        // At least three weekdays since the low under every weekday alignment.
        if age <= 5 {
            dec!(72) - Decimal::from(age) * dec!(0.4)
        } else {
            dec!(70) + Decimal::from(age - 5) * dec!(1.2)
        }
    } else if path.rising_week {
        // A modest fund decline over 14d/30d, with a visible 7d rebound.
        path.end + Decimal::from(age) * dec!(0.01)
            - Decimal::from((7 - (age - 7).abs()).max(0)) * dec!(0.17) / dec!(7)
    } else {
        path.end
            + (dec!(100) - path.end) * Decimal::from(age) / dec!(30)
            + (movement(age, dec!(0.12)) - movement(30, dec!(0.12))) * Decimal::from(age) / dec!(30)
    };
    // Cached pre-split closes have not been refreshed onto the post-split basis.
    let split_basis = if path.split_stale && age > SPLIT_AGE_DAYS {
        dec!(2)
    } else {
        Decimal::ONE
    };
    (value * path.scale * split_basis).round_dp(4)
}

pub(super) fn fetched_at(
    today: NaiveDate,
    price: &DemoPrice,
    instrument: &DemoInstrument,
) -> String {
    // A real Yahoo split-stale look-back: older closes predate the split,
    // while recent closes and every current window remain usable.
    let fetched_on = if instrument.scenario.prices.split_stale
        && price.date < today - Duration::days(SPLIT_AGE_DAYS)
    {
        today - Duration::days(SPLIT_AGE_DAYS + 1)
    } else {
        today
    };
    format!("{fetched_on}T12:00:00Z")
}
