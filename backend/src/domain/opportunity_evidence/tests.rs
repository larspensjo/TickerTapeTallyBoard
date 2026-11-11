use super::{lookback_series::*, market_reference::*, price_checks::*, *};
use crate::domain::{ProviderCode, RecentWindow};
use chrono::{Datelike, Duration, NaiveDate, Weekday};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

fn d(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

fn close(date: NaiveDate, value: Decimal) -> HistoryClose {
    HistoryClose {
        date,
        close: value,
        currency: "SEK".into(),
        provider: ProviderCode::new("YAHOO"),
        fetched_at: "2026-11-12T00:00:00Z".parse().unwrap(),
    }
}

fn rows(count: usize) -> Vec<HistoryClose> {
    (0..count)
        .map(|index| {
            close(
                d("2026-11-11") - Duration::days(365)
                    + Duration::days((index * 365 / (count - 1)) as i64),
                dec!(100),
            )
        })
        .collect()
}

fn prepare(rows: &[HistoryClose], splits: &[NaiveDate]) -> LookbackSeries {
    prepare_lookback(d("2026-11-11"), "SEK", rows.last(), rows, splits, &[], true)
}

fn unusual(series: &LookbackSeries) -> UnusualDropOutcome {
    unusual_drop(
        series,
        RecentWindow::OneWeek,
        d("2026-11-04"),
        dec!(-10),
        None,
    )
}

#[test]
fn minimum_history_boundaries_are_shared_with_top_up_classification() {
    for count in [119, 120, 199, 200] {
        let mut data = rows(count);
        data[10].close = dec!(101);
        let series = prepare(&data, &[]);
        assert_eq!(
            unusual(&series).state,
            if count >= 120 {
                YesNoUnknown::Yes
            } else {
                YesNoUnknown::Unknown
            }
        );
        assert_eq!(
            off_high(&series).state,
            if count >= 200 {
                OffHighState::NotDeep
            } else {
                OffHighState::Unknown
            }
        );
        assert_eq!(
            series.top_up_needs(),
            if count < 200 {
                vec![HistoryTopUpNeed::Short {
                    closes: count,
                    required: 200,
                }]
            } else {
                vec![]
            }
        );
        if count < 120 {
            assert_eq!(
                unusual(&series).reasons,
                vec![EvidenceReason::PriceHistoryTooShort {
                    closes: count,
                    required: 120
                }]
            );
        }
        if count < 200 {
            assert_eq!(
                off_high(&series).reasons,
                vec![EvidenceReason::PriceHistoryTooShort {
                    closes: count,
                    required: 200
                }]
            );
        }
    }
}

#[test]
fn five_missing_weekdays_pass_six_fail_including_at_the_year_start() {
    for (left, right, missing, pass) in [
        ("2026-01-01", "2026-01-09", 5, true),
        ("2026-01-01", "2026-01-12", 6, false),
        ("2026-04-01", "2026-04-08", 4, true), // Easter-shaped gap
        ("2026-12-23", "2026-12-30", 4, true), // Christmas-shaped gap
    ] {
        let (left, right) = (d(left), d(right));
        assert_eq!(missing_weekdays(left, right), missing);
        assert_eq!(history_gaps(left, &[right]).is_empty(), pass);
        assert_eq!(history_gaps(left, &[left, right]).is_empty(), pass);
    }
    let mut data = rows(200);
    data.retain(|close| close.date >= d("2025-11-24"));
    let series = prepare(&data, &[]);
    let reason = EvidenceReason::PriceHistoryGap {
        start: d("2025-11-11"),
    };
    assert!(unusual(&series).reasons.contains(&reason));
    assert!(off_high(&series).reasons.contains(&reason));
    assert!(series.top_up_needs().contains(&HistoryTopUpNeed::Gap {
        start: d("2025-11-11")
    }));
    let mut data = rows(260);
    data.retain(|close| close.date <= d("2026-03-01") || close.date >= d("2026-03-16"));
    let series = prepare(&data, &[]);
    assert!(matches!(
        unusual(&series).reasons.as_slice(),
        [EvidenceReason::PriceHistoryGap { .. }]
    ));
    assert!(matches!(
        off_high(&series).reasons.as_slice(),
        [EvidenceReason::PriceHistoryGap { .. }]
    ));
}

#[test]
fn ledger_and_provider_splits_require_adjustment_and_every_fetch_utc_date_strictly_after() {
    for provider_split in [false, true] {
        for (fetched, blocked) in [
            ("2026-06-09T23:59:59Z", true),
            ("2026-06-10T23:59:59Z", true),
            ("2026-06-11T00:00:00Z", false),
            ("2026-06-11T01:00:00+02:00", true),
        ] {
            let mut data = rows(200);
            // One stale close is enough; newer fetches on all others cannot hide it.
            data[0].fetched_at = chrono::DateTime::parse_from_rfc3339(fetched)
                .unwrap()
                .to_utc();
            let split = [d("2026-06-10")];
            let series = prepare_lookback(
                d("2026-11-11"),
                "SEK",
                data.last(),
                &data,
                if provider_split { &[] } else { &split },
                if provider_split { &split } else { &[] },
                true,
            );
            let reason = EvidenceReason::SplitInLookback { date: split[0] };
            assert_eq!(unusual(&series).reasons.contains(&reason), blocked);
            assert_eq!(off_high(&series).reasons.contains(&reason), blocked);
            assert_eq!(
                series
                    .top_up_needs()
                    .contains(&HistoryTopUpNeed::SplitStale { date: split[0] }),
                blocked
            );
        }
    }
    let mut data = rows(200);
    for code in [
        "NASDAQ_NORDIC",
        "MANUAL",
        "TWELVE_DATA",
        "YAHOO",
        "FUTURE_SOURCE",
    ] {
        data.iter_mut()
            .for_each(|close| close.provider = ProviderCode::new(code));
        let split = d("2026-06-10");
        let series = prepare_lookback(
            d("2026-11-11"),
            "SEK",
            data.last(),
            &data,
            &[split],
            &[],
            false,
        );
        assert_eq!(
            unusual(&series).reasons,
            vec![EvidenceReason::SplitInLookback {
                date: d("2026-06-10")
            }]
        );
        assert_eq!(off_high(&series).state, OffHighState::Unknown);
        assert_eq!(
            series.top_up_needs(),
            vec![HistoryTopUpNeed::SplitAdjustmentUnverified { date: split }]
        );
        // Source spelling has no bearing on the domain's split rule.
        let adjusted = prepare(&data, &[split]);
        assert!(adjusted.reasons(200).is_empty());
        assert!(adjusted.top_up_needs().is_empty());
    }
    // Splits outside the inclusive look-back have no effect.
    assert!(prepare(&rows(200), &[d("2025-11-10"), d("2026-11-12")])
        .split_dates
        .is_empty());
    assert_eq!(
        prepare(&rows(200), &[d("2025-11-11"), d("2026-11-11")])
            .split_dates
            .len(),
        2
    );
}

#[test]
fn source_changes_and_wrong_currency_are_missing_never_borrowed() {
    for wrong_currency in [false, true] {
        let mut data = rows(260);
        for close in &mut data[..50] {
            if wrong_currency {
                close.currency = "USD".into();
            } else {
                close.provider = ProviderCode::new("NASDAQ_NORDIC");
            }
        }
        let series = prepare(&data, &[]);
        assert_eq!(series.closes.len(), 210);
        assert_eq!(
            series.excluded_currency_closes,
            if wrong_currency { 50 } else { 0 }
        );
        assert_eq!(
            series.excluded_source_closes,
            if wrong_currency { 0 } else { 50 }
        );
        assert_eq!(unusual(&series).state, YesNoUnknown::Unknown);
        assert_eq!(off_high(&series).state, OffHighState::Unknown);
        assert!(series
            .reasons(200)
            .contains(&EvidenceReason::PriceHistoryGap {
                start: series.start
            }));
    }
    let mut data = rows(200);
    data[10].currency = "sek".into();
    assert_eq!(prepare(&data, &[]).closes.len(), 200);
    // Rows outside the year and after today cannot set the source or affect a check.
    data.insert(0, close(d("2025-11-10"), dec!(1000)));
    data.push(close(d("2026-11-12"), dec!(10000)));
    let series = prepare_lookback(d("2026-11-11"), "SEK", data.get(200), &data, &[], &[], true);
    assert_eq!(series.closes.len(), 200);
    assert_eq!(off_high(&series).high_close, Some(dec!(100)));
}

#[test]
fn absent_or_currency_excluded_latest_is_explicitly_unknown() {
    let series = prepare_lookback(d("2026-11-11"), "SEK", None, &rows(200), &[], &[], true);
    assert!(unusual(&series)
        .reasons
        .contains(&EvidenceReason::MissingLatestClose));
    assert!(off_high(&series)
        .reasons
        .contains(&EvidenceReason::MissingLatestClose));
    let mut data = rows(200);
    data.last_mut().unwrap().currency = "USD".into();
    let series = prepare(&data, &[]);
    assert!(unusual(&series)
        .reasons
        .contains(&EvidenceReason::MissingLatestClose));
    assert!(off_high(&series)
        .reasons
        .contains(&EvidenceReason::MissingLatestClose));
}

#[test]
fn historical_pairing_pins_calendar_windows_tolerance_and_inclusive_cutoff() {
    for window in [RecentWindow::OneWeek, RecentWindow::ThirtyDays] {
        let (series, cutoff, next) = historical_pairing_fixture(window);
        let outcome = unusual_drop(&series, window, cutoff, dec!(-10), None);
        // In both fixtures the four eligible returns are -10%, 0%, 10%, 0%.
        // Mean = 0; sample variance = (100 + 0 + 100 + 0) / (4 - 1)
        // = 200/3; sqrt(200/3), rounded to 12 places, is 8.164965809277.
        assert_eq!(outcome.sample_moves, 4, "{window:?}");
        assert_eq!(outcome.usual_move_percent, Some(dec!(8.164965809277)));
        assert_eq!(outcome.threshold_percent, Some(dec!(-12.2474487139155)));
        assert_eq!(outcome.state, YesNoUnknown::No);
        assert!(outcome.reasons.is_empty());
        // The cutoff itself contributes the fourth move; its next close must not.
        let next_outcome = unusual_drop(&series, window, next, dec!(-10), None);
        assert_eq!(next_outcome.sample_moves, 5);
        assert_ne!(next_outcome.usual_move_percent, outcome.usual_move_percent);
    }
}

fn historical_pairing_fixture(window: RecentWindow) -> (LookbackSeries, NaiveDate, NaiveDate) {
    let (points, cutoff, next): (&[(&str, Decimal)], _, _) = match window {
        RecentWindow::OneWeek => (
            &[
                ("2025-11-11", dec!(100)),
                ("2025-11-18", dec!(90)),  // exact target Nov 11: -10%
                ("2025-11-20", dec!(100)), // absent holiday target Nov 13: 0%
                ("2025-11-21", dec!(110)), // target Nov 14, exactly 3 weekdays old: 10%
                ("2025-11-24", dec!(999)), // target Nov 17, 4 weekdays old: skipped
                ("2025-11-25", dec!(90)),  // exact target Nov 18: 0%, included
                ("2025-11-26", dec!(999)), // next close: excluded
            ],
            "2025-11-25",
            "2025-11-26",
        ),
        RecentWindow::ThirtyDays => (
            &[
                ("2025-11-11", dec!(100)),
                ("2025-11-18", dec!(100)),
                ("2025-11-25", dec!(100)),
                ("2025-12-03", dec!(100)),
                ("2025-12-05", dec!(100)),
                ("2025-12-11", dec!(90)),  // exact target Nov 11: -10%
                ("2025-12-12", dec!(100)), // absent holiday target Nov 12: 0%
                ("2025-12-15", dec!(110)), // Saturday target Nov 15: 10%
                ("2025-12-17", dec!(999)), // target Nov 17, 4 weekdays old: skipped
                ("2025-12-18", dec!(100)), // exact target Nov 18: 0%, included
                ("2025-12-19", dec!(999)), // next close: excluded
            ],
            "2025-12-18",
            "2025-12-19",
        ),
        RecentWindow::TwoWeeks => unreachable!("fixture covers 7d and 30d"),
    };
    let mut data: Vec<_> = points
        .iter()
        .map(|(date, value)| close(d(date), *value))
        .collect();
    // Satisfy minimum history and coverage entirely AFTER the sample cutoff.
    // These ordinary weekday closes cannot add any historical sample moves.
    let mut date = d(next) + Duration::days(1);
    while date <= d("2026-11-11") {
        if !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) {
            data.push(close(date, dec!(100)));
        }
        date += Duration::days(1);
    }
    (prepare(&data, &[]), d(cutoff), d(next))
}

