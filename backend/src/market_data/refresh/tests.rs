use super::*;

use crate::{
    clock::Clock,
    db::{self, fx_rates, instruments, prices, provider_symbols, transactions},
    domain,
    market_data::effective_prices,
    providers::{
        DailyClose, FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider, FxProvider,
        FxRate, MarketDataProvider, SymbolSearchMatch,
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

async fn instrument_with_isin(
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

async fn sell(
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
            system_today(),
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
            system_today(),
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
            system_today(),
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
            system_today(),
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
async fn avanza_isin_refresh_seeds_yahoo_mapping_from_search() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "MSFT".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(101),
        currency: "USD".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        rate: dec!(10.5),
    }]));
    let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    symbol_search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "MSFT".to_owned(),
        quote_type: Some("EQUITY".to_owned()),
        exchange: Some("NMS".to_owned()),
        name: Some("Microsoft Corporation".to_owned()),
        asset_class: None,
        currency: None,
    }]));

    let pool = db::memory_pool().await.expect("memory pool");
    let service = MarketDataService::with_symbol_search_providers(
        price_provider,
        fx_provider,
        symbol_search.clone(),
    );
    let msft = instrument_with_isin(&pool, "US5949181045", "AVANZA", "USD", "US5949181045").await;
    buy(&pool, msft, "2026-06-01", 10, "100", "USD", Some("10")).await;

    let response = service
        .refresh(
            &pool,
            system_today(),
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
    assert_eq!(response.unmapped_instruments, 0);
    assert_eq!(symbol_search.calls()[0].query, "US5949181045");

    let mapping =
        provider_symbols::find_by_instrument_provider(&pool, msft, MarketDataProvider::Yahoo)
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
    assert!(mapping.enabled);
    assert_eq!(mapping.provider_symbol, "MSFT");
}

#[tokio::test]
async fn avanza_known_isins_seed_yahoo_mappings_without_search() {
    let today = system_today();
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "IQQK.DE".to_owned(),
        date: today,
        close: dec!(115),
        currency: "EUR".to_owned(),
    }]));
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "GOOGL".to_owned(),
        date: today,
        close: dec!(245),
        currency: "USD".to_owned(),
    }]));
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "TSM".to_owned(),
        date: today,
        close: dec!(243),
        currency: "USD".to_owned(),
    }]));

    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "EUR".to_owned(),
        quote: "SEK".to_owned(),
        date: today,
        rate: dec!(11),
    }]));
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: today,
        rate: dec!(10),
    }]));

    let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    let pool = db::memory_pool().await.expect("memory pool");
    let service = MarketDataService::with_symbol_search_providers(
        price_provider,
        fx_provider,
        symbol_search.clone(),
    );

    let korea = instrument_with_isin(&pool, "IE00B0M63391", "AVANZA", "EUR", "IE00B0M63391").await;
    let alphabet =
        instrument_with_isin(&pool, "US02079K3059", "AVANZA", "USD", "US02079K3059").await;
    let tsm = instrument_with_isin(&pool, "US8740391003", "AVANZA", "USD", "US8740391003").await;
    provider_symbols::upsert(
        &pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: korea,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "IDKO.L".to_owned(),
            asset_class: None,
            currency: Some("EUR".to_owned()),
            enabled: false,
            created_at: now_iso8601(),
            updated_at: now_iso8601(),
        },
    )
    .await
    .expect("stale mapping should insert");
    provider_symbols::upsert(
        &pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: tsm,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "TSMN.MX".to_owned(),
            asset_class: None,
            currency: Some("USD".to_owned()),
            enabled: false,
            created_at: now_iso8601(),
            updated_at: now_iso8601(),
        },
    )
    .await
    .expect("stale mapping should insert");
    buy(&pool, korea, "2026-06-01", 10, "100", "EUR", Some("11")).await;
    buy(&pool, alphabet, "2026-06-01", 10, "100", "USD", Some("10")).await;
    buy(&pool, tsm, "2026-06-01", 10, "100", "USD", Some("10")).await;

    let response = service
        .refresh(
            &pool,
            system_today(),
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
    assert_eq!(response.prices_written, 3);
    assert_eq!(response.fx_rates_written, 2);
    assert_eq!(response.unmapped_instruments, 0);
    assert!(symbol_search.calls().is_empty());

    let cases = [(korea, "IQQK.DE"), (alphabet, "GOOGL"), (tsm, "TSM")];
    for (instrument_id, expected_symbol) in cases {
        let mapping = provider_symbols::find_by_instrument_provider(
            &pool,
            instrument_id,
            MarketDataProvider::Yahoo,
        )
        .await
        .expect("mapping lookup should succeed")
        .expect("mapping should exist");
        assert!(mapping.enabled);
        assert_eq!(mapping.provider_symbol, expected_symbol);
    }
}

