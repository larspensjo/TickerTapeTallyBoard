//! These tests use recorded responses and in-process providers, never a network.
use super::*;
use crate::{
    db::market_indexes,
    domain::market_index::{MarketIndex, MARKET_INDEXES},
    providers::{PriceHistory, PriceHistoryRequest, ProviderResult, YahooChartClient},
};
use chrono::Duration;
use std::sync::Mutex;

fn today() -> NaiveDate {
    "2026-11-13".parse().unwrap()
}
fn latest() -> RefreshPricesRequest {
    RefreshPricesRequest {
        mode: RefreshMode::Latest,
        start_date: None,
        end_date: None,
    }
}
fn recorded(index: &MarketIndex) -> PriceHistory {
    let body = match index.provider_symbol {
        "^GSPC" => include_str!("../../../../tests/fixtures/market_data/yahoo_index_gspc.json"),
        "^OMXSPI" => include_str!("../../../../tests/fixtures/market_data/yahoo_index_omxspi.json"),
        "^AEX" => include_str!("../../../../tests/fixtures/market_data/yahoo_index_aex.json"),
        "^STOXX" => include_str!("../../../../tests/fixtures/market_data/yahoo_index_stoxx.json"),
        _ => panic!("fixture missing"),
    };
    YahooChartClient::parse_response(index.provider_symbol, body).unwrap()
}
fn ordered_indexes() -> Vec<&'static MarketIndex> {
    let mut indexes: Vec<_> = MARKET_INDEXES.iter().collect();
    indexes.sort_by_key(|index| index.code);
    indexes
}
#[derive(Clone)]
struct RoutedPrices {
    indexes: FakePriceProvider,
    topup_error: Arc<Mutex<Option<ProviderError>>>,
    calls: Arc<Mutex<Vec<PriceHistoryRequest>>>,
}
impl RoutedPrices {
    fn new() -> Self {
        Self {
            indexes: FakePriceProvider::new(),
            topup_error: Arc::default(),
            calls: Arc::default(),
        }
    }
    fn service(&self) -> MarketDataService {
        MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_price_provider(MarketDataProvider::Yahoo, self.clone())
                .with_fx_provider(AlwaysFx),
        )
    }
}
#[async_trait::async_trait]
impl PriceProvider for RoutedPrices {
    async fn daily_history(&self, request: &PriceHistoryRequest) -> ProviderResult<PriceHistory> {
        self.calls.lock().unwrap().push(request.clone());
        if MARKET_INDEXES
            .iter()
            .any(|index| index.provider_symbol == request.symbol)
        {
            return self.indexes.daily_history(request).await;
        }
        if request.end - request.start > Duration::days(37) {
            if let Some(error) = self.topup_error.lock().unwrap().take() {
                return Err(error);
            }
        }
        Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: request.symbol.clone(),
            date: request.end,
            close: dec!(100),
            currency: request.quote_currency.clone().unwrap_or_else(|| {
                match request.symbol.rsplit_once('.').map(|(_, suffix)| suffix) {
                    Some("AS" | "DE") => "EUR",
                    Some("OL") => "NOK",
                    Some(_) => "SEK",
                    None => "USD",
                }
                .into()
            }),
        }]
        .into())
    }
}
#[derive(Clone)]
struct AlwaysFx;
#[async_trait::async_trait]
impl FxRateProvider for AlwaysFx {
    async fn fx_history(
        &self,
        base: &str,
        quote: &str,
        _start: NaiveDate,
        end: NaiveDate,
    ) -> ProviderResult<Vec<FxRate>> {
        Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: base.into(),
            quote: quote.into(),
            date: end,
            rate: dec!(10),
        }])
    }
}
async fn holding(pool: &SqlitePool, symbol: &str, currency: &str, held: bool) -> i64 {
    let id = instrument(pool, symbol, "BROKER_TAG", currency).await;
    buy(pool, id, "2026-01-01", 1, "100", currency, Some("10")).await;
    if !held {
        sell(pool, id, "2026-02-01", 1, "100", currency, Some("10")).await;
    }
    provider_symbols::upsert(
        pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: symbol.into(),
            asset_class: None,
            currency: Some(currency.into()),
            enabled: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        },
    )
    .await
    .unwrap();
    id
}
async fn all_holdings(pool: &SqlitePool) {
    for (symbol, currency) in [
        ("US", "USD"),
        ("ST.ST", "SEK"),
        ("NL.AS", "EUR"),
        ("EU.DE", "EUR"),
    ] {
        holding(pool, symbol, currency, true).await;
    }
}
async fn run(pool: &SqlitePool, service: &MarketDataService) -> RefreshPricesResponse {
    service
        .refresh(
            pool,
            &Clock::fixed(today()),
            RefreshTrigger::Manual,
            latest(),
        )
        .await
        .unwrap()
}
fn index_items(response: &RefreshPricesResponse) -> Vec<&RefreshItem> {
    response
        .items
        .iter()
        .filter(|item| item.kind == RefreshItemKind::MarketIndex)
        .collect()
}
async fn stored(pool: &SqlitePool, index: &MarketIndex) -> Vec<market_indexes::IndexCloseRow> {
    market_indexes::list_in_range(
        pool,
        index.code,
        MarketDataProvider::Yahoo,
        index.provider_symbol,
        today() - Duration::days(37),
        today(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn recorded_closes_are_stored_separately_after_history_and_overwritten_on_refresh() {
    let pool = db::testing::memory_pool().await;
    all_holdings(&pool).await;
    let routed = RoutedPrices::new();
    let service = routed.service();
    for index in ordered_indexes() {
        routed.indexes.push_history_response(Ok(recorded(index)));
    }
    let response = run(&pool, &service).await;
    assert_eq!(index_items(&response).len(), 4);
    assert_eq!(response.prices_written, 4);
    assert_eq!(response.failed_items, 0);
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    let calls = routed.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 12); // Four prices, four top-ups, then four indexes.
    assert!(calls[..8].iter().all(|call| !call.symbol.starts_with('^')));
    for call in &calls[8..] {
        assert_eq!(call.start, today() - Duration::days(37));
        assert_eq!(call.end, today());
    }
    for index in ordered_indexes() {
        let fixture = recorded(index);
        let rows = stored(&pool, index).await;
        assert_eq!(rows.len(), 27);
        assert_eq!(rows.first().unwrap().date, "2026-10-07");
        assert_eq!(rows.last().unwrap().date, "2026-11-12");
        for (row, expected) in rows.iter().zip(&fixture.closes) {
            assert_eq!(row.close_decimal().unwrap(), expected.close);
            assert_eq!(row.date_value().unwrap(), expected.date);
            assert_eq!(row.currency, index.currency);
            assert_eq!(row.provider, "YAHOO");
            assert_eq!(row.provider_symbol, index.provider_symbol);
        }
        let mut changed = fixture;
        changed.closes[26].close += dec!(1);
        // Events are discarded even if a provider unexpectedly supplies them.
        changed.dividends = crate::providers::DividendCapture::Valid(vec![]);
        routed.indexes.push_history_response(Ok(changed));
    }
    run(&pool, &service).await;
    for index in ordered_indexes() {
        let rows = stored(&pool, index).await;
        assert_eq!(rows.len(), 27);
        assert_eq!(
            rows.last().unwrap().close_decimal().unwrap(),
            recorded(index).closes[26].close + dec!(1)
        );
        assert!(market_indexes::list_in_range(
            &pool,
            index.code,
            MarketDataProvider::NasdaqNordic,
            index.provider_symbol,
            today() - Duration::days(37),
            today()
        )
        .await
        .unwrap()
        .is_empty());
        assert_eq!(
            market_indexes::list_in_range(
                &pool,
                index.code,
                MarketDataProvider::Yahoo,
                index.provider_symbol,
                "2026-11-12".parse().unwrap(),
                "2026-11-12".parse().unwrap()
            )
            .await
            .unwrap()
            .len(),
            1
        );
    }
    assert_eq!(instruments::list(&pool).await.unwrap().len(), 4);
    assert_eq!(
        service
            .status(&pool, &Clock::fixed(today()))
            .await
            .unwrap()
            .instruments
            .len(),
        4
    );
    let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dividend_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(events, 0);
    assert_eq!(prices::list(&pool).await.unwrap().len(), 4);
}

#[tokio::test]
async fn only_unique_indexes_for_currently_held_listings_are_requested() {
    let pool = db::testing::memory_pool().await;
    holding(&pool, "A.ST", "SEK", true).await;
    holding(&pool, "B.ST", "SEK", true).await;
    let sold = holding(&pool, "SOLD.AS", "EUR", false).await;
    set_conviction(&pool, sold, "HIGH").await; // Still a latest price target, not held.
    holding(&pool, "OSLO.OL", "NOK", true).await;
    let disabled = holding(&pool, "X.AS", "EUR", true).await;
    map_yahoo_symbol(&pool, disabled, "X.AS", false).await;
    let routed = RoutedPrices::new();
    routed
        .indexes
        .push_history_response(Ok(recorded(&MARKET_INDEXES[1])));
    routed
        .indexes
        .push_history_response(Ok(recorded(&MARKET_INDEXES[3])));
    let response = run(&pool, &routed.service()).await;
    let calls = routed.indexes.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].symbol, "^STOXX");
    assert!(calls.iter().all(|call| call.symbol != "^AEX"));
    assert_eq!(index_items(&response)[1].symbol_or_pair, "STOXX600");
    assert_eq!(calls[0].symbol, "^OMXSPI");
    assert_eq!(index_items(&response)[0].symbol_or_pair, "OMXSPI");
    assert_eq!(
        serde_json::to_value(index_items(&response)[0]).unwrap()["kind"],
        "market_index"
    );
}

