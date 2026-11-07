use super::*;
use crate::{
    db::{
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{
            self, AttemptStatus, Classification, FundamentalsSource, LinkDefinition, LinkOrigin,
            LinkStatus, PreviousLink,
        },
        market_data_runs,
    },
    domain::fundamentals::{FundamentalValue, FundamentalsStatus, Metric, PeriodType},
    market_data::fundamentals_refresh::FundamentalsItemStatus,
    providers::{yahoo_fundamentals::ListingDetails, FakeFundamentalsProvider},
};
use std::time::Duration as StdDuration;

fn day() -> NaiveDate {
    "2026-11-07".parse().unwrap()
}
fn request() -> RefreshPricesRequest {
    RefreshPricesRequest {
        mode: RefreshMode::Latest,
        start_date: None,
        end_date: None,
    }
}
fn value(currency: &str) -> FundamentalValue {
    FundamentalValue {
        period_type: PeriodType::Quarter,
        period_end: "2026-09-30".parse().unwrap(),
        metric: Metric::Revenue,
        value: dec!(100),
        currency: Some(currency.into()),
    }
}
fn definition(symbol: &str) -> LinkDefinition {
    LinkDefinition {
        source: Some(FundamentalsSource::Yahoo),
        symbol: Some(symbol.into()),
        security_type: Some("EQUITY".into()),
        classification: Classification::Equity,
        origin: LinkOrigin::Manual,
        status: LinkStatus::Usable,
        candidates: "[]".into(),
    }
}
fn listing() -> ListingDetails {
    ListingDetails {
        security_type: Some("EQUITY".into()),
        splits: vec![],
    }
}
fn prices_for(fake: &FakePriceProvider, symbol: &str) {
    fake.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: symbol.into(),
        date: day(),
        close: dec!(100),
        currency: "SEK".into(),
    }]));
}
fn registry(
    price: FakePriceProvider,
    fx: FakeFxRateProvider,
    figures: FakeFundamentalsProvider,
) -> ProviderRegistry {
    ProviderRegistry::new()
        .with_price_provider(MarketDataProvider::Yahoo, price)
        .with_fx_provider(fx)
        .with_fundamentals_provider(figures)
}
async fn target(pool: &SqlitePool, symbol: &str) -> i64 {
    let id = instrument(pool, symbol, "STOCKHOLM", "SEK").await;
    set_conviction(pool, id, "HIGH").await;
    map_yahoo_symbol(pool, id, symbol, true).await;
    id
}
fn eur_fx(fake: &FakeFxRateProvider) {
    fake.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "EUR".into(),
        quote: "SEK".into(),
        date: day(),
        rate: dec!(11),
    }]));
}

