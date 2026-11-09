use super::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

macro_rules! codes {
    ($name:ident { $($variant:ident => $code:literal),+ }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $name { $(#[serde(rename = $code)] $variant),+ }
        impl $name {
            pub fn as_str(self) -> &'static str { match self { $(Self::$variant => $code),+ } }
            pub fn from_db_str(s: &str) -> Option<Self> {
                match s { $($code => Some(Self::$variant)),+, _ => None }
            }
        }
    };
}
codes!(ReportCalendarSource { NasdaqUs => "NASDAQ_US" });
codes!(AnnouncementKind { CompanyAnnounced => "company_announced", Expected => "expected" });
codes!(AnnouncementState { Active => "active", Superseded => "superseded", Withdrawn => "withdrawn", Resolved => "resolved" });
codes!(ReportSession { BeforeMarketOpen => "before_market_open", AfterMarketClose => "after_market_close" });
codes!(LookupOutcome { Dated => "dated", None => "none", UnknownSymbol => "unknown_symbol", Failed => "failed" });

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnouncementDate {
    pub date: NaiveDate,
    pub kind: AnnouncementKind,
    pub fiscal_quarter: Option<NaiveDate>,
    pub session: Option<ReportSession>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolReportDate {
    Dated(AnnouncementDate),
    None,
    UnknownSymbol,
}
impl SymbolReportDate {
    pub fn outcome(&self) -> LookupOutcome {
        match self {
            Self::Dated(_) => LookupOutcome::Dated,
            Self::None => LookupOutcome::None,
            Self::UnknownSymbol => LookupOutcome::UnknownSymbol,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarSighting {
    pub symbol: String,
    pub report: AnnouncementDate,
    /// Report-day, past-day and results rows prove presence only.
    pub presence_only: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub id: i64,
    pub instrument_id: i64,
    pub source: ReportCalendarSource,
    pub symbol: String,
    pub report: AnnouncementDate,
    pub first_seen: NaiveDate,
    pub last_seen: NaiveDate,
    /// The latest quarter already known when this announcement was first seen.
    pub baseline_period_end: Option<NaiveDate>,
    pub state: AnnouncementState,
    pub resolved_period_end: Option<NaiveDate>,
    pub superseded_by: Option<i64>,
}
#[derive(Clone, Debug, Default)]
pub struct LookupState {
    pub last_lookup_day: Option<NaiveDate>,
    pub last_success: Option<NaiveDate>,
    pub last_outcome: Option<LookupOutcome>,
}

/// The caller supplies only an enabled Yahoo price symbol. A figures fallback
/// is eligible only for a USD listing. Suffixes are never stripped for matching.
pub fn us_ticker<'a>(
    yahoo: Option<&'a str>,
    figures: Option<&'a str>,
    currency: &str,
) -> Option<&'a str> {
    let valid =
        |s: &&str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-');
    yahoo.filter(valid).or_else(|| {
        (currency == "USD")
            .then_some(figures)
            .flatten()
            .filter(valid)
    })
}

pub fn lookup_due(today: NaiveDate, state: &LookupState, entries: &[Announcement]) -> bool {
    if state.last_lookup_day.is_some_and(|d| d >= today) {
        return false;
    }
    let active: Vec<_> = entries
        .iter()
        .filter(|a| a.state == AnnouncementState::Active)
        .collect();
    active.is_empty()
        || active
            .iter()
            .any(|a| (a.report.date - today).num_days() <= 7)
        || state
            .last_success
            .is_none_or(|d| (today - d).num_days() > 3)
}

/// No horizon or backfill: only days of stored unresolved announcements.
pub fn scan_days(
    today: NaiveDate,
    entries: &[Announcement],
    answer: Option<&SymbolReportDate>,
    scanned_today: &BTreeSet<NaiveDate>,
) -> BTreeSet<NaiveDate> {
    entries
        .iter()
        .filter(|a| a.state == AnnouncementState::Active)
        .filter(|a| {
            a.report.date <= today
                || matches!(
                    answer,
                    Some(SymbolReportDate::None | SymbolReportDate::UnknownSymbol)
                )
        })
        .map(|a| a.report.date)
        .filter(|d| !scanned_today.contains(d))
        .collect()
}

fn same_quarter(a: NaiveDate, b: NaiveDate) -> bool {
    a.year() == b.year() && a.month() == b.month()
}

/// The same month-end grid used by app estimates, including 52/53-week reporters.
pub fn implied_quarter(baseline: Option<NaiveDate>) -> Option<NaiveDate> {
    month_end(baseline?.checked_add_months(Months::new(3))?)
}

fn quarter(entry: &Announcement) -> Option<NaiveDate> {
    entry
        .report
        .fiscal_quarter
        .or_else(|| implied_quarter(entry.baseline_period_end))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnouncementChange {
    Seen { id: i64, report: AnnouncementDate },
    Insert(AnnouncementDate),
    Supersede { id: i64 },
    Withdraw { id: i64 },
    Resolve { id: i64, period_end: NaiveDate },
}

/// Only dated evidence can move an announcement. A missing symbol answer has
/// no reconciliation effects. A different fiscal quarter is a separate report.
pub fn reconcile_date(
    entries: &[Announcement],
    report: &AnnouncementDate,
    presence_only: bool,
    today: NaiveDate,
    baseline: Option<NaiveDate>,
) -> Vec<AnnouncementChange> {
    let incoming_quarter = report.fiscal_quarter.or_else(|| implied_quarter(baseline));
    if entries.iter().any(|a| {
        a.state == AnnouncementState::Resolved
            && a.report.date == report.date
            && a.report
                .fiscal_quarter
                .zip(report.fiscal_quarter)
                .is_none_or(|(a, b)| same_quarter(a, b))
    }) {
        return vec![];
    }
    let matching = entries.iter().find(|a| {
        a.state == AnnouncementState::Active
            && a.report.date == report.date
            && (a.report.fiscal_quarter.is_none()
                || report.fiscal_quarter.is_none()
                || a.report
                    .fiscal_quarter
                    .zip(report.fiscal_quarter)
                    .is_some_and(|(a, b)| same_quarter(a, b)))
    });
    let mut changes: Vec<_> = entries
        .iter()
        .filter(|a| {
            !presence_only
                && a.state == AnnouncementState::Active
                && a.report.date != report.date
                && (quarter(a)
                    .zip(incoming_quarter)
                    .is_some_and(|(a, b)| same_quarter(a, b))
                    || (a.report.fiscal_quarter.is_none() && a.report.date >= today))
        })
        .map(|a| AnnouncementChange::Supersede { id: a.id })
        .collect();
    if let Some(a) = matching {
        let mut updated = if presence_only {
            a.report.clone()
        } else {
            report.clone()
        };
        updated.fiscal_quarter = updated
            .fiscal_quarter
            .or(report.fiscal_quarter)
            .or(a.report.fiscal_quarter);
        changes.push(AnnouncementChange::Seen {
            id: a.id,
            report: updated,
        });
        return changes;
    }
    // Past calendar results are solely a presence check, never a new schedule.
    if presence_only {
        return vec![];
    }
    changes.push(AnnouncementChange::Insert(report.clone()));
    changes
}

pub fn reconcile_absence(
    entries: &[Announcement],
    day: NaiveDate,
    sightings: &[CalendarSighting],
    symbol: &str,
) -> Vec<AnnouncementChange> {
    entries
        .iter()
        .filter(|a| a.state == AnnouncementState::Active && a.report.date == day)
        .filter(|a| {
            !sightings.iter().any(|s| {
                s.symbol.eq_ignore_ascii_case(symbol)
                    && s.report.date == day
                    && a.report
                        .fiscal_quarter
                        .zip(s.report.fiscal_quarter)
                        .is_none_or(|(a, b)| same_quarter(a, b))
            })
        })
        .map(|a| AnnouncementChange::Withdraw { id: a.id })
        .collect()
}

pub fn resolve_announcements(
    entries: &[Announcement],
    observations: &[Observation],
) -> Vec<AnnouncementChange> {
    entries
        .iter()
        .filter(|a| a.state == AnnouncementState::Active)
        .filter_map(|a| {
            let expected = quarter(a);
            let found = observations
                .iter()
                .filter(|o| match expected {
                    Some(q) => same_quarter(q, o.period_end),
                    None => {
                        o.first_seen >= a.report.date
                            && o.first_seen >= a.first_seen
                            && a.baseline_period_end.is_none_or(|b| o.period_end > b)
                    }
                })
                .min_by_key(|o| (o.first_seen, o.period_end));
            if let Some(found) = found {
                Some(AnnouncementChange::Resolve {
                    id: a.id,
                    period_end: found.period_end,
                })
            } else if expected.is_some_and(|q| {
                observations
                    .iter()
                    .any(|o| o.period_end > q && !same_quarter(o.period_end, q))
            }) {
                // A skipped quarter must not pin next-report or claim a report date
                // for the later quarter that actually arrived.
                Some(AnnouncementChange::Supersede { id: a.id })
            } else {
                None
            }
        })
        .collect()
}

pub fn real_report_history(
    entries: &[Announcement],
    observations: &[Observation],
) -> Vec<(NaiveDate, NaiveDate)> {
    let mut real: Vec<_> = observations
        .iter()
        .filter_map(|o| observed_report(o.first_seen, o.absent_on).map(|d| (o.period_end, d)))
        .collect();
    // Confirmed schedules win over an observed window for the same quarter.
    for a in entries.iter().filter(|a| {
        a.state == AnnouncementState::Resolved
            && a.report.kind == AnnouncementKind::CompanyAnnounced
    }) {
        if let Some(end) = a.resolved_period_end.filter(|end| {
            observations
                .iter()
                .any(|o| o.period_end == *end && o.first_seen >= a.report.date)
        }) {
            real.retain(|(p, _)| *p != end);
            real.push((end, a.report.date));
        }
    }
    real
}

pub fn scheduled_next(
    today: NaiveDate,
    fallback: ReportDate,
    entries: &[Announcement],
) -> ReportDate {
    if fallback.certainty == ReportCertainty::NotApplicable {
        return fallback;
    }
    let Some(a) = entries
        .iter()
        .filter(|a| a.state == AnnouncementState::Active)
        .min_by_key(|a| {
            (
                a.report.date,
                a.report.kind != AnnouncementKind::CompanyAnnounced,
                a.id,
            )
        })
    else {
        return fallback;
    };
    let confirmed = a.report.kind == AnnouncementKind::CompanyAnnounced;
    ReportDate {
        certainty: if confirmed {
            ReportCertainty::Confirmed
        } else {
            ReportCertainty::Estimated
        },
        basis: Some(if confirmed {
            EstimateBasis::CompanyAnnounced
        } else {
            EstimateBasis::NasdaqExpected
        }),
        date: Some(a.report.date),
        days_until: Some((a.report.date - today).num_days()),
        period_end: a.report.fiscal_quarter,
        observed_between: None,
        reasons: if a.report.date < today {
            vec![Reason::new(if confirmed {
                "report_date_passed"
            } else {
                "estimate_passed"
            })]
        } else {
            vec![]
        },
    }
}

pub fn confirmed_last(
    today: NaiveDate,
    fallback: ReportDate,
    entries: &[Announcement],
    observations: &[Observation],
) -> ReportDate {
    if let Some(a) = entries
        .iter()
        .filter(|a| {
            a.state == AnnouncementState::Resolved
                && a.report.kind == AnnouncementKind::CompanyAnnounced
                && a.resolved_period_end == fallback.period_end
                && observations.iter().any(|o| {
                    Some(o.period_end) == a.resolved_period_end && o.first_seen >= a.report.date
                })
        })
        .max_by_key(|a| a.report.date)
    {
        return ReportDate {
            certainty: ReportCertainty::Confirmed,
            basis: Some(EstimateBasis::CompanyAnnounced),
            date: Some(a.report.date),
            days_until: Some((a.report.date - today).num_days()),
            period_end: a.resolved_period_end,
            observed_between: None,
            reasons: vec![],
        };
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }
    fn entry(date: &str, kind: AnnouncementKind, quarter: Option<&str>) -> Announcement {
        Announcement {
            id: 1,
            instrument_id: 7,
            source: ReportCalendarSource::NasdaqUs,
            symbol: "TEST".into(),
            report: AnnouncementDate {
                date: d(date),
                kind,
                fiscal_quarter: quarter.map(d),
                session: None,
            },
            first_seen: d("2026-10-01"),
            last_seen: d("2026-10-01"),
            baseline_period_end: Some(d("2026-06-30")),
            state: AnnouncementState::Active,
            resolved_period_end: None,
            superseded_by: None,
        }
    }
    fn source() -> SourceState {
        use crate::domain::fundamentals::{Classification, LinkStatus};
        SourceState {
            classification: Classification::Equity,
            status: LinkStatus::Usable,
            awaiting_figures: false,
            unreachable: false,
            last_success: None,
        }
    }
    #[test]
    fn matching_uses_enabled_suffix_free_price_or_usd_figures() {
        assert_eq!(us_ticker(Some("MSFT"), None, "USD"), Some("MSFT"));
        assert_eq!(us_ticker(Some("EVO.ST"), None, "SEK"), None);
        assert_eq!(us_ticker(None, Some("TSM"), "USD"), Some("TSM"));
        assert_eq!(us_ticker(None, Some("MSFT"), "SEK"), None);
        assert_eq!(
            us_ticker(Some("MSFT.DE"), Some("MSFT"), "USD"),
            Some("MSFT")
        );
        assert_eq!(us_ticker(Some("BRK-B"), None, "USD"), Some("BRK-B"));
        for symbol in ["", "MSFT.ST", "^GSPC", " ABC", "ABC/DEF"] {
            assert_eq!(us_ticker(Some(symbol), None, "USD"), None);
        }
    }
    #[test]
    fn lookups_continue_for_confirmed_dates_with_daily_and_age_limits() {
        let today = d("2026-11-09");
        let mut state = LookupState::default();
        let mut entries = vec![entry(
            "2026-12-02",
            AnnouncementKind::CompanyAnnounced,
            Some("2026-10-31"),
        )];
        assert!(lookup_due(today, &state, &entries));
        state.last_lookup_day = Some(today);
        assert!(!lookup_due(today, &state, &[]));
        state.last_lookup_day = Some(today - Duration::days(1));
        state.last_success = Some(today - Duration::days(3));
        assert!(!lookup_due(today, &state, &entries));
        state.last_success = Some(today - Duration::days(4));
        assert!(lookup_due(today, &state, &entries));
        state.last_success = Some(today - Duration::days(1));
        entries[0].report.date = today + Duration::days(7);
        assert!(lookup_due(today, &state, &entries));
        entries[0].report.date = today - Duration::days(8);
        assert!(lookup_due(today, &state, &entries));
        assert!(lookup_due(today, &state, &[]));
    }
    #[test]
    fn scans_only_stored_days_not_already_attempted_today() {
        let today = d("2026-11-09");
        let past = entry("2026-11-07", AnnouncementKind::CompanyAnnounced, None);
        let future = entry("2026-12-02", AnnouncementKind::Expected, None);
        let entries = vec![past, future];
        assert_eq!(
            scan_days(today, &entries, None, &BTreeSet::new()),
            BTreeSet::from([d("2026-11-07")])
        );
        for answer in [SymbolReportDate::None, SymbolReportDate::UnknownSymbol] {
            assert_eq!(
                scan_days(today, &entries, Some(&answer), &BTreeSet::new()).len(),
                2
            );
            assert!(scan_days(
                today,
                &entries,
                Some(&answer),
                &BTreeSet::from([d("2026-11-07"), d("2026-12-02")])
            )
            .is_empty());
        }
        assert!(scan_days(today, &[], Some(&SymbolReportDate::None), &BTreeSet::new()).is_empty());
    }
    #[test]
    fn moves_require_same_quarter_marks_follow_fresh_sightings_and_absence_withdraws() {
        let old = entry("2026-12-02", AnnouncementKind::Expected, Some("2026-10-31"));
        let mut moved = old.report.clone();
        moved.date = d("2026-12-03");
        assert_eq!(
            reconcile_date(
                std::slice::from_ref(&old),
                &moved,
                false,
                d("2026-11-09"),
                old.baseline_period_end
            ),
            vec![
                AnnouncementChange::Supersede { id: 1 },
                AnnouncementChange::Insert(moved.clone())
            ]
        );
        moved.fiscal_quarter = Some(d("2027-01-31"));
        assert_eq!(
            reconcile_date(
                std::slice::from_ref(&old),
                &moved,
                false,
                d("2026-11-09"),
                old.baseline_period_end
            ),
            vec![AnnouncementChange::Insert(moved)]
        );
        let mut marked = old.report.clone();
        marked.kind = AnnouncementKind::CompanyAnnounced;
        assert_eq!(
            reconcile_date(
                std::slice::from_ref(&old),
                &marked,
                false,
                d("2026-11-09"),
                old.baseline_period_end
            ),
            vec![AnnouncementChange::Seen {
                id: 1,
                report: marked.clone()
            }]
        );
        let confirmed = Announcement {
            report: marked,
            ..old.clone()
        };
        assert_eq!(
            reconcile_date(
                std::slice::from_ref(&confirmed),
                &old.report,
                true,
                d("2026-11-09"),
                old.baseline_period_end
            ),
            vec![AnnouncementChange::Seen {
                id: 1,
                report: confirmed.report.clone()
            }]
        );
        assert_eq!(
            reconcile_absence(
                std::slice::from_ref(&confirmed),
                old.report.date,
                &[],
                "TEST"
            ),
            vec![AnnouncementChange::Withdraw { id: 1 }]
        );
        let sighting = CalendarSighting {
            symbol: "TEST".into(),
            report: old.report.clone(),
            presence_only: true,
        };
        assert!(reconcile_absence(&[confirmed], old.report.date, &[sighting], "TEST").is_empty());
    }
    #[test]
    fn resolution_matches_fiscal_month_or_first_new_quarter_without_using_old_quarters() {
        let mut a = entry(
            "2026-11-02",
            AnnouncementKind::CompanyAnnounced,
            Some("2026-09-30"),
        );
        let observations = vec![
            Observation {
                period_end: d("2026-06-30"),
                first_seen: d("2026-11-03"),
                absent_on: None,
            },
            Observation {
                period_end: d("2026-09-30"),
                first_seen: d("2026-11-04"),
                absent_on: None,
            },
        ];
        assert_eq!(
            resolve_announcements(std::slice::from_ref(&a), &observations),
            vec![AnnouncementChange::Resolve {
                id: 1,
                period_end: d("2026-09-30")
            }]
        );
        a.report.fiscal_quarter = None;
        assert_eq!(
            resolve_announcements(std::slice::from_ref(&a), &observations),
            vec![AnnouncementChange::Resolve {
                id: 1,
                period_end: d("2026-09-30")
            }]
        );
        assert!(resolve_announcements(&[a], &observations[..1]).is_empty());
    }
    #[test]
    fn selection_retains_overdue_dates_and_expected_never_confirms_last() {
        let today = d("2026-11-09");
        let latest = Some(d("2026-06-30"));
        let base = || next_report(today, latest, &[], &[], &source());
        let confirmed = entry(
            "2026-11-02",
            AnnouncementKind::CompanyAnnounced,
            Some("2026-09-30"),
        );
        let next = scheduled_next(today, base(), std::slice::from_ref(&confirmed));
        assert_eq!(next.certainty, ReportCertainty::Confirmed);
        assert_eq!(next.reasons[0].code, "report_date_passed");
        for offset in [1, 7, 21, 22, 90] {
            let now = d("2026-11-02") + Duration::days(offset);
            assert_eq!(
                fetch_decision(
                    now,
                    &FetchTiming {
                        last_attempt: Some(now - Duration::days(1)),
                        last_success: Some(now - Duration::days(1)),
                        previous_failed: false,
                        next_report: next.date,
                        unresolved_announcement: true,
                        newer_quarter_seen_on: None
                    }
                ),
                FetchDecision::Due
            );
        }
        let mut expected = confirmed.clone();
        expected.report.kind = AnnouncementKind::Expected;
        let next = scheduled_next(today, base(), std::slice::from_ref(&expected));
        assert_eq!(next.basis, Some(EstimateBasis::NasdaqExpected));
        assert_eq!(next.reasons[0].code, "estimate_passed");
        let next = scheduled_next(today, base(), &[expected.clone(), confirmed.clone()]);
        assert_eq!(next.certainty, ReportCertainty::Confirmed);
        expected.report.date = d("2026-11-01");
        assert_eq!(
            scheduled_next(today, base(), &[confirmed.clone(), expected.clone()]).date,
            Some(d("2026-11-01"))
        );
        let observations = [Observation {
            period_end: d("2026-09-30"),
            first_seen: d("2026-11-03"),
            absent_on: None,
        }];
        let (_, last) = report_dates(today, Some(d("2026-09-30")), &[], &observations, &source());
        expected.state = AnnouncementState::Resolved;
        expected.resolved_period_end = Some(d("2026-09-30"));
        assert_eq!(
            confirmed_last(today, last, std::slice::from_ref(&expected), &observations).certainty,
            ReportCertainty::PeriodOnly
        );
        assert!(real_report_history(&[expected], &observations).is_empty());
        let mut confirmed = confirmed;
        confirmed.state = AnnouncementState::Resolved;
        confirmed.resolved_period_end = Some(d("2026-09-30"));
        let (_, last) = report_dates(today, Some(d("2026-09-30")), &[], &observations, &source());
        assert_eq!(
            confirmed_last(today, last, std::slice::from_ref(&confirmed), &observations).certainty,
            ReportCertainty::Confirmed
        );
        let real = real_report_history(&[confirmed], &observations);
        let estimate = app_estimate(Some(d("2027-06-30")), &[], &real).unwrap();
        assert_eq!(estimate.basis, EstimateBasis::LastYearDate);
        assert_eq!(estimate.date, d("2027-11-01"));
    }

    #[test]
    fn quarterless_algorithmic_dates_move_by_implied_quarter_and_unknown_future_safety_net() {
        let mut old = entry("2026-12-02", AnnouncementKind::Expected, None);
        old.baseline_period_end = Some(d("2026-07-31"));
        let announced = AnnouncementDate {
            date: d("2026-12-04"),
            kind: AnnouncementKind::CompanyAnnounced,
            fiscal_quarter: Some(d("2026-10-31")),
            session: Some(ReportSession::AfterMarketClose),
        };
        // Even after the old date has passed, its implied quarter matches.
        for today in [d("2026-11-18"), d("2026-12-03")] {
            assert_eq!(
                reconcile_date(
                    std::slice::from_ref(&old),
                    &announced,
                    false,
                    today,
                    old.baseline_period_end
                ),
                vec![
                    AnnouncementChange::Supersede { id: 1 },
                    AnnouncementChange::Insert(announced.clone())
                ]
            );
        }
        let revised = AnnouncementDate {
            fiscal_quarter: None,
            kind: AnnouncementKind::Expected,
            ..announced.clone()
        };
        assert!(matches!(
            reconcile_date(
                std::slice::from_ref(&old),
                &revised,
                false,
                d("2026-12-03"),
                old.baseline_period_end
            )[0],
            AnnouncementChange::Supersede { id: 1 }
        ));
        // The safety net also covers absent or imperfect baseline evidence, but
        // does not remove an already-passed date for an unrelated quarter.
        for baseline in [None, Some(d("2026-06-30"))] {
            old.baseline_period_end = baseline;
            assert_eq!(
                reconcile_date(
                    std::slice::from_ref(&old),
                    &announced,
                    false,
                    d("2026-11-18"),
                    baseline
                )
                .len(),
                2
            );
            assert_eq!(
                reconcile_date(
                    std::slice::from_ref(&old),
                    &announced,
                    false,
                    d("2026-12-03"),
                    baseline
                ),
                vec![AnnouncementChange::Insert(announced.clone())]
            );
        }
        assert_eq!(
            implied_quarter(Some(d("2026-07-25"))),
            Some(d("2026-10-31"))
        );
    }

    #[test]
    fn skipped_explicit_or_implied_quarters_close_without_confirming_later_reports() {
        for explicit in [None, Some("2026-09-30")] {
            let old = entry("2026-11-02", AnnouncementKind::CompanyAnnounced, explicit);
            let observations = [Observation {
                period_end: d("2026-12-31"),
                first_seen: d("2027-02-01"),
                absent_on: None,
            }];
            assert_eq!(
                resolve_announcements(std::slice::from_ref(&old), &observations),
                vec![AnnouncementChange::Supersede { id: 1 }]
            );
            let closed = Announcement {
                state: AnnouncementState::Superseded,
                ..old
            };
            assert!(real_report_history(std::slice::from_ref(&closed), &observations).is_empty());
            let (_, last) = report_dates(
                d("2027-02-01"),
                Some(d("2026-12-31")),
                &[],
                &observations,
                &source(),
            );
            assert_eq!(
                confirmed_last(d("2027-02-01"), last, &[closed], &observations).certainty,
                ReportCertainty::PeriodOnly
            );
        }
        // Early figures resolve a future algorithmic date through its implied quarter.
        let old = entry("2026-12-02", AnnouncementKind::Expected, None);
        assert_eq!(
            resolve_announcements(
                &[old],
                &[Observation {
                    period_end: d("2026-09-30"),
                    first_seen: d("2026-11-30"),
                    absent_on: None
                }]
            ),
            vec![AnnouncementChange::Resolve {
                id: 1,
                period_end: d("2026-09-30")
            }]
        );
    }

    #[test]
    fn presence_sightings_fill_quarter_without_changing_the_scheduled_kind_or_session() {
        let mut old = entry("2026-11-02", AnnouncementKind::CompanyAnnounced, None);
        old.report.session = Some(ReportSession::AfterMarketClose);
        let sighting = AnnouncementDate {
            kind: AnnouncementKind::Expected,
            fiscal_quarter: Some(d("2026-09-30")),
            session: None,
            ..old.report.clone()
        };
        let expected = AnnouncementDate {
            fiscal_quarter: sighting.fiscal_quarter,
            ..old.report.clone()
        };
        assert_eq!(
            reconcile_date(&[old], &sighting, true, d("2026-11-02"), None),
            vec![AnnouncementChange::Seen {
                id: 1,
                report: expected
            }]
        );
    }
}