#[tokio::test]
async fn stale_avanza_symbol_is_replaced_only_after_newer_candidate_is_verified() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "SKHYV".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 7, 10).expect("date"),
        close: dec!(168),
        currency: "USD".to_owned(),
    }]));
    price_provider.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "SKHY".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
        close: dec!(170),
        currency: "USD".to_owned(),
    }]));
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
        rate: dec!(9.65),
    }]));
    let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    symbol_search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "SKHYV".to_owned(),
        quote_type: Some("EQUITY".to_owned()),
        exchange: Some("NGM".to_owned()),
        name: Some("SK hynix Inc.".to_owned()),
        asset_class: None,
        currency: None,
    }]));
    symbol_search.push_response(Ok(vec![
        SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SKHYV".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NGM".to_owned()),
            name: Some("SK hynix Inc.".to_owned()),
            asset_class: None,
            currency: None,
        },
        SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "SKHY".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NGM".to_owned()),
            name: Some("SK hynix Inc.".to_owned()),
            asset_class: None,
            currency: None,
        },
    ]));

    let pool = db::memory_pool().await.expect("memory pool");
    let service = MarketDataService::with_symbol_search_providers(
        price_provider.clone(),
        fx_provider,
        symbol_search.clone(),
    );
    let (instrument, _) = instruments::upsert(
        &pool,
        &crate::db::instruments::NewInstrument {
            symbol: "US78392B2060".to_owned(),
            exchange: "AVANZA".to_owned(),
            name: "SK Hynix Inc".to_owned(),
            kind: "STOCK".to_owned(),
            currency: "USD".to_owned(),
            isin: Some("US78392B2060".to_owned()),
        },
    )
    .await
    .expect("instrument should insert");
    let sk_hynix = instrument.id;
    map_yahoo_symbol(&pool, sk_hynix, "SKHYV", true).await;
    buy(
        &pool,
        sk_hynix,
        "2026-07-10",
        10,
        "168",
        "USD",
        Some("9.65"),
    )
    .await;

    let response = service
        .refresh(
            &pool,
            system_today(),
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
    assert_eq!(price_provider.calls()[0].symbol, "SKHYV");
    assert_eq!(price_provider.calls()[1].symbol, "SKHY");
    assert_eq!(symbol_search.calls()[0].query, "US78392B2060");
    assert_eq!(symbol_search.calls()[1].query, "SK Hynix Inc");
    let mapping =
        provider_symbols::find_by_instrument_provider(&pool, sk_hynix, MarketDataProvider::Yahoo)
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
    assert!(mapping.enabled);
    assert_eq!(mapping.provider_symbol, "SKHY");
    assert_eq!(mapping.currency.as_deref(), Some("USD"));
}

