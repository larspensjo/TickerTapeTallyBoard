use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::{api::error::ApiError, market_data::SymbolSearchLookupResponse, state::AppState};

#[derive(Debug, Deserialize)]
pub struct InstrumentLookupQuery {
    pub query: String,
}

pub async fn lookup(
    State(state): State<AppState>,
    Query(query): Query<InstrumentLookupQuery>,
) -> Result<Json<SymbolSearchLookupResponse>, ApiError> {
    let query = query.query.trim().to_owned();
    if query.is_empty() {
        return Err(ApiError::bad_request("invalid_query", "query is required"));
    }

    if state.demo_mode {
        crate::engine_info!("instrument lookup unavailable in demo mode query={query}");
        return Ok(Json(SymbolSearchLookupResponse::provider_unavailable(
            query,
        )));
    }

    Ok(Json(state.market_data.lookup_symbol_search(&query).await))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::{
        api::router,
        db,
        market_data::{MarketDataService, ProviderRegistry},
        providers::{
            FakeFxRateProvider, FakePriceProvider, FakeSymbolSearchProvider, MarketDataProvider,
            ProviderError, SymbolSearchMatch,
        },
        state::AppState,
    };

    async fn send(state: &AppState, uri: &str) -> (StatusCode, Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request builds");
        let response = router(state.clone())
            .oneshot(request)
            .await
            .expect("request completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    #[tokio::test]
    async fn lookup_returns_supported_matches() {
        let search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        search.push_response(Ok(vec![
            SymbolSearchMatch {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                quote_type: Some("EQUITY".to_owned()),
                exchange: Some("NMS".to_owned()),
                name: Some("Microsoft Corporation".to_owned()),
                asset_class: None,
                currency: None,
            },
            SymbolSearchMatch {
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT.SWAP".to_owned(),
                quote_type: Some("OPTION".to_owned()),
                exchange: Some("NMS".to_owned()),
                name: Some("Unsupported".to_owned()),
                asset_class: None,
                currency: None,
            },
        ]));

        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_symbol_search_providers(
                FakePriceProvider::new(),
                FakeFxRateProvider::new(),
                search,
            ),
        );

        let (status, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["query"], "MSFT");
        assert_eq!(body["status"], "matches");
        assert_eq!(body["matches"].as_array().expect("matches").len(), 1);
        assert_eq!(body["matches"][0]["provider"], "YAHOO");
        assert_eq!(body["matches"][0]["provider_symbol"], "MSFT");
    }

    #[tokio::test]
    async fn lookup_returns_no_match_when_only_unsupported_matches_exist() {
        let search = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        search.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            quote_type: Some("OPTION".to_owned()),
            exchange: Some("NMS".to_owned()),
            name: Some("Microsoft Option".to_owned()),
            asset_class: None,
            currency: None,
        }]));

        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_symbol_search_providers(
                FakePriceProvider::new(),
                FakeFxRateProvider::new(),
                search,
            ),
        );

        let (status, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "no_match");
        assert!(body["matches"].as_array().expect("matches").is_empty());
    }

    #[tokio::test]
    async fn lookup_fans_out_to_nasdaq_when_yahoo_has_no_supported_match() {
        let yahoo = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "AVA.ST".to_owned(),
            quote_type: Some("OPTION".to_owned()),
            exchange: Some("NMS".to_owned()),
            name: Some("Unsupported Yahoo result".to_owned()),
            asset_class: None,
            currency: Some("SEK".to_owned()),
        }]));
        let nasdaq = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: "TX2997672".to_owned(),
            quote_type: None,
            exchange: Some("Warrants".to_owned()),
            name: Some("AVA SAMSUNG TRACKER".to_owned()),
            asset_class: Some("TRACKER_CERTIFICATES".to_owned()),
            currency: Some("SEK".to_owned()),
        }]));

        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo)
                .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq),
        );
        let state =
            AppState::with_market_data(db::memory_pool().await.expect("memory pool"), service);

        let (_, body) = send(&state, "/api/instruments/lookup?query=JE00BJ7HNC92").await;

        assert_eq!(body["status"], "matches");
        assert_eq!(body["matches"].as_array().expect("matches").len(), 1);
        assert_eq!(body["matches"][0]["provider"], "NASDAQ_NORDIC");
        assert_eq!(body["matches"][0]["asset_class"], "TRACKER_CERTIFICATES");
        assert_eq!(body["matches"][0]["currency"], "SEK");
    }

    #[tokio::test]
    async fn lookup_lists_matches_in_provider_precedence_order() {
        let yahoo = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NMS".to_owned()),
            name: Some("Microsoft Corporation".to_owned()),
            asset_class: None,
            currency: Some("USD".to_owned()),
        }]));
        let nasdaq = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq.push_response(Ok(vec![SymbolSearchMatch {
            provider: MarketDataProvider::NasdaqNordic,
            provider_symbol: "TX123".to_owned(),
            quote_type: None,
            exchange: Some("Shares".to_owned()),
            name: Some("Microsoft Corporation".to_owned()),
            asset_class: Some("SHARES".to_owned()),
            currency: Some("USD".to_owned()),
        }]));

        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq)
                .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo),
        );
        let state =
            AppState::with_market_data(db::memory_pool().await.expect("memory pool"), service);

        let (_, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;
        let matches = body["matches"].as_array().expect("matches");

        assert_eq!(body["status"], "matches");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0]["provider"], "YAHOO");
        assert_eq!(matches[1]["provider"], "NASDAQ_NORDIC");
    }

    #[tokio::test]
    async fn lookup_returns_no_match_when_one_provider_errors_and_another_answers_empty() {
        let yahoo = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo.push_response(Err(ProviderError::transport(
            MarketDataProvider::Yahoo.as_str(),
            "Yahoo is unreachable",
        )));
        let nasdaq = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq.push_response(Ok(Vec::new()));

        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo)
                .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq),
        );
        let state =
            AppState::with_market_data(db::memory_pool().await.expect("memory pool"), service);

        let (_, body) = send(&state, "/api/instruments/lookup?query=UNKNOWN").await;

        assert_eq!(body["status"], "no_match");
        assert!(body["matches"].as_array().expect("matches").is_empty());
    }

    #[tokio::test]
    async fn lookup_returns_provider_unavailable_when_every_provider_errors() {
        let yahoo = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        yahoo.push_response(Err(ProviderError::transport(
            MarketDataProvider::Yahoo.as_str(),
            "Yahoo is unreachable",
        )));
        let nasdaq = FakeSymbolSearchProvider::with_provider(MarketDataProvider::NasdaqNordic);
        nasdaq.push_response(Err(ProviderError::transport(
            MarketDataProvider::NasdaqNordic.as_str(),
            "Nasdaq is unreachable",
        )));

        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new()
                .with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo)
                .with_symbol_search_provider(MarketDataProvider::NasdaqNordic, nasdaq),
        );
        let state =
            AppState::with_market_data(db::memory_pool().await.expect("memory pool"), service);

        let (_, body) = send(&state, "/api/instruments/lookup?query=UNKNOWN").await;

        assert_eq!(body["status"], "provider_unavailable");
        assert!(body["matches"].as_array().expect("matches").is_empty());
    }

    #[tokio::test]
    async fn lookup_deduplicates_provider_and_symbol_within_result_set() {
        let yahoo = FakeSymbolSearchProvider::with_provider(MarketDataProvider::Yahoo);
        let duplicate = SymbolSearchMatch {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            quote_type: Some("EQUITY".to_owned()),
            exchange: Some("NMS".to_owned()),
            name: Some("Microsoft Corporation".to_owned()),
            asset_class: None,
            currency: None,
        };
        yahoo.push_response(Ok(vec![duplicate.clone(), duplicate]));

        let service = MarketDataService::with_provider_registry(
            ProviderRegistry::new().with_symbol_search_provider(MarketDataProvider::Yahoo, yahoo),
        );
        let state =
            AppState::with_market_data(db::memory_pool().await.expect("memory pool"), service);

        let (_, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;

        assert_eq!(body["status"], "matches");
        assert_eq!(body["matches"].as_array().expect("matches").len(), 1);
    }

    #[tokio::test]
    async fn lookup_returns_provider_unavailable_without_symbol_search_provider() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );

        let (status, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "provider_unavailable");
        assert!(body["matches"].as_array().expect("matches").is_empty());
    }

    #[tokio::test]
    async fn lookup_returns_provider_unavailable_in_demo_mode() {
        let state = AppState::new(
            db::memory_pool().await.expect("memory pool"),
            std::sync::Arc::new(MarketDataService::live()),
        )
        .with_demo_mode(true);

        let (status, body) = send(&state, "/api/instruments/lookup?query=MSFT").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "provider_unavailable");
        assert!(body["matches"].as_array().expect("matches").is_empty());
    }
}
