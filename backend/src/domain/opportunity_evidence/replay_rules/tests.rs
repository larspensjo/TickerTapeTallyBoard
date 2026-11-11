use super::*;
use crate::domain::ProviderCode;
use chrono::{Duration, TimeZone, Utc};
use rust_decimal_macros::dec;

fn split_adjusted(source: &str) -> bool {
    source == "YAHOO"
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 1, 6).unwrap()
}
fn close(offset: i64) -> HistoryClose {
    let date = day() + Duration::days(offset);
    HistoryClose {
        date,
        close: dec!(100) + Decimal::from(offset),
        currency: "SEK".into(),
        provider: ProviderCode::new("YAHOO"),
        fetched_at: Utc
            .from_utc_datetime(&(date + Duration::days(1)).and_hms_opt(0, 0, 0).unwrap()),
    }
}
fn forward(rows: &[HistoryClose], splits: &[NaiveDate]) -> Result<Decimal, Vec<Exclusion>> {
    forward_return(
        rows,
        &close(0),
        rows.len() - 1,
        "SEK",
        splits,
        &split_adjusted,
    )
}

#[test]
fn finality_uses_utc_date_and_later_refetches_remain_final() {
    let mut c = close(0);
    c.fetched_at = "2025-01-06T23:59:59Z".parse().unwrap();
    assert!(!is_final(&c));
    c.fetched_at = "2025-01-07T00:00:00Z".parse().unwrap();
    assert!(is_final(&c));
    c.fetched_at = "2026-01-07T00:00:00Z".parse().unwrap();
    assert!(is_final(&c));
    c.fetched_at = chrono::DateTime::parse_from_rfc3339("2025-01-07T00:30:00+02:00")
        .unwrap()
        .with_timezone(&Utc);
    assert!(!is_final(&c));
    assert!(final_closes_as_of(&[c], day()).is_empty());
}

#[test]
fn horizon_counts_sessions_before_filters_and_explains_every_exclusion() {
    let clean = vec![close(0), close(1), close(2)];
    assert_eq!(forward(&clean, &[]), Ok(dec!(2)));
    for reason in [
        Exclusion::NonFinal,
        Exclusion::Source,
        Exclusion::Currency,
        Exclusion::InvalidClose,
    ] {
        let mut rows = clean.clone();
        match reason {
            Exclusion::NonFinal => {
                rows[2].fetched_at = rows[2].date.and_hms_opt(12, 0, 0).unwrap().and_utc()
            }
            Exclusion::Source => rows[2].provider = ProviderCode::new("NASDAQ_NORDIC"),
            Exclusion::Currency => rows[2].currency = "USD".into(),
            Exclusion::InvalidClose => rows[0].close = Decimal::ZERO,
            _ => unreachable!(),
        }
        assert!(
            forward_return(&rows, &rows[0], 2, "SEK", &[], &split_adjusted)
                .unwrap_err()
                .contains(&reason)
        );
    }
    assert!(forward(&[close(0), close(10)], &[])
        .unwrap_err()
        .contains(&Exclusion::Gap));
    assert!(forward(&clean, &[day() + Duration::days(1)])
        .unwrap_err()
        .contains(&Exclusion::Split));
    let mut restated = clean.clone();
    for c in &mut restated {
        c.fetched_at = close(20).fetched_at;
    }
    assert!(forward_return(
        &restated,
        &restated[0],
        2,
        "SEK",
        &[day() + Duration::days(1)],
        &split_adjusted
    )
    .is_ok());
    for c in &mut restated {
        c.provider = ProviderCode::new("NASDAQ_NORDIC");
    }
    assert!(forward_return(
        &restated,
        &restated[0],
        2,
        "SEK",
        &[day() + Duration::days(1)],
        &split_adjusted
    )
    .unwrap_err()
    .contains(&Exclusion::Split));
    assert_eq!(
        forward_return(&clean, &close(0), 5, "SEK", &[], &split_adjusted),
        Err(vec![Exclusion::Incomplete])
    );
    assert_eq!(
        forward_return(&clean, &close(-1), 1, "SEK", &[], &split_adjusted),
        Err(vec![Exclusion::Incomplete])
    );
}

#[test]
fn non_final_interior_bars_are_missing_and_the_gap_rule_applies() {
    let mut rows: Vec<_> = (0..12).map(close).collect();
    // One missing bar is tolerated; it does not shift the chosen endpoint.
    rows[1].fetched_at = rows[1].date.and_hms_opt(12, 0, 0).unwrap().and_utc();
    assert_eq!(forward(&rows, &[]), Ok(dec!(11)));
    for c in &mut rows[1..11] {
        c.fetched_at = c.date.and_hms_opt(12, 0, 0).unwrap().and_utc();
    }
    assert!(forward(&rows, &[]).unwrap_err().contains(&Exclusion::Gap));
}

