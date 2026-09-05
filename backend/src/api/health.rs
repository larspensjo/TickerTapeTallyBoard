use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

use crate::state::AppState;

pub(super) async fn handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        mode: state.mode,
        ledger: LedgerInfo {
            mode: if state.ledger_path.is_some() {
                "file"
            } else {
                "memory"
            },
            path: state
                .ledger_path
                .as_ref()
                .map(|path| path.display().to_string()),
        },
        build: BuildInfo {
            package: env!("CARGO_PKG_NAME"),
            profile: build_profile(),
        },
    })
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    mode: crate::config::Mode,
    ledger: LedgerInfo,
    build: BuildInfo,
}

#[derive(Debug, Serialize)]
struct BuildInfo {
    package: &'static str,
    profile: &'static str,
}

#[derive(Debug, Serialize)]
struct LedgerInfo {
    mode: &'static str,
    path: Option<String>,
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_returns_status_and_build_info() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_ledger_path(Some(std::path::PathBuf::from("C:/target/portfolio.sqlite")));
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("body should be JSON");

        assert_eq!(body["status"], "ok");
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["mode"], "production");
        assert_eq!(body["ledger"]["mode"], "file");
        assert_eq!(body["ledger"]["path"], "C:/target/portfolio.sqlite");
        assert!(body.get("demo").is_none());
        assert_eq!(body["build"]["package"], env!("CARGO_PKG_NAME"));
        assert!(body["build"]["profile"].is_string());
    }

    #[tokio::test]
    async fn health_endpoint_returns_demo_ledger() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_mode(crate::config::Mode::Demo);
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should complete");

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body should be readable");
        let body: Value = serde_json::from_slice(&body).expect("body should be JSON");

        assert_eq!(body["mode"], "demo");
        assert_eq!(body["ledger"]["mode"], "memory");
        assert!(body["ledger"]["path"].is_null());
        assert!(body.get("demo").is_none());
    }
}
