use super::*;

use crate::{
    clock::Clock,
    db::{self, instruments, provider_symbols},
    import::now_iso8601,
    providers::{
        DailyClose, FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider, FxProvider,
        FxRate, MarketDataProvider, SymbolSearchMatch,
    },
};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

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
