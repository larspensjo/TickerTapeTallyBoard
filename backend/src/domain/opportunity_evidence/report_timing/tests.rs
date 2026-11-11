use super::*;
use crate::domain::fundamentals::{Classification, LinkStatus};
use crate::domain::opportunity_evidence::test_support::*;

struct Case {
    source: SourceState,
    latest: NaiveDate,
    observations: Vec<Observation>,
    last: ReportDate,
    next: ReportDate,
    app_next: ReportDate,
}
impl Case {
    fn new() -> Self {
        Self {
            source: source(),
            latest: date("2026-09-30"),
            observations: vec![],
            last: report(None, None),
            next: report(None, None),
            app_next: report(None, None),
        }
    }
    fn observed(&mut self, first: &str, absent: Option<&str>) {
        self.observations = vec![Observation {
            period_end: self.latest,
            first_seen: date(first),
            absent_on: absent.map(date),
        }];
        // A display midpoint must never replace the observation interval.
        self.last.date = Some(date("2026-10-31"));
        self.last.certainty = ReportCertainty::PeriodOnly;
    }
    fn run(&self, rule: NoReportRule) -> ReportInPeriodOutcome {
        report_in_period(
            &ReportTimingInputs {
                today: today(),
                period_start: date("2026-10-12"),
                latest_period: Some(self.latest),
                source: &self.source,
                last: &self.last,
                observations: &self.observations,
                earliest_first_seen: self.observations.iter().map(|o| o.first_seen).min(),
                next: &self.next,
                app_next: &self.app_next,
            },
            rule,
        )
    }
}

#[test]
fn confirmed_dates_use_the_widened_window_and_include_today() {
    let mut c = Case::new();
    c.last.certainty = ReportCertainty::Confirmed;
    for (d, expected) in [
        ("2026-10-04", ReportInPeriodState::No),
        ("2026-10-05", ReportInPeriodState::Yes),
        ("2026-11-10", ReportInPeriodState::Yes),
        ("2026-11-11", ReportInPeriodState::Yes),
        ("2026-11-12", ReportInPeriodState::Unknown),
    ] {
        c.last.date = Some(date(d));
        for rule in [NoReportRule::Strict, NoReportRule::Relaxed] {
            let out = c.run(rule);
            assert_eq!(out.state, expected, "{d}");
            assert_eq!(out.window_start, date("2026-10-05"));
            assert_eq!(
                out.last_report,
                Some(LastReportEvidence::Confirmed { date: date(d) })
            );
        }
    }
}

#[test]
fn confirmed_report_today_with_figures_is_in_period() {
    let mut c = Case::new();
    c.last = report(Some(today()), Some(EstimateBasis::CompanyAnnounced));
    c.next = report(Some(date("2027-02-11")), Some(EstimateBasis::LastYearDate));
    for rule in [NoReportRule::Strict, NoReportRule::Relaxed] {
        let out = c.run(rule);
        assert_eq!(out.state, ReportInPeriodState::Yes);
        assert_eq!(out.reasons, vec![Reason::new("confirmed_report")]);
        assert!(out.scheduled_report.is_none());
        assert_eq!(
            report_coming_soon(today(), &c.next).state,
            ReportComingSoonState::No
        );
    }
}

#[test]
fn scheduled_report_today_without_figures_is_only_coming_soon() {
    let mut c = Case::new();
    c.observed("2026-10-01", None);
    for basis in [
        EstimateBasis::CompanyAnnounced,
        EstimateBasis::NasdaqExpected,
    ] {
        c.next = report(Some(today()), Some(basis));
        for rule in [NoReportRule::Strict, NoReportRule::Relaxed] {
            let out = c.run(rule);
            assert_eq!(out.state, ReportInPeriodState::No);
            assert_eq!(out.reasons, vec![Reason::new("strict_absence")]);
            assert!(out.scheduled_report.is_none());
            assert_eq!(
                report_coming_soon(today(), &c.next).state,
                ReportComingSoonState::Yes
            );
        }
    }
}

