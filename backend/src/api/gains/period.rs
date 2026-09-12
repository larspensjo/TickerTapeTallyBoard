use chrono::{Datelike, Duration, NaiveDate};

use crate::api::error::ApiError;

/// A report period the backend has resolved. `start: None` means "since the
/// first transaction", which the caller derives from the ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeriod {
    pub start: Option<NaiveDate>,
    pub end: NaiveDate,
}

/// Resolve the period a report covers from the intent supplied by the client.
pub fn resolve_period(
    period: Option<&str>,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    today: NaiveDate,
) -> Result<ResolvedPeriod, ApiError> {
    let resolved = match period {
        None | Some("custom") => ResolvedPeriod {
            start: start_date,
            end: end_date.unwrap_or(today),
        },
        Some("today") => ResolvedPeriod {
            start: Some(today),
            end: today,
        },
        Some("7d") => ResolvedPeriod {
            start: Some(today - Duration::days(7)),
            end: today,
        },
        Some("12m") => ResolvedPeriod {
            start: Some(today.with_year(today.year() - 1).unwrap_or_else(|| {
                NaiveDate::from_ymd_opt(today.year() - 1, 3, 1)
                    .expect("1 March exists in every year")
            })),
            end: today,
        },
        Some("ytd") => ResolvedPeriod {
            start: NaiveDate::from_ymd_opt(today.year(), 1, 1),
            end: today,
        },
        Some("all") => ResolvedPeriod {
            start: None,
            end: today,
        },
        Some(other) => {
            return Err(ApiError::bad_request(
                "invalid_period",
                format!("invalid period: {other}"),
            ));
        }
    };

    if let Some(start) = resolved.start {
        if start > resolved.end {
            return Err(ApiError::bad_request(
                "start_date_after_end_date",
                "start_date must not be after end_date",
            ));
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test]
    fn presets_resolve_against_today() {
        let today = d(2026, 9, 12);

        for (preset, start, end) in [
            ("today", Some(d(2026, 9, 12)), d(2026, 9, 12)),
            ("7d", Some(d(2026, 9, 5)), d(2026, 9, 12)),
            ("12m", Some(d(2025, 9, 12)), d(2026, 9, 12)),
            ("ytd", Some(d(2026, 1, 1)), d(2026, 9, 12)),
            ("all", None, d(2026, 9, 12)),
        ] {
            let resolved = resolve_period(Some(preset), None, None, today)
                .unwrap_or_else(|_| panic!("{preset} resolves"));
            assert_eq!(resolved.start, start, "{preset} start");
            assert_eq!(resolved.end, end, "{preset} end");
        }
    }

    #[test]
    fn twelve_months_back_from_a_leap_day_starts_on_first_march() {
        let leap_day = d(2028, 2, 29);
        let resolved = resolve_period(Some("12m"), None, None, leap_day).expect("resolves");

        assert_eq!(resolved.start, Some(d(2027, 3, 1)));
        assert_eq!(resolved.end, leap_day);
    }

    #[test]
    fn explicit_dates_win_and_default_their_end_to_today() {
        let today = d(2026, 9, 12);
        let resolved = resolve_period(None, Some(d(2026, 2, 1)), Some(d(2026, 6, 29)), today)
            .expect("explicit range resolves");
        assert_eq!(resolved.start, Some(d(2026, 2, 1)));
        assert_eq!(resolved.end, d(2026, 6, 29));

        let open_ended = resolve_period(Some("custom"), Some(d(2026, 2, 1)), None, today)
            .expect("open-ended custom range resolves");
        assert_eq!(open_ended.end, today);
    }

    #[test]
    fn an_unknown_preset_is_a_bad_request() {
        let error = resolve_period(Some("last-tuesday"), None, None, d(2026, 9, 12))
            .expect_err("unknown preset is rejected");
        assert_eq!(error.code(), "invalid_period");
    }

    #[test]
    fn a_start_after_the_end_is_a_bad_request() {
        let error = resolve_period(
            None,
            Some(d(2026, 9, 13)),
            Some(d(2026, 9, 12)),
            d(2026, 9, 12),
        )
        .expect_err("inverted range is rejected");
        assert_eq!(error.code(), "start_date_after_end_date");
    }
}
