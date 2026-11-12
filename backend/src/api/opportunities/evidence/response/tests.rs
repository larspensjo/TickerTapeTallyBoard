use super::*;
use crate::domain::{
    opportunity_evidence::{
        dividend_check::{went_ex_dividend, DividendCoverage, DividendEvent},
        lookback_series::{prepare_lookback, HistoryClose},
        price_checks::unusual_drop,
    },
    ProviderCode, RecentWindow,
};
use chrono::Duration;
use rust_decimal_macros::dec;
use serde_json::{json, to_value};

#[test]
fn move_sample_serializes_counts_only_after_the_sample_is_built() {
    let today: NaiveDate = "2026-10-15".parse().unwrap();
    let closes: Vec<_> = (0..130)
        .map(|offset| HistoryClose {
            date: today - Duration::days(129 - offset),
            close: dec!(100) + Decimal::from(offset % 3),
            currency: "SEK".into(),
            provider: ProviderCode::new("TEST"),
            fetched_at: "2026-10-16T12:00:00Z".parse().unwrap(),
        })
        .collect();
    let series = prepare_lookback(today, "SEK", closes.last(), &closes, &[], &[], true);
    for (start, expected) in [
        (closes[0].date, 0),
        (closes[7].date, 1),
        (today - Duration::days(7), 116),
    ] {
        let outcome = unusual_drop(&series, RecentWindow::OneWeek, start, dec!(-10), None);
        assert_eq!(outcome.sample_moves, expected);
        let response: Check<UnusualDropValues> = outcome.into();
        assert_eq!(to_value(response).unwrap()["sample_moves"], json!(expected));
    }
    let mut short = series;
    short.closes.truncate(2);
    let outcome = unusual_drop(&short, RecentWindow::OneWeek, today, dec!(-10), None);
    assert!(outcome
        .reasons
        .iter()
        .any(|reason| matches!(reason, EvidenceReason::PriceHistoryTooShort { .. })));
    let response: Check<UnusualDropValues> = outcome.into();
    assert_eq!(to_value(response).unwrap()["sample_moves"], json!(null));
}

#[test]
fn dividend_events_distinguish_unselected_empty_and_currency_mismatched_samples() {
    let start: NaiveDate = "2026-10-08".parse().unwrap();
    let end: NaiveDate = "2026-10-15".parse().unwrap();
    let coverage = [DividendCoverage {
        start,
        end,
        valid: true,
    }];
    let mismatched = [DividendEvent {
        ex_date: end,
        amount: dec!(1),
        currency: "USD".into(),
    }];
    for (reports_dividends, valid_interval, coverage, events, expected) in [
        (false, true, &coverage[..], &[][..], json!(null)),
        (true, false, &coverage[..], &[][..], json!(null)),
        (true, true, &[][..], &[][..], json!(null)),
        (
            true,
            true,
            &[DividendCoverage {
                start,
                end,
                valid: false,
            }][..],
            &[][..],
            json!(null),
        ),
        (true, true, &coverage[..], &[][..], json!([])),
        (
            true,
            true,
            &coverage[..],
            &mismatched[..],
            json!([{"ex_date": end, "amount": "1", "currency": "USD"}]),
        ),
    ] {
        let outcome = went_ex_dividend(
            start,
            end,
            if valid_interval { dec!(100) } else { dec!(0) },
            "SEK",
            reports_dividends,
            events,
            coverage,
        );
        let response: Check<DividendValues> = outcome.into();
        assert_eq!(to_value(response).unwrap()["events"], expected);
    }
}