#[tokio::test]
async fn latest_refresh_stores_figures_completes_new_eur_and_persists_separate_summary() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "EVO.ST").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "EVO.ST");
    let fx = FakeFxRateProvider::new();
    eur_fx(&fx);
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(listing()));
    // This quarter makes the estimated next report due on October 30. A second
    // fetch today would be due unless the once-per-day guard prevents it.
    let mut quarter = value("EUR");
    quarter.period_end = "2026-06-30".parse().unwrap();
    figures.push_response(Ok(vec![quarter]));
    let service = MarketDataService::with_provider_registry(registry(
        prices.clone(),
        fx.clone(),
        figures.clone(),
    ));
    let before = crate::data_revision::current(&pool).await.unwrap();
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
    assert_eq!(result.fx_rates_written, 0);
    assert_eq!(result.fundamentals.status, FundamentalsStatus::Succeeded);
    assert_eq!(result.fundamentals.fetched, 1);
    assert_eq!(result.fundamentals.failed, 0);
    assert!(result
        .fundamentals
        .items
        .iter()
        .any(|i| i.fx_pair.as_deref() == Some("EUR/SEK")
            && i.status == FundamentalsItemStatus::Fetched));
    assert_eq!(
        fx.calls()
            .iter()
            .map(|r| r.base.as_str())
            .collect::<Vec<_>>(),
        ["EUR"]
    );
    assert_eq!(
        fundamental_values::active_values(&pool, id)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        fundamental_values::active_observations(&pool, id)
            .await
            .unwrap()[0]
            .absent_fetch_on,
        None
    );
    let stored = market_data_runs::latest(&pool).await.unwrap().unwrap();
    assert_eq!(
        stored.fundamentals_fetched,
        result.fundamentals.fetched as i64
    );
    assert_eq!(stored.fundamentals_status, result.fundamentals.status);
    let status = service.status(&pool, &Clock::fixed(day())).await.unwrap();
    assert_eq!(
        status.latest_run.unwrap().fundamentals.fetched,
        result.fundamentals.fetched
    );
    let counter = |s: &str| s.rsplit(':').next().unwrap().parse::<i64>().unwrap();
    assert_eq!(
        counter(&crate::data_revision::current(&pool).await.unwrap()) - counter(&before),
        2
    );
    prices_for(&prices, "EVO.ST");
    eur_fx(&fx);
    let again = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(
        again
            .fundamentals
            .items
            .iter()
            .find(|i| i.fx_pair.is_none())
            .unwrap()
            .reason
            .as_deref(),
        Some("not_due")
    );
    assert_eq!(figures.calls().len(), 1);
    assert_eq!(again.fundamentals.status, FundamentalsStatus::Succeeded);
    assert_eq!(again.fundamentals.fetched, 0);
    assert_eq!(again.fundamentals.skipped, 1);
    assert_eq!(again.fundamentals.failed, 0);
    assert_eq!(again.fundamentals.superseded, 0);
    let stored = market_data_runs::latest(&pool).await.unwrap().unwrap();
    assert_eq!(stored.fundamentals_fetched, 0);
    assert_eq!(stored.fundamentals_skipped, 1);
    assert_eq!(stored.fundamentals_failed, 0);
    let observations = fundamental_values::active_observations(&pool, id)
        .await
        .unwrap();
    assert_eq!(observations[0].first_seen_on, day());
}

#[tokio::test]
async fn first_rate_limit_stops_remaining_fetches_and_prices_still_succeed() {
    let pool = db::testing::memory_pool().await;
    let prices = FakePriceProvider::new();
    for symbol in ["AAA.ST", "BBB.ST"] {
        target(&pool, symbol).await;
        prices_for(&prices, symbol);
    }
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(listing()));
    figures.push_response(Err(ProviderError::with_http_status(
        "YAHOO",
        429,
        "rate limited",
    )));
    let service = MarketDataService::with_provider_registry(registry(
        prices,
        FakeFxRateProvider::new(),
        figures.clone(),
    ));
    let result = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Launch,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(result.status, RefreshRunStatus::Succeeded);
    assert_eq!(result.failed_items, 0);
    assert_eq!(result.fundamentals.failed, 1);
    assert_eq!(result.fundamentals.skipped, 1);
    assert_eq!(figures.calls().len(), 1);
    assert_eq!(figures.listing_calls().len(), 1);
    assert_eq!(
        result.fundamentals.items[1].reason.as_deref(),
        Some("skipped_rate_limited")
    );
    assert_eq!(
        market_data_runs::latest(&pool)
            .await
            .unwrap()
            .unwrap()
            .fundamentals_failed,
        1
    );
}

