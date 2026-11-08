use super::*;
use crate::{
    db::{
        self,
        fundamental_values::{self, FetchWrite},
        fundamentals_sources::{self, AttemptStatus, GenerationWrite},
        instruments, market_data_runs,
    },
    market_data::ProviderRegistry,
    providers::{
        FakeFundamentalsProvider, FakeSymbolSearchProvider, YahooFundamentalsClient,
        YahooSearchClient,
    },
};
use std::sync::{atomic::AtomicBool, Arc};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fundamentals")
            .join(name),
    )
    .unwrap()
}
fn candidate(symbol: &str, kind: Option<&str>) -> SymbolSearchMatch {
    SymbolSearchMatch {
        provider: MarketDataProvider::Yahoo,
        provider_symbol: symbol.into(),
        quote_type: kind.map(str::to_owned),
        exchange: None,
        name: Some("Test".into()),
        asset_class: None,
        currency: None,
    }
}
async fn run(
    matches: Vec<SymbolSearchMatch>,
    values: Vec<FundamentalValue>,
    chart_type: Option<&str>,
) -> (Discovery, FakeFundamentalsProvider) {
    let pool = db::testing::memory_pool().await;
    let (instrument, _) = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "TEST".into(),
            exchange: "NYSE".into(),
            name: "Test".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: Some("US0000000001".into()),
        },
    )
    .await
    .unwrap();
    let search = FakeSymbolSearchProvider::new();
    search.push_response(Ok(matches));
    search.push_response(Ok(vec![]));
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(ListingDetails {
        security_type: chart_type.map(str::to_owned),
        splits: vec![],
    }));
    figures.push_response(Ok(values));
    let providers = ProviderSet::from_registry(
        ProviderRegistry::new()
            .with_symbol_search_provider(MarketDataProvider::Yahoo, search)
            .with_fundamentals_provider(figures.clone()),
    );
    let clock = crate::clock::Clock::fixed("2026-11-07".parse().unwrap());
    let run_id = match market_data_runs::try_claim_run(
        &pool,
        "MANUAL",
        "discovery-test",
        clock.now_utc(),
        market_data_runs::REFRESH_CLAIM_STALE_AFTER,
    )
    .await
    .unwrap()
    {
        market_data_runs::ClaimResult::Claimed { run, .. } => run.id,
        _ => panic!("claim"),
    };
    let lease = RefreshLease {
        run_id,
        owner: "discovery-test".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let result = discover(&providers, &pool, &instrument, clock.today(), &lease, 0)
        .await
        .unwrap();
    let write = FetchWrite {
        instrument_id: instrument.id,
        expected: None,
        discovery: Some(result.definition.clone()),
        verified: None,
        values: result.values.clone(),
        splits: result.splits.clone(),
        at: clock.now_utc().to_rfc3339(),
        seen_on: clock.today(),
        previous_success_on: None,
        status: AttemptStatus::Succeeded,
        reason: result.reason.clone(),
    };
    let written = market_data_runs::fenced_write(&pool, run_id, "discovery-test", |conn| {
        Box::pin(async move { fundamental_values::write_fetch(conn, &write).await })
    })
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(written, GenerationWrite::Applied(_)));
    assert_eq!(
        fundamentals_sources::find(&pool, instrument.id)
            .await
            .unwrap()
            .unwrap()
            .definition,
        result.definition
    );
    (result, figures)
}
fn values() -> Vec<FundamentalValue> {
    YahooFundamentalsClient::parse_response(&fixture("yahoo_timeseries_msft.json")).unwrap()
}

#[tokio::test]
async fn all_etf_matches_store_not_applicable_without_company_or_chart_fetch() {
    let matches =
        YahooSearchClient::parse_response("IE00B0M63391", &fixture("yahoo_search_etf_isin.json"))
            .unwrap();
    let (result, figures) = run(matches, vec![], None).await;
    assert_eq!(result.definition.status, LinkStatus::NotApplicable);
    assert_eq!(result.definition.classification, Classification::Fund);
    assert!(figures.calls().is_empty());
    assert!(figures.listing_calls().is_empty());
}
#[tokio::test]
async fn mixed_search_keeps_equities_before_selection() {
    let (result, figures) = run(
        vec![
            candidate("ETF", Some("ETF")),
            candidate("TEST", Some("EQUITY")),
        ],
        values(),
        Some("EQUITY"),
    )
    .await;
    assert_eq!(result.definition.symbol.as_deref(), Some("TEST"));
    assert_eq!(result.definition.status, LinkStatus::Usable);
    assert_eq!(figures.calls().len(), 1);
}
#[tokio::test]
async fn absent_quote_type_is_usable_only_when_quarterly_figures_verify_it() {
    let (result, _) = run(vec![candidate("TEST", None)], values(), None).await;
    assert_eq!(result.definition.classification, Classification::Figures);
    assert_eq!(result.definition.status, LinkStatus::Usable);
    let (result, _) = run(vec![candidate("TEST", None)], vec![], None).await;
    assert_eq!(result.definition.classification, Classification::Unknown);
    assert_eq!(result.definition.status, LinkStatus::Unavailable);
    assert_eq!(result.reason.as_deref(), Some("security_type_unknown"));
}
#[tokio::test]
async fn ambiguity_and_no_match_never_fetch_company_data() {
    let (result, figures) = run(
        vec![
            candidate("ONE", Some("EQUITY")),
            candidate("TWO", Some("EQUITY")),
        ],
        vec![],
        None,
    )
    .await;
    assert_eq!(result.definition.status, LinkStatus::Ambiguous);
    assert!(result.definition.candidates.contains("ONE"));
    assert!(figures.calls().is_empty());
    let (result, figures) = run(vec![], vec![], None).await;
    assert_eq!(result.definition.status, LinkStatus::NoMatch);
    assert!(figures.calls().is_empty());
}

