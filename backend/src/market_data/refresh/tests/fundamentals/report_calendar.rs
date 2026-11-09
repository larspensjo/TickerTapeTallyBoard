use super::*;
use crate::{
    db::report_announcements as repo, domain::report_dates::*,
    providers::FakeEarningsCalendarProvider,
};

fn announcement(date: NaiveDate, kind: AnnouncementKind) -> AnnouncementDate {
    AnnouncementDate {
        date,
        kind,
        fiscal_quarter: Some("2026-12-31".parse().unwrap()),
        session: (kind == AnnouncementKind::CompanyAnnounced)
            .then_some(ReportSession::AfterMarketClose),
    }
}
async fn seed(pool: &SqlitePool, id: i64, date: NaiveDate, kind: AnnouncementKind) {
    let ticker = repo::ticker(pool, id).await.unwrap().unwrap();
    let mut tx = pool.begin().await.unwrap();
    repo::apply(
        &mut tx,
        id,
        &ticker,
        day() - chrono::Duration::days(5),
        &[AnnouncementChange::Insert(announcement(date, kind))],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
async fn wait_for_calendar(fake: &FakeEarningsCalendarProvider) {
    tokio::time::timeout(StdDuration::from_secs(10), async {
        while fake.calls().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn nasdaq_outage_preserves_price_status_figures_and_daily_lookup_bookkeeping() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "TEST").await;
    store(&pool, id, "TEST", "SEK").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "TEST");
    let figures = FakeFundamentalsProvider::new();
    figures.push_response(Ok(vec![value("SEK")]));
    figures.push_listing(Ok(listing()));
    let dates = FakeEarningsCalendarProvider::new();
    dates.push_response(Err(ProviderError::transport("NASDAQ_US", "offline")));
    let service = MarketDataService::with_provider_registry(
        registry(prices.clone(), FakeFxRateProvider::new(), figures)
            .with_earnings_calendar_provider(dates.clone()),
    );
    let result = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(result.status, RefreshRunStatus::Succeeded);
    assert_eq!(result.failed_items, 0);
    assert_eq!(result.fundamentals.failed, 1);
    assert_eq!(result.fundamentals.status, FundamentalsStatus::Partial);
    assert_eq!(
        result.fundamentals.items[0].status,
        FundamentalsItemStatus::Fetched
    );
    assert_eq!(
        result.fundamentals.report_dates[0].reason.as_deref(),
        Some("nasdaq_provider_unavailable")
    );
    assert_eq!(
        fundamental_values::active_values(&pool, id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        market_data_runs::latest(&pool)
            .await
            .unwrap()
            .unwrap()
            .fundamentals_failed,
        1
    );
    prices_for(&prices, "TEST");
    service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(dates.calls(), ["TEST"]);
}
#[tokio::test]
async fn a_none_answer_preserves_schedule_and_past_results_preserve_kind_once_per_day() {
    let pool = db::testing::memory_pool().await;
    let mut ids = vec![];
    let prices = FakePriceProvider::new();
    let dates = FakeEarningsCalendarProvider::new();
    for symbol in ["AAA", "BBB"] {
        let id = target(&pool, symbol).await;
        ids.push(id);
        seed(
            &pool,
            id,
            day() - chrono::Duration::days(2),
            AnnouncementKind::CompanyAnnounced,
        )
        .await;
        prices_for(&prices, symbol);
        dates.push_response(Ok(SymbolReportDate::None));
    }
    let past = day() - chrono::Duration::days(2);
    dates.push_calendar(Ok(["AAA", "BBB"]
        .into_iter()
        .map(|symbol| CalendarSighting {
            symbol: symbol.into(),
            report: announcement(past, AnnouncementKind::Expected),
            presence_only: true,
        })
        .collect()));
    let service = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, prices.clone())
            .with_fx_provider(FakeFxRateProvider::new())
            .with_earnings_calendar_provider(dates.clone()),
    );
    service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    for id in &ids {
        let entries = repo::list(&pool, *id).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].state, AnnouncementState::Active);
        assert_eq!(entries[0].report.kind, AnnouncementKind::CompanyAnnounced);
    }
    for symbol in ["AAA", "BBB"] {
        prices_for(&prices, symbol);
    }
    service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(dates.calendar_calls(), [past]);
    assert_eq!(dates.calls().len(), 2);
    // A fresh successful empty scan tomorrow withdraws; no rows are deleted.
    for symbol in ["AAA", "BBB"] {
        prices_for(&prices, symbol);
        dates.push_response(Ok(SymbolReportDate::UnknownSymbol));
    }
    dates.push_calendar(Ok(vec![]));
    service
        .refresh(
            &pool,
            &Clock::fixed(day() + chrono::Duration::days(1)),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    for id in ids {
        assert_eq!(
            repo::list(&pool, id).await.unwrap()[0].state,
            AnnouncementState::Withdrawn
        );
    }
}
#[tokio::test]
async fn failed_day_scan_is_attempted_only_once_and_never_withdraws() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "TEST").await;
    seed(&pool, id, day(), AnnouncementKind::CompanyAnnounced).await;
    let prices = FakePriceProvider::new();
    let dates = FakeEarningsCalendarProvider::new();
    dates.push_response(Ok(SymbolReportDate::None));
    dates.push_calendar(Err(ProviderError::transport("NASDAQ_US", "offline")));
    let service = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, prices.clone())
            .with_fx_provider(FakeFxRateProvider::new())
            .with_earnings_calendar_provider(dates.clone()),
    );
    for _ in 0..2 {
        prices_for(&prices, "TEST");
        service
            .refresh(
                &pool,
                &Clock::fixed(day()),
                RefreshTrigger::Manual,
                request(),
            )
            .await
            .unwrap();
    }
    assert_eq!(dates.calendar_calls(), [day()]);
    assert_eq!(
        repo::list(&pool, id).await.unwrap()[0].state,
        AnnouncementState::Active
    );
}
#[tokio::test]
async fn same_run_figures_resolve_confirmed_announcement_by_fiscal_quarter() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "TEST").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "TEST");
    let figures = FakeFundamentalsProvider::new();
    figures.push_response(Ok(vec![value("SEK")]));
    figures.push_listing(Ok(listing()));
    let dates = FakeEarningsCalendarProvider::new();
    let mut report = announcement(day(), AnnouncementKind::CompanyAnnounced);
    report.fiscal_quarter = Some(value("SEK").period_end);
    dates.push_response(Ok(SymbolReportDate::Dated(report.clone())));
    dates.push_calendar(Ok(vec![CalendarSighting {
        symbol: "TEST".into(),
        report,
        presence_only: false,
    }]));
    let service = MarketDataService::with_provider_registry(
        registry(prices, FakeFxRateProvider::new(), figures)
            .with_earnings_calendar_provider(dates.clone()),
    );
    service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    let entries = repo::list(&pool, id).await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].state, AnnouncementState::Resolved);
    assert_eq!(
        entries[0].resolved_period_end,
        Some(value("SEK").period_end)
    );
    assert!(dates.calendar_calls().is_empty());
}
#[tokio::test]
async fn identity_changes_during_lookup_supersede_all_announcement_and_lookup_writes() {
    for link_fallback in [false, true] {
        let (first, second, path) =
            super::super::lease_coordination::two_file_pools("report-date-identity").await;
        let id = if link_fallback {
            let id = instrument(&first, "OLD.ST", "NASDAQ", "USD").await;
            set_conviction(&first, id, "HIGH").await;
            map_yahoo_symbol(&first, id, "OLD.ST", true).await;
            id
        } else {
            target(&first, "OLD").await
        };
        if link_fallback {
            // USD listing with a suffix-free figures link; the Yahoo price
            // mapping itself has an exchange suffix and is not a US ticker.
            fundamentals_sources::correct(
                &first,
                id,
                Some(definition("OLD")),
                None,
                "2026-11-07T00:00:00Z",
            )
            .await
            .unwrap();
        }
        let prices = FakePriceProvider::new();
        prices_for(&prices, if link_fallback { "OLD.ST" } else { "OLD" });
        let dates = FakeEarningsCalendarProvider::new();
        dates.push_response(Ok(SymbolReportDate::Dated(announcement(
            day() + chrono::Duration::days(10),
            AnnouncementKind::CompanyAnnounced,
        ))));
        let gate = Arc::new(tokio::sync::Notify::new());
        dates.block_next_call_on(gate.clone());
        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, prices)
                .with_fx_provider(FakeFxRateProvider::new())
                .with_earnings_calendar_provider(dates.clone()),
        );
        let task = {
            let pool = first.clone();
            tokio::spawn(async move {
                service
                    .refresh(
                        &pool,
                        &Clock::fixed(day()),
                        RefreshTrigger::Manual,
                        request(),
                    )
                    .await
            })
        };
        wait_for_calendar(&dates).await;
        if link_fallback {
            fundamentals_sources::correct(
                &second,
                id,
                Some(definition("RIGHT")),
                Some(PreviousLink::SameCompany),
                "2026-11-07T00:00:01Z",
            )
            .await
            .unwrap();
        } else {
            map_yahoo_symbol(&second, id, "RIGHT", true).await;
        }
        gate.notify_one();
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.fundamentals.superseded, 1);
        assert!(repo::list(&second, id).await.unwrap().is_empty());
        let ticker = repo::ticker(&second, id).await.unwrap().unwrap();
        assert!(repo::lookup(&second, id, &ticker)
            .await
            .unwrap()
            .last_lookup_day
            .is_none());
        super::super::lease_coordination::close_file_pools(first, second, path).await;
    }
}
#[tokio::test]
async fn lease_loss_during_nasdaq_io_refuses_announcements_and_lookup_bookkeeping() {
    let (first, second, path) =
        super::super::lease_coordination::two_file_pools("report-date-lease").await;
    let id = target(&first, "TEST").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "TEST");
    let dates = FakeEarningsCalendarProvider::new();
    dates.push_response(Ok(SymbolReportDate::Dated(announcement(
        day() + chrono::Duration::days(10),
        AnnouncementKind::CompanyAnnounced,
    ))));
    let gate = Arc::new(tokio::sync::Notify::new());
    dates.block_next_call_on(gate.clone());
    let service = MarketDataService::with_provider_registry_and_lease(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, prices)
            .with_fx_provider(FakeFxRateProvider::new())
            .with_earnings_calendar_provider(dates.clone()),
        "holder".into(),
        StdDuration::from_secs(120),
        StdDuration::from_secs(3600),
    );
    let task = {
        let pool = first.clone();
        tokio::spawn(async move {
            service
                .refresh(
                    &pool,
                    &Clock::fixed(day()),
                    RefreshTrigger::Manual,
                    request(),
                )
                .await
        })
    };
    wait_for_calendar(&dates).await;
    assert!(matches!(
        market_data_runs::try_claim_run(
            &second,
            "MANUAL",
            "replacement",
            Clock::fixed(day()).now_utc() + chrono::Duration::seconds(121),
            StdDuration::from_secs(120)
        )
        .await
        .unwrap(),
        market_data_runs::ClaimResult::Claimed { .. }
    ));
    gate.notify_one();
    let result = task.await.unwrap().unwrap();
    assert_eq!(result.message.as_deref(), Some("lease_lost"));
    assert!(repo::list(&second, id).await.unwrap().is_empty());
    let ticker = repo::ticker(&second, id).await.unwrap().unwrap();
    assert!(repo::lookup(&second, id, &ticker)
        .await
        .unwrap()
        .last_lookup_day
        .is_none());
    super::super::lease_coordination::close_file_pools(first, second, path).await;
}