async fn store(pool: &SqlitePool, id: i64, symbol: &str, currency: &str) {
    let link = fundamentals_sources::correct(
        pool,
        id,
        Some(definition(symbol)),
        None,
        "2026-10-01T12:00:00Z",
    )
    .await
    .unwrap();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    fundamental_values::write_fetch(
        &mut tx,
        &FetchWrite {
            instrument_id: id,
            expected: Some(link.token),
            discovery: None,
            values: vec![value(currency)],
            splits: vec![],
            at: "2026-10-01T12:00:00Z".into(),
            seen_on: "2026-10-01".parse().unwrap(),
            previous_success_on: None,
            status: AttemptStatus::Succeeded,
            reason: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
#[tokio::test]
async fn stored_report_currency_uses_ecb_set_and_completion_failure_is_separate() {
    let pool = db::testing::memory_pool().await;
    let prices = FakePriceProvider::new();
    for (symbol, currency) in [("AAA.ST", "EUR"), ("BBB.ST", "TWD")] {
        let id = target(&pool, symbol).await;
        store(&pool, id, symbol, currency).await;
        prices_for(&prices, symbol);
    }
    let figures = FakeFundamentalsProvider::new();
    for _ in 0..2 {
        figures.push_response(Err(ProviderError::transport("YAHOO", "offline")));
    }
    let fx = FakeFxRateProvider::new();
    fx.push_response(Err(ProviderError::transport("FRANKFURTER", "offline")));
    let service = MarketDataService::with_provider_registry(registry(prices, fx.clone(), figures));
    let result = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(
        fx.calls()
            .iter()
            .map(|r| r.base.as_str())
            .collect::<Vec<_>>(),
        ["EUR"]
    );
    assert_eq!(result.status, RefreshRunStatus::Succeeded);
    assert_eq!(result.failed_items, 0);
    assert_eq!(result.fundamentals.failed, 2);
    assert_eq!(
        result
            .fundamentals
            .items
            .iter()
            .filter(|item| item.fx_pair.is_some() && item.status == FundamentalsItemStatus::Failed)
            .count(),
        1
    );
    let stored = market_data_runs::latest(&pool).await.unwrap().unwrap();
    assert_eq!(stored.fundamentals_failed, 2);
}

#[tokio::test]
async fn backfill_never_calls_fundamentals() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "EVO.ST").await;
    buy(&pool, id, "2026-10-01", 1, "100", "SEK", None).await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "EVO.ST");
    let figures = FakeFundamentalsProvider::new();
    let service = MarketDataService::with_provider_registry(registry(
        prices,
        FakeFxRateProvider::new(),
        figures.clone(),
    ));
    let result = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some("2026-10-01".into()),
                end_date: Some("2026-11-07".into()),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.fundamentals.status, FundamentalsStatus::NotRun);
    assert!(figures.calls().is_empty());
    assert!(figures.listing_calls().is_empty());
}

#[tokio::test]
async fn backfill_publishes_committed_prices_when_finishing_the_run_fails() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "EVO.ST").await;
    buy(&pool, id, "2026-10-01", 1, "100", "SEK", None).await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "EVO.ST");
    let service = MarketDataService::with_provider_registry(registry(
        prices,
        FakeFxRateProvider::new(),
        FakeFundamentalsProvider::new(),
    ));
    // Reject both finish and cancellation cleanup so only the recovery bump
    // can expose committed prices.
    sqlx::query("CREATE TRIGGER reject_finish BEFORE UPDATE OF finished_at ON market_data_refresh_runs BEGIN SELECT RAISE(FAIL, 'forced finish failure'); END")
        .execute(&pool).await.unwrap();
    let before = crate::data_revision::current(&pool).await.unwrap();
    let error = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some("2026-10-01".into()),
                end_date: Some("2026-11-07".into()),
            },
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("forced finish failure"));
    assert_eq!(prices::list(&pool).await.unwrap().len(), 1);
    assert_ne!(crate::data_revision::current(&pool).await.unwrap(), before);
}

