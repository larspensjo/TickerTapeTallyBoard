//! App estimates and fetch timing, using dates supplied by the application clock.
use chrono::{Datelike, Duration, Months, NaiveDate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchDecision {
    NeverFetched,
    Due,
    Backstop,
    Retry,
    NotDue,
}

pub struct FetchTiming {
    pub last_attempt: Option<NaiveDate>,
    pub last_success: Option<NaiveDate>,
    pub previous_failed: bool,
    pub next_report: Option<NaiveDate>,
    pub newer_quarter_seen_on: Option<NaiveDate>,
}

pub fn fetch_decision(today: NaiveDate, timing: &FetchTiming) -> FetchDecision {
    if timing.last_attempt.is_some_and(|date| date >= today) {
        return FetchDecision::NotDue;
    }
    let Some(success) = timing.last_success else {
        return FetchDecision::NeverFetched;
    };
    if timing.next_report.is_some_and(|due| {
        (0..=21).contains(&(today - due).num_days())
            && !timing.newer_quarter_seen_on.is_some_and(|seen| seen >= due)
    }) {
        return FetchDecision::Due;
    }
    if (today - success).num_days() > 7 {
        return FetchDecision::Backstop;
    }
    if timing.previous_failed {
        return FetchDecision::Retry;
    }
    FetchDecision::NotDue
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimateBasis {
    LastYearDate,
    TypicalGap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportEstimate {
    pub date: NaiveDate,
    pub period_end: NaiveDate,
    pub basis: EstimateBasis,
}

/// Only narrow observed windows qualify as real report history. A first fetch
/// without an absence baseline cannot claim an announcement date.
pub fn observed_report(first_seen: NaiveDate, absent_on: Option<NaiveDate>) -> Option<NaiveDate> {
    absent_on
        .filter(|absent| (0..=7).contains(&(first_seen - *absent).num_days()))
        .map(|absent| absent + Duration::days((first_seen - absent).num_days() / 2))
}

pub fn app_estimate(
    latest: Option<NaiveDate>,
    annual_ends: &[NaiveDate],
    real_reports: &[(NaiveDate, NaiveDate)],
) -> Option<ReportEstimate> {
    let latest = latest?;
    let shifted = latest.checked_add_months(Months::new(3))?;
    let first_next_month = if shifted.month() == 12 {
        NaiveDate::from_ymd_opt(shifted.year() + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(shifted.year(), shifted.month() + 1, 1)?
    };
    let period_end = first_next_month - Duration::days(1);
    if let Some((_, report)) = real_reports.iter().find(|(period, _)| {
        period.month() == period_end.month() && period.year() == period_end.year() - 1
    }) {
        return Some(ReportEstimate {
            date: *report + Duration::weeks(52),
            period_end,
            basis: EstimateBasis::LastYearDate,
        });
    }
    let mut lags: Vec<_> = real_reports
        .iter()
        .map(|(period, report)| (*report - *period).num_days())
        .filter(|lag| *lag >= 0)
        .collect();
    lags.sort();
    let lag = if lags.is_empty() {
        if annual_ends
            .iter()
            .any(|end| end.month() == period_end.month())
        {
            45
        } else {
            30
        }
    } else if lags.len() % 2 == 0 {
        (lags[lags.len() / 2 - 1] + lags[lags.len() / 2]) / 2
    } else {
        lags[lags.len() / 2]
    };
    Some(ReportEstimate {
        date: period_end + Duration::days(lag),
        period_end,
        basis: EstimateBasis::TypicalGap,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }
    #[test]
    fn fetch_timing_covers_due_retry_backstop_and_daily_limits() {
        let today = d("2026-11-07");
        let mut t = FetchTiming {
            last_attempt: None,
            last_success: None,
            previous_failed: false,
            next_report: None,
            newer_quarter_seen_on: None,
        };
        assert_eq!(fetch_decision(today, &t), FetchDecision::NeverFetched);
        t.last_attempt = Some(today);
        assert_eq!(fetch_decision(today, &t), FetchDecision::NotDue);
        t.last_attempt = Some(today - Duration::days(1));
        t.last_success = t.last_attempt;
        t.next_report = Some(today);
        assert_eq!(fetch_decision(today, &t), FetchDecision::Due);
        t.newer_quarter_seen_on = Some(today);
        assert_eq!(fetch_decision(today, &t), FetchDecision::NotDue);
        t.newer_quarter_seen_on = None;
        t.next_report = Some(today - Duration::days(21));
        assert_eq!(fetch_decision(today, &t), FetchDecision::Due);
        t.next_report = Some(today - Duration::days(22));
        assert_eq!(fetch_decision(today, &t), FetchDecision::NotDue);
        t.previous_failed = true;
        assert_eq!(fetch_decision(today, &t), FetchDecision::Retry);
        t.last_success = Some(today - Duration::days(8));
        assert_eq!(fetch_decision(today, &t), FetchDecision::Backstop);
        t.last_attempt = Some(today);
        assert_eq!(fetch_decision(today, &t), FetchDecision::NotDue);
    }
    #[test]
    fn estimates_use_month_ends_fiscal_lag_and_real_history() {
        let latest = Some(d("2026-09-30"));
        let estimate = app_estimate(latest, &[d("2025-12-31")], &[]).unwrap();
        assert_eq!(estimate.period_end, d("2026-12-31"));
        assert_eq!(estimate.date, d("2027-02-14"));
        let estimate = app_estimate(latest, &[], &[(d("2025-12-31"), d("2026-02-03"))]).unwrap();
        assert_eq!(estimate.basis, EstimateBasis::LastYearDate);
        assert_eq!(estimate.date, d("2027-02-02"));
        assert!(app_estimate(None, &[], &[]).is_none());
        assert!(observed_report(d("2026-11-07"), None).is_none());
        assert!(observed_report(d("2026-11-07"), Some(d("2026-10-30"))).is_none());
    }
}
