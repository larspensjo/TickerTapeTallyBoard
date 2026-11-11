use super::*;
use ticker_tape_tally_board_backend::{db, demo};

async fn fixture(truncated: bool) -> Vec<ReplayInstrument> {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let pool = db::memory_pool().await.unwrap();
    demo::seed(&pool, today).await.unwrap();
    let seed = ticker_tape_tally_board_backend::ledger_analysis::replay_inputs::load(&pool, today)
        .await
        .unwrap();
    pool.close().await;
    demo_fixture(seed, today, truncated).unwrap()
}

#[tokio::test]
async fn full_and_truncated_demo_reach_both_outcomes_without_printing_identifiers() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    for (truncated, expected) in [(false, "Calibrated"), (true, "Insufficient history")] {
        let inputs = fixture(truncated).await;
        let out = run(&inputs, today, false).unwrap();
        assert!(out.contains(&format!("Overall outcome: {expected}")));
        if !truncated {
            for prefix in [
                "window=30d horizon=20 group=flagged_stopped_yes period=whole",
                "window=7d horizon=20 group=buy_more_rose period=whole",
            ] {
                let row = out.lines().find(|line| line.starts_with(prefix)).unwrap();
                let count = row
                    .split_whitespace()
                    .find_map(|word| word.strip_prefix("count="))
                    .unwrap()
                    .parse::<usize>()
                    .unwrap();
                assert!(count >= 5, "demo must exercise usable comparison groups");
            }
        }
        for required in [
            "horizon=5",
            "horizon=10",
            "horizon=20",
            "first_half",
            "second_half",
            "buy_more_flagged",
            "benchmark_unavailable",
            "unusual=1.0",
            "unusual=2.5",
            "deep=30",
            "stopped_age=3",
            "p10_excess_pp=",
        ] {
            assert!(out.contains(required), "missing aggregate category");
        }
        for forbidden in [
            "2026-11-11",
            "YAHOO",
            "MSFT",
            "SEK",
            "instrument_id",
            "quantity",
            "fetched_at",
        ] {
            assert!(!out.contains(forbidden), "identifier escaped");
        }
    }
}

#[test]
fn empty_replay_is_successfully_insufficient_and_all_statistics_are_suppressed() {
    let out = run(&[], NaiveDate::from_ymd_opt(2026, 11, 11).unwrap(), false).unwrap();
    assert!(out.contains("Overall outcome: Insufficient history"));
    assert!(out.contains("median_excess_pp=suppressed_or_unavailable positive=suppressed p10_excess_pp=suppressed_or_unavailable"));
    assert_eq!(overall([]), Sufficiency::InsufficientHistory);
}

#[tokio::test]
async fn ledger_held_dates_and_pre_buy_order_control_membership() {
    let mut inputs = fixture(false).await;
    let instrument = &mut inputs[0];
    let buy = instrument.ledger[0].clone();
    assert!(!position(&instrument.ledger, buy.trade_date - Duration::days(1)).unwrap());
    assert!(position(&instrument.ledger, buy.trade_date).unwrap());
    let mut next = buy.clone();
    next.id += 1;
    let mut close = buy.clone();
    close.id += 2;
    close.kind = TransactionKind::Sell;
    close.quantity = -2;
    let mut reopen = buy.clone();
    reopen.id += 3;
    let mut split = buy.clone();
    split.id += 4;
    split.kind = TransactionKind::Split;
    let mut after_split = buy.clone();
    after_split.id += 5;
    let mut final_sell = buy.clone();
    final_sell.id += 6;
    final_sell.kind = TransactionKind::Sell;
    final_sell.quantity = -3;
    instrument.ledger = vec![buy, next, close, reopen, split, after_split, final_sell];
    // Only the second buy and the buy after the split qualify. Reopening does
    // not, and the final closed position must not erase either historical buy.
    let events = buy_more_dates(&inputs[..1]).unwrap();
    assert_eq!(events.values().flatten().count(), 2);
    assert!(!position(
        &inputs[0].ledger,
        NaiveDate::from_ymd_opt(2026, 11, 11).unwrap()
    )
    .unwrap());
}

#[tokio::test]
async fn halves_split_dates_with_universe_members_even_when_history_is_only_recent() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let mut inputs = fixture(false).await;
    for instrument in &mut inputs {
        instrument.ledger[0].trade_date -= Duration::days(2000);
        instrument
            .stored
            .retain(|c| c.date >= today - Duration::days(365));
        instrument.sessions =
            ticker_tape_tally_board_backend::ledger_analysis::replay_inputs::resolve_closes(
                &instrument.stored,
                false,
            );
        instrument.finals =
            ticker_tape_tally_board_backend::ledger_analysis::replay_inputs::resolve_closes(
                &instrument.stored,
                true,
            );
    }
    let schedule = dates(&inputs, today);
    let midpoint = universe_midpoint(&inputs, &schedule, today).unwrap();
    assert!(schedule[schedule.len() / 2] < today - Duration::days(365));
    assert!(midpoint > today - Duration::days(365));
    let mut eligible = Vec::new();
    for date in &schedule {
        if inputs.iter().any(|i| {
            WINDOWS.into_iter().any(|w| {
                checks(i, *date, w).is_some_and(|c| {
                    c.unusual.state != YesNoUnknown::Unknown
                        || c.off_high.state != OffHighState::Unknown
                })
            })
        }) {
            eligible.push(*date);
        }
    }
    assert_eq!(midpoint, eligible[eligible.len() / 2]);
    let out = run(&inputs, today, false).unwrap();
    for half in ["first_half", "second_half"] {
        let row = out
            .lines()
            .find(|line| {
                line.starts_with(&format!(
                    "window=7d horizon=20 group=unconditional period={half} "
                ))
            })
            .unwrap();
        assert!(!row.contains("median_excess_pp=suppressed"));
    }
}

