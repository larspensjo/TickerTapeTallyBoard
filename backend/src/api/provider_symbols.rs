use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::error::ApiError,
    db::{
        instruments,
        provider_symbols::{self, NewProviderSymbol, ProviderSymbolRow},
    },
    import::now_iso8601,
    providers::MarketDataProvider,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct UpdateProviderSymbolRequest {
    pub provider_symbol: String,
    pub currency: Option<String>,
    /// Required for Nasdaq Nordic, which needs it on every price-history call.
    /// Omitted on an update, the stored asset class is preserved.
    pub asset_class: Option<String>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ProviderSymbolResponse {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
    pub asset_class: Option<String>,
    pub currency: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<ProviderSymbolRow> for ProviderSymbolResponse {
    fn from(row: ProviderSymbolRow) -> Self {
        Self {
            id: row.id,
            instrument_id: row.instrument_id,
            provider: row.provider.to_string(),
            provider_symbol: row.provider_symbol,
            asset_class: row.asset_class,
            currency: row.currency,
            enabled: row.enabled,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

pub async fn update(
    State(state): State<AppState>,
    Path((instrument_id, provider)): Path<(i64, String)>,
    Json(body): Json<UpdateProviderSymbolRequest>,
) -> Result<(StatusCode, Json<ProviderSymbolResponse>), ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    if instruments::find(&state.pool, instrument_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("instrument", instrument_id));
    }

    let provider = provider.trim().to_ascii_uppercase();
    let provider = MarketDataProvider::from_db_str(&provider).ok_or_else(|| {
        ApiError::bad_request(
            "invalid_provider",
            format!("unsupported provider {:?}", provider),
        )
    })?;

    let provider_symbol = body.provider_symbol.trim().to_owned();
    if provider_symbol.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_provider_symbol",
            "provider_symbol is required",
        ));
    }
    let existing =
        provider_symbols::find_by_instrument_provider(&state.pool, instrument_id, provider).await?;
    let asset_class =
        trimmed(body.asset_class).or_else(|| existing.and_then(|row| row.asset_class));
    let currency = trimmed(body.currency);

    // Nasdaq needs both to be fetchable at all: the asset class is a required
    // query parameter, and the price-history payload carries no currency, so the
    // mapping's currency is the only thing that can stamp the stored rows.
    if provider == MarketDataProvider::NasdaqNordic {
        if asset_class.is_none() {
            return Err(ApiError::bad_request(
                "missing_asset_class",
                "asset_class is required for NASDAQ_NORDIC mappings",
            ));
        }
        if currency.is_none() {
            return Err(ApiError::bad_request(
                "missing_source_currency",
                "currency is required for NASDAQ_NORDIC mappings",
            ));
        }
    }

    let now = now_iso8601();
    let row = provider_symbols::upsert(
        &state.pool,
        &NewProviderSymbol {
            instrument_id,
            provider,
            provider_symbol,
            asset_class,
            currency,
            enabled: body.enabled,
            created_at: now.clone(),
            updated_at: now,
        },
    )
    .await?;

    Ok((StatusCode::OK, Json(row.into())))
}

fn default_enabled() -> bool {
    true
}

