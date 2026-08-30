use super::availability::{Availability, ValuationReason};
use super::ProviderCode;
use chrono::{Datelike, NaiveDate, Weekday};
use rust_decimal::Decimal;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataFreshness {
    Fresh,
    MinorStale { trading_days: i64 },
    WarningStale { trading_days: i64 },
}

impl DataFreshness {
    fn from_age(trading_days: i64) -> Self {
        match trading_days {
            0 => Self::Fresh,
            1..=2 => Self::MinorStale { trading_days },
            _ => Self::WarningStale { trading_days },
        }
    }

    pub fn trading_days(self) -> i64 {
        match self {
            Self::Fresh => 0,
            Self::MinorStale { trading_days } | Self::WarningStale { trading_days } => trading_days,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceCandidate {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub source: ProviderCode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxCandidate {
    pub date: NaiveDate,
    pub rate: Decimal,
    pub base: String,
    pub quote: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PriceSnapshot {
    pub date: NaiveDate,
    pub close: Decimal,
    pub currency: String,
    pub source: ProviderCode,
    pub freshness: DataFreshness,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FxSnapshot {
    pub date: NaiveDate,
    pub rate: Decimal,
    pub base: String,
    pub quote: String,
    pub freshness: DataFreshness,
}

pub(crate) fn fx_snapshot(
    native_currency: &str,
    valuation_date: NaiveDate,
    candidate: Option<FxCandidate>,
    previous: bool,
) -> Availability<FxSnapshot> {
    if native_currency.eq_ignore_ascii_case("SEK") {
        return Availability::available(FxSnapshot {
            date: valuation_date,
            rate: Decimal::ONE,
            base: "SEK".to_owned(),
            quote: "SEK".to_owned(),
            freshness: DataFreshness::Fresh,
        });
    }

    match candidate {
        Some(candidate) => Availability::available(FxSnapshot {
            date: candidate.date,
            rate: candidate.rate,
            base: candidate.base,
            quote: candidate.quote,
            freshness: data_freshness(valuation_date, candidate.date),
        }),
        None => {
            if previous {
                Availability::unavailable(ValuationReason::MissingPreviousFx)
            } else {
                Availability::unavailable(ValuationReason::MissingFx)
            }
        }
    }
}

pub(crate) fn data_freshness(valuation_date: NaiveDate, data_date: NaiveDate) -> DataFreshness {
    DataFreshness::from_age(trading_days_stale(valuation_date, data_date))
}

pub(crate) fn trading_days_stale(valuation_date: NaiveDate, data_date: NaiveDate) -> i64 {
    if data_date >= valuation_date {
        return 0;
    }

    let mut count = 0;
    let mut day = data_date;
    while let Some(next) = day.succ_opt() {
        if next > valuation_date {
            break;
        }
        if is_weekday(next) {
            count += 1;
        }
        day = next;
    }
    count
}

pub(crate) fn is_weekday(date: NaiveDate) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

pub(crate) fn append_snapshot_reasons(
    reasons: &mut Vec<ValuationReason>,
    state: &Availability<PriceSnapshot>,
) {
    match state {
        Availability::Available(snapshot) => match snapshot.freshness {
            DataFreshness::Fresh => {}
            DataFreshness::MinorStale { trading_days }
            | DataFreshness::WarningStale { trading_days } => {
                reasons.push(ValuationReason::StalePrice { trading_days });
            }
        },
        Availability::Unavailable {
            reasons: state_reasons,
        } => reasons.extend(state_reasons.clone()),
    }
}

pub(crate) fn append_fx_reasons(
    reasons: &mut Vec<ValuationReason>,
    state: &Availability<FxSnapshot>,
) {
    match state {
        Availability::Available(snapshot) => match snapshot.freshness {
            DataFreshness::Fresh => {}
            DataFreshness::MinorStale { trading_days }
            | DataFreshness::WarningStale { trading_days } => {
                reasons.push(ValuationReason::StaleFx { trading_days });
            }
        },
        Availability::Unavailable {
            reasons: state_reasons,
        } => reasons.extend(state_reasons.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::DataFreshness;
    use chrono::NaiveDate;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test]
    fn weekday_staleness_counts_public_holidays_as_trading_days_for_now() {
        assert_eq!(
            super::data_freshness(d(2026, 6, 22), d(2026, 6, 18)),
            DataFreshness::MinorStale { trading_days: 2 }
        );
    }
}