#[tokio::test]
async fn future_closes_splits_and_sell_preserve_replay_membership_and_checks() {
    use ticker_tape_tally_board_backend::ledger_analysis::replay_inputs::resolve_closes;
    let end = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let mut inputs = fixture(false).await;
    for i in &mut inputs {
        i.stored.retain(|c| c.date <= end);
        i.ledger.retain(|t| t.trade_date <= end);
        i.sessions = resolve_closes(&i.stored, false);
        i.finals = resolve_closes(&i.stored, true);
    }
    let before = run(&inputs, end, false).unwrap();
    let original = inputs.clone();
    for i in &mut inputs {
        let mut close = i.stored.last().unwrap().clone();
        close.date = end + Duration::days(1);
        close.close = dec!(99999);
        close.fetched_at = (end + Duration::days(10))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc();
        i.stored.push(close);
        i.sessions = resolve_closes(&i.stored, false);
        i.finals = resolve_closes(&i.stored, true);
        i.provider_splits.push(end + Duration::days(1));
        i.ledger_splits.push(end + Duration::days(2));
        let mut split = i.ledger[0].clone();
        split.id = 9998;
        split.trade_date = end + Duration::days(2);
        split.kind = TransactionKind::Split;
        i.ledger.push(split);
        let mut sell = i.ledger[0].clone();
        sell.id = 9999;
        sell.trade_date = end + Duration::days(3);
        sell.kind = TransactionKind::Sell;
        sell.quantity = -1;
        i.ledger.push(sell);
    }
    let after = run(&inputs, end, false).unwrap();
    let counts = |out: &str| -> Vec<String> {
        out.lines()
            .filter(|line| line.starts_with("window=") && line.contains(" selected="))
            .map(|line| line.split(" count=").next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(counts(&before), counts(&after));
    for (a, b) in original.iter().zip(&inputs) {
        for window in WINDOWS {
            let a = checks(a, end, window).unwrap();
            let b = checks(b, end, window).unwrap();
            assert_eq!(a.unusual, b.unusual);
            assert_eq!(a.off_high, b.off_high);
            assert_eq!(a.stopped, b.stopped);
            assert_eq!(a.change, b.change);
        }
    }
}

#[tokio::test]
async fn buy_more_groups_distinguish_rises_absent_unknown_and_unavailable() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let inputs = fixture(false).await;
    let mut c = checks(&inputs[0], today, WINDOWS[0]).unwrap();
    c.change = dec!(1);
    assert_eq!(buy_more_group(Some(&c)), "buy_more_rose");
    c.change = Decimal::ZERO;
    assert_eq!(buy_more_group(Some(&c)), "buy_more_rose");
    c.change = dec!(-1);
    c.unusual.state = YesNoUnknown::Unknown;
    c.off_high.state = OffHighState::Unknown;
    assert_eq!(buy_more_group(Some(&c)), "buy_more_undetermined");
    c.unusual.state = YesNoUnknown::No;
    c.off_high.state = OffHighState::NotDeep;
    assert_eq!(buy_more_group(Some(&c)), "buy_more_unflagged");
    c.unusual.state = YesNoUnknown::Yes;
    assert_eq!(buy_more_group(Some(&c)), "buy_more_flagged");
    assert_eq!(buy_more_group(None), "buy_more_unavailable");
}

#[tokio::test]
async fn buy_more_returns_use_closes_independently_of_trade_price() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let mut inputs = fixture(true).await;
    let before = run(&inputs, today, false).unwrap();
    for instrument in &mut inputs {
        for trade in &mut instrument.ledger {
            trade.price = Some(dec!(999));
        }
    }
    assert_eq!(run(&inputs, today, false).unwrap(), before);
}

#[tokio::test]
async fn company_pairs_require_recent_latest_and_same_month_one_year_earlier() {
    let today = NaiveDate::from_ymd_opt(2026, 11, 11).unwrap();
    let mut inputs = fixture(true).await;
    let i = &mut inputs[0];
    let latest = NaiveDate::from_ymd_opt(2026, 9, 30).unwrap();
    let prior = NaiveDate::from_ymd_opt(2025, 9, 30).unwrap();
    i.observed_eps = vec![(prior, today), (latest, today)];
    assert!(observed_company_pair(i, today));
    assert!(!observed_company_pair(i, today - Duration::days(1)));
    assert!(!observed_company_pair(i, latest + Duration::days(201)));
    i.observed_eps[0].0 = NaiveDate::from_ymd_opt(2025, 8, 31).unwrap();
    assert!(!observed_company_pair(i, today));
}
