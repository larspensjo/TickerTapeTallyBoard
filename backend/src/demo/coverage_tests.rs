//! Public-contract coverage of the offline demo, under every weekday alignment.
use crate::{
    clock::Clock,
    config::Mode,
    domain::{
        company_figures::EpsConsistencyState,
        opportunity_evidence::{
            business_check::BusinessState,
            market_reference::MarketState,
            price_checks::OffHighState,
            report_timing::{ReportComingSoonState, ReportInPeriodState},
            room_to_buy::RoomToBuy,
            verdict::{Caution, VerdictCode},
            YesNoUnknown,
        },
        report_dates::EstimateBasis,
    },
    state::AppState,
};
use chrono::{Duration, NaiveDate};
use std::collections::{BTreeMap, BTreeSet};

// The match is deliberately exhaustive. Adding a domain variant breaks this
// test at compile time until its serialized state is included and reached.
macro_rules! states {
    ($enum:ident { $($variant:ident => $name:literal),+ $(,)? }) => {{
        let name = |state: $enum| match state { $($enum::$variant => $name),+ };
        BTreeSet::from([$(name($enum::$variant)),+])
    }};
}

fn required_checks() -> BTreeMap<&'static str, BTreeSet<&'static str>> {
    let yes_no = states!(YesNoUnknown { Yes => "yes", No => "no", Unknown => "unknown" });
    let mut market = states!(MarketState {
        FellMoreThanMarket => "fell_more_than_market", MovedWithMarket => "moved_with_market",
        FellLessThanMarket => "fell_less_than_market", Unknown => "unknown",
    });
    // Named unreachable exception: fewer than five other available held rows
    // contradicts keeping every held row's change available in every window.
    const MARKET_UNKNOWN_NEEDS_FEWER_AVAILABLE_PEERS: &str = "unknown";
    assert!(market.remove(MARKET_UNKNOWN_NEEDS_FEWER_AVAILABLE_PEERS));
    let mut room = states!(RoomToBuy { RoomToBuy => "room_to_buy", AlreadyAtTarget => "already_at_target", NoTarget => "no_target", Unknown => "unknown" });
    // Named exception: needs a held row with unavailable valuation, which would
    // leave the demo portfolio partly unvalued. Covered by an API fixture instead.
    const ROOM_UNKNOWN_NEEDS_UNAVAILABLE_VALUATION: &str = "unknown";
    assert!(room.remove(ROOM_UNKNOWN_NEEDS_UNAVAILABLE_VALUATION));
    BTreeMap::from([
        ("unusual_drop", yes_no.clone()),
        ("stopped_falling", yes_no.clone()),
        ("ex_dividend", yes_no),
        (
            "off_high",
            states!(OffHighState { Deep => "deep", NotDeep => "not_deep", Unknown => "unknown" }),
        ),
        (
            "report_in_period",
            states!(ReportInPeriodState { Yes => "yes", No => "no", Unknown => "unknown", NotApplicable => "not_applicable" }),
        ),
        (
            "report_coming_soon",
            states!(ReportComingSoonState { Yes => "yes", No => "no", Unknown => "unknown", NotApplicable => "not_applicable" }),
        ),
        (
            "business",
            states!(BusinessState { Intact => "intact", Weakening => "weakening", LossMaking => "loss_making", Unknown => "unknown", NotApplicable => "not_applicable" }),
        ),
        ("market", market),
        ("room_to_buy", room),
    ])
}