#[test]
fn unusual_drop_includes_exact_threshold_and_only_adjusts_the_test_change() {
    let (series, cutoff, _) = historical_pairing_fixture(RecentWindow::OneWeek);
    // Independently calculated: -1.5 * sqrt(200/3), after rounding the root to 12 places.
    let threshold = dec!(-12.2474487139155);
    assert_eq!(
        unusual_drop(&series, RecentWindow::OneWeek, cutoff, threshold, None).state,
        YesNoUnknown::Yes
    );
    assert_eq!(
        unusual_drop(
            &series,
            RecentWindow::OneWeek,
            cutoff,
            threshold + dec!(0.000000000001),
            None
        )
        .state,
        YesNoUnknown::No
    );
    let adjusted = unusual_drop(
        &series,
        RecentWindow::OneWeek,
        cutoff,
        dec!(-99),
        Some(dec!(0)),
    );
    assert_eq!(adjusted.state, YesNoUnknown::No);
    assert_eq!(adjusted.change_percent, dec!(-99));
    assert_eq!(adjusted.adjusted_change_percent, Some(dec!(0)));
    assert_eq!(adjusted.threshold_percent, Some(threshold));
}

#[test]
fn zero_dispersion_is_unknown_even_for_a_large_fall() {
    let outcome = unusual(&prepare(&rows(200), &[]));
    assert_eq!(outcome.state, YesNoUnknown::Unknown);
    assert_eq!(outcome.reasons, vec![EvidenceReason::NoPriceVariation]);
    assert_eq!(outcome.usual_move_percent, Some(Decimal::ZERO));
    assert_eq!(outcome.threshold_percent, None);
    assert!(outcome.sample_moves > 1);
}