#[test]
fn horizon_starts_at_the_selected_final_close_and_never_skips_an_invalid_session() {
    let mut rows = vec![close(0), close(1), close(4)];
    let mut selected = rows[0].clone();
    selected.provider = ProviderCode::new("NASDAQ_NORDIC");
    rows[0].fetched_at = day().and_hms_opt(12, 0, 0).unwrap().and_utc();
    for c in &mut rows[1..] {
        c.provider = selected.provider.clone();
    }
    assert_eq!(
        forward_return(&rows, &selected, 2, "SEK", &[], &split_adjusted),
        Ok(dec!(4))
    );
    rows[1].fetched_at = rows[1].date.and_hms_opt(12, 0, 0).unwrap().and_utc();
    assert_eq!(
        forward_return(&rows, &selected, 1, "SEK", &[], &split_adjusted),
        Err(vec![Exclusion::NonFinal])
    );
    // Every horizon close across a split must actually be Yahoo-restated.
    let mut rows = vec![close(0), close(1), close(2)];
    for c in &mut rows {
        c.fetched_at = close(20).fetched_at;
    }
    rows[1].provider = ProviderCode::new("NASDAQ_NORDIC");
    assert!(forward_return(
        &rows,
        &rows[0],
        2,
        "SEK",
        &[day() + Duration::days(1)],
        &split_adjusted
    )
    .unwrap_err()
    .contains(&Exclusion::Split));
}

#[test]
fn unusual_drop_can_be_computed_without_full_year_range_coverage() {
    let rows: Vec<_> = (0..160)
        .map(|i| {
            let mut c = close(i);
            c.close = dec!(300) - Decimal::from(i % 50);
            c
        })
        .collect();
    let result = checks_as_of(
        &CheckHistory {
            currency: "SEK",
            split_adjusted_at_fetch: &split_adjusted,
            stored: &rows,
            effective_finals: &rows,
            ledger_splits: &[],
            provider_splits: &[],
        },
        rows.last().unwrap().date,
        RecentWindow::OneWeek,
    )
    .unwrap();
    assert_ne!(result.unusual.state, super::super::YesNoUnknown::Unknown);
    assert_eq!(result.off_high.state, price_checks::OffHighState::Unknown);
}

#[test]
fn benchmark_needs_five_usable_others_and_medians_are_exact() {
    assert_eq!(
        benchmark([Some(dec!(1)); 4]),
        Err(Exclusion::BenchmarkUnavailable)
    );
    assert_eq!(benchmark([Some(dec!(1)); 5]), Ok(dec!(1)));
    assert_eq!(
        benchmark([
            Some(dec!(1)),
            Some(dec!(2)),
            Some(dec!(3)),
            Some(dec!(4)),
            None
        ]),
        Err(Exclusion::BenchmarkUnavailable)
    );
    assert_eq!(
        median(&[dec!(6), dec!(1), dec!(5), dec!(2), dec!(4), dec!(3)]),
        Some(dec!(3.5))
    );
    assert_eq!(median(&[dec!(3), dec!(1), dec!(2)]), Some(dec!(2)));
    assert_eq!(median(&[]), None);
}

#[test]
fn sufficiency_requires_every_floor_and_empty_is_insufficient() {
    let size = SampleSize {
        dips: CALIBRATION_MIN_DIPS,
        flagged: CALIBRATION_MIN_FLAGGED_DIPS,
        flagged_instruments: CALIBRATION_MIN_INSTRUMENTS,
        flagged_weeks: CALIBRATION_MIN_WEEKS,
    };
    assert_eq!(sufficiency(size), Sufficiency::Calibrated);
    for smaller in [
        SampleSize {
            dips: size.dips - 1,
            ..size
        },
        SampleSize {
            flagged: size.flagged - 1,
            ..size
        },
        SampleSize {
            flagged_instruments: size.flagged_instruments - 1,
            ..size
        },
        SampleSize {
            flagged_weeks: size.flagged_weeks - 1,
            ..size
        },
        SampleSize::default(),
    ] {
        assert_eq!(sufficiency(smaller), Sufficiency::InsufficientHistory);
    }
}

#[test]
fn later_closes_cannot_change_an_as_of_check() {
    let rows: Vec<_> = (0..380)
        .map(|i| {
            let mut c = close(i);
            c.close = dec!(300) - Decimal::from(i % 100);
            c
        })
        .collect();
    let as_of = day() + Duration::days(365);
    let history = |r| CheckHistory {
        currency: "SEK",
        split_adjusted_at_fetch: &split_adjusted,
        stored: r,
        effective_finals: r,
        ledger_splits: &[],
        provider_splits: &[],
    };
    let before = checks_as_of(&history(&rows[..366]), as_of, RecentWindow::OneWeek).unwrap();
    let mut later = rows.clone();
    for c in &mut later[366..] {
        c.close = dec!(100000);
        c.provider = ProviderCode::new("OTHER");
    }
    let after = checks_as_of(&history(&later), as_of, RecentWindow::OneWeek).unwrap();
    assert_eq!(before.unusual, after.unusual);
    assert_eq!(before.off_high, after.off_high);
    assert_eq!(before.stopped, after.stopped);
    assert_eq!(before.change, after.change);
}

