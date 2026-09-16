use axum::{body::Body, response::IntoResponse, Router};
use http::{header, Request, Response, Uri};
use tower::ServiceExt;

use crate::content_security_policy;

/// The one definition of the custom scheme used by the native window.
pub const WEBVIEW_SCHEME: &str = "tttb";

/// The custom-protocol URL opened by the window.
pub fn launch_url() -> tauri::WebviewUrl {
    tauri::WebviewUrl::CustomProtocol(
        format!("{WEBVIEW_SCHEME}://localhost/")
            .parse()
            .expect("the fixed launch URL is valid"),
    )
}

/// The browser-visible Windows origin for the custom protocol.
pub fn browser_visible_origin() -> String {
    format!("http://{WEBVIEW_SCHEME}.localhost")
}

/// Feed a custom-protocol request into Axum without using a socket.
pub async fn serve_request(router: Router, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let (parts, body) = request.into_parts();
    let method = parts.method.clone();
    let path = normalized_path_and_query(&parts.uri);
    let path_for_log = path.clone();
    let uri = path
        .parse::<Uri>()
        .expect("normalizing a request URI always yields origin-form URI");
    let mut request = Request::from_parts(parts, Body::from(body));
    *request.uri_mut() = uri;
    let (mut parts, body) = match router.clone().oneshot(request).await {
        Ok(response) => response.into_parts(),
        Err(never) => match never {},
    };

    let body = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(body) => body.to_vec(),
        Err(error) => {
            ticker_tape_tally_board_backend::engine_error!(
                "in-process HTTP bridge could not collect response body for {} {}: {error}",
                method,
                path_for_log
            );
            let response = ticker_tape_tally_board_backend::api::ApiError::internal(
                "The desktop bridge could not collect the response body.",
            )
            .into_response();
            let (error_parts, error_body) = response.into_parts();
            parts = error_parts;
            axum::body::to_bytes(error_body, usize::MAX)
                .await
                .expect("ApiError response body is in memory")
                .to_vec()
        }
    };
    parts.headers.insert(
        header::CONTENT_SECURITY_POLICY,
        header::HeaderValue::from_static(content_security_policy::VALUE),
    );
    Response::from_parts(parts, body)
}