#[tokio::test]
async fn nasdaq_rate_limit_skips_remaining_symbols_and_calendar_requests_for_the_run() {
    let pool = db::testing::memory_pool().await;
    let prices = FakePriceProvider::new();
    let dates = FakeEarningsCalendarProvider::new();
    for symbol in ["AAA", "BBB", "CCC", "DDD", "EEE", "FFF", "GGG", "HHH"] {
        let id = target(&pool, symbol).await;
        seed(&pool, id, day(), AnnouncementKind::Expected).await;
        prices_for(&prices, symbol);
    }
    dates.push_response(Err(ProviderError::with_http_status(
        "NASDAQ_US",
        429,
        "limited",
    )));
    let result = MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, prices)
            .with_fx_provider(FakeFxRateProvider::new())
            .with_earnings_calendar_provider(dates.clone()),
    )
    .refresh(
        &pool,
        &Clock::fixed(day()),
        RefreshTrigger::Manual,
        request(),
    )
    .await
    .unwrap();
    assert_eq!(dates.calls(), ["AAA"]);
    assert!(dates.calendar_calls().is_empty());
    assert_eq!(result.fundamentals.failed, 1);
    assert_eq!(result.fundamentals.skipped, 7);
    for outcome in result
        .fundamentals
        .report_dates
        .iter()
        .filter(|i| i.symbol.as_deref() != Some("AAA"))
    {
        assert_eq!(outcome.status, FundamentalsItemStatus::Skipped);
        assert_eq!(outcome.reason.as_deref(), Some("skipped_rate_limited"));
        let ticker = repo::ticker(&pool, outcome.instrument_id)
            .await
            .unwrap()
            .unwrap();
        assert!(repo::lookup(&pool, outcome.instrument_id, &ticker)
            .await
            .unwrap()
            .last_lookup_day
            .is_none());
    }
}