#[test]
fn configurable_thresholds_preserve_defaults_and_apply_grid_boundaries() {
    let rows: Vec<_> = (0..366)
        .map(|i| {
            let mut c = close(i);
            c.close = dec!(300) - Decimal::from(i % 100);
            c
        })
        .collect();
    let as_of = rows.last().unwrap().date;
    let result = checks_as_of(
        &CheckHistory {
            currency: "SEK",
            split_adjusted_at_fetch: &split_adjusted,
            stored: &rows,
            effective_finals: &rows,
            ledger_splits: &[],
            provider_splits: &[],
        },
        as_of,
        RecentWindow::OneWeek,
    )
    .unwrap();
    assert_eq!(
        result.unusual,
        price_checks::unusual_drop_with_threshold(
            &result.series,
            RecentWindow::OneWeek,
            result.start,
            result.change,
            None,
            UNUSUAL_DROP_DEVIATIONS
        )
    );
    assert_eq!(
        result.off_high,
        price_checks::off_high_with_threshold(&result.series, DEEP_BELOW_HIGH_PERCENT)
    );
    assert_eq!(
        result.stopped,
        price_checks::stopped_falling_with_age(
            &result.series,
            result.start,
            result.end,
            STOPPED_FALLING_MIN_DAYS
        )
    );
    let deviation = result.unusual.usual_move_percent.unwrap();
    assert!(deviation > Decimal::ZERO);
    let at_one = price_checks::unusual_drop_with_threshold(
        &result.series,
        RecentWindow::OneWeek,
        result.start,
        -deviation,
        None,
        dec!(1),
    );
    let at_two = price_checks::unusual_drop_with_threshold(
        &result.series,
        RecentWindow::OneWeek,
        result.start,
        -deviation,
        None,
        dec!(2),
    );
    assert_eq!(at_one.state, super::super::YesNoUnknown::Yes);
    assert_eq!(at_two.state, super::super::YesNoUnknown::No);
    assert_eq!(
        price_checks::off_high_with_threshold(&result.series, dec!(20)).state,
        price_checks::OffHighState::Deep
    );
    assert_eq!(
        price_checks::off_high_with_threshold(&result.series, dec!(30)).state,
        price_checks::OffHighState::NotDeep
    );
}

#[test]
fn split_adjustment_uses_supplied_capability_and_not_source_identity() {
    let mut rows = vec![close(0), close(1), close(2)];
    for c in &mut rows {
        c.provider = ProviderCode::new("SYNTHETIC");
        c.fetched_at = close(20).fetched_at;
    }
    let split = [day() + Duration::days(1)];
    assert!(forward_return(&rows, &rows[0], 2, "SEK", &split, &|_| true).is_ok());
    assert!(
        forward_return(&rows, &rows[0], 2, "SEK", &split, &|_| false)
            .unwrap_err()
            .contains(&Exclusion::Split)
    );
    let history = CheckHistory {
        currency: "SEK",
        stored: &rows,
        effective_finals: &rows,
        ledger_splits: &[],
        provider_splits: &split,
        split_adjusted_at_fetch: &|_| true,
    };
    assert!(checks_as_of(&history, rows[2].date, RecentWindow::OneWeek).is_none());
    // A full history propagates the same capability into the look-back series.
    let mut rows: Vec<_> = (0..160).map(close).collect();
    for c in &mut rows {
        c.provider = ProviderCode::new("SYNTHETIC");
    }
    let history = CheckHistory {
        currency: "SEK",
        stored: &rows,
        effective_finals: &rows,
        ledger_splits: &[],
        provider_splits: &[],
        split_adjusted_at_fetch: &|_| true,
    };
    assert!(
        checks_as_of(&history, rows[159].date, RecentWindow::OneWeek)
            .unwrap()
            .series
            .split_adjusted_at_fetch
    );
}

#[test]
fn negative_and_zero_returns_are_valid_and_missing_session_masks_other_reasons() {
    let mut rows = vec![close(0), close(1)];
    rows[1].close = dec!(90);
    assert_eq!(
        forward_return(&rows, &rows[0], 1, "SEK", &[], &split_adjusted),
        Ok(dec!(-10))
    );
    rows[1].close = rows[0].close;
    assert_eq!(
        forward_return(&rows, &rows[0], 1, "SEK", &[], &split_adjusted),
        Ok(Decimal::ZERO)
    );
    rows[0].close = Decimal::ZERO;
    rows[0].fetched_at = day().and_hms_opt(0, 0, 0).unwrap().and_utc();
    assert_eq!(
        forward_return(&rows, &rows[0], 2, "USD", &[day()], &split_adjusted),
        Err(vec![Exclusion::Incomplete])
    );
}