#[tokio::test]
async fn repository_failures_keep_prior_counts_and_allow_later_instruments() {
    for fail_read in [false, true] {
        let pool = db::testing::memory_pool().await;
        let prices = FakePriceProvider::new();
        let figures = FakeFundamentalsProvider::new();
        let mut ids = Vec::new();
        for symbol in ["AAA.ST", "BBB.ST", "CCC.ST"] {
            let id = target(&pool, symbol).await;
            ids.push(id);
            prices_for(&prices, symbol);
            fundamentals_sources::correct(
                &pool,
                id,
                Some(definition(symbol)),
                None,
                "2026-10-01T12:00:00Z",
            )
            .await
            .unwrap();
            if symbol != "BBB.ST" || !fail_read {
                figures.push_response(Ok(vec![value("SEK")]));
                figures.push_listing(Ok(listing()));
            }
        }
        if fail_read {
            sqlx::query("UPDATE instrument_fundamentals_sources SET classification='INVALID' WHERE instrument_id=?")
                .bind(ids[1]).execute(&pool).await.unwrap();
        } else {
            sqlx::query("CREATE TRIGGER reject_figures BEFORE INSERT ON fundamental_values WHEN NEW.instrument_id=(SELECT id FROM instruments WHERE symbol='BBB.ST') BEGIN SELECT RAISE(FAIL, 'forced figures failure'); END")
                .execute(&pool).await.unwrap();
        }
        let service = MarketDataService::with_provider_registry(registry(
            prices,
            FakeFxRateProvider::new(),
            figures,
        ));
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
        assert_eq!(result.fundamentals.status, FundamentalsStatus::Partial);
        assert_eq!(result.fundamentals.fetched, 2);
        assert_eq!(result.fundamentals.failed, 1);
        let failed = result
            .fundamentals
            .items
            .iter()
            .find(|item| item.instrument_id == ids[1])
            .unwrap();
        assert_eq!(failed.status, FundamentalsItemStatus::Failed);
        assert_eq!(failed.reason.as_deref(), Some("repository_error"));
        let stored = market_data_runs::latest(&pool).await.unwrap().unwrap();
        assert_eq!(stored.fundamentals_status, FundamentalsStatus::Partial);
        assert_eq!(stored.fundamentals_fetched, 2);
        assert_eq!(stored.fundamentals_failed, 1);
        assert_eq!(
            fundamental_values::active_values(&pool, ids[0])
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(fundamental_values::active_values(&pool, ids[1])
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            fundamental_values::active_values(&pool, ids[2])
                .await
                .unwrap()
                .len(),
            1
        );
    }
}

#[tokio::test]
async fn deletion_during_discovery_is_an_instrument_failure_and_refresh_continues() {
    let (first, second, path) =
        super::lease_coordination::two_file_pools("fundamentals-deletion").await;
    let prices = FakePriceProvider::new();
    let figures = FakeFundamentalsProvider::new();
    let removed = target(&first, "AAA.ST").await;
    let retained = target(&first, "BBB.ST").await;
    for symbol in ["AAA.ST", "BBB.ST"] {
        prices_for(&prices, symbol);
        figures.push_response(Ok(vec![value("SEK")]));
        figures.push_listing(Ok(listing()));
    }
    let gate = Arc::new(tokio::sync::Notify::new());
    figures.block_next_call_on(gate.clone());
    let service = MarketDataService::with_provider_registry(registry(
        prices,
        FakeFxRateProvider::new(),
        figures.clone(),
    ));
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
    wait_for_call(&figures).await;
    let mut tx = second.begin_with("BEGIN IMMEDIATE").await.unwrap();
    provider_symbols::delete_by_instrument_id_in_tx(&mut tx, removed)
        .await
        .unwrap();
    prices::delete_by_instrument_id_in_tx(&mut tx, removed)
        .await
        .unwrap();
    fundamental_values::delete_by_instrument_in_tx(&mut tx, removed)
        .await
        .unwrap();
    instruments::delete_in_tx(&mut tx, removed).await.unwrap();
    tx.commit().await.unwrap();
    gate.notify_one();
    let result = task.await.unwrap().unwrap();
    assert_eq!(result.status, RefreshRunStatus::Succeeded);
    assert_eq!(result.fundamentals.fetched, 1);
    assert_eq!(result.fundamentals.failed, 1);
    assert_eq!(result.fundamentals.items[0].instrument_id, removed);
    assert_eq!(
        result.fundamentals.items[0].status,
        FundamentalsItemStatus::Failed
    );
    assert_eq!(
        fundamental_values::active_values(&second, retained)
            .await
            .unwrap()
            .len(),
        1
    );
    let stored = market_data_runs::latest(&second).await.unwrap().unwrap();
    assert_eq!(stored.fundamentals_fetched, 1);
    assert_eq!(stored.fundamentals_failed, 1);
    super::lease_coordination::close_file_pools(first, second, path).await;
}

#[tokio::test]
async fn mapped_etf_is_classified_and_stored_without_fetching_figures() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "IQQK.DE").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "IQQK.DE");
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(ListingDetails {
        security_type: Some("ETF".into()),
        splits: vec![],
    }));
    let service = MarketDataService::with_provider_registry(registry(
        prices,
        FakeFxRateProvider::new(),
        figures.clone(),
    ));
    let result = service
        .refresh(
            &pool,
            &Clock::fixed(day()),
            RefreshTrigger::Manual,
            request(),
        )
        .await
        .unwrap();
    assert_eq!(result.fundamentals.skipped, 1);
    assert!(figures.calls().is_empty());
    assert_eq!(
        fundamentals_sources::find(&pool, id)
            .await
            .unwrap()
            .unwrap()
            .definition
            .status,
        LinkStatus::NotApplicable
    );
}

