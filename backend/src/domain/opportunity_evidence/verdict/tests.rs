use super::*;
use crate::domain::{
    opportunity_evidence::{
        business_check::business_check, report_timing::report_coming_soon, test_support::*,
    },
    report_dates::EstimateBasis,
};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn checks(
    price: PriceEvidence,
    report_state: ReportInPeriodState,
    business_state: BusinessState,
) -> (
    UnusualDropOutcome,
    OffHighOutcome,
    ReportInPeriodOutcome,
    BusinessOutcome,
) {
    let unusual = UnusualDropOutcome {
        state: match price {
            PriceEvidence::Present => YesNoUnknown::Yes,
            PriceEvidence::Absent => YesNoUnknown::No,
            PriceEvidence::Undetermined => YesNoUnknown::Unknown,
        },
        reasons: vec![],
        change_percent: dec!(-10),
        adjusted_change_percent: None,
        usual_move_percent: None,
        threshold_percent: None,
        closes: 0,
        sample_moves: 0,
    };
    let off = OffHighOutcome {
        state: OffHighState::NotDeep,
        reasons: vec![],
        closes: 0,
        latest_close: None,
        high_close: None,
        low_close: None,
        below_high_percent: None,
        above_low_percent: None,
        near_low: None,
    };
    let report = ReportInPeriodOutcome {
        state: report_state,
        reasons: vec![],
        window_start: today(),
        window_end: today(),
        last_report: None,
        scheduled_report: None,
        no_report_expected_before: None,
    };
    let mut business = business_check(today(), &figures(), &source(), &report_soon_none());
    business.state = business_state;
    (unusual, off, report, business)
}
fn report_soon_none() -> ReportDate {
    report(None, None)
}

#[test]
fn exhaustive_verdict_truth_table_is_explicit_for_all_sixty_combinations() {
    use VerdictCode::{
        FellForAReason as F, NormalMovement as N, NotEnoughData as D,
        PossibleCandidateReportTimingUnknown as P, SentimentDipCandidate as C,
    };
    // Columns: intact, weakening, loss-making, unknown, n/a.
    // Rows in each price block: report yes, no, unknown, n/a.
    let expected = [
        [
            [F, F, F, F, F],
            [C, F, C, D, D],
            [P, F, P, D, D],
            [D, F, D, D, C],
        ], // present
        [
            [N, N, N, N, N],
            [N, N, N, N, N],
            [N, N, N, N, N],
            [N, N, N, N, N],
        ], // absent (first rule)
        [
            [F, F, F, F, F],
            [D, F, D, D, D],
            [D, F, D, D, D],
            [D, F, D, D, D],
        ], // undetermined
    ];
    for (p, price) in [
        PriceEvidence::Present,
        PriceEvidence::Absent,
        PriceEvidence::Undetermined,
    ]
    .into_iter()
    .enumerate()
    {
        for (r, report) in [
            ReportInPeriodState::Yes,
            ReportInPeriodState::No,
            ReportInPeriodState::Unknown,
            ReportInPeriodState::NotApplicable,
        ]
        .into_iter()
        .enumerate()
        {
            for (b, business) in [
                BusinessState::Intact,
                BusinessState::Weakening,
                BusinessState::LossMaking,
                BusinessState::Unknown,
                BusinessState::NotApplicable,
            ]
            .into_iter()
            .enumerate()
            {
                let (unusual, high, report_out, business_out) = checks(price, report, business);
                let out = verdict(
                    &unusual,
                    &high,
                    &report_out,
                    &business_out,
                    &report_coming_soon(today(), &report_soon_none()),
                    RoomToBuy::Unknown,
                );
                assert_eq!(
                    out.code, expected[p][r][b],
                    "{price:?} {report:?} {business:?}"
                );
                assert!(!out.deciding.is_empty());
                if out.code == D {
                    assert!(!out.missing.is_empty());
                } else {
                    assert!(out.missing.is_empty());
                }
                assert_eq!(
                    out.cautions
                        .iter()
                        .any(|c| matches!(c, Caution::LossMaking)),
                    business == BusinessState::LossMaking && matches!(out.code, C | P)
                );
            }
        }
    }
}

#[test]
fn price_evidence_uses_both_existing_price_checks() {
    use PriceEvidence::*;
    let expected = [
        [Present, Present, Present],
        [Present, Absent, Undetermined],
        [Present, Undetermined, Undetermined],
    ];
    for (u, unusual) in [YesNoUnknown::Yes, YesNoUnknown::No, YesNoUnknown::Unknown]
        .into_iter()
        .enumerate()
    {
        for (h, high) in [
            OffHighState::Deep,
            OffHighState::NotDeep,
            OffHighState::Unknown,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(price_evidence(unusual, high), expected[u][h]);
        }
    }
}