#[tokio::test]
async fn stale_avanza_symbol_is_preserved_when_candidate_is_not_newer() {
    let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    for symbol in ["OLD", "POSSIBLE"] {
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: symbol.to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 7, 10).expect("date"),
            close: dec!(168),
            currency: "USD".to_owned(),
        }]));
    }
    let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
    fx_provider.push_response(Ok(vec![FxRate {
        provider: FxProvider::Frankfurter,
        base: "USD".to_owned(),
        quote: "SEK".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 7, 13).expect("date"),
        rate: dec!(9.65),
    }]));
    let symbol_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
    symbol_search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "OLD".to_owned(),
        quote_type: Some("EQUITY".to_owned()),
        exchange: Some("NGM".to_owned()),
        name: Some("Example Inc.".to_owned()),
        asset_class: None,
        currency: None,
    }]));
    symbol_search.push_response(Ok(vec![SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "POSSIBLE".to_owned(),
        quote_type: Some("EQUITY".to_owned()),
        exchange: Some("NGM".to_owned()),
        name: Some("Example Inc.".to_owned()),
        asset_class: None,
        currency: None,
    }]));

    let pool = db::memory_pool().await.expect("memory pool");
    let service = MarketDataService::with_symbol_search_providers(
        price_provider.clone(),
        fx_provider,
        symbol_search,
    );
    let (instrument, _) = instruments::upsert(
        &pool,
        &crate::db::instruments::NewInstrument {
            symbol: "US0000000001".to_owned(),
            exchange: "AVANZA".to_owned(),
            name: "Example Inc".to_owned(),
            kind: "STOCK".to_owned(),
            currency: "USD".to_owned(),
            isin: Some("US0000000001".to_owned()),
        },
    )
    .await
    .expect("instrument should insert");
    map_yahoo_symbol(&pool, instrument.id, "OLD", true).await;
    buy(
        &pool,
        instrument.id,
        "2026-07-10",
        10,
        "168",
        "USD",
        Some("9.65"),
    )
    .await;

    let response = service
        .refresh(
            &pool,
            system_today(),
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
    assert_eq!(price_provider.calls().len(), 2);
    let mapping = provider_symbols::find_by_instrument_provider(
        &pool,
        instrument.id,
        MarketDataProvider::Yahoo,
    )
    .await
    .expect("mapping lookup should succeed")
    .expect("mapping should exist");
    assert_eq!(mapping.provider_symbol, "OLD");
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
            system_today(),
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
            system_today(),
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
            system_today(),
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
            system_today(),
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
                system_today(),
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
            system_today(),
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

// ----- Multi-provider refresh -------------------------------------------

fn nasdaq_search_match(orderbook_id: &str, currency: &str, asset_class: &str) -> SymbolSearchMatch {
    SymbolSearchMatch {
        provider: MarketDataProvider::NasdaqNordic,
        provider_symbol: orderbook_id.to_owned(),
        quote_type: None,
        exchange: Some("Warrants".to_owned()),
        name: Some("AVA SAMSUNG TRACKER".to_owned()),
        asset_class: Some(asset_class.to_owned()),
        currency: Some(currency.to_owned()),
    }
}

fn nasdaq_close(orderbook_id: &str, day: u32, close: &str) -> DailyClose {
    DailyClose {
        provider: MarketDataProvider::NasdaqNordic,
        provider_symbol: orderbook_id.to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, day).expect("date"),
        close: close.parse().expect("close"),
        currency: "SEK".to_owned(),
    }
}

/// A service with Yahoo and Nasdaq registered as both price and search
/// providers, plus FX, so a run exercises the full dispatch.
pub(crate) fn multi_provider_service(
    yahoo_price: FakePriceProvider,
    yahoo_search: FakeSymbolSearchProvider,
    nasdaq_price: FakePriceProvider,
    nasdaq_search: FakeSymbolSearchProvider,
    fx_provider: FakeFxRateProvider,
) -> MarketDataService {
    MarketDataService::with_provider_registry(
        ProviderRegistry::new()
            .with_price_provider(MarketDataProvider::Yahoo, yahoo_price)
            .with_price_provider(MarketDataProvider::NasdaqNordic, nasdaq_price)
            .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo_search)
            .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq_search)
            .with_fx_provider(fx_provider),
    )
}

pub(crate) fn silent_fake_price() -> FakePriceProvider {
    FakePriceProvider::with_provider(MarketDataProvider::Yahoo)
}

pub(crate) fn empty_search(provider: MarketDataProvider) -> FakeSymbolSearchProvider {
    let search = FakeSymbolSearchProvider::with_provider(provider);
    for _ in 0..8 {
        search.push_response(Ok(Vec::new()));
    }
    search
}