#[tokio::test]
async fn discovered_link_rediscovers_after_three_fetches_without_quarterly_data() {
    let pool = db::testing::memory_pool().await;
    let id = target(&pool, "EVO.ST").await;
    let prices = FakePriceProvider::new();
    let figures = FakeFundamentalsProvider::new();
    for _ in 0..3 {
        let mut annual = value("SEK");
        annual.period_type = PeriodType::Annual;
        figures.push_response(Ok(vec![annual]));
        figures.push_listing(Ok(listing()));
    }
    figures.push_response(Ok(vec![value("SEK")]));
    figures.push_listing(Ok(listing()));
    let service = MarketDataService::with_provider_registry(registry(
        prices.clone(),
        FakeFxRateProvider::new(),
        figures.clone(),
    ));
    let mut first_generation = None;
    for attempt in 0..4 {
        prices_for(&prices, "EVO.ST");
        service
            .refresh(
                &pool,
                &Clock::fixed(day() + chrono::Duration::days(attempt * 8)),
                RefreshTrigger::Manual,
                request(),
            )
            .await
            .unwrap();
        let link = fundamentals_sources::find(&pool, id)
            .await
            .unwrap()
            .unwrap();
        if attempt < 3 {
            assert_eq!(link.consecutive_empty, attempt + 1);
            assert_eq!(
                link.token.generation,
                *first_generation.get_or_insert(link.token.generation)
            );
        } else {
            assert_eq!(link.consecutive_empty, 0);
            assert_eq!(link.token.generation, first_generation.unwrap() + 1);
        }
    }
    assert_eq!(figures.calls().len(), 4);
}