#[test]
fn candidate_and_possible_candidate_carry_identical_cautions_with_source_labels() {
    for report_state in [ReportInPeriodState::No, ReportInPeriodState::Unknown] {
        let (u, h, r, b) = checks(
            PriceEvidence::Present,
            report_state,
            BusinessState::LossMaking,
        );
        for basis in [
            EstimateBasis::CompanyAnnounced,
            EstimateBasis::NasdaqExpected,
            EstimateBasis::LastYearDate,
            EstimateBasis::TypicalGap,
        ] {
            let next = report(Some(today()), Some(basis));
            let soon = report_coming_soon(today(), &next);
            for room in [
                RoomToBuy::AlreadyAtTarget,
                RoomToBuy::NoTarget,
                RoomToBuy::RoomToBuy,
                RoomToBuy::Unknown,
            ] {
                let out = verdict(&u, &h, &r, &b, &soon, room);
                assert_eq!(
                    out.code,
                    if report_state == ReportInPeriodState::No {
                        VerdictCode::SentimentDipCandidate
                    } else {
                        VerdictCode::PossibleCandidateReportTimingUnknown
                    }
                );
                assert!(matches!(out.cautions[0], Caution::LossMaking));
                match &out.cautions[1] {
                    Caution::ReportComingSoon(report) => {
                        assert_eq!(report.basis, Some(basis));
                        assert_eq!(report.certainty, next.certainty);
                    }
                    _ => panic!("missing report caution"),
                }
                assert_eq!(
                    out.cautions.len(),
                    if matches!(room, RoomToBuy::AlreadyAtTarget | RoomToBuy::NoTarget) {
                        3
                    } else {
                        2
                    }
                );
            }
        }
    }
}

#[test]
fn deciding_and_missing_checks_identify_the_matching_evidence() {
    use DecidingCheck::*;
    let (mut u, mut h, r, b) = checks(
        PriceEvidence::Present,
        ReportInPeriodState::No,
        BusinessState::Intact,
    );
    let soon = report_coming_soon(today(), &report_soon_none());
    assert_eq!(
        verdict(&u, &h, &r, &b, &soon, RoomToBuy::Unknown).deciding,
        vec![UnusualDrop, ReportInPeriod, Business]
    );
    u.state = YesNoUnknown::Unknown;
    h.state = OffHighState::Deep;
    assert_eq!(
        verdict(&u, &h, &r, &b, &soon, RoomToBuy::Unknown).deciding,
        vec![OffHigh, ReportInPeriod, Business]
    );
    let out = decide(
        YesNoUnknown::Unknown,
        OffHighState::Unknown,
        ReportInPeriodState::Unknown,
        BusinessState::Unknown,
    );
    assert_eq!(
        out.missing,
        vec![UnusualDrop, OffHigh, ReportInPeriod, Business]
    );
    assert_eq!(
        decide(
            YesNoUnknown::Yes,
            OffHighState::Deep,
            ReportInPeriodState::Yes,
            BusinessState::Weakening
        )
        .deciding,
        vec![ReportInPeriod, Business]
    );
}

