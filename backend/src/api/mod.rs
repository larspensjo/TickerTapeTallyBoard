pub mod body_limits;
mod cors;
mod data_version;
mod error;
mod extract;
mod gains;
mod health;
mod holdings;
mod import;
mod instrument_lookup;
mod instrument_prices;
mod instruments;
mod portfolio;
mod prices;
mod provider_symbols;
mod rebalance;
mod root;
pub(crate) mod static_assets;
#[cfg(test)]
mod test_support;
mod transactions;
mod valuation;
mod valued_holdings;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{Method, Request},
    middleware::{self, Next},
    routing::{any, delete, get, post, put},
    Router,
};
use std::path::Path;

use crate::state::AppState;

pub use error::ApiError;

pub fn reject_demo_mutation(state: &AppState) -> Result<(), ApiError> {
    if state.is_demo() {
        return Err(ApiError::demo_read_only());
    }
    Ok(())
}

pub fn router(state: AppState) -> Router {
    api_mount(&state)
        .route("/", get(root::handler))
        .layer(cors::layer())
        .with_state(state)
}

pub fn router_with_static_assets(static_assets_dir: impl AsRef<Path>, state: AppState) -> Router {
    let static_assets_dir = static_assets_dir.as_ref();

    api_mount(&state)
        .fallback_service(static_assets::service(static_assets_dir))
        .layer(cors::layer())
        .with_state(state)
}

fn api_mount(state: &AppState) -> Router<AppState> {
    let api = api_router()
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            demo_read_only_layer,
        ))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            data_revision_layer,
        ));

    Router::new()
        .nest("/api", api)
        .route("/api/", any(error::unmatched_route))
}

fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health::handler))
        .route(
            "/import/sharesight/preview",
            post(import::sharesight_preview)
                .layer(DefaultBodyLimit::max(body_limits::IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/import/avanza/preview",
            post(import::avanza_preview)
                .layer(DefaultBodyLimit::max(body_limits::IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/import/sharesight/commit",
            post(import::sharesight_commit)
                .layer(DefaultBodyLimit::max(body_limits::IMPORT_BODY_LIMIT_BYTES)),
        )
        .route(
            "/import/avanza/commit",
            post(import::avanza_commit)
                .layer(DefaultBodyLimit::max(body_limits::IMPORT_BODY_LIMIT_BYTES)),
        )
        .route("/import/rollback/{batch_id}", post(import::rollback))
        .route(
            "/import/sharesight/rollback/{batch_id}",
            post(import::rollback),
        )
        .route("/prices/refresh", post(prices::refresh))
        .route("/prices/status", get(prices::status))
        .route("/data-version", get(data_version::handler))
        .route("/portfolio/value-history", get(portfolio::value_history))
        .route("/rebalance", get(rebalance::handler))
        .route("/holdings", get(holdings::list))
        .route("/gains", get(gains::list))
        .route(
            "/instruments",
            get(instruments::list).post(instruments::create),
        )
        .route("/instruments/lookup", get(instrument_lookup::lookup))
        .route("/instruments/{id}", delete(instruments::remove))
        .route(
            "/instruments/convictions",
            put(instruments::update_convictions),
        )
        .route(
            "/instruments/{id}/conviction",
            put(instruments::update_conviction),
        )
        .route("/instruments/{id}/prices", get(instrument_prices::list))
        .route(
            "/instruments/{id}/provider-symbols/{provider}",
            put(provider_symbols::update),
        )
        .route(
            "/transactions",
            get(transactions::list).post(transactions::create),
        )
        .route(
            "/transactions/{id}",
            put(transactions::replace).delete(transactions::remove),
        )
        .fallback(error::unmatched_route)
        .layer(DefaultBodyLimit::max(body_limits::API_BODY_LIMIT_BYTES))
}

async fn demo_read_only_layer(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<axum::response::Response, ApiError> {
    if state.is_demo() && is_mutating_method(request.method()) {
        return Err(ApiError::demo_read_only());
    }

    Ok(next.run(request).await)
}

/// Records that stored data may have changed. Every mutation reaches the app
/// through a mutating HTTP method, so bumping here means no handler has to
/// remember to do it.
///
/// A client error means the request was rejected before touching anything, so
/// the revision stays put. A server error does not: a refresh can write prices
/// and still fail afterwards, and that data must not stay hidden behind an
/// unchanged revision.
async fn data_revision_layer(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> axum::response::Response {
    let mutating = is_mutating_method(request.method());
    let response = next.run(request).await;

    if mutating && !response.status().is_client_error() {
        state.revision.bump();
    }

    response
}

fn is_mutating_method(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
        Router,
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

    fn generated_sharesight_csv(target_bytes: usize) -> Vec<u8> {
        let mut body =
            b"Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n\n"
                .to_vec();
        body.extend_from_slice(b"Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n");

        let row_prefix = b"STO,TEST,Test,Buy,12/06/2026,1,10,SEK,10,0,SEK,1,10,All Trades,";
        let row_suffix = b"\n";
        let standard_row = [row_prefix.as_slice(), b"synthetic", row_suffix].concat();
        while body.len() + standard_row.len() + row_prefix.len() + row_suffix.len() <= target_bytes
        {
            body.extend_from_slice(&standard_row);
        }

        let remaining = target_bytes - body.len();
        assert!(remaining >= row_prefix.len() + row_suffix.len());
        body.extend_from_slice(row_prefix);
        body.extend(std::iter::repeat_n(
            b'x',
            remaining - row_prefix.len() - row_suffix.len(),
        ));
        body.extend_from_slice(row_suffix);
        assert_eq!(body.len(), target_bytes);
        body
    }

    fn generated_instrument_json(target_bytes: usize) -> Vec<u8> {
        let mut value = json!({
            "symbol": "TEST",
            "exchange": "STO",
            "name": "",
            "type": "Stock",
            "currency": "SEK"
        });
        let base_len = serde_json::to_vec(&value)
            .expect("JSON should serialize")
            .len();
        value["name"] = Value::String("x".repeat(target_bytes - base_len));
        let body = serde_json::to_vec(&value).expect("JSON should serialize");
        assert_eq!(body.len(), target_bytes);
        body
    }

    async fn send_raw(
        router: Router,
        uri: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> (StatusCode, Value) {
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(uri)
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(body))
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let value = serde_json::from_slice(&body).expect("body should be JSON");
        (status, value)
    }

    #[tokio::test]
    async fn import_preview_accepts_body_derived_from_under_limit() {
        let state = crate::state::AppState::for_tests().await;
        assert!(!state.is_demo());
        let body = generated_sharesight_csv(body_limits::under_import_limit_bytes());
        let (status, value) = send_raw(
            router(state),
            "/api/import/sharesight/preview",
            "text/csv",
            body,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert!(value["metadata"].is_object());
    }

    #[tokio::test]
    async fn import_preview_rejects_body_derived_from_over_limit() {
        let state = crate::state::AppState::for_tests().await;
        assert!(!state.is_demo());
        let body = generated_sharesight_csv(body_limits::over_import_limit_bytes());
        let (status, value) = send_raw(
            router(state),
            "/api/import/sharesight/preview",
            "text/csv",
            body,
        )
        .await;

        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(value["error"]["code"], "payload_too_large");
        assert_eq!(
            value["error"]["message"],
            format!(
                "Request body exceeds the {} MB limit.",
                body_limits::IMPORT_BODY_LIMIT_BYTES / body_limits::BYTES_PER_MIB
            )
        );
    }

    #[tokio::test]
    async fn discriminating_body_size_uses_import_and_api_limits() {
        let state = crate::state::AppState::for_tests().await;
        assert!(!state.is_demo());
        let body_size = body_limits::API_BODY_LIMIT_BYTES + body_limits::API_BODY_LIMIT_BYTES / 2;
        let csv_body = generated_sharesight_csv(body_size);
        let json_body = generated_instrument_json(body_size);
        let app = router(state);

        let (import_status, _) = send_raw(
            app.clone(),
            "/api/import/sharesight/preview",
            "text/csv",
            csv_body,
        )
        .await;
        let (api_status, api_value) =
            send_raw(app, "/api/instruments", "application/json", json_body).await;

        assert_eq!(import_status, StatusCode::OK);
        assert_eq!(api_status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(api_value["error"]["code"], "payload_too_large");
        assert_eq!(
            api_value["error"]["message"],
            format!(
                "Request body exceeds the {} MB limit.",
                body_limits::API_BODY_LIMIT_BYTES / body_limits::BYTES_PER_MIB
            )
        );
    }

    #[tokio::test]
    async fn malformed_json_returns_standard_invalid_json_error() {
        let state = crate::state::AppState::for_tests().await;
        assert!(!state.is_demo());
        let (status, value) = send_raw(
            router(state),
            "/api/instruments",
            "application/json",
            b"{not valid json".to_vec(),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"]["code"], "invalid_json");
    }

    #[tokio::test]
    async fn demo_mode_rejects_mutating_routes() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_mode(crate::config::Mode::Demo);

        for (method, uri, body) in [
            (
                "POST",
                "/api/transactions",
                json!({"instrument_id":1,"type":"Buy","trade_date":"2026-06-12","quantity":1,"price":"10","currency":"SEK"}),
            ),
            (
                "PUT",
                "/api/transactions/1",
                json!({"instrument_id":1,"type":"Buy","trade_date":"2026-06-12","quantity":1,"price":"10","currency":"SEK"}),
            ),
            ("DELETE", "/api/transactions/1", Value::Null),
            (
                "POST",
                "/api/instruments",
                json!({"symbol":"TEST","exchange":"STO","name":"Test","type":"Stock","currency":"SEK"}),
            ),
            (
                "PUT",
                "/api/instruments/1/provider-symbols/YAHOO",
                json!({"provider_symbol":"TEST.ST","currency":"SEK","enabled":true}),
            ),
            (
                "PUT",
                "/api/instruments/1/provider-symbols/NASDAQ_NORDIC",
                json!({"provider_symbol":"TX2997672","asset_class":"SHARES","currency":"SEK","enabled":true}),
            ),
            (
                "PUT",
                "/api/instruments/1/conviction",
                json!({"conviction":"Low"}),
            ),
            ("DELETE", "/api/instruments/1", Value::Null),
            (
                "PUT",
                "/api/instruments/convictions",
                json!({"changes":[{"instrument_id":1,"conviction":"High"}]}),
            ),
            ("POST", "/api/import/sharesight/preview", json!({})),
            ("POST", "/api/import/avanza/preview", json!({})),
            ("POST", "/api/import/sharesight/commit", json!({})),
            ("POST", "/api/import/avanza/commit", json!({})),
            ("POST", "/api/import/rollback/1", Value::Null),
            ("POST", "/api/import/sharesight/rollback/1", Value::Null),
            ("POST", "/api/prices/refresh", json!({"mode":"latest"})),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("request should build");

            let response = router(state.clone())
                .oneshot(request)
                .await
                .expect("request should complete");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {uri}");

            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body should be readable");
            let body: Value = serde_json::from_slice(&body).expect("body should be JSON");
            assert_eq!(body["error"]["code"], "demo_read_only", "{method} {uri}");
        }
    }

    #[tokio::test]
    async fn unmatched_api_routes_return_documented_not_found_errors() {
        let fixture = static_assets::tests::StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        for router in [
            router(state.clone()),
            router_with_static_assets(fixture.path(), state.clone()),
        ] {
            for (uri, accept, expected_path) in [
                (
                    "/api/does-not-exist?ignored=true",
                    "*/*",
                    "/api/does-not-exist",
                ),
                ("/api/", "text/html", "/api/"),
                ("/api/", "*/*", "/api/"),
            ] {
                let response = router
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method("GET")
                            .uri(uri)
                            .header(header::ACCEPT, accept)
                            .body(Body::empty())
                            .expect("request should build"),
                    )
                    .await
                    .expect("request should complete");

                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri} {accept}");
                assert_eq!(
                    response.headers().get(header::CONTENT_TYPE),
                    Some(&header::HeaderValue::from_static("application/json")),
                    "{uri} {accept}"
                );
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body should be readable");
                let body: Value = serde_json::from_slice(&body).expect("body should be JSON");
                assert_eq!(body["error"]["code"], "not_found", "{uri} {accept}");
                assert_eq!(
                    body["error"]["message"],
                    format!("No API route matches GET {expected_path}"),
                    "{uri} {accept}"
                );
            }
        }
    }

    #[tokio::test]
    async fn api_method_mismatch_remains_method_not_allowed() {
        let fixture = static_assets::tests::StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        for router in [
            router(state.clone()),
            router_with_static_assets(fixture.path(), state.clone()),
        ] {
            let response = router
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/instruments/1")
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
                .expect("request should complete");

            assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        }
    }
}
