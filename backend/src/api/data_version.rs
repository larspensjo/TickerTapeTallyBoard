use axum::{extract::State, Json};
use serde::Serialize;

use crate::state::AppState;

/// What snapshot of the data the app is serving, and what day it thinks it is.
/// Clients name this in every data request so panels cannot mix snapshots.
#[derive(Debug, Serialize)]
pub struct DataVersionResponse {
    pub data_revision: String,
    pub valuation_date: String,
    pub prices_refreshing: bool,
}

pub async fn handler(State(state): State<AppState>) -> Json<DataVersionResponse> {
    Json(DataVersionResponse {
        data_revision: state.revision.current(),
        valuation_date: state.clock.today().format("%Y-%m-%d").to_string(),
        prices_refreshing: state.market_data.is_refreshing(),
    })
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
        assert_eq!(body["data_revision"], state.revision.current());
        assert_eq!(body["prices_refreshing"], false);
    }

    #[tokio::test]
    async fn reads_do_not_change_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

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

        assert_eq!(state.revision.current(), before);
    }

    #[tokio::test]
    async fn a_successful_mutation_changes_the_revision() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

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

        assert_ne!(state.revision.current(), before);
    }

    #[tokio::test]
    async fn a_rejected_mutation_leaves_the_revision_alone() {
        let state = AppState::for_tests().await;
        let before = state.revision.current();

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

        assert_eq!(state.revision.current(), before);
    }

    #[tokio::test]
    async fn a_demo_mode_mutation_leaves_the_revision_alone() {
        let state = AppState::for_tests()
            .await
            .with_mode(crate::config::Mode::Demo);
        let before = state.revision.current();

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
        assert_eq!(state.revision.current(), before);
    }
}