#[tokio::test]
async fn index_failures_never_change_price_fx_status_counts_or_stored_closes() {
    let pool = db::testing::memory_pool().await;
    all_holdings(&pool).await;
    let routed = RoutedPrices::new();
    let service = routed.service();
    for index in ordered_indexes() {
        routed.indexes.push_history_response(Ok(recorded(index)));
    }
    let before = run(&pool, &service).await;
    let price_status = serde_json::to_value(
        service
            .status(&pool, &Clock::fixed(today()))
            .await
            .unwrap()
            .instruments,
    )
    .unwrap();
    let errors = [
        ProviderError::transport("YAHOO", "connection failed"),
        ProviderError::new(
            "YAHOO",
            ProviderMissingReason::ProviderUnavailable,
            "HTTP 503",
        ),
        YahooChartClient::parse_response("^GSPC", "broken JSON").unwrap_err(),
        ProviderError::new(
            "YAHOO",
            ProviderMissingReason::NoDataInRange,
            "empty series",
        ),
    ];
    for error in errors {
        let attempts = if error.reason() == ProviderMissingReason::ProviderUnavailable {
            1
        } else {
            4
        };
        for _ in 0..attempts {
            routed.indexes.push_response(Err(error.clone()));
        }
        let after = run(&pool, &service).await;
        assert_eq!(
            (
                after.status,
                after.prices_written,
                after.fx_rates_written,
                after.unmapped_instruments,
                after.failed_items
            ),
            (
                before.status,
                before.prices_written,
                before.fx_rates_written,
                before.unmapped_instruments,
                before.failed_items
            )
        );
        assert_eq!(index_items(&after).len(), attempts);
        assert!(index_items(&after)
            .iter()
            .all(|item| item.reason.as_deref() == Some(error.reason_code())
                && item.rows_written == 0));
        assert_eq!(
            serde_json::to_value(
                service
                    .status(&pool, &Clock::fixed(today()))
                    .await
                    .unwrap()
                    .instruments
            )
            .unwrap(),
            price_status
        );
        for index in ordered_indexes() {
            assert_eq!(stored(&pool, index).await.len(), 27);
        }
    }
}