#[test]
fn historical_sample_excludes_every_move_after_current_start_close() {
    let mut data = rows(200);
    let original = unusual(&prepare(&data, &[]));
    for close in data.iter_mut().filter(|close| close.date > d("2026-11-04")) {
        close.close = dec!(1);
    }
    let changed = unusual(&prepare(&data, &[]));
    assert_eq!(original.usual_move_percent, changed.usual_move_percent);
    assert_eq!(original.sample_moves, changed.sample_moves);
    assert_eq!(changed.usual_move_percent, Some(Decimal::ZERO));
    for window in [
        RecentWindow::OneWeek,
        RecentWindow::TwoWeeks,
        RecentWindow::ThirtyDays,
    ] {
        let result = unusual_drop(
            &prepare(&data, &[]),
            window,
            d("2025-11-11"),
            dec!(-1),
            None,
        );
        assert_eq!(result.state, YesNoUnknown::Unknown);
        assert_eq!(
            result.reasons,
            vec![EvidenceReason::InsufficientHistoricalMoves { moves: 0 }]
        );
    }
}

#[test]
fn range_deep_and_near_low_thresholds_are_inclusive_and_keep_exact_values() {
    for (latest, deep, near_low) in [
        (dec!(80), true, true),
        (dec!(80.00000001), false, true),
        (dec!(84), false, true),
        (dec!(84.00000001), false, false),
    ] {
        let mut data = rows(200);
        data[50].close = dec!(80);
        data.last_mut().unwrap().close = latest;
        let result = off_high(&prepare(&data, &[]));
        assert_eq!(
            result.state,
            if deep {
                OffHighState::Deep
            } else {
                OffHighState::NotDeep
            }
        );
        assert_eq!(result.near_low, Some(near_low));
        assert_eq!(result.high_close, Some(dec!(100)));
        assert_eq!(result.low_close, Some(dec!(80)));
        assert_eq!(result.below_high_percent, Some(dec!(100) - latest));
        assert!(result.reasons.is_empty());
    }
}