pub(crate) async fn map_nasdaq_symbol(
    pool: &SqlitePool,
    instrument_id: i64,
    orderbook_id: &str,
    asset_class: &str,
    currency: &str,
    enabled: bool,
) {
    let now = now_iso8601();
    provider_symbols::upsert(
        pool,
        &crate::db::provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: orderbook_id.to_owned(),
            asset_class: Some(asset_class.to_owned()),
            currency: Some(currency.to_owned()),
            enabled,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await
    .expect("nasdaq mapping upsert should succeed");
}

/// The reported defect: instrument 32's shape. Yahoo carries nothing, so the
/// instrument had no mapping, no prices and no valuation at all.
#[tokio::test]
async fn instrument_yahoo_cannot_price_is_valued_from_nasdaq() {
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![
        nasdaq_close("TX2997672", 10, "123.30"),
        nasdaq_close("TX2997672", 11, "124.10"),
    ]));
    let nasdaq_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_search.push_response(Ok(vec![nasdaq_search_match(
        "TX2997672",
        "SEK",
        "TRACKER_CERTIFICATES",
    )]));

    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        nasdaq_search,
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let tracker =
        instrument_with_isin(&pool, "JE00BJ7HNC92", "AVANZA", "SEK", "JE00BJ7HNC92").await;
    buy(&pool, tracker, "2026-06-01", 10, "120", "SEK", None).await;

    let response = service
        .refresh(
            &pool,
            system_today(),
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
    assert_eq!(response.unmapped_instruments, 0);
    assert_eq!(response.prices_written, 2);
    assert_eq!(response.items[0].provider.as_deref(), Some("NASDAQ_NORDIC"));

    let mapping = provider_symbols::find_by_instrument_provider(
        &pool,
        tracker,
        MarketDataProvider::NasdaqNordic,
    )
    .await
    .expect("mapping lookup should succeed")
    .expect("nasdaq mapping should exist");
    assert!(mapping.enabled);
    assert_eq!(mapping.provider_symbol, "TX2997672");
    assert_eq!(mapping.asset_class.as_deref(), Some("TRACKER_CERTIFICATES"));
    assert_eq!(mapping.currency.as_deref(), Some("SEK"));

    let candidate = effective_prices::effective_latest_on_or_before(
        &pool,
        tracker,
        NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
    )
    .await
    .expect("resolution should succeed")
    .expect("a price should resolve");
    assert_eq!(candidate.source.as_str(), "NASDAQ_NORDIC");
    assert_eq!(candidate.close, dec!(124.10));
}

/// Instrument 38's shape: a disabled Yahoo mapping with stored rows in the
/// wrong currency. Those rows must never be promoted into valuation, and the
/// disabled mapping must not be switched back on.
#[tokio::test]
async fn disabled_yahoo_mapping_is_repaired_by_nasdaq_without_promoting_its_rows() {
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![nasdaq_close("TX271", 11, "1369.00")]));
    let nasdaq_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_search.push_response(Ok(vec![nasdaq_search_match("TX271", "SEK", "SHARES")]));

    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        nasdaq_search,
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let azn = instrument_with_isin(&pool, "GB0009895292", "AVANZA", "SEK", "GB0009895292").await;
    buy(&pool, azn, "2026-06-01", 10, "1300", "SEK", None).await;
    map_yahoo_symbol(&pool, azn, "AZN.L", false).await;
    prices::upsert(
        &pool,
        &prices::NewPrice {
            instrument_id: azn,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "AZN.L".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
            close: dec!(110.00),
            currency: "GBP".to_owned(),
            fetched_at: now_iso8601(),
        },
    )
    .await
    .expect("stored yahoo row should insert");

    service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    let yahoo =
        provider_symbols::find_by_instrument_provider(&pool, azn, MarketDataProvider::Yahoo)
            .await
            .expect("mapping lookup should succeed")
            .expect("yahoo mapping should still exist");
    assert!(!yahoo.enabled, "a disabled mapping must not be re-enabled");

    let candidate = effective_prices::effective_latest_on_or_before(
        &pool,
        azn,
        NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
    )
    .await
    .expect("resolution should succeed")
    .expect("a price should resolve");
    assert_eq!(candidate.source.as_str(), "NASDAQ_NORDIC");
    assert_eq!(candidate.close, dec!(1369.00));
}

