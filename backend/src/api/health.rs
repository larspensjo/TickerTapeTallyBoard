use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

use crate::state::AppState;

pub(super) async fn handler(State(state): State<AppState>) -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        demo: state.demo_mode,
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
    demo: bool,
    build: BuildInfo,
}

#[derive(Debug, Serialize)]
struct BuildInfo {
    package: &'static str,
    profile: &'static str,
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
        let state = crate::state::AppState::for_tests().await;
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
        assert_eq!(body["demo"], false);
        assert_eq!(body["build"]["package"], env!("CARGO_PKG_NAME"));
        assert!(body["build"]["profile"].is_string());
    }

    #[tokio::test]
    async fn health_endpoint_returns_demo_flag_for_demo_state() {
        let state = crate::state::AppState::for_tests()
            .await
            .with_demo_mode(true);
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

        assert_eq!(body["demo"], true);
    }
}
