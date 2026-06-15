use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use ticker_tape_tally_board_backend::{api, db, import::raw_file_hash, state::AppState};
use tower::ServiceExt;

const SYNTHETIC: &[u8] = include_bytes!("fixtures/sharesight_synthetic.csv");
const MALFORMED: &[u8] = b"not,a,sharesight,report\n";

async fn test_state() -> AppState {
    AppState::new(db::memory_pool().await.expect("memory pool"))
}

async fn send_bytes(state: &AppState, uri: &str, body: &[u8]) -> (StatusCode, Value) {
    let request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "text/csv")
        .body(Body::from(body.to_vec()))
        .expect("request builds");
    let response = api::router(state.clone())
        .oneshot(request)
        .await
        .expect("completes");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json")
    };
    (status, value)
}

#[tokio::test]
async fn preview_returns_counts_and_writes_nothing() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 4);
    assert_eq!(body["counts"]["buys"], 2);
    assert_eq!(body["counts"]["sells"], 1);
    assert_eq!(body["counts"]["splits"], 1);
    assert_eq!(body["counts"]["new_instruments"], 2);
    assert_eq!(body["counts"]["errors"], 0);
    assert_eq!(body["duplicate_of_batch_id"], Value::Null);

    let instruments = db::instruments::list(&state.pool).await.expect("list");
    assert!(instruments.is_empty());

    let transactions = db::transactions::list(&state.pool).await.expect("list");
    assert!(transactions.is_empty());
}

#[tokio::test]
async fn preview_returns_plan_shaped_parse_errors() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", MALFORMED).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["metadata"], Value::Null);
    assert_eq!(body["counts"]["errors"], 1);
    let errors = body["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "header_not_found");
}

#[tokio::test]
async fn preview_reports_duplicate_batch_when_hash_exists() {
    let state = test_state().await;
    let batch_id: i64 = sqlx::query_scalar(
        "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?) RETURNING id",
    )
        .bind("SHARESIGHT")
        .bind("2026-06-15T00:00:00Z")
        .bind(raw_file_hash(SYNTHETIC))
        .fetch_one(&state.pool)
        .await
        .expect("seed duplicate batch");

    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["duplicate_of_batch_id"], batch_id);
}
