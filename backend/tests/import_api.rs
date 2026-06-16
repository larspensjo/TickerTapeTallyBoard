use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use ticker_tape_tally_board_backend::{api, db, import::raw_file_hash, state::AppState};
use tower::ServiceExt;

const SYNTHETIC: &[u8] = include_bytes!("fixtures/sharesight_synthetic.csv");
const MALFORMED: &[u8] = b"not,a,sharesight,report\n";

async fn test_state() -> AppState {
    AppState::for_tests().await
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

async fn send_json(state: &AppState, method: &str, uri: &str) -> (StatusCode, Value) {
    send_json_body(state, method, uri, serde_json::Value::Null).await
}

async fn send_json_body(
    state: &AppState,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
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

#[tokio::test]
async fn commit_writes_one_atomic_batch() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body["batch_id"].as_i64().expect("batch id") >= 1);
    assert_eq!(body["counts"]["rows"], 4);
    assert_eq!(body["counts"]["new_instruments"], 2);

    let (status, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(holdings.as_array().expect("array").len(), 2);
}

#[tokio::test]
async fn second_commit_of_same_file_is_rejected_unless_allowed() {
    let state = test_state().await;
    let (first, _) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(first, StatusCode::OK);

    let (duplicate, body) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(duplicate, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "duplicate_import");

    let (allowed, _) = send_bytes(
        &state,
        "/api/import/sharesight/commit?allow_duplicate=true",
        SYNTHETIC,
    )
    .await;
    assert_eq!(allowed, StatusCode::OK);
}

#[tokio::test]
async fn commit_returns_bad_request_for_malformed_csv() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", MALFORMED).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "header_not_found");
}

#[tokio::test]
async fn hard_error_is_rejected_before_any_write() {
    let state = test_state().await;
    let bad = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Sell,12/06/2026,−4,\"10,00\",USD,\"0,00\",\"0,00\",SEK,\"1,000000\",\"−40,00\",All Trades,\n",
    );

    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", bad.as_bytes()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "sell_exceeds_position");

    let instruments = db::instruments::list(&state.pool).await.expect("list");
    assert!(instruments.is_empty());

    let transactions = db::transactions::list(&state.pool).await.expect("list");
    assert!(transactions.is_empty());

    let batches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_batches")
        .fetch_one(&state.pool)
        .await
        .expect("count batches");
    assert_eq!(batches, 0);
}

#[tokio::test]
async fn hard_error_takes_precedence_over_duplicate_hash() {
    let state = test_state().await;
    let bad = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Sell,12/06/2026,−4,\"10,00\",USD,\"0,00\",\"0,00\",SEK,\"1,000000\",\"−40,00\",All Trades,\n",
    );
    sqlx::query("INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES (?, ?, ?)")
        .bind("SHARESIGHT")
        .bind("2026-06-15T00:00:00Z")
        .bind(raw_file_hash(bad.as_bytes()))
        .execute(&state.pool)
        .await
        .expect("seed duplicate batch");

    let (status, body) = send_bytes(&state, "/api/import/sharesight/commit", bad.as_bytes()).await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "sell_exceeds_position");
}

#[tokio::test]
async fn rollback_removes_a_batch() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (status, body) = send_json(
        &state,
        "POST",
        &format!("/api/import/sharesight/rollback/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["batch_id"], batch_id);
    assert_eq!(body["removed"].as_u64().expect("removed"), 4);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 0);
}

#[tokio::test]
async fn rollback_is_rejected_when_a_dependent_manual_sell_exists() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let instruments = db::instruments::list(&state.pool).await.expect("list");
    let asml = instruments
        .iter()
        .find(|instrument| instrument.symbol == "ASML")
        .expect("asml");

    let (sell_status, _) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": asml.id,
            "type": "Sell",
            "trade_date": "2026-06-20",
            "quantity": 3,
            "price": "650",
            "currency": "EUR"
        }),
    )
    .await;
    assert_eq!(sell_status, StatusCode::CREATED);

    let (status, body) = send_json(
        &state,
        "POST",
        &format!("/api/import/sharesight/rollback/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "sell_exceeds_position");

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert!(!holdings.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn rollback_is_rejected_when_a_dependent_manual_split_exists() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let instruments = db::instruments::list(&state.pool).await.expect("list");
    let msft = instruments
        .iter()
        .find(|instrument| instrument.symbol == "MSFT")
        .expect("msft");

    let (split_status, _) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": msft.id,
            "type": "Split",
            "trade_date": "2026-06-20",
            "quantity": 2
        }),
    )
    .await;
    assert_eq!(split_status, StatusCode::CREATED);

    let (status, body) = send_json(
        &state,
        "POST",
        &format!("/api/import/sharesight/rollback/{batch_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "split_without_position");

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert!(!holdings.as_array().expect("array").is_empty());
}

#[tokio::test]
async fn rollback_unknown_batch_is_not_found() {
    let state = test_state().await;
    let (status, body) = send_json(&state, "POST", "/api/import/sharesight/rollback/999").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "not_found");
}

#[tokio::test]
async fn same_file_can_be_reimported_after_rollback() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (rolled, _) = send_json(
        &state,
        "POST",
        &format!("/api/import/sharesight/rollback/{batch_id}"),
    )
    .await;
    assert_eq!(rolled, StatusCode::OK);

    let (status, preview) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert!(preview["duplicate_of_batch_id"].is_null());

    let (recommit, _) = send_bytes(&state, "/api/import/sharesight/commit", SYNTHETIC).await;
    assert_eq!(recommit, StatusCode::OK);
}
