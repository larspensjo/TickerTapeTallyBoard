mod cors;
mod health;
mod root;
mod sharesight;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::{path::Path, sync::Arc};
use tower_http::services::ServeDir;

pub fn router() -> Router {
    api_routes()
        .route("/", get(root::handler))
        .layer(cors::layer())
}

pub fn router_with_static_assets(static_assets_dir: impl AsRef<Path>) -> Router {
    let static_assets_dir = static_assets_dir.as_ref();
    let static_assets = StaticAssets {
        index_path: Arc::from(static_assets_dir.join("index.html").into_boxed_path()),
    };

    api_routes()
        .fallback_service(
            ServeDir::new(static_assets_dir).fallback(get(static_index).with_state(static_assets)),
        )
        .layer(cors::layer())
}

fn api_routes() -> Router {
    Router::new().nest(
        "/api",
        Router::new().route("/health", get(health::handler)).route(
            "/import/sharesight/schema-preview",
            get(sharesight::handler),
        ),
    )
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
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn static_router_serves_frontend_index_for_root() {
        let fixture = StaticFixture::new();

        let response = router_with_static_assets(fixture.path())
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

        let response = router_with_static_assets(fixture.path())
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

        let response = router_with_static_assets(fixture.path())
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

        let response = router_with_static_assets(fixture.path())
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

        let response = router_with_static_assets(fixture.path())
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