#[tokio::test]
async fn changed_price_mapping_supersedes_old_schedule_without_scanning_or_resolving_it() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "OLD").await;
    seed(
        &pool,
        id,
        day() - chrono::Duration::days(2),
        AnnouncementKind::CompanyAnnounced,
    )
    .await;
    map_yahoo_symbol(&pool, id, "RIGHT", true).await;
    assert!(repo::current(&pool, id).await.unwrap().is_empty());
    let prices = FakePriceProvider::new();
    prices_for(&prices, "RIGHT");
    let dates = FakeEarningsCalendarProvider::new();
    dates.push_response(Ok(SymbolReportDate::None));
    MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, prices)
            .with_fx_provider(FakeFxRateProvider::new())
            .with_earnings_calendar_provider(dates.clone()),
    )
    .refresh(
        &pool,
        &Clock::fixed(day()),
        RefreshTrigger::Manual,
        request(),
    )
    .await
    .unwrap();
    assert_eq!(dates.calls(), ["RIGHT"]);
    assert!(dates.calendar_calls().is_empty());
    let rows = repo::list(&pool, id).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, AnnouncementState::Superseded);
    assert!(rows[0].resolved_period_end.is_none());
}

#[tokio::test]
async fn simultaneous_provider_failures_keep_the_figures_reason_and_count_the_instrument_once() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "TEST").await;
    store(&pool, id, "TEST", "SEK").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "TEST");
    let figures = FakeFundamentalsProvider::new();
    figures.push_response(Err(ProviderError::with_http_status(
        "YAHOO", 429, "limited",
    )));
    let dates = FakeEarningsCalendarProvider::new();
    dates.push_response(Err(ProviderError::transport("NASDAQ_US", "offline")));
    let result = MarketDataService::with_provider_registry(
        registry(prices, FakeFxRateProvider::new(), figures).with_earnings_calendar_provider(dates),
    )
    .refresh(
        &pool,
        &Clock::fixed(day()),
        RefreshTrigger::Manual,
        request(),
    )
    .await
    .unwrap();
    assert_eq!(result.fundamentals.failed, 1);
    assert_eq!(
        result.fundamentals.items[0].reason.as_deref(),
        Some("rate_limited")
    );
    assert_eq!(
        result.fundamentals.report_dates[0].reason.as_deref(),
        Some("nasdaq_provider_unavailable")
    );
}
