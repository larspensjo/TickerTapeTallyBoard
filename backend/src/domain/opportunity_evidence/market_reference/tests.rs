use super::*;
use rust_decimal_macros::dec;

fn d(value: &str) -> NaiveDate {
    value.parse().unwrap()
}
fn outcome(closes: &[(NaiveDate, Decimal)], change: Decimal) -> MarketOutcome {
    market_reference_with_index(
        99,
        change,
        RecentWindow::OneWeek,
        &(0..5).map(|id| (id, Some(dec!(-1)))).collect::<Vec<_>>(),
        (d("2026-11-06"), d("2026-11-13")),
        Some(IndexReference {
            code: "TEST",
            display_name: "Test index",
            closes,
        }),
    )
}

#[test]
fn pairs_latest_on_or_before_each_holding_date_even_with_unsorted_closes() {
    let result = outcome(
        &[
            (d("2026-11-14"), dec!(500)),
            (d("2026-11-13"), dec!(99)),
            (d("2026-11-07"), dec!(600)),
            (d("2026-11-06"), dec!(100)),
            (d("2026-11-05"), dec!(90)),
        ],
        dec!(-8),
    );
    assert_eq!(
        result.source,
        MarketReferenceSource::MarketIndex {
            code: "TEST".into(),
            display_name: "Test index".into(),
            start_date: d("2026-11-06"),
            end_date: d("2026-11-13")
        }
    );
    assert_eq!(result.reference_percent, Some(dec!(-1)));
    assert_eq!(result.difference_percent, Some(dec!(-7)));
    assert_eq!(result.state, MarketState::FellMoreThanMarket);
    assert!(result.reasons.is_empty());
}

#[test]
fn different_index_calendar_pairs_one_two_and_three_sessions_behind() {
    for lag in 1..=START_CLOSE_MAX_AGE_TRADING_DAYS {
        let start = d("2026-11-06") - chrono::Duration::days(lag);
        let end = d("2026-11-13") - chrono::Duration::days(lag);
        let result = outcome(&[(start, dec!(100)), (end, dec!(99))], dec!(-8));
        assert!(
            matches!(result.source, MarketReferenceSource::MarketIndex { start_date, end_date, .. } if start_date == start && end_date == end)
        );
    }
    // Friday's index also pairs with a Monday holding across the weekend.
    let result = market_reference_with_index(
        99,
        dec!(-8),
        RecentWindow::OneWeek,
        &[],
        (d("2026-11-09"), d("2026-11-16")),
        Some(IndexReference {
            code: "TEST",
            display_name: "Test index",
            closes: &[(d("2026-11-06"), dec!(100)), (d("2026-11-13"), dec!(99))],
        }),
    );
    assert_eq!(result.reference_percent, Some(dec!(-1)));
}

#[test]
fn unavailable_index_falls_back_with_reason_and_preserves_all_median_rules() {
    for closes in [
        vec![],
        vec![(d("2026-11-02"), dec!(100)), (d("2026-11-13"), dec!(99))], // start too old
        vec![(d("2026-11-06"), dec!(100)), (d("2026-11-09"), dec!(99))], // end too old
        vec![(d("2026-11-07"), dec!(100)), (d("2026-11-13"), dec!(99))], // future start forbidden
        vec![(d("2026-11-06"), dec!(0)), (d("2026-11-13"), dec!(99))],
        vec![(d("2026-11-06"), dec!(100)), (d("2026-11-13"), dec!(-1))],
        vec![
            (d("2026-11-06"), dec!(0.0000000000000000000000000001)),
            (d("2026-11-13"), Decimal::MAX),
        ],
    ] {
        let result = outcome(&closes, dec!(-8));
        assert_eq!(result.source, MarketReferenceSource::OtherHoldingsMedian);
        assert_eq!(result.reference_percent, Some(dec!(-1)));
        assert_eq!(result.state, MarketState::FellMoreThanMarket);
        assert_eq!(
            result.reasons,
            vec![EvidenceReason::MarketIndexUnavailable {
                display_name: "Test index".into()
            }]
        );
    }
    let result = market_reference_with_index(
        99,
        dec!(-8),
        RecentWindow::OneWeek,
        &[],
        (d("2026-11-06"), d("2026-11-13")),
        Some(IndexReference {
            code: "TEST",
            display_name: "Test index",
            closes: &[],
        }),
    );
    assert_eq!(result.state, MarketState::Unknown);
    assert!(matches!(
        result.reasons[0],
        EvidenceReason::InsufficientMarketRows {
            count: 0,
            required: 5
        }
    ));
    assert!(matches!(
        result.reasons[1],
        EvidenceReason::MarketIndexUnavailable { .. }
    ));
    // Difference overflow also makes an otherwise valid index unusable.
    let result = outcome(
        &[(d("2026-11-06"), dec!(1)), (d("2026-11-13"), dec!(2))],
        Decimal::MIN,
    );
    assert!(result
        .reasons
        .iter()
        .any(|reason| matches!(reason, EvidenceReason::MarketIndexUnavailable { .. })));
}

#[test]
fn index_ending_on_or_before_the_holding_start_falls_back_even_within_tolerance() {
    for last_index_date in ["2026-11-06", "2026-11-05"] {
        let peers: Vec<_> = (0..5).map(|id| (id, Some(dec!(-1)))).collect();
        let closes = [(d(last_index_date), dec!(100))];
        let result = market_reference_with_index(
            99,
            dec!(-8),
            RecentWindow::OneWeek,
            &peers,
            (d("2026-11-06"), d("2026-11-10")),
            Some(IndexReference {
                code: "TEST",
                display_name: "Test index",
                closes: &closes,
            }),
        );
        let mut expected = market_reference(99, dec!(-8), RecentWindow::OneWeek, &peers);
        expected
            .reasons
            .push(EvidenceReason::MarketIndexUnavailable {
                display_name: "Test index".into(),
            });
        assert_eq!(result, expected, "last index close: {last_index_date}");
    }
}

#[test]
fn listing_without_index_is_exactly_the_existing_median_without_extra_reasons() {
    for count in [4, 5] {
        let peers: Vec<_> = (0..count).map(|id| (id, Some(dec!(-1)))).collect();
        assert_eq!(
            market_reference_with_index(
                99,
                dec!(-8),
                RecentWindow::OneWeek,
                &peers,
                (d("2026-11-06"), d("2026-11-13")),
                None
            ),
            market_reference(99, dec!(-8), RecentWindow::OneWeek, &peers)
        );
    }
}

#[test]
fn index_uses_the_same_margin_and_three_states_as_the_median_in_every_window() {
    let closes = [(d("2026-11-06"), dec!(100)), (d("2026-11-13"), dec!(99))];
    for window in [
        RecentWindow::OneWeek,
        RecentWindow::TwoWeeks,
        RecentWindow::ThirtyDays,
    ] {
        let margin = market_margin(window);
        for (difference, expected) in [
            (-margin - dec!(0.01), MarketState::FellMoreThanMarket),
            (-margin, MarketState::MovedWithMarket),
            (margin, MarketState::MovedWithMarket),
            (margin + dec!(0.01), MarketState::FellLessThanMarket),
        ] {
            let result = market_reference_with_index(
                99,
                dec!(-1) + difference,
                window,
                &[],
                (d("2026-11-06"), d("2026-11-13")),
                Some(IndexReference {
                    code: "TEST",
                    display_name: "Test index",
                    closes: &closes,
                }),
            );
            assert_eq!(result.state, expected);
            assert_eq!(result.margin_percent, margin);
        }
    }
}
