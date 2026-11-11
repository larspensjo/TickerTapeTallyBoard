use super::*;
use crate::domain::opportunity_evidence::test_support::date;
use rust_decimal_macros::dec;

#[test]
fn ex_dates_exclude_start_close_and_include_end_close_with_exact_amount_percent() {
    let start = date("2026-11-01");
    let end = date("2026-11-10");
    let coverage = [DividendCoverage {
        start,
        end,
        valid: true,
    }];
    for (day, state) in [
        ("2026-10-31", YesNoUnknown::No),
        ("2026-11-01", YesNoUnknown::No),
        ("2026-11-02", YesNoUnknown::Yes),
        ("2026-11-10", YesNoUnknown::Yes),
        ("2026-11-11", YesNoUnknown::No),
    ] {
        let event = DividendEvent {
            ex_date: date(day),
            amount: dec!(1.25),
            currency: "SEK".into(),
        };
        let out = went_ex_dividend(start, end, dec!(50), "SEK", true, &[event], &coverage);
        assert_eq!(out.state, state);
        assert_eq!(
            out.percent_of_start_close,
            Some(if state == YesNoUnknown::Yes {
                dec!(2.5)
            } else {
                Decimal::ZERO
            })
        );
    }
    let events = [
        DividendEvent {
            ex_date: date("2026-11-02"),
            amount: dec!(1.25),
            currency: "SEK".into(),
        },
        DividendEvent {
            ex_date: end,
            amount: dec!(0.75),
            currency: "SEK".into(),
        },
    ];
    let out = went_ex_dividend(start, end, dec!(50), "SEK", true, &events, &coverage);
    assert_eq!(out.events, events);
    assert_eq!(out.amount, Some(dec!(2)));
    assert_eq!(out.percent_of_start_close, Some(dec!(4)));
}

#[test]
fn missing_coverage_or_failed_validation_override_retained_events() {
    let start = date("2026-11-01");
    let end = date("2026-11-10");
    let event = [DividendEvent {
        ex_date: date("2026-11-05"),
        amount: dec!(1),
        currency: "SEK".into(),
    }];
    let cases = [
        (false, vec![], "dividends_not_reported"),
        (true, vec![], "dividend_coverage_missing"),
        (
            true,
            vec![DividendCoverage {
                start,
                end: date("2026-11-09"),
                valid: true,
            }],
            "dividend_coverage_missing",
        ),
        (
            true,
            vec![
                DividendCoverage {
                    start,
                    end,
                    valid: true,
                },
                DividendCoverage {
                    start: date("2026-11-04"),
                    end: date("2026-11-06"),
                    valid: false,
                },
            ],
            "dividend_capture_invalid",
        ),
    ];
    for (source, coverage, reason) in cases {
        for events in [&[][..], &event[..]] {
            let out = went_ex_dividend(start, end, dec!(50), "SEK", source, events, &coverage);
            assert_eq!(out.state, YesNoUnknown::Unknown);
            assert_eq!(out.reasons[0].code, reason);
            assert_eq!(out.amount, None);
        }
    }
}

#[test]
fn adjacent_valid_captures_cover_but_a_hole_does_not() {
    let start = date("2026-11-01");
    let end = date("2026-11-10");
    let mut coverage = [
        DividendCoverage {
            start,
            end: date("2026-11-04"),
            valid: true,
        },
        DividendCoverage {
            start: date("2026-11-05"),
            end,
            valid: true,
        },
    ];
    assert_eq!(
        went_ex_dividend(start, end, dec!(50), "SEK", true, &[], &coverage).state,
        YesNoUnknown::No
    );
    coverage[1].start = date("2026-11-06");
    assert_eq!(
        went_ex_dividend(start, end, dec!(50), "SEK", true, &[], &coverage).state,
        YesNoUnknown::Unknown
    );
}

#[test]
fn currency_mismatch_is_unknown_and_never_sums_incompatible_amounts() {
    let start = date("2026-11-01");
    let end = date("2026-11-10");
    let coverage = [DividendCoverage {
        start,
        end,
        valid: true,
    }];
    let events = [
        DividendEvent {
            ex_date: date("2026-11-02"),
            amount: dec!(1),
            currency: "SEK".into(),
        },
        DividendEvent {
            ex_date: end,
            amount: dec!(2),
            currency: "EUR".into(),
        },
    ];
    let out = went_ex_dividend(start, end, dec!(50), "SEK", true, &events, &coverage);
    assert_eq!(out.state, YesNoUnknown::Unknown);
    assert_eq!(out.reasons, vec![Reason::new("dividend_currency_mismatch")]);
    assert_eq!(out.events, events);
    assert_eq!(out.amount, None);
    assert_eq!(out.percent_of_start_close, None);

    // Currency validation applies only to events within the close-to-close interval.
    for ex_date in [start, end + chrono::Duration::days(1)] {
        let outside = [DividendEvent {
            ex_date,
            amount: dec!(2),
            currency: "EUR".into(),
        }];
        let out = went_ex_dividend(start, end, dec!(50), "SEK", true, &outside, &coverage);
        assert_eq!(out.state, YesNoUnknown::No);
        assert!(out.reasons.is_empty());
        assert_eq!(out.amount, Some(Decimal::ZERO));
    }
}
