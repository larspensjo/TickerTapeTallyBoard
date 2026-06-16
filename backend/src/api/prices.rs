use axum::{extract::State, Json};

use crate::{
    api::error::ApiError,
    market_data::{
        MarketDataError, PriceStatusResponse, RefreshMode, RefreshPricesRequest,
        RefreshPricesResponse, RefreshTrigger,
    },
    state::AppState,
};

pub async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<RefreshPricesRequest>,
) -> Result<Json<RefreshPricesResponse>, ApiError> {
    let trigger = match body.mode {
        RefreshMode::Latest => RefreshTrigger::Manual,
        RefreshMode::Backfill => RefreshTrigger::Backfill,
    };
    let response = state
        .market_data
        .refresh(&state.pool, trigger, body)
        .await
        .map_err(api_error)?;
    Ok(Json(response))
}

pub async fn status(State(state): State<AppState>) -> Result<Json<PriceStatusResponse>, ApiError> {
    let response = state
        .market_data
        .status(&state.pool)
        .await
        .map_err(api_error)?;
    Ok(Json(response))
}

fn api_error(error: MarketDataError) -> ApiError {
    match error {
        MarketDataError::InvalidRequest { code, message } => ApiError::bad_request(code, message),
        MarketDataError::Internal(message) => ApiError::internal(message),
        MarketDataError::Repo(error) => ApiError::from(error),
    }
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
        db::{self, instruments, provider_symbols, transactions},
        market_data::MarketDataService,
        providers::{
            DailyClose, FakeFxRateProvider, FakePriceProvider, FxProvider, FxRate,
            MarketDataProvider,
        },
        state::AppState,
    };
    use chrono::NaiveDate;
    use rust_decimal_macros::dec;

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
        let (row, _) = instruments::upsert(
            pool,
            &crate::db::instruments::NewInstrument {
                symbol: "MSFT".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Microsoft".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
            },
        )
        .await
        .expect("instrument upsert should succeed");
        row.id
    }

    #[tokio::test]
    async fn refresh_endpoint_populates_latest_data_and_status() {
        let pool = db::memory_pool().await.expect("memory pool");
        let price_provider = FakePriceProvider::with_provider(MarketDataProvider::Yahoo);
        let fx_provider = FakeFxRateProvider::with_provider(FxProvider::Frankfurter);
        price_provider.push_response(Ok(vec![DailyClose {
            provider: MarketDataProvider::Yahoo,
            provider_symbol: "MSFT".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            close: dec!(101),
            currency: "USD".to_owned(),
        }]));
        fx_provider.push_response(Ok(vec![FxRate {
            provider: FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: NaiveDate::from_ymd_opt(2026, 6, 12).expect("date"),
            rate: dec!(10.5),
        }]));

        let state = AppState::with_market_data(
            pool,
            MarketDataService::with_providers(price_provider, fx_provider),
        );
        let msft = instrument(&state.pool).await;
        transactions::insert(
            &state.pool,
            &crate::db::transactions::NewTransaction {
                instrument_id: msft,
                kind: crate::domain::TransactionKind::Buy,
                trade_date: NaiveDate::from_ymd_opt(2026, 6, 10).expect("date"),
                quantity: 10,
                price: Some(dec!(100)),
                currency: Some("USD".to_owned()),
                fx_rate_to_base: Some(dec!(10)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction insert should succeed");

        let (status, body) = send(
            &state,
            "POST",
            "/api/prices/refresh",
            json!({"mode":"latest"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "succeeded");
        assert_eq!(body["prices_written"], 1);
        assert_eq!(body["fx_rates_written"], 1);

        let (status, status_body) = send(&state, "GET", "/api/prices/status", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(status_body["refreshing"], false);
        assert_eq!(status_body["latest_run"]["prices_written"], 1);
        assert_eq!(status_body["latest_run"]["fx_rates_written"], 1);
        assert_eq!(
            status_body["instruments"][0]["latest_price"]["status"],
            "available"
        );
        assert_eq!(
            status_body["instruments"][0]["latest_fx"]["status"],
            "available"
        );

        let mapping = provider_symbols::find_by_instrument_provider(&state.pool, msft, "YAHOO")
            .await
            .expect("mapping lookup should succeed")
            .expect("mapping should exist");
        assert!(mapping.enabled);
    }

    #[tokio::test]
    async fn refresh_endpoint_rejects_invalid_date_range() {
        let state = AppState::for_tests().await;
        let (status, body) = send(
            &state,
            "POST",
            "/api/prices/refresh",
            json!({"mode":"backfill","start_date":"2026-06-12","end_date":"2026-06-11"}),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date_range");
    }
}
