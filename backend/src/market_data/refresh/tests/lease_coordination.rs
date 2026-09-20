use super::*;

use std::{
    fs,
    path::PathBuf,
    time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
};

async fn two_file_pools(name: &str) -> (SqlitePool, SqlitePool, PathBuf) {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = crate::test_support::workspace_target_path("refresh-service-tests")
        .join(format!("{name}-{nonce}.sqlite"));
    fs::create_dir_all(path.parent().expect("parent")).expect("test parent");
    let location = crate::ledger::resolve(
        &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
        crate::config::Mode::Production,
    )
    .expect("location");
    let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
        .await
        .expect("first pool");
    crate::db::migrate(&first.pool).await.expect("migrate");
    let second = crate::db::open(&location, crate::db::CreateMissing::No)
        .await
        .expect("second pool");
    (first.pool, second.pool, path)
}

fn leased_service(
    price_provider: FakePriceProvider,
    fx_provider: FakeFxRateProvider,
    owner: &str,
    stale_after: StdDuration,
    heartbeat_interval: StdDuration,
) -> MarketDataService {
    MarketDataService::with_provider_registry_and_lease(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, price_provider)
            .with_fx_provider(fx_provider),
        owner.to_owned(),
        stale_after,
        heartbeat_interval,
    )
}

fn leased_search_service(
    price_provider: FakePriceProvider,
    fx_provider: FakeFxRateProvider,
    symbol_search_provider: FakeSymbolSearchProvider,
    owner: &str,
    stale_after: StdDuration,
    heartbeat_interval: StdDuration,
) -> MarketDataService {
    MarketDataService::with_provider_registry_and_lease(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, price_provider)
            .with_symbol_search_provider(MarketDataProvider::Yahoo, symbol_search_provider)
            .with_fx_provider(fx_provider),
        owner.to_owned(),
        stale_after,
        heartbeat_interval,
    )
}

async fn close_file_pools(first: SqlitePool, second: SqlitePool, path: PathBuf) {
    first.close().await;
    second.close().await;
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(format!("{}-wal", path.display()));
    let _ = fs::remove_file(format!("{}-shm", path.display()));
}

#[tokio::test]
async fn independent_services_share_a_running_refresh_and_skip_provider_calls() {
    let (first_pool, second_pool, path) = two_file_pools("shared-flight").await;
    let first_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let first_fx = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let second_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let second_fx = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let first_service = leased_service(
        first_price.clone(),
        first_fx,
        "owner-a",
        StdDuration::from_secs(120),
        StdDuration::from_secs(30),
    );
    let second_service = leased_service(
        second_price.clone(),
        second_fx.clone(),
        "owner-b",
        StdDuration::from_secs(120),
        StdDuration::from_secs(30),
    );
    let instrument_id = instrument(&first_pool, "MSFT", "NASDAQ", "USD").await;
    buy(
        &first_pool,
        instrument_id,
        "2026-06-01",
        10,
        "100",
        "USD",
        Some("10"),
    )
    .await;
    map_yahoo_symbol(&first_pool, instrument_id, "MSFT", true).await;

    let gate = Arc::new(tokio::sync::Notify::new());
    first_price.block_next_call_on(Arc::clone(&gate));
    first_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "MSFT".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 9, 18).expect("date"),
        close: dec!(101),
        currency: "USD".to_owned(),
    }]));
    let clock = Clock::fixed_instant(
        chrono::DateTime::parse_from_rfc3339("2026-09-18T10:00:00Z")
            .expect("time")
            .to_utc(),
    );
    let first_task = {
        let service = first_service.clone();
        let pool = first_pool.clone();
        let clock = clock.clone();
        tokio::spawn(async move {
            service
                .refresh(
                    &pool,
                    &clock,
                    RefreshTrigger::Manual,
                    RefreshPricesRequest {
                        mode: RefreshMode::Latest,
                        start_date: None,
                        end_date: None,
                    },
                )
                .await
        })
    };
    while first_price.calls().is_empty() {
        tokio::task::yield_now().await;
    }

    let held = second_service
        .refresh(
            &second_pool,
            &clock,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("held response");
    let first_status = first_service
        .status(&first_pool, &clock)
        .await
        .expect("first status");
    let second_status = second_service
        .status(&second_pool, &clock)
        .await
        .expect("second status");
    assert_eq!(held.status, RefreshRunStatus::Running);
    assert_eq!(
        held.run_id,
        first_status.latest_run.as_ref().unwrap().run_id
    );
    assert_eq!(
        held.run_id,
        second_status.latest_run.as_ref().unwrap().run_id
    );
    assert!(first_status.refreshing);
    assert!(second_status.refreshing);
    assert!(second_price.calls().is_empty());
    assert!(second_fx.calls().is_empty());

    gate.notify_waiters();
    first_task
        .await
        .expect("task joins")
        .expect("refresh succeeds");
    close_file_pools(first_pool, second_pool, path).await;
}