async fn wait_for_call(figures: &FakeFundamentalsProvider) {
    tokio::time::timeout(StdDuration::from_secs(5), async {
        while figures.calls().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn correction_during_io_supersedes_values_observations_and_bookkeeping() {
    for discovery_race in [false, true] {
        let (first, second, path) =
            super::lease_coordination::two_file_pools("fundamentals-correction").await;
        let id = target(&first, "EVO.ST").await;
        if !discovery_race {
            store(&first, id, "OLD.ST", "SEK").await;
        }
        let prices = FakePriceProvider::new();
        prices_for(&prices, "EVO.ST");
        let figures = FakeFundamentalsProvider::new();
        figures.push_response(Ok(vec![value("EUR")]));
        figures.push_listing(Ok(listing()));
        let gate = Arc::new(tokio::sync::Notify::new());
        figures.block_next_call_on(gate.clone());
        let service = MarketDataService::with_provider_registry(registry(
            prices,
            FakeFxRateProvider::new(),
            figures.clone(),
        ));
        let task = {
            let service = service.clone();
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
        wait_for_call(&figures).await;
        let corrected = fundamentals_sources::correct(
            &second,
            id,
            Some(definition("RIGHT.ST")),
            Some(PreviousLink::WrongCompany),
            "2026-11-07T12:00:00Z",
        )
        .await
        .unwrap();
        gate.notify_one();
        let result = task.await.unwrap().unwrap();
        assert_eq!(result.fundamentals.superseded, 1);
        assert_eq!(result.status, RefreshRunStatus::Succeeded);
        assert!(fundamental_values::active_values(&second, id)
            .await
            .unwrap()
            .is_empty());
        assert!(fundamental_values::active_observations(&second, id)
            .await
            .unwrap()
            .is_empty());
        for sql in [
            "SELECT COUNT(*) FROM fundamental_values WHERE instrument_id=? AND set_aside_reason='wrong company'",
            "SELECT COUNT(*) FROM fundamental_period_observations WHERE instrument_id=? AND set_aside_reason='wrong company'",
        ] {
            let count: i64 = sqlx::query_scalar(sql).bind(id).fetch_one(&second).await.unwrap();
            assert_eq!(count, i64::from(!discovery_race));
        }
        let after = fundamentals_sources::find(&second, id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(after, corrected);
        super::lease_coordination::close_file_pools(first, second, path).await;
    }
}

#[tokio::test]
async fn lease_lost_during_fundamentals_io_refuses_all_writes_and_publishes_prices_early() {
    let (first, second, path) =
        super::lease_coordination::two_file_pools("fundamentals-lease").await;
    let id = target(&first, "EVO.ST").await;
    let prices = FakePriceProvider::new();
    prices_for(&prices, "EVO.ST");
    let figures = FakeFundamentalsProvider::new();
    figures.push_response(Ok(vec![value("SEK")]));
    figures.push_listing(Ok(listing()));
    let gate = Arc::new(tokio::sync::Notify::new());
    figures.block_next_call_on(gate.clone());
    let service = MarketDataService::with_provider_registry_and_lease(
        registry(prices, FakeFxRateProvider::new(), figures.clone()),
        "holder".into(),
        StdDuration::from_secs(120),
        StdDuration::from_secs(3600),
    );
    let before = crate::data_revision::current(&second).await.unwrap();
    let task = {
        let pool = first.clone();
        let service = service.clone();
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
    wait_for_call(&figures).await;
    assert_ne!(
        crate::data_revision::current(&second).await.unwrap(),
        before
    );
    assert!(
        service
            .status(&second, &Clock::fixed(day()))
            .await
            .unwrap()
            .refreshing
    );
    let now = Clock::fixed(day()).now_utc() + chrono::Duration::seconds(121);
    assert!(matches!(
        market_data_runs::try_claim_run(
            &second,
            "MANUAL",
            "replacement",
            now,
            StdDuration::from_secs(120)
        )
        .await
        .unwrap(),
        market_data_runs::ClaimResult::Claimed { .. }
    ));
    gate.notify_one();
    let result = task.await.unwrap().unwrap();
    assert_eq!(result.status, RefreshRunStatus::Failed);
    assert_eq!(result.message.as_deref(), Some("lease_lost"));
    assert!(fundamental_values::active_values(&second, id)
        .await
        .unwrap()
        .is_empty());
    assert!(fundamentals_sources::find(&second, id)
        .await
        .unwrap()
        .is_none());
    super::lease_coordination::close_file_pools(first, second, path).await;
}