#[test]
fn invalid_prices_never_turn_into_a_pass_and_numeric_overflow_is_unknown() {
    for invalid in [Decimal::ZERO, dec!(-1)] {
        let mut data = rows(200);
        data[10].close = invalid;
        let series = prepare(&data, &[]);
        let reason = EvidenceReason::NonPositiveClose {
            date: data[10].date,
        };
        assert_eq!(unusual(&series).reasons, vec![reason.clone()]);
        assert_eq!(off_high(&series).reasons, vec![reason]);
    }
    let mut data = rows(200);
    for close in &mut data {
        close.close = dec!(0.0000000000000000000000000001);
    }
    data[40].close = Decimal::MAX;
    data.last_mut().unwrap().close = Decimal::MAX;
    let series = prepare(&data, &[]);
    assert_eq!(
        unusual(&series).reasons,
        vec![EvidenceReason::CalculationOutOfRange]
    );
    assert_eq!(
        off_high(&series).reasons,
        vec![EvidenceReason::CalculationOutOfRange]
    );
}

#[test]
fn stopped_falling_requires_a_rise_and_two_weekdays_since_the_most_recent_low() {
    for (low_date, latest, expected, age) in [
        ("2026-11-10", dec!(90), YesNoUnknown::No, 1),
        ("2026-11-09", dec!(90), YesNoUnknown::Yes, 2),
        ("2026-11-09", dec!(80), YesNoUnknown::No, 0),
    ] {
        let data = vec![
            close(d("2026-11-04"), dec!(100)),
            close(d(low_date), dec!(80)),
            close(d("2026-11-11"), latest),
        ];
        let outcome = stopped_falling(&prepare(&data, &[]), d("2026-11-04"), d("2026-11-11"));
        assert_eq!(outcome.state, expected);
        assert_eq!(outcome.low_age_trading_days, Some(age));
        assert_eq!(outcome.latest_close, Some(latest));
        assert_eq!(outcome.low_close, Some(dec!(80)));
    }
    let data = vec![
        close(d("2026-11-04"), dec!(100)),
        close(d("2026-11-06"), dec!(80)),
        close(d("2026-11-09"), dec!(90)),
    ];
    let outcome = stopped_falling(&prepare(&data, &[]), d("2026-11-04"), d("2026-11-09"));
    assert_eq!(outcome.low_age_trading_days, Some(1)); // weekend adds no age
    assert_eq!(outcome.state, YesNoUnknown::No);
}

