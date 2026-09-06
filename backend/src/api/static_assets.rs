//! Static frontend files with a browser-navigation fallback to the SPA shell.
//!
//! Existing files are served for every request. A missing path receives the shell only when its
//! `Accept` header explicitly accepts HTML. This deliberately leaves one residual: entering a
//! missing asset URL in a browser address bar can request HTML and therefore receive the shell.
//! The routing contract does not infer asset paths from a frontend build-directory name.

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::{path::Path, sync::Arc};
use tower_http::services::ServeDir;

const INDEX_FILE_NAME: &str = "index.html";

#[derive(Clone)]
struct StaticAssets {
    index_path: Arc<Path>,
}

impl StaticAssets {
    fn new(static_assets_dir: &Path) -> Self {
        Self {
            index_path: Arc::from(index_path(static_assets_dir).into_boxed_path()),
        }
    }
}

pub(crate) fn index_path(static_assets_dir: &Path) -> std::path::PathBuf {
    static_assets_dir.join(INDEX_FILE_NAME)
}

pub(super) fn service(static_assets_dir: &Path) -> Router {
    let static_assets = StaticAssets::new(static_assets_dir);
    Router::new().fallback_service(
        ServeDir::new(static_assets_dir).fallback(get(static_index).with_state(static_assets)),
    )
}

async fn static_index(
    State(static_assets): State<StaticAssets>,
    request: Request,
) -> impl IntoResponse {
    if !accepts_html(
        request
            .headers()
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok()),
    ) {
        return StatusCode::NOT_FOUND.into_response();
    }

    match tokio::fs::read_to_string(static_assets.index_path.as_ref()).await {
        Ok(index) => Html(index).into_response(),
        Err(error) => {
            crate::engine_error!(
                "failed to read SPA index at {}: {error}",
                static_assets.index_path.display()
            );
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// Treats only an explicit, non-rejected `text/html` media range as a page navigation.
///
/// Missing headers and `*/*` are intentionally not navigations. A browser address-bar request for
/// a missing asset can still qualify because browsers commonly send `text/html` in that case.
fn accepts_html(accept: Option<&str>) -> bool {
    accept.is_some_and(|value| {
        value.split(',').any(|media_range| {
            let mut parameters = media_range.split(';');
            let media_type = parameters.next().unwrap_or_default().trim();
            if !media_type.eq_ignore_ascii_case("text/html") {
                return false;
            }

            !parameters.any(|parameter| {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    return false;
                };
                name.trim().eq_ignore_ascii_case("q")
                    && value
                        .trim()
                        .parse::<f32>()
                        .is_ok_and(|quality| quality == 0.0)
            })
        })
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::{accepts_html, index_path};
    use crate::api::router_with_static_assets;
    use axum::{
        body::{to_bytes, Body},
        http::{header, Method, Request, StatusCode},
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;

    static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn navigation_requires_an_accepted_html_media_type() {
        assert!(accepts_html(Some("text/html")));
        assert!(accepts_html(Some(
            "text/html,application/xhtml+xml,application/xml;q=0.9"
        )));
        assert!(!accepts_html(Some("application/json")));
        assert!(!accepts_html(Some("*/*")));
        assert!(!accepts_html(Some("text/html;q=0")));
        assert!(!accepts_html(None));
    }

    #[tokio::test]
    async fn static_router_serves_frontend_index_for_root() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;
        let router = router_with_static_assets(fixture.path(), state);

        for accept in [Some("*/*"), None] {
            let mut request = Request::builder().uri("/");
            if let Some(accept) = accept {
                request = request.header(header::ACCEPT, accept);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).expect("request should build"))
                .await
                .expect("request should complete");

            assert_eq!(response.status(), StatusCode::OK, "Accept: {accept:?}");
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body should be readable");
            assert!(
                String::from_utf8_lossy(&body).contains("TTTB static fixture"),
                "root should serve built frontend index for Accept: {accept:?}"
            );
        }
    }

    #[tokio::test]
    async fn static_router_uses_index_fallback_for_frontend_routes() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/portfolio/holdings")
                    .header(
                        header::ACCEPT,
                        "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                    )
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
                    .header(header::ACCEPT, "*/*")
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
    async fn static_router_returns_not_found_for_missing_non_html_assets() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/assets/nope-does-not-exist.js")
                    .header(header::ACCEPT, "application/javascript")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(
            !String::from_utf8_lossy(&body).contains("TTTB static fixture"),
            "missing non-HTML assets must not receive the SPA shell"
        );
    }

    #[tokio::test]
    async fn static_router_uses_index_fallback_for_missing_html_navigation() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;

        let response = router_with_static_assets(fixture.path(), state)
            .oneshot(
                Request::builder()
                    .uri("/assets/nope-does-not-exist.js")
                    .header(header::ACCEPT, "text/html")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        assert!(String::from_utf8_lossy(&body).contains("TTTB static fixture"));
    }

    #[tokio::test]
    async fn static_router_serves_asset_files_with_content_type() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;
        let router = router_with_static_assets(fixture.path(), state);

        for accept in ["application/json", "text/html", "*/*"] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/assets/app.css")
                        .header(header::ACCEPT, accept)
                        .body(Body::empty())
                        .expect("request should build"),
                )
                .await
                .expect("request should complete");

            assert_eq!(response.status(), StatusCode::OK, "Accept: {accept}");
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
    }

    #[tokio::test]
    async fn static_router_supports_head_for_root_and_frontend_routes() {
        let fixture = StaticFixture::new();
        let state = crate::state::AppState::for_tests().await;
        let router = router_with_static_assets(fixture.path(), state);

        for (uri, accept) in [("/", None), ("/board", Some("text/html"))] {
            let mut request = Request::builder().method(Method::HEAD).uri(uri);
            if let Some(accept) = accept {
                request = request.header(header::ACCEPT, accept);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).expect("request should build"))
                .await
                .expect("request should complete");

            assert_eq!(response.status(), StatusCode::OK, "HEAD {uri}");
        }
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

    pub(in crate::api) struct StaticFixture {
        dir: PathBuf,
    }

    impl StaticFixture {
        pub(in crate::api) fn new() -> Self {
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
                index_path(&dir),
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

        pub(in crate::api) fn path(&self) -> &Path {
            &self.dir
        }
    }

    impl Drop for StaticFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}