#[test]
fn relaxed_absence_sentence_uses_the_earlier_schedule_without_changing_the_rule() {
    let mut c = Case::new();
    c.latest = date("2026-06-30");
    c.observed("2026-08-01", None);
    c.app_next = report(
        Some(today() + Duration::days(14)),
        Some(EstimateBasis::LastYearDate),
    );
    for basis in [
        EstimateBasis::CompanyAnnounced,
        EstimateBasis::NasdaqExpected,
    ] {
        for days in [0, 7, 13, 14, 21] {
            c.next = report(Some(today() + Duration::days(days)), Some(basis));
            assert_eq!(
                c.run(NoReportRule::Strict).state,
                ReportInPeriodState::Unknown
            );
            let out = c.run(NoReportRule::Relaxed);
            assert_eq!(out.state, ReportInPeriodState::No);
            assert_eq!(out.reasons, vec![Reason::new("report_history_horizon")]);
            assert_eq!(
                out.no_report_expected_before,
                Some(today() + Duration::days(days.min(14)))
            );
        }
    }
}

#[test]
fn observations_retain_narrow_and_period_only_intervals_and_upper_bounds() {
    let mut c = Case::new();
    for (first, absent, expected, reason) in [
        (
            "2026-11-05",
            Some("2026-11-02"),
            ReportInPeriodState::Yes,
            "observed_report_inside_window",
        ),
        (
            "2026-11-05",
            Some("2026-10-05"),
            ReportInPeriodState::Yes,
            "observed_report_inside_window",
        ),
        (
            "2026-10-15",
            Some("2026-09-20"),
            ReportInPeriodState::Unknown,
            "observed_interval_overlaps_window",
        ),
        (
            "2026-11-05",
            None,
            ReportInPeriodState::Unknown,
            "first_fetch_without_baseline",
        ),
        (
            "2026-10-04",
            None,
            ReportInPeriodState::No,
            "strict_absence",
        ),
        (
            "2026-11-12",
            Some("2026-11-10"),
            ReportInPeriodState::Unknown,
            "observation_after_as_of",
        ),
    ] {
        c.observed(first, absent);
        for rule in [NoReportRule::Strict, NoReportRule::Relaxed] {
            let out = c.run(rule);
            assert_eq!(out.state, expected);
            assert_eq!(out.reasons[0].code, reason);
            assert_eq!(
                out.last_report,
                Some(match absent {
                    Some(a) => LastReportEvidence::Interval {
                        period_end: c.latest,
                        absent_on: date(a),
                        first_seen: date(first)
                    },
                    None => LastReportEvidence::UpperBound {
                        period_end: c.latest,
                        first_seen: date(first)
                    },
                })
            );
        }
    }
}

#[test]
fn passed_schedules_preserve_source_certainty_and_estimates_are_unknown() {
    let mut c = Case::new();
    c.observed("2026-10-01", None);
    for basis in [
        EstimateBasis::CompanyAnnounced,
        EstimateBasis::NasdaqExpected,
        EstimateBasis::LastYearDate,
        EstimateBasis::TypicalGap,
    ] {
        c.next = report(Some(date("2026-11-10")), Some(basis));
        let expected = if matches!(
            basis,
            EstimateBasis::CompanyAnnounced | EstimateBasis::NasdaqExpected
        ) {
            ReportInPeriodState::Yes
        } else {
            ReportInPeriodState::Unknown
        };
        let out = c.run(NoReportRule::Relaxed);
        assert_eq!(out.state, expected);
        let schedule = out.scheduled_report.unwrap();
        assert_eq!(schedule.basis, Some(basis));
        assert_eq!(schedule.certainty, c.next.certainty);
        c.next.date = Some(today());
        assert_eq!(c.run(NoReportRule::Relaxed).state, ReportInPeriodState::No);
    }
}