#[test]
fn stopped_falling_unknown_reasons_and_old_holes_do_not_contaminate_the_window() {
    let data = vec![
        close(d("2026-11-04"), dec!(100)),
        close(d("2026-11-09"), dec!(80)),
        close(d("2026-11-11"), dec!(90)),
    ];
    let series = prepare(&data, &[d("2026-06-01")]);
    assert_eq!(
        stopped_falling(&series, d("2026-11-04"), d("2026-11-11")).state,
        YesNoUnknown::Yes
    );
    let result = stopped_falling(&series, d("2026-11-03"), d("2026-11-11"));
    assert_eq!(result.state, YesNoUnknown::Unknown);
    assert_eq!(result.reasons, vec![EvidenceReason::MissingWindowCloses]);
    let mut data = data;
    data[1].close = Decimal::ZERO;
    assert_eq!(
        stopped_falling(&prepare(&data, &[]), data[0].date, data[2].date).reasons,
        vec![EvidenceReason::NonPositiveClose { date: data[1].date }]
    );
    data[1].close = dec!(80);
    data[0].fetched_at = "2026-11-09T00:00:00Z".parse().unwrap();
    assert_eq!(
        stopped_falling(
            &prepare(&data, &[d("2026-11-09")]),
            data[0].date,
            data[2].date
        )
        .reasons,
        vec![EvidenceReason::SplitInLookback {
            date: d("2026-11-09")
        }]
    );
    let data = vec![
        close(d("2026-10-01"), dec!(100)),
        close(d("2026-11-11"), dec!(90)),
    ];
    assert_eq!(
        stopped_falling(&prepare(&data, &[]), data[0].date, data[1].date).reasons,
        vec![EvidenceReason::PriceHistoryGap {
            start: data[0].date
        }]
    );
}

#[test]
fn market_minimum_excludes_self_and_unavailable_rows_and_uses_odd_and_even_medians() {
    let mut changes: Vec<_> = [9, 1, 5, 3]
        .into_iter()
        .enumerate()
        .map(|(i, value)| (i as i64, Some(Decimal::from(value))))
        .collect();
    changes.extend([(99, Some(dec!(-999))), (100, None)]);
    let result = market_reference(99, dec!(-10), RecentWindow::OneWeek, &changes);
    assert_eq!(result.state, MarketState::Unknown);
    assert_eq!(result.count, 4);
    assert_eq!(
        result.reasons,
        vec![EvidenceReason::InsufficientMarketRows {
            count: 4,
            required: 5
        }]
    );
    changes.push((4, Some(dec!(7))));
    let result = market_reference(99, dec!(-10), RecentWindow::OneWeek, &changes);
    assert_eq!(result.count, 5);
    assert_eq!(result.reference_percent, Some(dec!(5)));
    assert_eq!(result.state, MarketState::FellMoreThanMarket);
    assert_eq!(result.source, MarketReferenceSource::OtherHoldingsMedian);
    changes.push((5, Some(dec!(6))));
    assert_eq!(
        market_reference(99, dec!(-10), RecentWindow::OneWeek, &changes).reference_percent,
        Some(dec!(5.5))
    );
    let tiny = dec!(0.0000000000000000000000000001);
    let changes: Vec<_> = (0..6).map(|id| (id, Some(tiny))).collect();
    assert_eq!(
        market_reference(99, dec!(-1), RecentWindow::OneWeek, &changes).reference_percent,
        Some(tiny)
    );
}

