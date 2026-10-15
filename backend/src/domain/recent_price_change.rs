use chrono::{Duration, NaiveDate};
use rust_decimal::Decimal;

use super::market_snapshot::trading_days_stale;
use super::{PriceCandidate, PriceSnapshot};

pub const START_CLOSE_MAX_AGE_TRADING_DAYS: i64 = 3;

/// Calendar days of closes ending today that every window's start close can
/// come from: the longest window plus a margin wider than the start-close
/// tolerance's calendar span (5 days for the current 3-weekday tolerance).
pub const RECENT_CHANGE_HISTORY_DAYS: i64 = RecentWindow::ThirtyDays.days() + 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentWindow {
    OneWeek,
    TwoWeeks,
    ThirtyDays,
}

impl RecentWindow {
    pub const fn days(self) -> i64 {
        match self {
            Self::OneWeek => 7,
            Self::TwoWeeks => 14,
            Self::ThirtyDays => 30,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::OneWeek => "7d",
            Self::TwoWeeks => "14d",
            Self::ThirtyDays => "30d",
        }
    }

    pub fn from_query(value: &str) -> Option<Self> {
        match value {
            "7d" => Some(Self::OneWeek),
            "14d" => Some(Self::TwoWeeks),
            "30d" => Some(Self::ThirtyDays),
            _ => None,
        }
    }

