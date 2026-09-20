use super::*;

mod avanza_symbol_mapping;
mod instrument_selection;
mod lease_coordination;
mod multi_provider;

pub(crate) use multi_provider::{
    empty_search, map_nasdaq_symbol, multi_provider_service, silent_fake_price,
};

use crate::{
    clock::Clock,
    db::{self, fx_rates, instruments, prices, provider_symbols, transactions},
    domain,
    import::now_iso8601,
    providers::{
        DailyClose, FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider, FxProvider,
        FxRate, MarketDataProvider, ProviderMissingReason, SymbolSearchMatch,
    },
};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

fn system_today() -> NaiveDate {
    Clock::System.today()
}

pub(crate) async fn test_state(
    price_provider: FakePriceProvider,
    fx_provider: FakeFxRateProvider,
) -> (SqlitePool, MarketDataService) {
    let pool = db::memory_pool().await.expect("memory pool");
    let service = MarketDataService::with_providers(price_provider, fx_provider);
    (pool, service)
}

#[test]
fn default_services_use_one_process_start_identity() {
    let first =
        MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new());
    let second =
        MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new());
    assert_eq!(first.inner.owner, second.inner.owner);
}

pub(crate) async fn instrument(
    pool: &SqlitePool,
    symbol: &str,
    exchange: &str,
    currency: &str,
) -> i64 {
    let (row, _) = instruments::upsert(
        pool,
        &crate::db::instruments::NewInstrument {
            symbol: symbol.to_owned(),
            exchange: exchange.to_owned(),
            name: symbol.to_owned(),
            kind: "STOCK".to_owned(),
            currency: currency.to_owned(),
            isin: None,
        },
    )
    .await
    .expect("instrument upsert should succeed");
    row.id
}

pub(super) async fn instrument_with_isin(
    pool: &SqlitePool,
    symbol: &str,
    exchange: &str,
    currency: &str,
    isin: &str,
) -> i64 {
    let (row, _) = instruments::upsert(
        pool,
        &crate::db::instruments::NewInstrument {
            symbol: symbol.to_owned(),
            exchange: exchange.to_owned(),
            name: symbol.to_owned(),
            kind: "STOCK".to_owned(),
            currency: currency.to_owned(),
            isin: Some(isin.to_owned()),
        },
    )
    .await
    .expect("instrument upsert should succeed");
    row.id
}

pub(crate) async fn buy(
    pool: &SqlitePool,
    instrument_id: i64,
    trade_date: &str,
    quantity: i64,
    price: &str,
    currency: &str,
    fx_rate_to_base: Option<&str>,
) {
    transactions::insert(
        pool,
        &crate::db::transactions::NewTransaction {
            instrument_id,
            kind: domain::TransactionKind::Buy,
            trade_date: NaiveDate::parse_from_str(trade_date, "%Y-%m-%d").expect("date"),
            quantity,
            price: Some(price.parse().expect("price")),
            dividend_per_share: None,
            currency: Some(currency.to_owned()),
            fx_rate_to_base: fx_rate_to_base.map(|value| value.parse().expect("fx")),
            brokerage: None,
            note: None,
        },
    )
    .await
    .expect("transaction insert should succeed");
}

pub(super) async fn sell(
    pool: &SqlitePool,
    instrument_id: i64,
    trade_date: &str,
    quantity: i64,
    price: &str,
    currency: &str,
    fx_rate_to_base: Option<&str>,
) {
    transactions::insert(
        pool,
        &crate::db::transactions::NewTransaction {
            instrument_id,
            kind: domain::TransactionKind::Sell,
            trade_date: NaiveDate::parse_from_str(trade_date, "%Y-%m-%d").expect("date"),
            quantity: -quantity,
            price: Some(price.parse().expect("price")),
            dividend_per_share: None,
            currency: Some(currency.to_owned()),
            fx_rate_to_base: fx_rate_to_base.map(|value| value.parse().expect("fx")),
            brokerage: None,
            note: None,
        },
    )
    .await
    .expect("transaction insert should succeed");
}