fn trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::json;
    use tower::ServiceExt;

    use crate::{
        db::{self, provider_symbols},
        market_data::MarketDataService,
        providers::{FakeFxRateProvider, FakePriceProvider, MarketDataProvider},
        state::AppState,
    };

    async fn send(
        state: &AppState,
        method: &str,
        uri: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request builds");
        let response = crate::api::router(state.clone())
            .oneshot(request)
            .await
            .expect("request completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        let value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    async fn instrument(pool: &sqlx::sqlite::SqlitePool) -> i64 {
        let (row, _) = crate::db::instruments::upsert(
            pool,
            &crate::db::instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
        row.id
    }

    #[tokio::test]
    async fn provider_symbol_update_round_trips() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;

        let (status, body) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{instrument_id}/provider-symbols/YAHOO"),
            json!({"provider_symbol":"MSFT","currency":"USD","enabled":true}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["provider"], "YAHOO");
        assert_eq!(body["provider_symbol"], "MSFT");
        assert_eq!(body["enabled"], true);

        let row = provider_symbols::find_by_instrument_provider(
            &state.pool,
            instrument_id,
            MarketDataProvider::Yahoo,
        )
        .await
        .expect("lookup should succeed")
        .expect("row should exist");
        assert!(row.enabled);
    }

    #[tokio::test]
    async fn provider_symbol_update_preserves_existing_asset_class() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: MarketDataProvider::Yahoo,
                provider_symbol: "MSFT".to_owned(),
                asset_class: Some("EQUITY".to_owned()),
                currency: Some("USD".to_owned()),
                enabled: false,
                created_at: "2026-08-28T08:00:00Z".to_owned(),
                updated_at: "2026-08-28T08:00:00Z".to_owned(),
            },
        )
        .await
        .expect("provider symbol seed should succeed");

        let (status, _) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{instrument_id}/provider-symbols/YAHOO"),
            json!({"provider_symbol":"MSFT","currency":"USD","enabled":true}),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        let row = provider_symbols::find_by_instrument_provider(
            &state.pool,
            instrument_id,
            MarketDataProvider::Yahoo,
        )
        .await
        .expect("lookup should succeed")
        .expect("row should exist");
        assert_eq!(row.asset_class.as_deref(), Some("EQUITY"));
        assert!(row.enabled);
    }

    #[tokio::test]
    async fn invalid_provider_is_rejected_before_write() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;

        let (status, body) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{instrument_id}/provider-symbols/BAD"),
            json!({"provider_symbol":"MSFT","currency":"USD","enabled":true}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_provider");
    }

    #[tokio::test]
    async fn blank_provider_symbol_is_rejected_before_write() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;

        let (status, body) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{instrument_id}/provider-symbols/YAHOO"),
            json!({"provider_symbol":"   ","currency":"USD","enabled":true}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_provider_symbol");
    }

    #[tokio::test]
    async fn nasdaq_mapping_round_trips_asset_class() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;

        let (status, body) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{instrument_id}/provider-symbols/NASDAQ_NORDIC"),
            json!({
                "provider_symbol": "TX2997672",
                "asset_class": "TRACKER_CERTIFICATES",
                "currency": "SEK",
                "enabled": true
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["provider"], "NASDAQ_NORDIC");
        assert_eq!(body["provider_symbol"], "TX2997672");
        assert_eq!(body["asset_class"], "TRACKER_CERTIFICATES");
        assert_eq!(body["currency"], "SEK");

        let row = provider_symbols::find_by_instrument_provider(
            &state.pool,
            instrument_id,
            MarketDataProvider::NasdaqNordic,
        )
        .await
        .expect("lookup should succeed")
        .expect("row should exist");
        assert_eq!(row.asset_class.as_deref(), Some("TRACKER_CERTIFICATES"));
        assert_eq!(row.currency.as_deref(), Some("SEK"));
    }

    /// Nasdaq cannot fetch without either field, so both are rejected before a
    /// write rather than stored as an unfetchable mapping.
    #[tokio::test]
    async fn nasdaq_mapping_requires_asset_class_and_currency() {
        let state = AppState::with_market_data(
            db::memory_pool().await.expect("memory pool"),
            MarketDataService::with_providers(FakePriceProvider::new(), FakeFxRateProvider::new()),
        );
        let instrument_id = instrument(&state.pool).await;
        let uri = format!("/api/instruments/{instrument_id}/provider-symbols/NASDAQ_NORDIC");

        let (status, body) = send(
            &state,
            "PUT",
            &uri,
            json!({"provider_symbol": "TX2997672", "currency": "SEK"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "missing_asset_class");

        let (status, body) = send(
            &state,
            "PUT",
            &uri,
            json!({"provider_symbol": "TX2997672", "asset_class": "SHARES"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "missing_source_currency");

        assert!(provider_symbols::find_by_instrument_provider(
            &state.pool,
            instrument_id,
            MarketDataProvider::NasdaqNordic,
        )
        .await
        .expect("lookup should succeed")
        .is_none());
    }
}