#[tokio::test]
async fn two_same_currency_candidates_stay_unmapped_and_report_ambiguous() {
    let nasdaq_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_search.push_response(Ok(vec![
        nasdaq_search_match("TX69", "SEK", "SHARES"),
        nasdaq_search_match("TX70", "SEK", "SHARES"),
    ]));

    let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    yahoo_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "WORKS".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(50.00),
        currency: "SEK".to_owned(),
    }]));
    let service = multi_provider_service(
        yahoo_price,
        empty_search(MarketDataProvider::Yahoo),
        FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic),
        nasdaq_search,
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    // A holding that prices normally, so the run is a realistic PARTIAL
    // rather than the degenerate "nothing was written at all" case.
    let works = instrument(&pool, "WORKS", "STO", "SEK").await;
    buy(&pool, works, "2026-06-01", 10, "50", "SEK", None).await;
    map_yahoo_symbol(&pool, works, "WORKS", true).await;
    let ericsson =
        instrument_with_isin(&pool, "SE0000108656", "AVANZA", "SEK", "SE0000108656").await;
    buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;

    let response = service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should complete");

    assert_eq!(response.status, RefreshRunStatus::Partial);
    assert_eq!(response.unmapped_instruments, 1);
    let ambiguous = response
        .items
        .iter()
        .find(|item| item.instrument_id == Some(ericsson))
        .expect("the ambiguous instrument should be reported");
    assert_eq!(ambiguous.status, RefreshItemStatus::Ambiguous);
    assert!(ambiguous
        .reason
        .as_deref()
        .expect("reason")
        .contains("TX69"));

    assert!(provider_symbols::find_by_instrument_provider(
        &pool,
        ericsson,
        MarketDataProvider::NasdaqNordic
    )
    .await
    .expect("mapping lookup should succeed")
    .is_none());
}

#[tokio::test]
async fn currency_narrowing_connects_the_matching_listing() {
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));
    let nasdaq_search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_search.push_response(Ok(vec![
        nasdaq_search_match("TX50143", "EUR", "SHARES"),
        nasdaq_search_match("TX69", "SEK", "SHARES"),
    ]));

    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        nasdaq_search,
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let ericsson =
        instrument_with_isin(&pool, "SE0000108656", "AVANZA", "SEK", "SE0000108656").await;
    buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;

    service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    let mapping = provider_symbols::find_by_instrument_provider(
        &pool,
        ericsson,
        MarketDataProvider::NasdaqNordic,
    )
    .await
    .expect("mapping lookup should succeed")
    .expect("nasdaq mapping should exist");
    assert_eq!(mapping.provider_symbol, "TX69");
    assert_eq!(mapping.currency.as_deref(), Some("SEK"));
}

/// Yahoo wins a shared date; Nasdaq fills the dates Yahoo does not cover.
#[tokio::test]
async fn per_date_precedence_prefers_yahoo_and_lets_nasdaq_fill_gaps() {
    let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    yahoo_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "ERIC-B.ST".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(78.00),
        currency: "SEK".to_owned(),
    }]));
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![
        nasdaq_close("TX69", 11, "77.90"),
        nasdaq_close("TX69", 12, "79.40"),
    ]));

    let service = multi_provider_service(
        yahoo_price,
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let ericsson = instrument(&pool, "ERIC", "STO", "SEK").await;
    buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;
    map_yahoo_symbol(&pool, ericsson, "ERIC-B.ST", true).await;
    map_nasdaq_symbol(&pool, ericsson, "TX69", "SHARES", "SEK", true).await;

    service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");

    let series = effective_prices::effective_series(&pool, ericsson, "SEK", None, None)
        .await
        .expect("series should resolve");
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].source.as_str(), "YAHOO");
    assert_eq!(series[0].close, dec!(78.00));
    assert_eq!(series[1].source.as_str(), "NASDAQ_NORDIC");
    assert_eq!(series[1].close, dec!(79.40));

    let latest = effective_prices::effective_latest_on_or_before(
        &pool,
        ericsson,
        NaiveDate::from_ymd_opt(2026, 6, 13).expect("date"),
    )
    .await
    .expect("resolution should succeed")
    .expect("a price should resolve");
    assert_eq!(latest.source.as_str(), "NASDAQ_NORDIC");
}

/// PARTIAL must keep meaning "action needed": a Nasdaq-only instrument is
/// fully priced and must not inflate the unmapped counter.
#[tokio::test]
async fn partial_still_means_action_needed() {
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));

    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let nordic = instrument(&pool, "ERIC", "STO", "SEK").await;
    buy(&pool, nordic, "2026-06-01", 10, "80", "SEK", None).await;
    map_nasdaq_symbol(&pool, nordic, "TX69", "SHARES", "SEK", true).await;

    let succeeded = service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should succeed");
    assert_eq!(succeeded.status, RefreshRunStatus::Succeeded);
    assert_eq!(succeeded.unmapped_instruments, 0);

    let orphan = instrument(&pool, "NOFEED", "STO", "SEK").await;
    buy(&pool, orphan, "2026-06-01", 10, "80", "SEK", None).await;

    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));
    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let partial = service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should complete");
    assert_eq!(partial.status, RefreshRunStatus::Partial);
    assert_eq!(partial.unmapped_instruments, 1);
}

