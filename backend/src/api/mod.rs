mod cors;
mod error;
mod gains;
mod health;
mod holdings;
mod import;
mod instrument_prices;
mod instruments;
mod portfolio;
mod prices;
mod provider_symbols;
mod rebalance;
mod root;
#[cfg(test)]
mod test_support;
mod transactions;
mod valuation;
mod valued_holdings;

use axum::{
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse},
    routing::{get, post, put},
    Router,
};
use std::{path::Path, sync::Arc};
use tower_http::services::ServeDir;

use crate::state::AppState;

pub use error::ApiError;

pub fn reject_demo_mutation(state: &AppState) -> Result<(), ApiError> {
    if state.demo_mode {
        return Err(ApiError::demo_read_only());
    }
    Ok(())
}

pub fn router(state: AppState) -> Router {
    let api = api_router().route_layer(middleware::from_fn_with_state(
        state.clone(),
        demo_read_only_layer,
    ));

    Router::new()
        .route("/", get(root::handler))
        .nest("/api", api)
        .layer(cors::layer())
        .with_state(state)
}

pub fn router_with_static_assets(static_assets_dir: impl AsRef<Path>, state: AppState) -> Router {
    let static_assets_dir = static_assets_dir.as_ref();
    let api = api_router().route_layer(middleware::from_fn_with_state(
        state.clone(),
        demo_read_only_layer,
    ));
    let static_assets = StaticAssets {
        index_path: Arc::from(static_assets_dir.join("index.html").into_boxed_path()),
    };

    Router::new()
        .nest("/api", api)
        .fallback_service(
            ServeDir::new(static_assets_dir).fallback(get(static_index).with_state(static_assets)),
        )
        .layer(cors::layer())
        .with_state(state)
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
        .route("/portfolio/value-history", get(portfolio::value_history))
        .route("/rebalance", get(rebalance::handler))
        .route("/holdings", get(holdings::list))
        .route("/gains", get(gains::list))
        .route(
            "/instruments",
            get(instruments::list).post(instruments::create),
        )
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
}

async fn demo_read_only_layer(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<axum::response::Response, ApiError> {
    if state.demo_mode && is_mutating_method(request.method()) {
        return Err(ApiError::demo_read_only());
    }

    Ok(next.run(request).await)
}

fn is_mutating_method(method: &Method) -> bool {
    !matches!(method, &Method::GET | &Method::HEAD | &Method::OPTIONS)
}

#[derive(Clone)]
struct StaticAssets {
    index_path: Arc<Path>,
}

async fn static_index(State(static_assets): State<StaticAssets>) -> impl IntoResponse {
    match tokio::fs::read_to_string(static_assets.index_path.as_ref()).await {
        Ok(index) => Html(index).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Request, StatusCode},
    };
    use serde_json::{json, Value};
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn demo_mode_rejects_mutating_routes() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_demo_mode(true);

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
                "/api/instruments/1/conviction",
                json!({"conviction":"Low"}),
            ),
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
    async fn static_router_serves_frontend_index_for_root() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(
            String::from_utf8_lossy(&body).contains("TTTB static fixture"),
            "root should serve built frontend index"
        );
    }

    #[tokio::test]
    async fn static_router_uses_index_fallback_for_frontend_routes() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/portfolio/holdings")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(
            String::from_utf8_lossy(&body).contains("TTTB static fixture"),
            "frontend routes should fall back to index.html"
        );
    }

    #[tokio::test]
    async fn static_router_serves_root_static_files_before_spa_fallback() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/manifest.json")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(
            String::from_utf8_lossy(&body).contains("TTTB manifest fixture"),
            "root static files should be served before SPA fallback"
        );
    }

    #[tokio::test]
    async fn static_router_serves_asset_files_with_content_type() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/assets/app.css")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/css")),
            "CSS assets should be served with a CSS content type"
        );

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(
            String::from_utf8_lossy(&body).contains(".fixture"),
            "asset response should contain the static file body"
        );
    }

    #[tokio::test]
    async fn static_router_keeps_api_routes_available() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
    }

    struct StaticFixture {
        dir: PathBuf,
    }

    impl StaticFixture {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should be after epoch")
                .as_nanos();
            let unique = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "tttb-static-fixture-{}-{timestamp}-{unique}",
                std::process::id()
            ));

            fs::create_dir_all(dir.join("assets")).expect("fixture directory should be created");
            fs::write(
                dir.join("index.html"),
                "<!doctype html><html><body>TTTB static fixture</body></html>",
            )
            .expect("fixture index should be written");
            fs::write(
                dir.join("manifest.json"),
                r#"{"name":"TTTB manifest fixture"}"#,
            )
            .expect("fixture manifest should be written");
            fs::write(dir.join("assets/app.css"), ".fixture { color: white; }")
                .expect("fixture asset should be written");

            Self { dir }
        }

        fn path(&self) -> &Path {
            &self.dir
        }
    }

    impl Drop for StaticFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}