#[tokio::test]
async fn heartbeat_keeps_a_long_running_service_claim_alive() {
    let (first_pool, second_pool, path) = two_file_pools("heartbeat").await;
    let first_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let first_service = leased_service(
        first_price.clone(),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        "owner-a",
        StdDuration::from_millis(40),
        StdDuration::from_millis(5),
    );
    let second_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let second_service = leased_service(
        second_price.clone(),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        "owner-b",
        StdDuration::from_millis(40),
        StdDuration::from_millis(5),
    );
    let instrument_id = instrument(&first_pool, "MSFT", "NASDAQ", "SEK").await;
    buy(
        &first_pool,
        instrument_id,
        "2026-06-01",
        1,
        "100",
        "SEK",
        None,
    )
    .await;
    map_yahoo_symbol(&first_pool, instrument_id, "MSFT", true).await;
    let gate = Arc::new(tokio::sync::Notify::new());
    first_price.block_next_call_on(Arc::clone(&gate));
    first_price.push_response(Ok(Vec::new()));
    let start = chrono::DateTime::parse_from_rfc3339("2026-09-18T10:00:00Z")
        .expect("time")
        .to_utc();
    let clock = Clock::fixed_instant(start);
    let first_task = {
        let service = first_service.clone();
        let pool = first_pool.clone();
        let clock = clock.clone();
        tokio::spawn(async move {
            service
                .refresh(
                    &pool,
                    &clock,
                    RefreshTrigger::Manual,
                    RefreshPricesRequest {
                        mode: RefreshMode::Latest,
                        start_date: None,
                        end_date: None,
                    },
                )
                .await
        })
    };
    while first_price.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    clock.set_instant(start + chrono::Duration::seconds(10));
    tokio::time::sleep(StdDuration::from_millis(20)).await;

    let held = second_service
        .refresh(
            &second_pool,
            &clock,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("fresh heartbeat keeps claim");
    assert_eq!(held.status, RefreshRunStatus::Running);
    assert!(second_price.calls().is_empty());

    gate.notify_waiters();
    let completed = first_task
        .await
        .expect("task joins")
        .expect("refresh succeeds");
    assert_eq!(completed.started_at, start.to_rfc3339());
    assert_eq!(
        completed.finished_at.as_deref(),
        Some(
            (start + chrono::Duration::seconds(10))
                .to_rfc3339()
                .as_str()
        )
    );
    close_file_pools(first_pool, second_pool, path).await;
}

#[tokio::test]
async fn reclaimed_owner_cannot_write_provider_results_and_stops_work() {
    let (first_pool, second_pool, path) = two_file_pools("stale-write-fence").await;
    let first_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let first_fx = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let first_service = leased_search_service(
        first_price.clone(),
        first_fx.clone(),
        FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo),
        "owner-a",
        StdDuration::from_millis(40),
        StdDuration::from_secs(3600),
    );
    let second_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let second_fx = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let second_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    let second_service = leased_search_service(
        second_price.clone(),
        second_fx.clone(),
        second_search.clone(),
        "owner-b",
        StdDuration::from_millis(40),
        StdDuration::from_secs(3600),
    );
    let instrument_id =
        instrument_with_isin(&first_pool, "OLD", "AVANZA", "USD", "US5949181045").await;
    buy(
        &first_pool,
        instrument_id,
        "2026-06-01",
        1,
        "100",
        "USD",
        Some("10"),
    )
    .await;
    map_yahoo_symbol(&first_pool, instrument_id, "OLD", true).await;

    let gate = Arc::new(tokio::sync::Notify::new());
    first_price.block_next_call_on(Arc::clone(&gate));
    first_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "OLD".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 9, 18).expect("date"),
        close: dec!(100),
        currency: "USD".to_owned(),
    }]));
    first_fx.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 9, 18).expect("date"),
        rate: dec!(10),
    }]));

    second_price.push_response(Err(ProviderError::new(
        "YAHOO",
        ProviderMissingReason::NotListed,
        "old symbol",
    )));
    second_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "NEW".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 9, 18).expect("date"),
        close: dec!(200),
        currency: "USD".to_owned(),
    }]));
    second_search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "NEW".to_owned(),
        quote_type: Some("EQUITY".to_owned()),
        exchange: Some("NYQ".to_owned()),
        name: Some("OLD".to_owned()),
        asset_class: None,
        currency: Some("USD".to_owned()),
    }]));
    second_fx.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 9, 18).expect("date"),
        rate: dec!(20),
    }]));

    let start = chrono::DateTime::parse_from_rfc3339("2026-09-18T10:00:00Z")
        .expect("time")
        .to_utc();
    let clock = Clock::fixed_instant(start);
    let first_task = {
        let service = first_service.clone();
        let pool = first_pool.clone();
        let clock = clock.clone();
        tokio::spawn(async move {
            service
                .refresh(
                    &pool,
                    &clock,
                    RefreshTrigger::Manual,
                    RefreshPricesRequest {
                        mode: RefreshMode::Latest,
                        start_date: None,
                        end_date: None,
                    },
                )
                .await
        })
    };
    while first_price.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    let first_run = market_data_runs::latest(&first_pool)
        .await
        .expect("latest")
        .expect("run");
    clock.set_instant(start + chrono::Duration::seconds(10));

    let second = second_service
        .refresh(
            &second_pool,
            &clock,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("reclaimer refresh succeeds");
    assert_eq!(second.status, RefreshRunStatus::Succeeded);
    gate.notify_waiters();
    let first = first_task
        .await
        .expect("task joins")
        .expect("lease loss is a response, not an error");
    assert_eq!(first.status, RefreshRunStatus::Failed);
    assert_eq!(first.message.as_deref(), Some("lease_lost"));
    assert_eq!(
        first_price.calls().len(),
        1,
        "lease loss stops further calls"
    );
    assert!(first_fx.calls().is_empty());

    let price_rows = prices::list(&second_pool).await.expect("prices");
    assert_eq!(price_rows.len(), 1);
    assert_eq!(price_rows[0].close, "200");
    let fx_rows = fx_rates::list(&second_pool).await.expect("fx rates");
    assert_eq!(fx_rows.len(), 1);
    assert_eq!(fx_rows[0].rate, "20");
    let mapping = provider_symbols::find_by_instrument_provider(
        &second_pool,
        instrument_id,
        MarketDataProvider::Yahoo,
    )
    .await
    .expect("mapping")
    .expect("mapping exists");
    assert_eq!(mapping.provider_symbol, "NEW");
    let abandoned = market_data_runs::find(&second_pool, first_run.id)
        .await
        .expect("run")
        .expect("abandoned row");
    assert_eq!(abandoned.status, "FAILED");
    assert_eq!(abandoned.message.as_deref(), Some("abandoned"));
    assert_eq!(
        market_data_runs::find(&second_pool, second.run_id)
            .await
            .expect("run")
            .expect("reclaimer row")
            .status,
        "SUCCEEDED"
    );
    close_file_pools(first_pool, second_pool, path).await;
}