/// A provider outage is named rather than silently sliding into staleness,
/// and it does not stop the other sources from being written.
#[tokio::test]
async fn provider_outage_reports_unavailable_and_leaves_other_sources_intact() {
    let yahoo_price = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
    yahoo_price.push_response(Ok(vec![DailyClose {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: "ERIC-B.ST".to_owned(),
        date: NaiveDate::from_ymd_opt(2026, 6, 11).expect("date"),
        close: dec!(78.00),
        currency: "SEK".to_owned(),
    }]));
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Err(ProviderError::transport(
        MarketDataProvider::NasdaqNordic.as_str(),
        "connection refused",
    )));

    let service = multi_provider_service(
        yahoo_price,
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let ericsson = instrument(&pool, "ERIC", "STO", "SEK").await;
    buy(&pool, ericsson, "2026-06-01", 10, "80", "SEK", None).await;
    map_yahoo_symbol(&pool, ericsson, "ERIC-B.ST", true).await;
    map_nasdaq_symbol(&pool, ericsson, "TX69", "SHARES", "SEK", true).await;
    // A stored Nasdaq row from an earlier run: valuation must fall back to
    // it with normal staleness rather than losing the holding.
    prices::upsert(
        &pool,
        &prices::NewPrice {
            instrument_id: ericsson,
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: "TX69".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 5).expect("date"),
            close: dec!(74.10),
            currency: "SEK".to_owned(),
            fetched_at: now_iso8601(),
        },
    )
    .await
    .expect("stored nasdaq row should insert");

    let response = service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Manual,
            RefreshPricesRequest {
                mode: RefreshMode::Latest,
                start_date: None,
                end_date: None,
            },
        )
        .await
        .expect("refresh should complete");

    let outage = response
        .items
        .iter()
        .find(|item| item.status == RefreshItemStatus::Unavailable)
        .expect("an unavailable item should be reported");
    assert_eq!(outage.provider.as_deref(), Some("NASDAQ_NORDIC"));
    assert_eq!(outage.reason.as_deref(), Some("provider_unavailable"));

    assert_eq!(response.prices_written, 1);
    let series = effective_prices::effective_series(&pool, ericsson, "SEK", None, None)
        .await
        .expect("series should resolve");
    assert_eq!(series.len(), 2);
    assert_eq!(series[0].source.as_str(), "NASDAQ_NORDIC");
    assert_eq!(series[1].source.as_str(), "YAHOO");
}

#[tokio::test]
async fn backfill_reports_history_clamped_when_rows_start_well_after_the_window() {
    let nasdaq_price = FakePriceProvider::with_provider(MarketDataProvider::NasdaqNordic);
    nasdaq_price.push_response(Ok(vec![nasdaq_close("TX69", 11, "78.20")]));

    let service = multi_provider_service(
        silent_fake_price(),
        empty_search(MarketDataProvider::Yahoo),
        nasdaq_price,
        empty_search(MarketDataProvider::NasdaqNordic),
        FakeFxRateProvider::with_provider(FxProvider::Frankfurter),
    );
    let pool = db::memory_pool().await.expect("memory pool");
    let nordic = instrument(&pool, "ERIC", "STO", "SEK").await;
    buy(&pool, nordic, "2016-01-04", 10, "80", "SEK", None).await;
    map_nasdaq_symbol(&pool, nordic, "TX69", "SHARES", "SEK", true).await;

    let response = service
        .refresh(
            &pool,
            system_today(),
            RefreshTrigger::Backfill,
            RefreshPricesRequest {
                mode: RefreshMode::Backfill,
                start_date: None,
                end_date: Some("2026-06-12".to_owned()),
            },
        )
        .await
        .expect("backfill should complete");

    let item = response
        .items
        .iter()
        .find(|item| item.instrument_id == Some(nordic))
        .expect("a price item should be reported");
    assert_eq!(item.status, RefreshItemStatus::Fetched);
    assert_eq!(item.reason.as_deref(), Some("history_clamped"));
}
