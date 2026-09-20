use super::*;

#[tokio::test]
async fn latest_refresh_fetches_never_traded_convicted_instrument() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "WATCH".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(101),
        currency: "SEK".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

    let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
    let watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
    map_yahoo_symbol(&pool, watch, "WATCH", true).await;
    set_conviction(&pool, watch, "LOW").await;

    let response = service
        .refresh(
            &pool,
            &Clock::System,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 1);
    assert_eq!(response.fx_rates_written, 0);
    assert_eq!(price_provider.calls().len(), 1);
    assert_eq!(price_provider.calls()[0].symbol, "WATCH");
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].status, RefreshItemStatus::Fetched);
    assert_eq!(response.items[0].symbol_or_pair, "WATCH");
}

#[tokio::test]
async fn latest_refresh_skips_closed_other_instrument() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

    let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
    let closed = instrument(&pool, "CLOSED", "NASDAQ", "SEK").await;
    buy(&pool, closed, "2026-06-01", 10, "100", "SEK", None).await;
    sell(&pool, closed, "2026-06-02", 10, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, closed, "CLOSED", true).await;

    let response = service
        .refresh(
            &pool,
            &Clock::System,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    assert!(price_provider.calls().is_empty());
    assert!(response.items.is_empty());
}

#[tokio::test]
async fn latest_refresh_fetches_closed_convicted_instrument() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "CLOSED".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(101),
        currency: "SEK".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

    let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
    let closed = instrument(&pool, "CLOSED", "NASDAQ", "SEK").await;
    buy(&pool, closed, "2026-06-01", 10, "100", "SEK", None).await;
    sell(&pool, closed, "2026-06-02", 10, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, closed, "CLOSED", true).await;
    set_conviction(&pool, closed, "HIGH").await;

    let response = service
        .refresh(
            &pool,
            &Clock::System,
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 1);
    assert_eq!(response.fx_rates_written, 0);
    assert_eq!(price_provider.calls().len(), 1);
    assert_eq!(price_provider.calls()[0].symbol, "CLOSED");
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].status, RefreshItemStatus::Fetched);
}

#[tokio::test]
async fn backfill_refresh_skips_never_traded_instruments() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "HELD.ST".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
        close: dec!(100),
        currency: "SEK".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);

    let (pool, service) = test_state(price_provider.clone(), fx_provider).await;
    let held = instrument(&pool, "HELD", "XSTO", "SEK").await;
    let convicted_watch = instrument(&pool, "WATCH", "NASDAQ", "SEK").await;
    let other_watch = instrument(&pool, "OTHER", "NASDAQ", "SEK").await;
    buy(&pool, held, "2026-06-10", 3, "100", "SEK", None).await;
    map_yahoo_symbol(&pool, held, "HELD.ST", true).await;
    map_yahoo_symbol(&pool, convicted_watch, "WATCH", true).await;
    map_yahoo_symbol(&pool, other_watch, "OTHER", true).await;
    set_conviction(&pool, convicted_watch, "HIGH").await;

    let response = service
        .refresh(
            &pool,
            &Clock::System,
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: None,
                end_date: Some("2026-06-12".to_owned()),
            },
        )
        .await
        .expect("refresh should succeed");

    let calls = price_provider.calls();
    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 1);
    assert_eq!(response.unmapped_instruments, 0);
    assert_eq!(response.failed_items, 0);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].symbol, "HELD.ST");
}
