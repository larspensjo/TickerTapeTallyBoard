mod cors;
mod data_version;
mod error;
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
    extract::State,
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
            post(import::sharesight_preview),
        )
        .route("/import/avanza/preview", post(import::avanza_preview))
        .route("/import/sharesight/commit", post(import::sharesight_commit))
        .route("/import/avanza/commit", post(import::avanza_commit))
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
    };
    use serde_json::{json, Value};
    use tower::ServiceExt;

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
