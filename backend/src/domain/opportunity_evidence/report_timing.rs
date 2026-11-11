use super::parameters::*;
use crate::domain::{
    company_figures::{Reason, SourceState},
    fundamentals::Classification,
    report_dates::{implied_quarter, EstimateBasis, Observation, ReportCertainty, ReportDate},
};
use chrono::{Duration, NaiveDate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportInPeriodState {
    Yes,
    No,
    Unknown,
    NotApplicable,
}

impl ReportInPeriodState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
            Self::Unknown => "unknown",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Period-only knowledge stays an interval or upper bound; it never becomes a date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LastReportEvidence {
    Confirmed {
        date: NaiveDate,
    },
    Interval {
        period_end: NaiveDate,
        absent_on: NaiveDate,
        first_seen: NaiveDate,
    },
    UpperBound {
        period_end: NaiveDate,
        first_seen: NaiveDate,
    },
}

#[derive(Clone, Debug)]
pub struct ReportInPeriodOutcome {
    pub state: ReportInPeriodState,
    pub reasons: Vec<Reason>,
    pub window_start: NaiveDate,
    pub window_end: NaiveDate,
    pub last_report: Option<LastReportEvidence>,
    pub scheduled_report: Option<ReportDate>,
    pub no_report_expected_before: Option<NaiveDate>,
}

pub struct ReportTimingInputs<'a> {
    pub today: NaiveDate,
    pub period_start: NaiveDate,
    pub latest_period: Option<NaiveDate>,
    pub source: &'a SourceState,
    pub last: &'a ReportDate,
    pub observations: &'a [Observation],
    /// Earliest observation across annual and quarterly periods, for reason detail.
    pub earliest_first_seen: Option<NaiveDate>,
    pub next: &'a ReportDate,
    /// The app estimate independently of a preferred unresolved schedule.
    pub app_next: &'a ReportDate,
}

pub fn report_in_period(
    input: &ReportTimingInputs<'_>,
    rule: NoReportRule,
) -> ReportInPeriodOutcome {
    use ReportInPeriodState::*;
    let today = input.today;
    let start = input.period_start - Duration::days(REPORT_WINDOW_WIDENING_DAYS);
    let mut out = ReportInPeriodOutcome {
        state: Unknown,
        reasons: vec![],
        window_start: start,
        window_end: today,
        last_report: None,
        scheduled_report: None,
        no_report_expected_before: None,
    };
    macro_rules! answer {
        ($state:expr, $reason:expr) => {{
            out.state = $state;
            out.reasons = vec![Reason::new($reason)];
            return out;
        }};
    }
    if let Some(code) = input.source.not_applicable() {
        answer!(NotApplicable, code);
    }
    if let Some(reason) = input.source.blocking_reason(input.latest_period.is_some()) {
        out.reasons.push(reason);
        return out;
    }
    if input.source.classification == Classification::Unknown {
        answer!(Unknown, "security_type_unknown");
    }
    let mut uncertainty = None;
    let mut upper = None;
    if input.last.certainty == ReportCertainty::Confirmed {
        if let Some(date) = input.last.date {
            out.last_report = Some(LastReportEvidence::Confirmed { date });
            if date >= start && date <= today {
                answer!(Yes, "confirmed_report");
            }
            upper = Some(date);
        }
    } else if let Some(obs) = input
        .observations
        .iter()
        .find(|o| Some(o.period_end) == input.latest_period)
    {
        upper = Some(obs.first_seen);
        out.last_report = Some(match obs.absent_on {
            Some(absent_on) => LastReportEvidence::Interval {
                period_end: obs.period_end,
                absent_on,
                first_seen: obs.first_seen,
            },
            None => LastReportEvidence::UpperBound {
                period_end: obs.period_end,
                first_seen: obs.first_seen,
            },
        });
        if obs.first_seen >= start && obs.first_seen <= today {
            match obs.absent_on {
                Some(absent) if absent >= start && absent <= obs.first_seen => {
                    answer!(Yes, "observed_report_inside_window");
                }
                Some(_) => uncertainty = Some("observed_interval_overlaps_window"),
                None => {
                    uncertainty = Some(if input.earliest_first_seen == Some(obs.first_seen) {
                        "first_fetch_without_baseline"
                    } else {
                        "figures_without_absence_baseline"
                    })
                }
            }
        } else if obs.first_seen > today {
            uncertainty = Some("observation_after_as_of");
        }
    }
    if input
        .next
        .date
        .is_some_and(|date| date >= start && date < today)
    {
        out.scheduled_report = Some(input.next.clone());
        if matches!(
            input.next.basis,
            Some(EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected)
        ) {
            answer!(Yes, "scheduled_report_figures_not_seen");
        }
        uncertainty = Some("app_estimate_passed_in_window");
    }
    if let Some(reason) = uncertainty {
        answer!(Unknown, reason);
    }
    if !upper.is_some_and(|date| date < start) {
        answer!(Unknown, "last_report_upper_bound_unavailable");
    }
    let quarter_not_ended = implied_quarter(input.latest_period).is_some_and(|date| date > today);
    let fresh_check = input
        .source
        .last_success
        .is_some_and(|date| date >= today - Duration::days(1) && date <= today);
    if quarter_not_ended || fresh_check {
        answer!(No, "strict_absence");
    }
    if rule == NoReportRule::Relaxed
        && input.app_next.basis == Some(EstimateBasis::LastYearDate)
        && input
            .app_next
            .date
            .is_some_and(|date| date >= today + Duration::days(RELAXED_REPORT_HORIZON_DAYS))
    {
        out.no_report_expected_before = input.app_next.date;
        if matches!(
            input.next.basis,
            Some(EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected)
        ) {
            if let Some(date) = input.next.date.filter(|date| *date >= today) {
                out.no_report_expected_before = input.app_next.date.map(|app| app.min(date));
            }
        }
        answer!(No, "report_history_horizon");
    }
    answer!(Unknown, "next_quarter_ended_no_recent_check");
}

#[derive(Clone, Debug)]
pub struct ReportComingSoonOutcome {
    pub state: ReportComingSoonState,
    pub reasons: Vec<Reason>,
    pub report: ReportDate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportComingSoonState {
    Yes,
    No,
    Unknown,
    NotApplicable,
}

/// Preserve the selected report's source certainty, recomputing days from the input clock.
pub fn report_coming_soon(today: NaiveDate, next: &ReportDate) -> ReportComingSoonOutcome {
    use ReportComingSoonState::*;
    let mut report = next.clone();
    report.days_until = report.date.map(|date| (date - today).num_days());
    let state = match (next.certainty, report.days_until) {
        (ReportCertainty::NotApplicable, _) => NotApplicable,
        (_, Some(days)) if (REPORT_SOON_MIN_DAYS..=REPORT_SOON_MAX_DAYS).contains(&days) => Yes,
        (_, Some(_)) => No,
        (_, None) => Unknown,
    };
    let reasons = if state == Unknown && report.reasons.is_empty() {
        vec![Reason::new("next_report_unavailable")]
    } else {
        report.reasons.clone()
    };
    ReportComingSoonOutcome {
        state,
        reasons,
        report,
    }
}

#[cfg(test)]
mod tests;
