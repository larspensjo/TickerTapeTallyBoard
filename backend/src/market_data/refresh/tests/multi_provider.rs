use super::*;

use crate::{
    db::{self, prices, provider_symbols},
    import::now_iso8601,
    market_data::effective_prices,
    providers::{
        DailyClose, FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider, FxProvider,
        MarketDataProvider, ProviderError, SymbolSearchMatch,
    },
};
use chrono::NaiveDate;
use rust_decimal_macros::dec;

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
            &Clock::System,
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
            &Clock::System,
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
            &Clock::System,
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
            &Clock::System,
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