pub(crate) async fn map_yahoo_symbol(
    pool: &SqlitePool,
    instrument_id: i64,
    provider_symbol: &str,
    enabled: bool,
) {
    let now = now_iso8601();
    provider_symbols::upsert(
        pool,
        &crate::db::provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: provider_symbol.to_owned(),
            asset_class: None,
            currency: Some("SEK".to_owned()),
            enabled,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("provider symbol upsert should succeed");
}

pub(crate) async fn set_conviction(pool: &SqlitePool, instrument_id: i64, conviction: &str) {
    instruments::update_conviction(pool, instrument_id, conviction)
        .await
        .expect("update conviction should succeed");
}

#[tokio::test]
async fn latest_refresh_writes_prices_fx_and_seeds_mappings() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![
        DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date"),
            close: dec!(100),
            currency: "USD".to_owned(),
        },
        DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        },
    ]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        rate: dec!(10.5),
    }]));

    let (pool, service) = test_state(price_provider, fx_provider).await;
    let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
    buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

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
    assert_eq!(response.prices_written, 2);
    assert_eq!(response.fx_rates_written, 1);
    assert_eq!(response.unmapped_instruments, 0);
    assert_eq!(response.failed_items, 0);
    assert!(!response.items.is_empty());

    let mapping =
        provider_symbols::find_by_instrument_provider(&pool, msft, MarketDataProvider::Yahoo)
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
    assert!(mapping.enabled);
    assert_eq!(mapping.provider_symbol, "MSFT");

    let prices = prices::list(&pool).await.expect("price list");
    assert_eq!(prices.len(), 2);
    let fx = fx_rates::list(&pool).await.expect("fx list");
    assert_eq!(fx.len(), 1);
}

#[tokio::test]
async fn refresh_rejects_price_rows_with_wrong_currency() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "MSFT".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(101),
        currency: "EUR".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        rate: dec!(10.5),
    }]));
    let (pool, service) = test_state(price_provider, fx_provider).await;
    let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
    buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

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
        .expect("refresh should complete with item failure");

    assert_eq!(response.status, RefreshRunStatus::Partial);
    assert_eq!(response.prices_written, 0);
    assert_eq!(response.failed_items, 1);
    assert_eq!(
        response.items[0].reason.as_deref(),
        Some("currency_mismatch")
    );

    let prices = prices::list(&pool).await.expect("price list");
    assert!(prices.is_empty());
}

#[tokio::test]
async fn backfill_refresh_uses_earliest_transaction_date() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "ASML.DE".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
        close: dec!(600),
        currency: "EUR".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "EUR".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
        rate: dec!(11),
    }]));

    let (pool, service) = test_state(price_provider, fx_provider).await;
    let asml = instrument(&pool, "ASML", "XETR", "EUR").await;
    buy(&pool, asml, "2026-06-10", 3, "600", "EUR", None).await;
    provider_symbols::upsert(
        &pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: asml,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "ASML.DE".to_owned(),
            asset_class: None,
            currency: Some("EUR".to_owned()),
            enabled: true,
            created_at: now_iso8601(),
            updated_at: now_iso8601(),
        },
    )
    .await
    .expect("mapping upsert should succeed");

    let response = service
        .refresh(
            &pool,
            &Clock::System,
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    assert_eq!(response.status, RefreshRunStatus::Succeeded);
    assert_eq!(response.prices_written, 1);
    assert_eq!(response.fx_rates_written, 1);
}

#[tokio::test]
async fn unmapped_instruments_are_reported() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let (pool, service) = test_state(price_provider, fx_provider).await;
    let instrument_id = instrument(&pool, "ABC", "OTC", "USD").await;
    buy(
        &pool,
        instrument_id,
        "2026-06-10",
        1,
        "5",
        "USD",
        Some("10"),
    )
    .await;

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

    assert_eq!(response.unmapped_instruments, 1);
    assert_eq!(response.items[0].status, RefreshItemStatus::Unmapped);
    assert_eq!(response.items[0].reason.as_deref(), Some("symbol_unmapped"));
}

#[tokio::test]
async fn second_refresh_returns_current_running_status_without_starting_new_work() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    let (pool, service) = test_state(price_provider.clone(), fx_provider.clone()).await;
    let msft = instrument(&pool, "MSFT", "NASDAQ", "USD").await;
    buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

    let gate = Arc::new(tokio::sync::Notify::new());
    price_provider.block_next_call_on(Arc::clone(&gate));
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "MSFT".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(101),
        currency: "USD".to_owned(),
    }]));
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        rate: dec!(10.5),
    }]));

    let service_clone = service.clone();
    let pool_clone = pool.clone();
    let first = tokio::spawn(async move {
        service_clone
            .refresh(
                &pool_clone,
                &Clock::System,
                RefreshTrigger::Manual,
                RefreshPricesRequest {
                    mode: RefreshMode::Latest,
                    start_date: None,
                    end_date: None,
                },
            )
            .await
    });

    while price_provider.calls().is_empty() {
        tokio::task::yield_now().await;
    }
    let running = service
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
        .expect("running status should succeed");

    assert_eq!(running.status, RefreshRunStatus::Running);
    gate.notify_waiters();
    let completed = first
        .await
        .expect("task should complete")
        .expect("refresh should succeed");
    assert_eq!(completed.status, RefreshRunStatus::Succeeded);
}