#[tokio::test]
async fn rate_limit_and_outage_stop_subsequent_indexes_but_not_later_runs() {
    for reason in [
        ProviderMissingReason::RateLimited,
        ProviderMissingReason::ProviderUnavailable,
    ] {
        let pool = db::testing::memory_pool().await;
        all_holdings(&pool).await;
        let routed = RoutedPrices::new();
        let service = routed.service();
        routed.indexes.push_response(Ok(vec![]));
        routed.indexes.push_response(Err(ProviderError::new(
            "YAHOO",
            reason,
            "fake outage or limit",
        )));
        let response = run(&pool, &service).await;
        assert_eq!(routed.indexes.calls().len(), 2);
        let items = index_items(&response);
        // Only attempted requests get items: the empty response and the provider stop.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].reason.as_deref(), Some("no_data_in_range"));
        assert_eq!(items[1].reason.as_deref(), Some(reason.as_str()));
        assert_eq!(response.prices_written, 4);
        assert_eq!(response.fx_rates_written, 2);
        assert_eq!(response.failed_items, 0);
        assert_eq!(response.status, RefreshRunStatus::Succeeded);
        let status = serde_json::to_value(
            service
                .status(&pool, &Clock::fixed(today()))
                .await
                .unwrap()
                .instruments,
        )
        .unwrap();
        for index in ordered_indexes() {
            routed.indexes.push_history_response(Ok(recorded(index)));
        }
        let next = run(&pool, &service).await;
        assert_eq!(routed.indexes.calls().len(), 6);
        assert_eq!(index_items(&next).len(), 4);
        assert!(index_items(&next)
            .iter()
            .all(|item| item.status == RefreshItemStatus::Fetched));
        assert_eq!(
            serde_json::to_value(
                service
                    .status(&pool, &Clock::fixed(today()))
                    .await
                    .unwrap()
                    .instruments
            )
            .unwrap(),
            status
        );
    }
}

