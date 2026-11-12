use chrono::NaiveDate;
use ticker_tape_tally_board_backend::{
    api::CompanyFigureInputs,
    domain::{
        report_dates::{EstimateBasis, Observation, ReportCertainty, ReportDate},
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
    pub fn from_loaded(today: NaiveDate, loaded: &'a CompanyFigureInputs) -> Self {
        Self {
            today,
            loaded,
            observations: &loaded.observations,
            earliest_first_seen: loaded.earliest_first_seen,
            last: loaded.last_report.clone(),
            app_next: loaded.app_next_report.clone(),
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
    use ticker_tape_tally_board_backend::domain::opportunity_evidence::{
        parameters::NoReportRule,
        report_timing::{report_in_period, ReportTimingInputs},
    };
    // This tool reports only company yes/no/unknown categories, as before.
    if let Some(reason) = input
        .loaded
        .source
        .blocking_reason(input.loaded.latest.is_some())
    {
        return answer("unknown", reason.code);
    }
    let outcome = report_in_period(
        &ReportTimingInputs {
            today: input.today,
            period_start: window.start_date(input.today),
            latest_period: input.loaded.latest,
            source: &input.loaded.source,
            last: &input.last,
            observations: input.observations,
            earliest_first_seen: input.earliest_first_seen,
            next: &input.loaded.next_report,
            app_next: &input.app_next,
        },
        if relaxed {
            NoReportRule::Relaxed
        } else {
            NoReportRule::Strict
        },
    );
    answer(outcome.state.as_str(), outcome.reasons[0].code)
}