#[tokio::test]
async fn cancelling_a_refresh_future_releases_its_claim_promptly() {
    let (first_pool, second_pool, path) = two_file_pools("cancelled-refresh").await;
    let price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let service = leased_service(
        price.clone(),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
        "owner-a",
        StdDuration::from_secs(120),
        StdDuration::from_secs(30),
    );
    let instrument_id = instrument(&first_pool, "MSFT", "NASDAQ", "SEK").await;
    buy(
        &first_pool,
        instrument_id,
        "2026-06-01",
        1,
        "100",
        "SEK",
        None,
    )
    .await;
    map_yahoo_symbol(&first_pool, instrument_id, "MSFT", true).await;
    let gate = Arc::new(tokio::sync::Notify::new());
    price.block_next_call_on(gate);
    price.push_response(Ok(Vec::new()));
    let clock = Clock::fixed_instant(
        chrono::DateTime::parse_from_rfc3339("2026-09-18T10:00:00Z")
            .expect("time")
            .to_utc(),
    );
    let task = {
        let service = service.clone();
        let pool = first_pool.clone();
        let clock = clock.clone();
        tokio::spawn(async move {
            service
                .refresh(
                    &pool,
                    &clock,
                    RefreshTrigger::Manual,
                    RefreshPricesRequest {
                        mode: RefreshMode::Latest,
                        start_date: None,
                        end_date: None,
                    },
                )
                .await
        })
    };
    while price.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    let run_id = market_data_runs::latest(&second_pool)
        .await
        .expect("latest")
        .expect("run")
        .id;
    task.abort();
    let _ = task.await;

    tokio::time::timeout(StdDuration::from_secs(2), async {
        loop {
            let row = market_data_runs::find(&second_pool, run_id)
                .await
                .expect("find")
                .expect("row");
            if row.status != "RUNNING" {
                assert_eq!(row.status, "FAILED");
                assert_eq!(row.message.as_deref(), Some("cancelled"));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancel cleanup should not wait for lease expiry");
    assert!(!service
        .is_refreshing(&second_pool, &clock)
        .await
        .expect("live claim"));
    close_file_pools(first_pool, second_pool, path).await;
}
