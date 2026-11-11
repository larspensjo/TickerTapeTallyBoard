use super::*;
use crate::{
    db::{instruments, testing},
    market_data::provider_registry::ProviderRegistry,
    providers::{DailyClose, FakeFxRateProvider, FakePriceProvider},
};
use chrono::{Datelike, FixedOffset, Weekday};
use rust_decimal_macros::dec;
use std::sync::{atomic::AtomicBool, Arc};

fn d(value: &str) -> NaiveDate {
    value.parse().unwrap()
}

fn snapshot(needs: Vec<RemainingNeed>) -> NeedSnapshot {
    NeedSnapshot {
        version: 1,
        symbol: "TEST.ST".into(),
        asset_class: None,
        quote_currency: None,
        instrument_currency: "SEK".into(),
        needs,
    }
}

#[test]
fn internal_gap_becomes_covered_leading_gap_as_lookback_advances() {
    let old = snapshot(vec![RemainingNeed::Gap {
        left: d("2025-11-12"),
        right: d("2025-12-01"),
    }]);
    let current = snapshot(vec![RemainingNeed::LeadingGap {
        first: d("2025-12-01"),
    }]);
    assert!(current.covered_by(&old, d("2025-11-12")));
    assert!(current.covered_by(&old, d("2025-11-20")));
    assert!(!current.covered_by(&old, d("2025-11-11")));
    let expanded = snapshot(vec![RemainingNeed::LeadingGap {
        first: d("2025-12-02"),
    }]);
    assert!(!expanded.covered_by(&old, d("2025-11-20")));
}

#[test]
fn internal_gap_within_a_previous_leading_gap_is_covered() {
    let old = snapshot(vec![RemainingNeed::LeadingGap {
        first: d("2025-12-01"),
    }]);
    let current = snapshot(vec![RemainingNeed::Gap {
        left: d("2025-11-15"),
        right: d("2025-12-01"),
    }]);
    assert!(current.covered_by(&old, d("2025-11-11")));
    let expanded = snapshot(vec![RemainingNeed::Gap {
        left: d("2025-11-15"),
        right: d("2025-12-02"),
    }]);
    assert!(!expanded.covered_by(&old, d("2025-11-11")));
}

#[test]
fn same_split_need_stays_covered_across_days_but_new_split_does_not() {
    let old = snapshot(vec![RemainingNeed::SplitStale {
        date: d("2026-06-01"),
    }]);
    let current = snapshot(vec![RemainingNeed::SplitStale {
        date: d("2026-06-01"),
    }]);
    for start in ["2025-11-11", "2025-11-12", "2026-01-01"] {
        assert!(current.covered_by(&old, d(start)));
    }
    let new_split = snapshot(vec![RemainingNeed::SplitStale {
        date: d("2026-06-02"),
    }]);
    assert!(!new_split.covered_by(&old, d("2025-11-12")));
}

