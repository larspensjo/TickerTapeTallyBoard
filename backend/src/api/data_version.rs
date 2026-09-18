use axum::{extract::State, Json};
use serde::Serialize;

use crate::{api::ApiError, state::AppState};

/// What snapshot of the data the app is serving, and what day it thinks it is.
/// Clients name this in every data request so panels cannot mix snapshots.
#[derive(Debug, Serialize)]
pub struct DataVersionResponse {
    pub data_revision: String,
    pub valuation_date: String,
    pub prices_refreshing: bool,
}

pub async fn handler(State(state): State<AppState>) -> Result<Json<DataVersionResponse>, ApiError> {
    Ok(Json(DataVersionResponse {
        data_revision: state.revision.current().await.map_err(|error| {
            ApiError::internal(format!("could not read data revision: {error}"))
        })?,
        valuation_date: state.clock.today().format("%Y-%m-%d").to_string(),
        prices_refreshing: match state
            .market_data
            .is_refreshing(&state.pool, &state.clock)
            .await
        {
            Ok(refreshing) => refreshing,
            Err(error) => {
                crate::engine_warn!(
                    "could not read live market-data refresh claim operation=data_version error={error}"
                );
                false
            }
        },
    }))
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::state::AppState;
    use std::time::{SystemTime, UNIX_EPOCH};

    async fn get_data_version(state: &AppState) -> Value {
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/data-version")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body readable");
        serde_json::from_slice(&body).expect("body is JSON")
    }

    #[tokio::test]
    async fn reports_the_clock_date_and_current_revision() {
        let state = AppState::for_tests()
            .await
            .with_clock(crate::clock::Clock::fixed(
                chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
            ));

        let body = get_data_version(&state).await;

        assert_eq!(body["valuation_date"], "2026-03-05");
        assert_eq!(
            body["data_revision"],
            state.revision.current().await.expect("revision reads")
        );
        assert_eq!(body["prices_refreshing"], false);
    }

    #[tokio::test]
    async fn reads_do_not_change_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current().await.expect("revision reads");

        let _ = get_data_version(&state).await;
        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/holdings")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(response.status(), StatusCode::OK);

        assert_eq!(
            state.revision.current().await.expect("revision reads"),
            before
        );
    }

    #[tokio::test]
    async fn a_successful_mutation_changes_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current().await.expect("revision reads");

        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "TEST",
                            "exchange": "STO",
                            "name": "Test",
                            "type": "Stock",
                            "currency": "SEK"
                        })
                        .to_string(),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(response.status().is_success(), "{}", response.status());

        assert_ne!(
            state.revision.current().await.expect("revision reads"),
            before
        );
    }

    #[tokio::test]
    async fn a_rejected_mutation_leaves_the_revision_alone() {
        let state = AppState::for_tests().await;
        let before = state.revision.current().await.expect("revision reads");

        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(response.status().is_client_error(), "{}", response.status());

        assert_eq!(
            state.revision.current().await.expect("revision reads"),
            before
        );
    }

    #[tokio::test]
    async fn a_demo_mode_mutation_leaves_the_revision_alone() {
        let state = AppState::for_tests()
            .await
            .with_mode(crate::config::Mode::Demo);
        let before = state.revision.current().await.expect("revision reads");

        let response = crate::api::router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "TEST",
                            "exchange": "STO",
                            "name": "Test",
                            "type": "Stock",
                            "currency": "SEK"
                        })
                        .to_string(),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            state.revision.current().await.expect("revision reads"),
            before
        );
    }

    #[tokio::test]
    async fn independent_app_states_publish_each_others_mutation_revision() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = crate::test_support::workspace_target_path("revision-tests")
            .join(format!("shared-{nonce}.sqlite"));
        std::fs::create_dir_all(path.parent().expect("parent")).expect("test directory");
        let location = crate::ledger::resolve(
            &format!("sqlite://{}", path.to_string_lossy().replace('\\', "/")),
            crate::config::Mode::Production,
        )
        .expect("location");
        let first = crate::db::open(&location, crate::db::CreateMissing::Yes)
            .await
            .expect("first pool");
        crate::db::migrate(&first.pool).await.expect("migrate");
        let second = crate::db::open(&location, crate::db::CreateMissing::No)
            .await
            .expect("second pool");
        let price_provider = crate::providers::FakePriceProvider::with_provider(
            crate::providers::MarketDataProvider::Yahoo,
        );
        let fx_provider = crate::providers::FakeFxRateProvider::with_provider(
            crate::providers::FxProvider::Frankfurter,
        );
        let first_state = AppState::with_market_data(
            first.pool,
            crate::market_data::MarketDataService::with_providers(
                price_provider.clone(),
                fx_provider.clone(),
            ),
        );
        let second_state = AppState::with_market_data(
            second.pool,
            crate::market_data::MarketDataService::with_providers(
                crate::providers::FakePriceProvider::new(),
                crate::providers::FakeFxRateProvider::new(),
            ),
        );
        let before = get_data_version(&second_state).await["data_revision"]
            .as_str()
            .expect("revision string")
            .to_owned();

        let response = crate::api::router(first_state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "symbol": "SHARED",
                            "exchange": "STO",
                            "name": "Shared",
                            "type": "Stock",
                            "currency": "SEK"
                        })
                        .to_string(),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(response.status().is_success());
        let after = get_data_version(&second_state).await["data_revision"]
            .as_str()
            .expect("revision string")
            .to_owned();
        assert_ne!(after, before);

        let rejected = crate::api::router(first_state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/instruments")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert!(rejected.status().is_client_error());
        assert_eq!(
            get_data_version(&second_state).await["data_revision"],
            after
        );

        let (instrument, _) = crate::db::instruments::upsert(
            &first_state.pool,
            &crate::db::instruments::NewInstrument {
                symbol: "FAILFX".to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: "Fail after price".to_owned(),
                kind: "STOCK".to_owned(),
                currency: "USD".to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument");
        crate::db::transactions::insert(
            &first_state.pool,
            &crate::db::transactions::NewTransaction {
                instrument_id: instrument.id,
                kind: crate::domain::TransactionKind::Buy,
                trade_date: chrono::NaiveDate::from_ymd_opt(2026, 9, 1).expect("date"),
                quantity: 1,
                price: Some(rust_decimal_macros::dec!(100)),
                dividend_per_share: None,
                currency: Some("USD".to_owned()),
                fx_rate_to_base: Some(rust_decimal_macros::dec!(10)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction");
        let timestamp = "2026-09-18T10:00:00Z".to_owned();
        crate::db::provider_symbols::upsert(
            &first_state.pool,
            &crate::db::provider_symbols::NewProviderSymbol {
                instrument_id: instrument.id,
                provider: crate::providers::MarketDataProvider::Yahoo,
                provider_symbol: "FAILFX".to_owned(),
                asset_class: None,
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: timestamp.clone(),
                updated_at: timestamp,
            },
        )
        .await
        .expect("mapping");
        price_provider.push_response(Ok(vec![crate::providers::DailyClose {
            provider: crate::providers::MarketDataProvider::Yahoo,
            provider_symbol: "FAILFX".to_owned(),
            date: first_state.clock.today(),
            close: rust_decimal_macros::dec!(101),
            currency: "USD".to_owned(),
        }]));
        fx_provider.push_response(Ok(vec![crate::providers::FxRate {
            provider: crate::providers::FxProvider::Frankfurter,
            base: "USD".to_owned(),
            quote: "SEK".to_owned(),
            date: first_state.clock.today(),
            rate: rust_decimal_macros::dec!(10.5),
        }]));
        sqlx::query(
            "CREATE TRIGGER fail_fx_write BEFORE INSERT ON fx_rates \
             BEGIN SELECT RAISE(ABORT, 'forced fx failure'); END",
        )
        .execute(&first_state.pool)
        .await
        .expect("failure trigger");
        let before_failed_refresh = get_data_version(&second_state).await["data_revision"]
            .as_str()
            .expect("revision")
            .to_owned();
        let failed_refresh = crate::api::router(first_state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/prices/refresh")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"mode":"latest"}"#))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");
        assert_eq!(failed_refresh.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            crate::db::prices::list(&second_state.pool)
                .await
                .expect("prices")
                .len(),
            1,
            "price write committed before the later failure"
        );
        assert_ne!(
            get_data_version(&second_state).await["data_revision"],
            before_failed_refresh
        );

        first_state.pool.close().await;
        second_state.pool.close().await;
        let _ = std::fs::remove_file(path);
    }
}