#[tokio::test]
async fn stopped_history_pass_skips_indexes_silently_only_in_that_run() {
    for reason in [
        ProviderMissingReason::RateLimited,
        ProviderMissingReason::ProviderUnavailable,
    ] {
        for fail_bookkeeping in [false, true] {
            let pool = db::testing::memory_pool().await;
            all_holdings(&pool).await;
            if fail_bookkeeping {
                sqlx::query("CREATE TRIGGER fail_history_outcome BEFORE UPDATE ON history_topup_attempts WHEN NEW.outcome='FAILED' BEGIN SELECT RAISE(ABORT, 'fake bookkeeping failure'); END").execute(&pool).await.unwrap();
            }
            let routed = RoutedPrices::new();
            let service = routed.service();
            *routed.topup_error.lock().unwrap() =
                Some(ProviderError::new("YAHOO", reason, "fake outage or limit"));
            let response = run(&pool, &service).await;
            assert!(routed.indexes.calls().is_empty());
            assert!(index_items(&response).is_empty());
            assert_eq!(routed.calls.lock().unwrap().len(), 5); // Four prices, one top-up.
            assert_eq!(response.prices_written, 4);
            assert_eq!(response.fx_rates_written, 2);
            assert_eq!(response.failed_items, 0);
            assert_eq!(response.status, RefreshRunStatus::Succeeded);
            let history: Vec<_> = response
                .items
                .iter()
                .filter(|item| item.kind == RefreshItemKind::History)
                .collect();
            assert_eq!(history.len(), 1);
            assert_eq!(
                history[0].reason.as_deref(),
                Some(if fail_bookkeeping {
                    "repository_error"
                } else {
                    reason.as_str()
                })
            );
            for index in ordered_indexes() {
                routed.indexes.push_history_response(Ok(recorded(index)));
            }
            let next = run(&pool, &service).await;
            assert_eq!(routed.indexes.calls().len(), 4);
            assert_eq!(index_items(&next).len(), 4);
            assert!(index_items(&next)
                .iter()
                .all(|item| item.status == RefreshItemStatus::Fetched));
        }
    }
}

