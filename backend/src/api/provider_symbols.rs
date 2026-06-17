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
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct ProviderSymbolResponse {
    pub id: i64,
    pub instrument_id: i64,
    pub provider: String,
    pub provider_symbol: String,
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
            provider: row.provider,
            provider_symbol: row.provider_symbol,
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
    if instruments::find(&state.pool, instrument_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("instrument", instrument_id));
    }

    let provider = provider.trim().to_ascii_uppercase();
    if MarketDataProvider::from_db_str(&provider).is_none() {
        return Err(ApiError::bad_request(
            "invalid_provider",
            format!("unsupported provider {:?}", provider),
        ));
    }

    let provider_symbol = body.provider_symbol.trim().to_owned();
    if provider_symbol.is_empty() {
        return Err(ApiError::bad_request(
            "invalid_provider_symbol",
            "provider_symbol is required",
        ));
    }
    let now = now_iso8601();
    let row = provider_symbols::upsert(
        &state.pool,
        &NewProviderSymbol {
            instrument_id,
            provider,
            provider_symbol,
            currency: body.currency.map(|value| value.trim().to_owned()),
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
        providers::{FakeFxRateProvider, FakePriceProvider},
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

        let row =
            provider_symbols::find_by_instrument_provider(&state.pool, instrument_id, "YAHOO")
                .await
                .expect("lookup should succeed")
                .expect("row should exist");
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
}