#[test]
fn all_five_rules_have_exact_deciding_and_missing_lists() {
    use BusinessState::{Intact, NotApplicable as Bna, Unknown as Bu, Weakening};
    use DecidingCheck::{Business, OffHigh, ReportInPeriod, UnusualDrop};
    use OffHighState::{Deep, NotDeep, Unknown as Hu};
    use ReportInPeriodState::{No as Rno, NotApplicable as Rna, Unknown as Ru, Yes as Ry};
    use VerdictCode::*;
    use YesNoUnknown::{No, Unknown, Yes};

    for (unusual, high, report, business, code, deciding, missing) in [
        (
            No,
            NotDeep,
            Ru,
            Bu,
            NormalMovement,
            vec![UnusualDrop, OffHigh],
            vec![],
        ),
        (
            Yes,
            Deep,
            Ry,
            Intact,
            FellForAReason,
            vec![ReportInPeriod],
            vec![],
        ),
        (
            Unknown,
            Hu,
            Rno,
            Weakening,
            FellForAReason,
            vec![Business],
            vec![],
        ),
        (
            Yes,
            Deep,
            Ry,
            Weakening,
            FellForAReason,
            vec![ReportInPeriod, Business],
            vec![],
        ),
        (
            Yes,
            Deep,
            Rno,
            Intact,
            SentimentDipCandidate,
            vec![UnusualDrop, OffHigh, ReportInPeriod, Business],
            vec![],
        ),
        (
            Unknown,
            Deep,
            Rna,
            Bna,
            SentimentDipCandidate,
            vec![OffHigh, ReportInPeriod, Business],
            vec![],
        ),
        (
            Yes,
            NotDeep,
            Ru,
            Intact,
            PossibleCandidateReportTimingUnknown,
            vec![UnusualDrop, ReportInPeriod, Business],
            vec![],
        ),
        (
            Yes,
            Deep,
            Rno,
            Bu,
            NotEnoughData,
            vec![Business],
            vec![Business],
        ),
        (
            Unknown,
            NotDeep,
            Rno,
            Intact,
            NotEnoughData,
            vec![UnusualDrop],
            vec![UnusualDrop],
        ),
        (
            No,
            Hu,
            Ru,
            Intact,
            NotEnoughData,
            vec![OffHigh, ReportInPeriod],
            vec![OffHigh, ReportInPeriod],
        ),
        (
            Yes,
            Deep,
            Ru,
            Bu,
            NotEnoughData,
            vec![ReportInPeriod, Business],
            vec![ReportInPeriod, Business],
        ),
        (
            Unknown,
            Hu,
            Ru,
            Bu,
            NotEnoughData,
            vec![UnusualDrop, OffHigh, ReportInPeriod, Business],
            vec![UnusualDrop, OffHigh, ReportInPeriod, Business],
        ),
        (
            Yes,
            Deep,
            Rna,
            Intact,
            NotEnoughData,
            vec![ReportInPeriod],
            vec![ReportInPeriod],
        ),
        (
            Yes,
            Deep,
            Rno,
            Bna,
            NotEnoughData,
            vec![Business],
            vec![Business],
        ),
    ] {
        let out = decide(unusual, high, report, business);
        assert_eq!(
            out.code, code,
            "{unusual:?} {high:?} {report:?} {business:?}"
        );
        assert_eq!(out.deciding, deciding, "{code:?}");
        assert_eq!(out.missing, missing, "{code:?}");
    }
}

#[test]
fn a_confirmed_report_today_reaches_the_report_negative_verdict() {
    use crate::domain::opportunity_evidence::{
        parameters::NO_REPORT_RULE,
        report_timing::{report_in_period, ReportTimingInputs},
    };
    let (u, h, _, b) = checks(
        PriceEvidence::Present,
        ReportInPeriodState::Unknown,
        BusinessState::Intact,
    );
    let last = report(Some(today()), Some(EstimateBasis::CompanyAnnounced));
    let next = report(Some(date("2027-02-11")), Some(EstimateBasis::LastYearDate));
    let r = report_in_period(
        &ReportTimingInputs {
            today: today(),
            period_start: date("2026-10-12"),
            latest_period: Some(date("2026-09-30")),
            source: &source(),
            last: &last,
            observations: &[],
            earliest_first_seen: None,
            next: &next,
            app_next: &next,
        },
        NO_REPORT_RULE,
    );
    let out = verdict(
        &u,
        &h,
        &r,
        &b,
        &report_coming_soon(today(), &next),
        RoomToBuy::Unknown,
    );
    assert_eq!(out.code, VerdictCode::FellForAReason);
    assert_eq!(out.deciding, vec![DecidingCheck::ReportInPeriod]);
    assert!(out.missing.is_empty());
    assert!(out.cautions.is_empty());
}

#[test]
fn weakening_loss_makers_reach_negative_verdict_from_actual_subtests() {
    let mut f = figures();
    f.profit_margin.value = Some(dec!(-0.1));
    f.quarters[0].eps.value = Some(dec!(-1));
    f.quarters[0].net_income.value = Some(dec!(-10));
    f.quarters[4].eps.value = Some(dec!(-1));
    f.quarters[4].net_income.value = Some(dec!(-10));
    let (u, h, r, _) = checks(
        PriceEvidence::Present,
        ReportInPeriodState::No,
        BusinessState::Intact,
    );
    let next = report_soon_none();
    let soon = report_coming_soon(today(), &next);
    for shrinking_sales in [true, false] {
        if shrinking_sales {
            f.quarters[4].sales.value = Some(dec!(90));
            f.sales_growth.value = Some(dec!(-0.1));
        } else {
            f.quarters[4].sales.value = Some(dec!(100));
            f.sales_growth.value = Some(Decimal::ZERO);
            f.quarters[4].eps.value = Some(dec!(-1.2));
            f.quarters[4].net_income.value = Some(dec!(-12));
        }
        let business = business_check(today(), &f, &source(), &next);
        assert_eq!(business.state, BusinessState::Weakening);
        let out = verdict(&u, &h, &r, &business, &soon, RoomToBuy::NoTarget);
        assert_eq!(out.code, VerdictCode::FellForAReason);
        assert_eq!(out.deciding, vec![DecidingCheck::Business]);
        assert!(out.cautions.is_empty());
    }
}