#[tokio::test]
async fn opportunities_reach_every_verdict_check_state_and_caution_for_each_weekday() {
    let required = required_checks();
    let verdicts = states!(VerdictCode {
        SentimentDipCandidate => "sentiment_dip_candidate",
        PossibleCandidateReportTimingUnknown => "possible_candidate_report_timing_unknown",
        NotEnoughData => "not_enough_data", FellForAReason => "fell_for_a_reason",
        NormalMovement => "normal_movement",
    });
    // Payload-bearing variants use the same exhaustive list for the match and set.
    macro_rules! caution_states {
        ($($pattern:pat => $name:literal),+ $(,)?) => {{
            let _name = |caution: &Caution| match caution { $($pattern => $name),+ };
            BTreeSet::from([$($name),+])
        }};
    }
    let cautions = caution_states! {
        Caution::LossMaking => "loss_making",
        Caution::ReportComingSoon(_) => "report_coming_soon",
        Caution::AlreadyAtTarget => "already_at_target",
        Caution::NoTarget => "no_target",
    };
    let bases = states!(EstimateBasis {
        CompanyAnnounced => "company_announced", NasdaqExpected => "nasdaq_expected",
        LastYearDate => "last_year_date", TypicalGap => "typical_gap",
    });
    let eps_states = states!(EpsConsistencyState {
        Consistent => "consistent", Approximate => "approximate", Inconsistent => "inconsistent",
        NotCrossChecked => "not_cross_checked", Unavailable => "unavailable",
    });
    let dates = (0..7)
        .map(|offset| NaiveDate::from_ymd_opt(2026, 11, 9).unwrap() + Duration::days(offset))
        .chain(
            [(2026, 12, 31), (2027, 1, 1), (2028, 2, 29), (2027, 4, 1)]
                .map(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d).unwrap()),
        );
    for today in dates {
        let state = AppState::for_tests()
            .await
            .with_clock(Clock::fixed(today))
            .with_mode(Mode::Demo);
        super::seed(&state.pool, today).await.unwrap();
        crate::db::testing::set_query_only(&state.pool).await;
        let mut seen: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        let mut seen_verdicts = BTreeSet::new();
        let mut seen_cautions = BTreeSet::new();
        let mut soon_labels = BTreeSet::new();
        let mut seen_eps = BTreeSet::new();
        let mut details = BTreeSet::new();
        for window in ["7d", "14d", "30d"] {
            let body =
                super::tests::get_json(&state, &format!("/api/opportunities?window={window}"))
                    .await;
            let mut fallers = 0;
            let mut risers = 0;
            for row in body["rows"].as_array().unwrap() {
                assert_eq!(
                    row["price_change"]["status"], "available",
                    "{today} {window} {row}"
                );
                let evidence = &row["evidence"];
                let start: rust_decimal::Decimal = row["price_change"]["start_close"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                let end: rust_decimal::Decimal = row["price_change"]["end_close"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap();
                if end >= start {
                    assert!(evidence.is_null(), "{today} {window} {row}");
                    risers += usize::from(end > start);
                    continue;
                }
                fallers += 1;
                assert!(evidence.is_object(), "{row}");
                let code = evidence["verdict"]["code"].as_str().unwrap();
                seen_verdicts.insert(code.to_owned());
                let verdict = &evidence["verdict"];
                let deciding = verdict["deciding"].as_array().unwrap();
                assert!(!deciding.is_empty(), "{today} {window} {row}");
                for list in [deciding, verdict["missing"].as_array().unwrap()] {
                    assert!(
                        list.iter()
                            .all(|check| required.contains_key(check.as_str().unwrap())),
                        "{row}"
                    );
                }
                if code == "not_enough_data" {
                    assert!(!verdict["missing"].as_array().unwrap().is_empty(), "{row}");
                }
                for caution in evidence["verdict"]["cautions"].as_array().unwrap() {
                    assert!(matches!(
                        code,
                        "sentiment_dip_candidate" | "possible_candidate_report_timing_unknown"
                    ));
                    seen_cautions.insert(caution.as_str().unwrap().to_owned());
                    if caution == "loss_making" && code == "sentiment_dip_candidate" {
                        details.insert("loss_candidate".to_owned());
                    }
                }
                for &check in required.keys() {
                    let value = &evidence["checks"][check];
                    seen.entry(check)
                        .or_default()
                        .insert(value["state"].as_str().unwrap().to_owned());
                    for reason in value["reasons"].as_array().unwrap() {
                        details.insert(reason["code"].as_str().unwrap().to_owned());
                    }
                }
                let checks = &evidence["checks"];
                if let Some(near) = checks["off_high"]["near_low"].as_bool() {
                    details.insert(format!("near_low_{near}"));
                }
                if let Some(basis) = checks["report_coming_soon"]["report"]["basis"].as_str() {
                    soon_labels.insert(basis.to_owned());
                }
                let business = &checks["business"];
                seen_eps.insert(
                    business["eps_consistency"]["state"]
                        .as_str()
                        .unwrap()
                        .to_owned(),
                );
                if business["eps_consistency"]["state"] == "approximate" {
                    assert_eq!(business["earnings_not_sliding"]["approximate"], true);
                    details.insert("approximate_eps".into());
                }
                if business["eps_consistency"]["state"] == "inconsistent" {
                    assert_eq!(business["state"], "unknown");
                    details.insert("severe_eps_unknown".into());
                }
                if business["state"] == "weakening" {
                    assert_eq!(code, "fell_for_a_reason");
                    if business["sales_not_shrinking"]["passes"] == false {
                        details.insert("shrinking_sales".into());
                    }
                    if business["earnings_not_sliding"]["passes"] == false {
                        details.insert("sliding_eps".into());
                    }
                    if business["profitable"]["passes"] == false {
                        details.insert("weakening_loss_maker".into());
                    }
                }
            }
            assert!(fallers > 0, "{today} {window}: no fallers");
            // The fund rebounds in 7d while retaining normal falls in 14d/30d.
            if window == "7d" {
                assert!(risers > 0, "{today} {window}: no risers");
            }
        }
        for (check, expected) in &required {
            let actual = seen.get(check).unwrap();
            let expected: BTreeSet<String> = expected.iter().map(|s| (*s).to_owned()).collect();
            assert_eq!(actual, &expected, "{today} {check}");
        }
        assert_eq!(
            seen_verdicts,
            verdicts.iter().map(|s| (*s).to_owned()).collect(),
            "{today}"
        );
        assert_eq!(
            seen_cautions,
            cautions.iter().map(|s| (*s).to_owned()).collect(),
            "{today}"
        );
        assert_eq!(
            soon_labels,
            bases.iter().map(|s| (*s).to_owned()).collect(),
            "{today}"
        );
        assert_eq!(
            seen_eps,
            eps_states.iter().map(|s| (*s).to_owned()).collect(),
            "{today} EPS consistency"
        );
        for detail in [
            "near_low_true",
            "near_low_false",
            "price_history_too_short",
            "price_history_gap",
            "split_in_lookback",
            "scheduled_report_figures_not_seen",
            "observed_report_inside_window",
            "approximate_eps",
            "severe_eps_unknown",
            "eps_inconsistent",
            "newer_report_expected",
            "shrinking_sales",
            "sliding_eps",
            "weakening_loss_maker",
            "loss_candidate",
            "dividend_coverage_missing",
            "dividend_capture_invalid",
        ] {
            assert!(
                details.contains(detail),
                "{today} missing {detail}: {details:?}"
            );
        }
        state.pool.close().await;
    }
}
