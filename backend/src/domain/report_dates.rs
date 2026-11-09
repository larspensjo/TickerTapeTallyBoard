//! Report selection and refresh planning, using the application clock.
mod announcements;
use super::calendar::month_end;
use super::company_figures::{Reason, SourceState};
pub use announcements::*;
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
    pub unresolved_announcement: bool,
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
        (due <= today && (timing.unresolved_announcement || (today - due).num_days() <= 21))
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
    CompanyAnnounced,
    NasdaqExpected,
    LastYearDate,
    TypicalGap,
}

#[derive(Clone, Debug)]
pub struct Observation {
    pub period_end: NaiveDate,
    pub first_seen: NaiveDate,
    pub absent_on: Option<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportCertainty {
    Confirmed,
    Estimated,
    Observed,
    PeriodOnly,
    NotApplicable,
    Unavailable,
}

#[derive(Clone, Copy, Debug)]
pub struct ReportExpectation {
    pub date: NaiveDate,
    pub basis: EstimateBasis,
}
impl ReportExpectation {
    pub fn is_overdue(self, today: NaiveDate) -> bool {
        let grace = match self.basis {
            EstimateBasis::LastYearDate | EstimateBasis::TypicalGap => 21,
            EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected => 7,
        };
        (today - self.date).num_days() > grace
    }
}

/// Source-neutral report date selected from schedules, observations or app estimates.
#[derive(Clone, Debug)]
pub struct ReportDate {
    pub certainty: ReportCertainty,
    pub basis: Option<EstimateBasis>,
    pub date: Option<NaiveDate>,
    pub days_until: Option<i64>,
    pub period_end: Option<NaiveDate>,
    pub observed_between: Option<[NaiveDate; 2]>,
    pub reasons: Vec<Reason>,
}

impl ReportDate {
    pub fn expectation(&self) -> Option<ReportExpectation> {
        Some(ReportExpectation {
            date: self.date?,
            basis: self.basis?,
        })
    }
}

pub fn report_dates(
    today: NaiveDate,
    latest: Option<NaiveDate>,
    annual_ends: &[NaiveDate],
    observations: &[Observation],
    source: &SourceState,
) -> (ReportDate, ReportDate) {
    let empty = |certainty, reason| ReportDate {
        certainty,
        basis: None,
        date: None,
        days_until: None,
        period_end: None,
        observed_between: None,
        reasons: vec![reason],
    };
    if let Some(reason) = source.blocking_reason(latest.is_some()) {
        let certainty = if source.not_applicable().is_some() {
            ReportCertainty::NotApplicable
        } else {
            ReportCertainty::Unavailable
        };
        return (empty(certainty, reason.clone()), empty(certainty, reason));
    }
    let real: Vec<_> = observations
        .iter()
        .filter_map(|o| observed_report(o.first_seen, o.absent_on).map(|d| (o.period_end, d)))
        .collect();
    let next = next_report(today, latest, annual_ends, &real, source);
    let last = match latest {
        Some(period_end) => {
            let observation = observations.iter().find(|o| {
                o.period_end == period_end && observed_report(o.first_seen, o.absent_on).is_some()
            });
            ReportDate {
                certainty: if observation.is_some() {
                    ReportCertainty::Observed
                } else {
                    ReportCertainty::PeriodOnly
                },
                basis: None,
                date: observation.and_then(|o| observed_report(o.first_seen, o.absent_on)),
                days_until: observation
                    .and_then(|o| observed_report(o.first_seen, o.absent_on))
                    .map(|d| (d - today).num_days()),
                period_end: Some(period_end),
                observed_between: observation.and_then(|o| o.absent_on.map(|a| [a, o.first_seen])),
                reasons: vec![],
            }
        }
        None => empty(
            ReportCertainty::Unavailable,
            Reason::new("too_few_quarters"),
        ),
    };
    (next, last)
}

pub fn next_report(
    today: NaiveDate,
    latest: Option<NaiveDate>,
    annual_ends: &[NaiveDate],
    real: &[(NaiveDate, NaiveDate)],
    source: &SourceState,
) -> ReportDate {
    if let Some(reason) = source.blocking_reason(latest.is_some()) {
        return ReportDate {
            certainty: if source.not_applicable().is_some() {
                ReportCertainty::NotApplicable
            } else {
                ReportCertainty::Unavailable
            },
            basis: None,
            date: None,
            days_until: None,
            period_end: None,
            observed_between: None,
            reasons: vec![reason],
        };
    }
    match app_estimate(latest, annual_ends, real) {
        Some(e) => ReportDate {
            certainty: ReportCertainty::Estimated,
            basis: Some(e.basis),
            date: Some(e.date),
            days_until: Some((e.date - today).num_days()),
            period_end: Some(e.period_end),
            observed_between: None,
            reasons: if e.date < today {
                vec![Reason::new("estimate_passed")]
            } else {
                vec![]
            },
        },
        None => ReportDate {
            certainty: ReportCertainty::Unavailable,
            basis: None,
            date: None,
            days_until: None,
            period_end: None,
            observed_between: None,
            reasons: vec![Reason::new("too_few_quarters")],
        },
    }
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
    let period_end = month_end(shifted)?;
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
            unresolved_announcement: false,
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

    #[test]
    fn dates_retain_passed_estimates_and_only_claim_narrow_observations() {
        use crate::domain::fundamentals::{Classification, LinkStatus};
        let source = SourceState {
            classification: Classification::Equity,
            status: LinkStatus::Usable,
            awaiting_figures: false,
            unreachable: false,
            last_success: None,
        };
        let observations = vec![Observation {
            period_end: d("2026-06-30"),
            first_seen: d("2026-08-02"),
            absent_on: Some(d("2026-07-28")),
        }];
        let (next, last) = report_dates(
            d("2026-11-07"),
            Some(d("2026-06-30")),
            &[],
            &observations,
            &source,
        );
        assert_eq!(next.certainty, ReportCertainty::Estimated);
        assert_eq!(next.basis, Some(EstimateBasis::TypicalGap));
        assert_eq!(next.date, Some(d("2026-10-30")));
        assert_eq!(next.days_until, Some(-8));
        assert_eq!(next.reasons[0].code, "estimate_passed");
        assert_eq!(last.certainty, ReportCertainty::Observed);
        assert_eq!(last.date, Some(d("2026-07-30")));
        assert_eq!(
            last.observed_between,
            Some([d("2026-07-28"), d("2026-08-02")])
        );
        let (_, last) = report_dates(d("2026-11-07"), Some(d("2026-06-30")), &[], &[], &source);
        assert_eq!(last.certainty, ReportCertainty::PeriodOnly);
        assert!(last.date.is_none());
        assert_eq!(last.period_end, Some(d("2026-06-30")));
        let reports = [
            (d("2025-03-31"), d("2025-05-01")),
            (d("2025-06-30"), d("2025-08-02")),
        ];
        assert_eq!(
            app_estimate(Some(d("2026-01-31")), &[], &reports)
                .unwrap()
                .date,
            d("2026-06-01")
        );
        assert_eq!(
            app_estimate(Some(d("2026-10-31")), &[], &[])
                .unwrap()
                .period_end,
            d("2027-01-31")
        );
    }
}