#[tokio::test]
async fn local_day_ahead_of_utc_defers_split_only_and_omits_split_from_mixed_snapshot() {
    for other_need in [false, true] {
        let pool = testing::memory_pool().await;
        let instrument = instruments::upsert(
            &pool,
            &instruments::NewInstrument {
                symbol: "TEST".into(),
                exchange: "TEST".into(),
                name: "Test".into(),
                kind: "STOCK".into(),
                currency: "SEK".into(),
                isin: None,
            },
        )
        .await
        .unwrap()
        .0;
        let mapping = provider_symbols::upsert(
            &pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: instrument.id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "TEST.ST".into(),
                asset_class: None,
                currency: Some("SEK".into()),
                enabled: true,
                created_at: "test".into(),
                updated_at: "test".into(),
            },
        )
        .await
        .unwrap();
        let clock = Clock::fixed_instant("2026-11-11T23:30:00Z".parse().unwrap());
        // Supply the pass's local calendar explicitly, independent of the test host's timezone.
        let local_day = clock
            .now_utc()
            .with_timezone(&FixedOffset::east_opt(3600).unwrap())
            .date_naive();
        assert_eq!(local_day, d("2026-11-12"));
        let dates = (local_day - Duration::days(if other_need { 160 } else { 365 }))
            .iter_days()
            .take_while(|date| *date <= local_day)
            .filter(|date| !matches!(date.weekday(), Weekday::Sat | Weekday::Sun))
            .collect::<Vec<_>>();
        for date in &dates {
            prices::upsert(
                &pool,
                &prices::NewPrice {
                    instrument_id: instrument.id,
                    provider: MarketDataProvider::Yahoo,
                    provider_symbol: "TEST.ST".into(),
                    date: *date,
                    close: dec!(100),
                    currency: "SEK".into(),
                    fetched_at: "2026-11-10T00:00:00Z".into(),
                },
            )
            .await
            .unwrap();
        }
        sqlx::query("INSERT INTO fundamental_split_events (instrument_id, provider_symbol, split_date, numerator, denominator, updated_at) VALUES (?, 'TEST.ST', '2026-11-11', '2', '1', 'test')")
            .bind(instrument.id).execute(&pool).await.unwrap();
        let run = match market_data_runs::try_claim_run(
            &pool,
            "MANUAL",
            "test-owner",
            clock.now_utc(),
            std::time::Duration::from_secs(120),
        )
        .await
        .unwrap()
        {
            market_data_runs::ClaimResult::Claimed { run, .. } => run,
            _ => panic!("expected claim"),
        };
        let lease = RefreshLease {
            run_id: run.id,
            owner: "test-owner".into(),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let target = RefreshTarget {
            instrument,
            currency: "SEK".into(),
        };
        let (start, end) = request_range(local_day);
        let context = HistoryContext {
            lease: &lease,
            target: &target,
            provider: Some(mapping.provider),
            symbol: &mapping.provider_symbol,
            start,
            end,
        };
        let fake = FakePriceProvider::new();
        if other_need {
            fake.push_response(Ok(dates
                .iter()
                .map(|date| DailyClose {
                    provider: MarketDataProvider::Yahoo,
                    provider_symbol: "TEST.ST".into(),
                    date: *date,
                    close: dec!(100),
                    currency: "SEK".into(),
                })
                .collect()));
        }
        let providers = ProviderSet::from_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, fake.clone())
                .with_fx_provider(FakeFxRateProvider::new()),
        );
        let grouped = BTreeMap::new();
        let mut stop = false;
        let first = top_up_one(
            &providers, &pool, &mapping, &grouped, &clock, &context, &mut stop,
        )
        .await
        .unwrap();
        assert_eq!(first.is_some(), other_need);
        assert_eq!(
            history_topups::attempted_on(&pool, mapping.id, local_day)
                .await
                .unwrap(),
            other_need
        );
        if other_need {
            let saved = history_topups::last_success(&pool, mapping.id)
                .await
                .unwrap()
                .unwrap();
            let saved: NeedSnapshot =
                serde_json::from_str(saved.remaining_needs.as_deref().unwrap()).unwrap();
            assert!(!saved
                .needs
                .iter()
                .any(|need| matches!(need, RemainingNeed::SplitStale { .. })));
        }
        // For split-only history no reservation blocks a retry after UTC midnight,
        // even though the local calendar has not advanced.
        clock.set_instant(
            if other_need {
                "2026-11-12T23:30:00Z"
            } else {
                "2026-11-12T00:01:00Z"
            }
            .parse()
            .unwrap(),
        );
        let retry_day = clock
            .now_utc()
            .with_timezone(&FixedOffset::east_opt(3600).unwrap())
            .date_naive();
        assert_eq!(
            retry_day,
            if other_need {
                local_day + Duration::days(1)
            } else {
                local_day
            }
        );
        let (start, end) = request_range(retry_day);
        let retry_context = HistoryContext {
            start,
            end,
            ..context
        };
        fake.push_response(Ok(Vec::new()));
        let second = top_up_one(
            &providers,
            &pool,
            &mapping,
            &grouped,
            &clock,
            &retry_context,
            &mut stop,
        )
        .await
        .unwrap();
        assert!(second.is_some());
        assert_eq!(fake.calls().len(), 1 + usize::from(other_need));
        assert_eq!(fake.calls().last().unwrap().end, retry_day);
    }
}