    pub fn start_date(self, today: NaiveDate) -> NaiveDate {
        today - Duration::days(self.days())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentChangeReason {
    NoPriceSource,
    MissingLatestClose,
    NoCloseSincePeriodStart,
    MissingStartClose,
    StartCloseTooOld,
    StartCloseSourceMismatch,
    StartCloseCurrencyMismatch,
    ZeroStartClose,
    SplitInPeriod,
}

impl RecentChangeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoPriceSource => "no_price_source",
            Self::MissingLatestClose => "missing_latest_close",
            Self::NoCloseSincePeriodStart => "no_close_since_period_start",
            Self::MissingStartClose => "missing_start_close",
            Self::StartCloseTooOld => "start_close_too_old",
            Self::StartCloseSourceMismatch => "start_close_source_mismatch",
            Self::StartCloseCurrencyMismatch => "start_close_currency_mismatch",
            Self::ZeroStartClose => "zero_start_close",
            Self::SplitInPeriod => "split_in_period",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecentPriceChange {
    Available {
        start_date: NaiveDate,
        start_close: Decimal,
        end_date: NaiveDate,
        end_close: Decimal,
        change_percent: Decimal,
    },
    Unavailable(Vec<RecentChangeReason>),
}

/// Validates a covered price pair in priority order. The caller handles absent
/// source coverage, which cannot be inferred from a missing latest close.
pub fn recent_price_change(
    period_start: NaiveDate,
    end: Option<&PriceSnapshot>,
    start: Option<&PriceCandidate>,
    split_dates: &[NaiveDate],
) -> RecentPriceChange {
    use RecentChangeReason::*;
    let unavailable = |reason| RecentPriceChange::Unavailable(vec![reason]);
    let Some(end) = end else {
        return unavailable(MissingLatestClose);
    };
    if end.date <= period_start {
        return unavailable(NoCloseSincePeriodStart);
    }
    let Some(start) = start else {
        return unavailable(MissingStartClose);
    };
    if trading_days_stale(period_start, start.date) > START_CLOSE_MAX_AGE_TRADING_DAYS {
        return unavailable(StartCloseTooOld);
    }
    if start.source != end.source {
        return unavailable(StartCloseSourceMismatch);
    }
    if !start.currency.eq_ignore_ascii_case(&end.currency) {
        return unavailable(StartCloseCurrencyMismatch);
    }
    if start.close.is_zero() {
        return unavailable(ZeroStartClose);
    }
    if split_dates
        .iter()
        .any(|date| *date > start.date && *date <= end.date)
    {
        return unavailable(SplitInPeriod);
    }

    RecentPriceChange::Available {
        start_date: start.date,
        start_close: start.close,
        end_date: end.date,
        end_close: end.close,
        change_percent: (end.close - start.close) / start.close * Decimal::from(100),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PriceVersusAverageReason {
    MissingLatestClose,
    PriceCurrencyMismatch,
    ZeroAverageCost,
}

impl PriceVersusAverageReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingLatestClose => "missing_latest_close",
            Self::PriceCurrencyMismatch => "price_currency_mismatch",
            Self::ZeroAverageCost => "zero_average_cost",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PriceVersusAverage {
    Available {
        percent: Decimal,
        below_average: bool,
    },
    Unavailable(Vec<PriceVersusAverageReason>),
}

pub fn price_versus_average(
    latest: Option<&PriceSnapshot>,
    instrument_currency: &str,
    average_cost_native: Option<Decimal>,
) -> PriceVersusAverage {
    use PriceVersusAverageReason::*;
    let unavailable = |reason| PriceVersusAverage::Unavailable(vec![reason]);
    let Some(latest) = latest else {
        return unavailable(MissingLatestClose);
    };
    if !latest.currency.eq_ignore_ascii_case(instrument_currency) {
        return unavailable(PriceCurrencyMismatch);
    }
    let Some(average) = average_cost_native.filter(|average| !average.is_zero()) else {
        return unavailable(ZeroAverageCost);
    };
    PriceVersusAverage::Available {
        percent: (latest.close - average) / average * Decimal::from(100),
        below_average: latest.close < average,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{DataFreshness, ProviderCode};
    use rust_decimal_macros::dec;

    fn date(value: &str) -> NaiveDate {
        value.parse().expect("date")
    }

    fn candidate(day: &str, close: Decimal) -> PriceCandidate {
        PriceCandidate {
            date: date(day),
            close,
            currency: "SEK".into(),
            source: ProviderCode::new("YAHOO"),
        }
    }

    fn snapshot(day: &str, close: Decimal) -> PriceSnapshot {
        let price = candidate(day, close);
        PriceSnapshot {
            date: price.date,
            close,
            currency: price.currency,
            source: price.source,
            freshness: DataFreshness::Fresh,
        }
    }

    fn assert_reason(change: RecentPriceChange, reason: RecentChangeReason) {
        assert_eq!(change, RecentPriceChange::Unavailable(vec![reason]));
    }

    #[test]
    fn windows_accept_exact_query_values_and_resolve_calendar_dates() {
        for (window, days, text, start) in [
            (RecentWindow::OneWeek, 7, "7d", "2026-10-08"),
            (RecentWindow::TwoWeeks, 14, "14d", "2026-10-01"),
            (RecentWindow::ThirtyDays, 30, "30d", "2026-09-15"),
        ] {
            assert_eq!(window.days(), days);
            assert_eq!(window.as_str(), text);
            assert_eq!(RecentWindow::from_query(text), Some(window));
            assert_eq!(window.start_date(date("2026-10-15")), date(start));
        }
        for invalid in ["7D", "1w", "", " 7d", "7d ", "31d"] {
            assert_eq!(RecentWindow::from_query(invalid), None);
        }
    }

    #[test]
    fn tolerance_counts_weekdays_including_weekends_and_christmas_gap() {
        let end = snapshot("2026-12-31", dec!(90));
        for (period_start, start_date, available) in [
            ("2026-10-08", "2026-10-05", true),  // three weekdays
            ("2026-10-08", "2026-10-02", false), // four weekdays
            ("2026-10-11", "2026-10-09", true),  // Sunday / Friday
            ("2026-12-27", "2026-12-22", true),  // Christmas-shaped gap
        ] {
            let start = candidate(start_date, dec!(100));
            let change = recent_price_change(date(period_start), Some(&end), Some(&start), &[]);
            if available {
                assert!(
                    matches!(change, RecentPriceChange::Available { .. }),
                    "{change:?}"
                );
            } else {
                assert_reason(change, RecentChangeReason::StartCloseTooOld);
            }
        }
    }

    #[test]
    fn history_covers_the_oldest_acceptable_start_close_of_every_window() {
        // A Monday-to-Sunday cycle of `today` covers every weekday alignment.
        let monday = date("2026-10-12");
        for offset in 0..7 {
            let today = monday + Duration::days(offset);
            for window in [
                RecentWindow::OneWeek,
                RecentWindow::TwoWeeks,
                RecentWindow::ThirtyDays,
            ] {
                let period_start = window.start_date(today);
                let mut oldest = period_start;
                while trading_days_stale(period_start, oldest - Duration::days(1))
                    <= START_CLOSE_MAX_AGE_TRADING_DAYS
                {
                    oldest -= Duration::days(1);
                }
                assert!(
                    (today - oldest).num_days() <= RECENT_CHANGE_HISTORY_DAYS,
                    "{window:?} from {today} accepts a start close on {oldest}"
                );
            }
        }
    }

    #[test]
    fn missing_closes_and_latest_on_or_before_period_start_are_unavailable_in_priority_order() {
        let period_start = date("2026-10-08");
        assert_reason(
            recent_price_change(period_start, None, None, &[]),
            RecentChangeReason::MissingLatestClose,
        );
        for day in ["2026-10-08", "2026-10-07"] {
            assert_reason(
                recent_price_change(period_start, Some(&snapshot(day, dec!(100))), None, &[]),
                RecentChangeReason::NoCloseSincePeriodStart,
            );
        }
        assert_reason(
            recent_price_change(
                period_start,
                Some(&snapshot("2026-10-15", dec!(90))),
                None,
                &[],
            ),
            RecentChangeReason::MissingStartClose,
        );
    }

    #[test]
    fn source_currency_zero_and_split_guards_follow_table_order() {
        let period_start = date("2026-10-08");
        let end = snapshot("2026-10-15", dec!(90));
        let mut start = candidate("2026-10-08", Decimal::ZERO);
        start.source = ProviderCode::new("NASDAQ_NORDIC");
        start.currency = "USD".into();
        let split = [date("2026-10-09")];
        assert_reason(
            recent_price_change(period_start, Some(&end), Some(&start), &split),
            RecentChangeReason::StartCloseSourceMismatch,
        );
        start.source = end.source.clone();
        assert_reason(
            recent_price_change(period_start, Some(&end), Some(&start), &split),
            RecentChangeReason::StartCloseCurrencyMismatch,
        );
        start.currency = "sek".into();
        assert_reason(
            recent_price_change(period_start, Some(&end), Some(&start), &split),
            RecentChangeReason::ZeroStartClose,
        );
        start.close = dec!(100);
        assert_reason(
            recent_price_change(period_start, Some(&end), Some(&start), &split),
            RecentChangeReason::SplitInPeriod,
        );
        start.date = date("2026-10-01");
        assert_reason(
            recent_price_change(period_start, Some(&end), Some(&start), &split),
            RecentChangeReason::StartCloseTooOld,
        );
    }

    #[test]
    fn split_interval_excludes_start_and_includes_end_and_uses_actual_close_dates() {
        let start = candidate("2026-10-07", dec!(100));
        let end = snapshot("2026-10-14", dec!(90));
        for (day, blocked) in [
            ("2026-10-06", false),
            ("2026-10-07", false),
            ("2026-10-08", true),
            ("2026-10-14", true),
            ("2026-10-15", false),
        ] {
            let change =
                recent_price_change(date("2026-10-08"), Some(&end), Some(&start), &[date(day)]);
            if blocked {
                assert_reason(change, RecentChangeReason::SplitInPeriod);
            } else {
                assert!(matches!(change, RecentPriceChange::Available { .. }));
            }
        }
    }

    #[test]
    fn change_keeps_exact_decimal_precision_and_actual_closes() {
        let start = candidate("2026-10-08", dec!(412.5));
        let end = snapshot("2026-10-15", dec!(377.8));
        let change = recent_price_change(start.date, Some(&end), Some(&start), &[]);
        assert_eq!(
            change,
            RecentPriceChange::Available {
                start_date: start.date,
                start_close: start.close,
                end_date: end.date,
                end_close: end.close,
                change_percent: (dec!(377.8) - dec!(412.5)) / dec!(412.5) * dec!(100),
            }
        );
        let RecentPriceChange::Available { change_percent, .. } = change else {
            panic!("available")
        };
        assert!(change_percent < dec!(-8.4121) && change_percent > dec!(-8.4122));
    }

    #[test]
    fn tiny_negative_change_stays_exact_until_serialization() {
        let start = candidate("2026-10-08", dec!(100));
        let end = snapshot("2026-10-15", dec!(99.999));
        let RecentPriceChange::Available { change_percent, .. } =
            recent_price_change(start.date, Some(&end), Some(&start), &[])
        else {
            panic!("available")
        };
        assert_eq!(change_percent, dec!(-0.001));
        assert!(change_percent.round_dp(2).is_zero());
    }

    #[test]
    fn average_comparison_is_exact_below_equal_above_and_fractionally_below() {
        for (close, below, percent) in [
            (dec!(90), true, dec!(-10)),
            (dec!(100), false, dec!(0)),
            (dec!(110), false, dec!(10)),
        ] {
            assert_eq!(
                price_versus_average(Some(&snapshot("2026-10-15", close)), "SEK", Some(dec!(100))),
                PriceVersusAverage::Available {
                    percent,
                    below_average: below
                }
            );
        }
        let average = dec!(100.00000001);
        let comparison = price_versus_average(
            Some(&snapshot("2026-10-15", dec!(100))),
            "SEK",
            Some(average),
        );
        let PriceVersusAverage::Available {
            percent,
            below_average,
        } = comparison
        else {
            panic!("available")
        };
        assert!(below_average);
        assert!(percent < Decimal::ZERO);
        assert!(percent.round_dp(2).is_zero());
    }

    #[test]
    fn average_comparison_validates_latest_currency_and_nonzero_average_in_order() {
        use PriceVersusAverageReason::*;
        assert_eq!(
            price_versus_average(None, "SEK", None),
            PriceVersusAverage::Unavailable(vec![MissingLatestClose])
        );
        let latest = snapshot("2026-10-15", dec!(100));
        assert_eq!(
            price_versus_average(Some(&latest), "USD", Some(Decimal::ZERO)),
            PriceVersusAverage::Unavailable(vec![PriceCurrencyMismatch])
        );
        for average in [Some(Decimal::ZERO), None] {
            assert_eq!(
                price_versus_average(Some(&latest), "SEK", average),
                PriceVersusAverage::Unavailable(vec![ZeroAverageCost])
            );
        }
    }

    #[test]
    fn reason_codes_are_the_public_contract() {
        for (reason, code) in [
            (RecentChangeReason::NoPriceSource, "no_price_source"),
            (
                RecentChangeReason::MissingLatestClose,
                "missing_latest_close",
            ),
            (
                RecentChangeReason::NoCloseSincePeriodStart,
                "no_close_since_period_start",
            ),
            (RecentChangeReason::MissingStartClose, "missing_start_close"),
            (RecentChangeReason::StartCloseTooOld, "start_close_too_old"),
            (
                RecentChangeReason::StartCloseSourceMismatch,
                "start_close_source_mismatch",
            ),
            (
                RecentChangeReason::StartCloseCurrencyMismatch,
                "start_close_currency_mismatch",
            ),
            (RecentChangeReason::ZeroStartClose, "zero_start_close"),
            (RecentChangeReason::SplitInPeriod, "split_in_period"),
        ] {
            assert_eq!(reason.as_str(), code);
        }
        assert_eq!(
            PriceVersusAverageReason::MissingLatestClose.as_str(),
            "missing_latest_close"
        );
        assert_eq!(
            PriceVersusAverageReason::PriceCurrencyMismatch.as_str(),
            "price_currency_mismatch"
        );
        assert_eq!(
            PriceVersusAverageReason::ZeroAverageCost.as_str(),
            "zero_average_cost"
        );
    }
}