#[test]
fn every_market_window_uses_its_margin_with_inclusive_edges() {
    let changes: Vec<_> = (0..5).map(|id| (id, Some(dec!(-10)))).collect();
    for (window, margin) in [
        (RecentWindow::OneWeek, dec!(3)),
        (RecentWindow::TwoWeeks, dec!(4)),
        (RecentWindow::ThirtyDays, dec!(6)),
    ] {
        for (difference, expected) in [
            (-margin - dec!(0.00000001), MarketState::FellMoreThanMarket),
            (-margin, MarketState::MovedWithMarket),
            (Decimal::ZERO, MarketState::MovedWithMarket),
            (margin, MarketState::MovedWithMarket),
            (margin + dec!(0.00000001), MarketState::FellLessThanMarket),
        ] {
            let outcome = market_reference(99, dec!(-10) + difference, window, &changes);
            assert_eq!(outcome.state, expected);
            assert_eq!(outcome.margin_percent, margin);
            assert_eq!(outcome.difference_percent, Some(difference));
            assert!(outcome.reasons.is_empty());
        }
    }
}

#[test]
fn retesting_an_earlier_low_resets_its_age_and_changes_yes_to_no() {
    let mut data = vec![
        close(d("2026-11-04"), dec!(100)),
        close(d("2026-11-09"), dec!(80)),
        close(d("2026-11-10"), dec!(90)),
        close(d("2026-11-11"), dec!(90)),
    ];
    let before = stopped_falling(&prepare(&data, &[]), d("2026-11-04"), d("2026-11-11"));
    assert_eq!(before.state, YesNoUnknown::Yes);
    assert_eq!(before.low_date, Some(d("2026-11-09")));
    assert_eq!(before.low_age_trading_days, Some(2));

    data[2].close = dec!(80);
    let retested = stopped_falling(&prepare(&data, &[]), d("2026-11-04"), d("2026-11-11"));
    assert_eq!(retested.state, YesNoUnknown::No);
    assert_eq!(retested.low_date, Some(d("2026-11-10")));
    assert_eq!(retested.low_age_trading_days, Some(1));
    assert_eq!(retested.latest_close, Some(dec!(90)));
}

#[test]
fn market_overflows_are_unknown_with_an_explicit_reason() {
    for (count, reference, change) in [
        (6, Decimal::MAX, Decimal::ZERO), // even median addition overflows
        (6, Decimal::MIN, Decimal::ZERO),
        (5, Decimal::MIN, Decimal::MAX), // difference overflows
        (5, Decimal::MAX, Decimal::MIN),
    ] {
        let changes: Vec<_> = (0..count).map(|id| (id, Some(reference))).collect();
        let outcome = market_reference(99, change, RecentWindow::OneWeek, &changes);
        assert_eq!(outcome.state, MarketState::Unknown);
        assert_eq!(outcome.reasons, vec![EvidenceReason::CalculationOutOfRange]);
        assert_eq!(outcome.difference_percent, None);
    }
}

#[test]
fn reason_codes_are_stable() {
    for (reason, code) in [
        (EvidenceReason::MissingLatestClose, "missing_latest_close"),
        (EvidenceReason::NoPriceVariation, "no_price_variation"),
        (
            EvidenceReason::PriceHistoryGap {
                start: d("2026-01-01"),
            },
            "price_history_gap",
        ),
        (
            EvidenceReason::SplitInLookback {
                date: d("2026-01-01"),
            },
            "split_in_lookback",
        ),
        (
            EvidenceReason::PriceHistoryTooShort {
                closes: 119,
                required: 120,
            },
            "price_history_too_short",
        ),
        (
            EvidenceReason::InsufficientHistoricalMoves { moves: 1 },
            "insufficient_historical_moves",
        ),
        (
            EvidenceReason::NonPositiveClose {
                date: d("2026-01-01"),
            },
            "non_positive_close",
        ),
        (EvidenceReason::MissingWindowCloses, "missing_window_closes"),
        (
            EvidenceReason::InsufficientMarketRows {
                count: 4,
                required: 5,
            },
            "insufficient_market_rows",
        ),
        (
            EvidenceReason::CalculationOutOfRange,
            "calculation_out_of_range",
        ),
    ] {
        assert_eq!(reason.as_str(), code);
    }
}