#[test]
fn absence_requires_unended_quarter_fresh_check_or_relaxed_history_horizon() {
    let mut c = Case::new();
    c.observed("2026-10-01", None);
    assert_eq!(c.run(NoReportRule::Strict).state, ReportInPeriodState::No);
    c.latest = date("2026-06-30");
    c.observed("2026-08-01", None);
    for rule in [NoReportRule::Strict, NoReportRule::Relaxed] {
        for (checked, expected) in [
            ("2026-11-09", ReportInPeriodState::Unknown),
            ("2026-11-10", ReportInPeriodState::No),
            ("2026-11-11", ReportInPeriodState::No),
            ("2026-11-12", ReportInPeriodState::Unknown),
        ] {
            c.source.last_success = Some(date(checked));
            assert_eq!(c.run(rule).state, expected);
        }
    }
    c.source.last_success = None;
    for (days, basis, relaxed_expected) in [
        (
            13,
            EstimateBasis::LastYearDate,
            ReportInPeriodState::Unknown,
        ),
        (14, EstimateBasis::LastYearDate, ReportInPeriodState::No),
        (14, EstimateBasis::TypicalGap, ReportInPeriodState::Unknown),
    ] {
        c.app_next = report(Some(today() + Duration::days(days)), Some(basis));
        assert_eq!(
            c.run(NoReportRule::Strict).state,
            ReportInPeriodState::Unknown
        );
        let out = c.run(NoReportRule::Relaxed);
        assert_eq!(out.state, relaxed_expected);
        assert_eq!(
            out.no_report_expected_before,
            (relaxed_expected == ReportInPeriodState::No).then_some(date("2026-11-25"))
        );
    }
}

#[test]
fn applicability_and_source_reasons_are_explicit() {
    let mut c = Case::new();
    for classification in [
        Classification::Fund,
        Classification::Certificate,
        Classification::NotCompany,
    ] {
        c.source.classification = classification;
        assert_eq!(
            c.run(NO_REPORT_RULE).state,
            ReportInPeriodState::NotApplicable
        );
    }
    c.source.classification = Classification::Unknown;
    assert_eq!(
        c.run(NO_REPORT_RULE).reasons[0].code,
        "security_type_unknown"
    );
    c.source.classification = Classification::Equity;
    c.source.awaiting_figures = true;
    assert_eq!(c.run(NO_REPORT_RULE).reasons[0].code, "awaiting_figures");
    c.source.awaiting_figures = false;
    c.source.status = LinkStatus::Ambiguous;
    let result = report_in_period(
        &ReportTimingInputs {
            today: today(),
            period_start: today(),
            latest_period: None,
            source: &c.source,
            last: &c.last,
            observations: &[],
            earliest_first_seen: None,
            next: &c.next,
            app_next: &c.app_next,
        },
        NO_REPORT_RULE,
    );
    assert_eq!(result.reasons[0].code, "source_ambiguous");
}

#[test]
fn coming_soon_includes_today_and_preserves_all_certainty_labels() {
    for basis in [
        EstimateBasis::CompanyAnnounced,
        EstimateBasis::NasdaqExpected,
        EstimateBasis::LastYearDate,
        EstimateBasis::TypicalGap,
    ] {
        for (days, expected) in [
            (-1, ReportComingSoonState::No),
            (0, ReportComingSoonState::Yes),
            (7, ReportComingSoonState::Yes),
            (8, ReportComingSoonState::No),
        ] {
            let next = report(Some(today() + Duration::days(days)), Some(basis));
            let out = report_coming_soon(today(), &next);
            assert_eq!(out.state, expected);
            assert_eq!(out.report.days_until, Some(days));
            assert_eq!(out.report.certainty, next.certainty);
            assert_eq!(out.report.basis, Some(basis));
        }
    }
    let mut next = report(None, None);
    assert_eq!(
        report_coming_soon(today(), &next).state,
        ReportComingSoonState::Unknown
    );
    next.certainty = ReportCertainty::NotApplicable;
    assert_eq!(
        report_coming_soon(today(), &next).state,
        ReportComingSoonState::NotApplicable
    );
}
