use chrono::{Duration, NaiveDate};
use ticker_tape_tally_board_backend::{
    api::CompanyFigureInputs,
    domain::{
        report_dates::{
            self, Announcement, EstimateBasis, Observation, ReportCertainty, ReportDate,
        },
        RecentWindow,
    },
};

pub const KNOWLEDGE_LABELS: &[&str] = &[
    "confirmed",
    "observed_at_most_7_days",
    "observed_wider",
    "first_fetch_without_baseline",
    "figures_without_absence_baseline",
    "unavailable",
];

pub struct ReportInputs<'a> {
    pub today: NaiveDate,
    pub loaded: &'a CompanyFigureInputs,
    pub observations: &'a [Observation],
    pub earliest_first_seen: Option<NaiveDate>,
    pub last: ReportDate,
    pub app_next: ReportDate,
}
impl<'a> ReportInputs<'a> {
    pub fn new(
        today: NaiveDate,
        loaded: &'a CompanyFigureInputs,
        observations: &'a [Observation],
        announcements: &[Announcement],
    ) -> Self {
        let (_, last) = report_dates::report_dates(
            today,
            loaded.latest,
            &loaded.annual,
            observations,
            &loaded.source,
        );
        let last = report_dates::confirmed_last(today, last, announcements, observations);
        let history = report_dates::real_report_history(announcements, observations);
        let app_next = report_dates::next_report(
            today,
            loaded.latest,
            &loaded.annual,
            &history,
            &loaded.source,
        );
        Self {
            today,
            loaded,
            observations,
            earliest_first_seen: observations.iter().map(|o| o.first_seen).min(),
            last,
            app_next,
        }
    }
    pub fn first_fetch_without_baseline(&self) -> bool {
        self.latest_observation().is_some_and(|latest| {
            latest.absent_on.is_none() && self.earliest_first_seen == Some(latest.first_seen)
        })
    }
    fn no_baseline_reason(&self) -> &'static str {
        if self.first_fetch_without_baseline() {
            "first_fetch_without_baseline"
        } else {
            "figures_without_absence_baseline"
        }
    }
    fn latest_observation(&self) -> Option<&Observation> {
        self.observations
            .iter()
            .find(|o| Some(o.period_end) == self.loaded.latest)
    }
}

pub fn knowledge(input: &ReportInputs<'_>) -> &'static str {
    if input.last.certainty == ReportCertainty::Confirmed {
        return "confirmed";
    }
    if input.last.certainty == ReportCertainty::Unavailable {
        return "unavailable";
    }
    match input.latest_observation() {
        Some(Observation {
            absent_on: Some(start),
            first_seen,
            ..
        }) if (0..=7).contains(&(*first_seen - *start).num_days()) => "observed_at_most_7_days",
        Some(Observation {
            absent_on: Some(_), ..
        }) => "observed_wider",
        Some(_) => input.no_baseline_reason(),
        None => "unavailable",
    }
}

pub fn basis(report: &ReportDate) -> &'static str {
    match report.basis {
        Some(EstimateBasis::CompanyAnnounced) => "company_announced",
        Some(EstimateBasis::NasdaqExpected) => "nasdaq_expected",
        Some(EstimateBasis::LastYearDate) => "last_year_date",
        Some(EstimateBasis::TypicalGap) => "typical_gap",
        None => "unavailable",
    }
}

pub struct Outcome {
    pub state: &'static str,
    pub reason: &'static str,
}
fn answer(state: &'static str, reason: &'static str) -> Outcome {
    Outcome { state, reason }
}

pub fn in_period(input: &ReportInputs<'_>, window: RecentWindow, relaxed: bool) -> Outcome {
    let today = input.today;
    let start = window.start_date(today) - Duration::days(7);
    if let Some(reason) = input
        .loaded
        .source
        .blocking_reason(input.loaded.latest.is_some())
    {
        return answer("unknown", reason.code);
    }
    let mut uncertainty = None;
    let mut upper = None;
    if input.last.certainty == ReportCertainty::Confirmed {
        if let Some(date) = input.last.date {
            if date >= start && date < today {
                return answer("yes", "confirmed_report");
            }
            if date == today {
                uncertainty = Some("report_today");
            }
            upper = Some(date);
        }
    } else if let Some(observation) = input.latest_observation() {
        upper = Some(observation.first_seen);
        if observation.first_seen >= start && observation.first_seen <= today {
            match observation.absent_on {
                Some(absent) if absent >= start && absent <= observation.first_seen => {
                    return answer("yes", "observed_report_inside_window")
                }
                Some(_) => uncertainty = Some("observed_interval_overlaps_window"),
                None => uncertainty = Some(input.no_baseline_reason()),
            }
        } else if observation.first_seen > today {
            uncertainty = Some("observation_after_as_of");
        }
    }
    let next = &input.loaded.next_report;
    if next.date.is_some_and(|date| date >= start && date < today) {
        if matches!(
            next.basis,
            Some(EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected)
        ) {
            return answer("yes", "scheduled_report_figures_not_seen");
        }
        uncertainty = Some("app_estimate_passed_in_window");
    }
    if let Some(reason) = uncertainty {
        return answer("unknown", reason);
    }
    if !upper.is_some_and(|date| date < start) {
        return answer("unknown", "last_report_upper_bound_unavailable");
    }
    let quarter_not_ended =
        report_dates::implied_quarter(input.loaded.latest).is_some_and(|date| date > today);
    let fresh_check = input
        .loaded
        .source
        .last_success
        .is_some_and(|date| date >= today - Duration::days(1) && date <= today);
    if quarter_not_ended || fresh_check {
        return answer("no", "strict_absence");
    }
    if relaxed
        && input.app_next.basis == Some(EstimateBasis::LastYearDate)
        && input
            .app_next
            .date
            .is_some_and(|date| date >= today + Duration::days(14))
    {
        return answer("no", "report_history_horizon");
    }
    answer("unknown", "next_quarter_ended_no_recent_check")
}