#[tokio::test]
async fn backfill_never_fetches_indexes() {
    let pool = db::testing::memory_pool().await;
    all_holdings(&pool).await;
    let routed = RoutedPrices::new();
    let response = routed
        .service()
        .refresh(
            &pool,
            &Clock::fixed(today()),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: Some("2026-01-01".into()),
                end_date: None,
            },
        )
        .await
        .unwrap();
    assert!(routed.indexes.calls().is_empty());
    assert!(index_items(&response).is_empty());
}

#[tokio::test]
async fn lease_loss_during_index_io_cannot_store_closes_or_request_another_index() {
    let (pool, observer, path) = super::lease_coordination::two_file_pools("index-fence").await;
    all_holdings(&pool).await;
    let routed = RoutedPrices::new();
    let gate = Arc::new(tokio::sync::Notify::new());
    routed.indexes.block_next_call_on(gate.clone());
    routed
        .indexes
        .push_history_response(Ok(recorded(ordered_indexes()[0])));
    let service = MarketDataService::with_provider_registry_and_lease(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, routed.clone())
            .with_fx_provider(FakeFxRateProvider::new()),
        "index-owner".into(),
        std::time::Duration::from_secs(1),
        std::time::Duration::from_secs(3600),
    );
    let task_pool = pool.clone();
    let task = tokio::spawn(async move { run(&task_pool, &service).await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while routed.indexes.calls().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let claim = crate::db::market_data_runs::try_claim_run(
        &observer,
        "MANUAL",
        "new-owner",
        Clock::fixed(today()).now_utc() + Duration::minutes(1),
        std::time::Duration::from_secs(1),
    )
    .await
    .unwrap();
    assert!(matches!(
        claim,
        crate::db::market_data_runs::ClaimResult::Claimed { .. }
    ));
    let revision = crate::data_revision::current(&observer).await.unwrap();
    gate.notify_one();
    let response = task.await.unwrap();
    assert_eq!(response.message.as_deref(), Some("lease_lost"));
    assert_eq!(routed.indexes.calls().len(), 1);
    for index in ordered_indexes() {
        assert!(stored(&observer, index).await.is_empty());
    }
    assert_eq!(
        crate::data_revision::current(&observer).await.unwrap(),
        revision
    );
    super::lease_coordination::close_file_pools(pool, observer, path).await;
}

#[tokio::test]
async fn index_write_failure_rolls_back_the_series_and_continues_other_indexes() {
    let pool = db::testing::memory_pool().await;
    all_holdings(&pool).await;
    sqlx::query("CREATE TRIGGER reject_index BEFORE INSERT ON market_index_closes WHEN NEW.index_code='AEX' AND NEW.date='2026-11-12' BEGIN SELECT RAISE(ABORT, 'test index write failure'); END").execute(&pool).await.unwrap();
    let routed = RoutedPrices::new();
    for index in ordered_indexes() {
        routed.indexes.push_history_response(Ok(recorded(index)));
    }
    let response = run(&pool, &routed.service()).await;
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.failed_items, 0);
    assert_eq!(response.prices_written, 4);
    assert_eq!(routed.indexes.calls().len(), 4);
    let items = index_items(&response);
    assert_eq!(items[0].reason.as_deref(), Some("repository_error"));
    assert_eq!(items[0].rows_written, 0);
    for index in ordered_indexes() {
        assert_eq!(
            stored(&pool, index).await.len(),
            if index.code == "AEX" { 0 } else { 27 }
        );
    }
}