#[tokio::test]
async fn mapped_tsm_verifies_directly_despite_recorded_empty_isin_search() {
    let pool = db::testing::memory_pool().await;
    let (instrument, _) = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "TSM".into(),
            exchange: "NYSE".into(),
            name: "Taiwan Semiconductor".into(),
            kind: "STOCK".into(),
            currency: "USD".into(),
            isin: Some("US8740391003".into()),
        },
    )
    .await
    .unwrap();
    crate::db::provider_symbols::upsert(
        &pool,
        &crate::db::provider_symbols::NewProviderSymbol {
            instrument_id: instrument.id,
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "TSM".into(),
            asset_class: None,
            currency: Some("USD".into()),
            enabled: true,
            created_at: "2026-11-07T12:00:00Z".into(),
            updated_at: "2026-11-07T12:00:00Z".into(),
        },
    )
    .await
    .unwrap();
    let search = FakeSymbolSearchProvider::new();
    search.push_response(Ok(YahooSearchClient::parse_response(
        "US8740391003",
        &fixture("yahoo_search_tsm_isin_no_match.json"),
    )
    .unwrap()));
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(YahooFundamentalsClient::parse_listing(&fixture(
        "yahoo_chart_tsm.json",
    ))
    .unwrap()));
    figures.push_response(Ok(YahooFundamentalsClient::parse_response(&fixture(
        "yahoo_timeseries_tsm.json",
    ))
    .unwrap()));
    let providers = ProviderSet::from_registry(
        ProviderRegistry::new()
            .with_fundamentals_provider(figures.clone())
            .with_symbol_search_provider(MarketDataProvider::Yahoo, search.clone()),
    );
    let lease = RefreshLease {
        run_id: 1,
        owner: "test".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let result = discover(
        &providers,
        &pool,
        &instrument,
        "2026-11-07".parse().unwrap(),
        &lease,
        0,
    )
    .await
    .unwrap();
    assert_eq!(result.definition.status, LinkStatus::Usable);
    assert_eq!(result.definition.classification, Classification::Equity);
    assert!(result
        .values
        .iter()
        .any(|v| v.currency.as_deref() == Some("TWD")));
    assert!(search.calls().is_empty());
    assert_eq!(figures.calls().len(), 1);
}

#[tokio::test]
async fn nordic_search_prefers_same_exchange_using_identity_matched_ticker() {
    let pool = db::testing::memory_pool().await;
    let (instrument, _) = instruments::upsert(
        &pool,
        &instruments::NewInstrument {
            symbol: "GB0009895292".into(),
            exchange: "AVANZA".into(),
            name: "AstraZeneca PLC".into(),
            kind: "STOCK".into(),
            currency: "SEK".into(),
            isin: Some("GB0009895292".into()),
        },
    )
    .await
    .unwrap();
    crate::db::provider_symbols::upsert(
        &pool,
        &crate::db::provider_symbols::NewProviderSymbol {
            instrument_id: instrument.id,
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: "TX1".into(),
            asset_class: Some("SHARES".into()),
            currency: Some("SEK".into()),
            enabled: true,
            created_at: "2026-11-07T12:00:00Z".into(),
            updated_at: "2026-11-07T12:00:00Z".into(),
        },
    )
    .await
    .unwrap();
    let search = FakeSymbolSearchProvider::new();
    search.push_response(Ok(vec![candidate("AZN.L", Some("EQUITY"))]));
    search.push_response(Ok(vec![
        candidate("AZN.ST", Some("EQUITY")),
        candidate("WRONG", Some("EQUITY")),
    ]));
    let figures = FakeFundamentalsProvider::new();
    figures.push_listing(Ok(ListingDetails {
        security_type: Some("EQUITY".into()),
        splits: vec![],
    }));
    figures.push_response(Ok(values()));
    let providers = ProviderSet::from_registry(
        ProviderRegistry::new()
            .with_fundamentals_provider(figures)
            .with_symbol_search_provider(MarketDataProvider::Yahoo, search.clone()),
    );
    let lease = RefreshLease {
        run_id: 1,
        owner: "test".into(),
        cancelled: Arc::new(AtomicBool::new(false)),
    };
    let result = discover(
        &providers,
        &pool,
        &instrument,
        "2026-11-07".parse().unwrap(),
        &lease,
        0,
    )
    .await
    .unwrap();
    assert_eq!(result.definition.symbol.as_deref(), Some("AZN.ST"));
    assert_eq!(
        search
            .calls()
            .iter()
            .map(|c| c.query.as_str())
            .collect::<Vec<_>>(),
        ["GB0009895292", "AZN"]
    );
}