fn normalized_path_and_query(uri: &Uri) -> String {
    uri.path_and_query()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use axum::{
        body::{to_bytes, Body},
        extract::Query,
        http::{header, Request, StatusCode},
        routing::get,
        Json, Router,
    };
    use futures::stream;
    use serde_json::{json, Value};
    use ticker_tape_tally_board_backend::{
        api::{self, body_limits},
        config::Mode,
        state::AppState,
    };
    use tower::ServiceExt;

    use super::*;

    fn csv(target: usize) -> Vec<u8> {
        let mut body = b"Synthetic Portfolio - All Trades Report between 2025-06-12 and 2026-06-12\n\nMarket,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n".to_vec();
        let prefix = b"STO,TEST,Test,Buy,12/06/2026,1,10,SEK,10,0,SEK,1,10,All Trades,";
        assert!(target > body.len() + prefix.len());
        let standard = [prefix.as_slice(), b"synthetic\n"].concat();
        while body.len() + standard.len() + prefix.len() < target {
            body.extend_from_slice(&standard);
        }
        let remaining = target - body.len() - prefix.len() - 1;
        body.extend_from_slice(prefix);
        body.extend(std::iter::repeat_n(b'x', remaining));
        body.push(b'\n');
        assert_eq!(body.len(), target);
        body
    }

    async fn bridge(
        router: Router,
        request: Request<Vec<u8>>,
    ) -> (StatusCode, http::HeaderMap, Vec<u8>) {
        let response = serve_request(router, request).await;
        let (parts, body) = response.into_parts();
        (parts.status, parts.headers, body)
    }

    async fn bare(router: Router, request: Request<Vec<u8>>) -> (StatusCode, Vec<u8>) {
        let (parts, body) = request.into_parts();
        let request = Request::from_parts(parts, Body::from(body));
        let response = router.oneshot(request).await.expect("router response");
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body")
            .to_vec();
        (status, body)
    }

    async fn seeded_state() -> (AppState, i64) {
        let state = AppState::for_tests().await;
        let (_, instrument) = bare(api::router(state.clone()), Request::builder().method("POST")
            .uri("/api/instruments").header(header::CONTENT_TYPE, "application/json")
            .body(br#"{"symbol":"TEST","exchange":"STO","name":"Test","type":"Stock","currency":"SEK"}"#.to_vec()).unwrap()).await;
        let instrument_id = serde_json::from_slice::<Value>(&instrument).unwrap()["id"]
            .as_i64()
            .unwrap();
        let (_, transaction) = bare(api::router(state.clone()), Request::builder().method("POST")
            .uri("/api/transactions").header(header::CONTENT_TYPE, "application/json")
            .body(format!(r#"{{"instrument_id":{instrument_id},"type":"Buy","trade_date":"2026-01-02","quantity":1,"price":"10","currency":"SEK"}}"#).into_bytes()).unwrap()).await;
        (
            state,
            serde_json::from_slice::<Value>(&transaction).unwrap()["id"]
                .as_i64()
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn bridge_carries_rejected_write_status_and_error_body() {
        let state = AppState::for_tests().await.with_mode(Mode::Demo);
        assert!(state.is_demo(), "this test requires demo state");
        let request = Request::builder()
            .method("POST")
            .uri("/api/transactions")
            .header(header::CONTENT_TYPE, "application/json")
            .body(b"{}".to_vec())
            .unwrap();
        let (status, _, body) = bridge(api::router(state.clone()), request.clone()).await;
        let (bare_status, bare_body) = bare(api::router(state), request).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(status, bare_status);
        assert_eq!(body, bare_body);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
            "demo_read_only"
        );
    }

    #[tokio::test]
    async fn bridge_carries_no_content_without_a_body() {
        let (state, id) = seeded_state().await;
        assert!(!state.is_demo(), "this test requires non-demo state");
        let (status, headers, body) = bridge(
            api::router(state),
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/transactions/{id}"))
                .body(Vec::new())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert!(body.is_empty());
        // Preserve the composed router's existing zero length verbatim.
        assert_eq!(headers.get(header::CONTENT_LENGTH).unwrap(), "0");
    }

    #[tokio::test]
    async fn bridge_carries_large_csv_upload() {
        let state = AppState::for_tests().await;
        assert!(!state.is_demo(), "this test requires non-demo state");
        let body = csv(body_limits::under_import_limit_bytes());
        let expected_rows = body.iter().filter(|&&byte| byte == b'\n').count() - 3;
        let (status, _, response) = bridge(
            api::router(state),
            Request::builder()
                .method("POST")
                .uri("/api/import/sharesight/preview")
                .header(header::CONTENT_TYPE, "text/csv")
                .body(body)
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let response = serde_json::from_slice::<Value>(&response).unwrap();
        assert!(response["metadata"].is_object());
        assert_eq!(response["counts"]["rows"], expected_rows);
    }

    #[tokio::test]
    async fn bridge_matches_bare_router_for_oversized_upload() {
        let state = AppState::for_tests().await;
        assert!(!state.is_demo(), "this test requires non-demo state");
        let request = Request::builder()
            .method("POST")
            .uri("/api/import/sharesight/preview")
            .header(header::CONTENT_TYPE, "text/csv")
            .body(csv(body_limits::over_import_limit_bytes()))
            .unwrap();
        let (status, _, body) = bridge(api::router(state.clone()), request.clone()).await;
        let (bare_status, bare_body) = bare(api::router(state), request).await;
        assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(status, bare_status);
        assert_eq!(body, bare_body);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
            "payload_too_large"
        );
    }

    #[tokio::test]
    async fn probe_limited_route_matches_real_import_route() {
        let state = AppState::for_tests().await;
        assert!(!state.is_demo(), "this test requires non-demo state");
        let router = crate::webview_probe::wrap(api::router(state));
        let body = csv(body_limits::over_import_limit_bytes());
        let probe = Request::builder()
            .method("POST")
            .uri("/__probe/echo-limited")
            .header(header::CONTENT_TYPE, "text/csv")
            .body(body.clone())
            .unwrap();
        let real = Request::builder()
            .method("POST")
            .uri("/api/import/sharesight/preview")
            .header(header::CONTENT_TYPE, "text/csv")
            .body(body)
            .unwrap();
        let (probe_status, _, probe_body) = bridge(router.clone(), probe).await;
        let (real_status, _, real_body) = bridge(router, real).await;
        assert_eq!(probe_status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(probe_status, real_status);
        assert_eq!(probe_body, real_body);
        assert_eq!(
            serde_json::from_slice::<Value>(&probe_body).unwrap()["error"]["code"],
            "payload_too_large"
        );
    }

    async fn query_echo(
        Query(query): Query<std::collections::BTreeMap<String, String>>,
    ) -> Json<Value> {
        Json(json!({"period": query.get("period")}))
    }

    #[tokio::test]
    async fn bridge_routes_every_request_uri_form() {
        let router = Router::new().route("/report", get(query_echo));
        let expected = br#"{"period":"month"}"#.to_vec();
        for uri in [
            format!("{WEBVIEW_SCHEME}://localhost/report?period=month"),
            format!("http://{WEBVIEW_SCHEME}.localhost/report?period=month"),
            "/report?period=month".to_owned(),
        ] {
            let (status, _, body) = bridge(
                router.clone(),
                Request::builder().uri(uri).body(Vec::new()).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, expected);
        }
    }

    #[tokio::test]
    async fn probe_echo_accepts_under_limit_body() {
        let body = csv(body_limits::under_import_limit_bytes());
        let router = crate::webview_probe::wrap(Router::new());
        let (status, response) = bare(
            router,
            Request::builder()
                .method("POST")
                .uri("/__probe/echo")
                .header(header::CONTENT_TYPE, "text/csv")
                .body(body.clone())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            serde_json::from_slice::<Value>(&response).unwrap()["received_bytes"],
            body.len()
        );
    }

    #[tokio::test]
    async fn bridge_emits_content_security_policy_header() {
        let dir = fixture_dir();
        fs::create_dir_all(dir.path.join("assets")).unwrap();
        fs::write(dir.path.join("index.html"), "<html></html>").unwrap();
        fs::write(dir.path.join("assets/app.css"), "body{}").unwrap();
        let router = api::router_with_static_assets(&dir.path, AppState::for_tests().await);
        for uri in ["/", "/assets/app.css", "/api/health"] {
            let (_, headers, _) = bridge(
                router.clone(),
                Request::builder().uri(uri).body(Vec::new()).unwrap(),
            )
            .await;
            assert_eq!(
                headers.get(header::CONTENT_SECURITY_POLICY).unwrap(),
                content_security_policy::VALUE
            );
        }
    }

    #[tokio::test]
    async fn bridge_reports_body_collection_failure_as_json_error() {
        let router = Router::new().route(
            "/broken",
            get(|| async {
                axum::response::Response::new(Body::from_stream(stream::iter([Err::<
                    axum::body::Bytes,
                    std::io::Error,
                >(
                    std::io::Error::other("broken"),
                )])))
            }),
        );
        let (status, _, body) = bridge(
            router,
            Request::builder().uri("/broken").body(Vec::new()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"]["code"],
            "internal_error"
        );
    }

    struct FixtureDirectory {
        path: PathBuf,
    }

    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn fixture_dir() -> FixtureDirectory {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        FixtureDirectory {
            path: std::env::temp_dir().join(format!("desktop-bridge-{unique}")),
        }
    }
}
