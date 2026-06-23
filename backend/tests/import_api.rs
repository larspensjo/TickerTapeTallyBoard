use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
use ticker_tape_tally_board_backend::{api, db, import::raw_file_hash, state::AppState};
use tower::ServiceExt;

const SYNTHETIC: &[u8] = include_bytes!("fixtures/sharesight_synthetic.csv");
const AVANZA: &[u8] = include_bytes!("fixtures/avanza_synthetic.csv");
const AVANZA_V2: &[u8] = include_bytes!("fixtures/avanza_synthetic_v2.csv");
const MALFORMED: &[u8] = b"not,a,sharesight,report\n";
const TWO_ASSETS_ONE_BAD: &str = concat!(
    "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
    "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
    "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
    "XETR,ASML,ASML Holding,Sell,12/06/2026,−4,\"600,00\",EUR,\"0,00\",\"0,00\",SEK,\"0,100000\",\"−2400,00\",All Trades,\n",
);

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
async fn preview_reports_assets_and_extended_counts() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["counts"]["dividends"], 0);
    assert_eq!(body["counts"]["skipped"], 0);

    let assets = body["assets"].as_array().expect("assets array");
    assert_eq!(assets.len(), 2);
    assert!(assets
        .iter()
        .all(|asset| asset["default_selected"] == Value::Bool(true)));
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
async fn import_batches_accepts_avanza_source() {
    let state = test_state().await;
    let batch_id: i64 = sqlx::query_scalar(
        "INSERT INTO import_batches (source, imported_at, raw_file_hash) VALUES ('AVANZA', ?, ?) RETURNING id",
    )
    .bind("2026-06-16T00:00:00Z")
    .bind("deadbeef")
    .fetch_one(&state.pool)
    .await
    .expect("AVANZA source should be accepted");

    assert!(batch_id >= 1);
}

#[tokio::test]
async fn avanza_preview_counts_and_groups() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", AVANZA).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["counts"]["dividends"], 1);
    assert_eq!(body["counts"]["skipped"], 1);
    assert_eq!(body["counts"]["errors"], 0);

    let warnings = body["warnings"].as_array().expect("warnings");
    assert!(warnings
        .iter()
        .any(|warning| warning["code"] == "missing_fx"));
    assert!(warnings
        .iter()
        .any(|warning| warning["code"] == "non_integer_quantity"));
}

#[tokio::test]
async fn avanza_preview_returns_plan_shaped_parse_errors() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", MALFORMED).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["metadata"], Value::Null);
    assert_eq!(body["counts"]["errors"], 1);
    let errors = body["errors"].as_array().expect("errors array");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["code"], "header_not_found");
}

#[tokio::test]
async fn avanza_commit_writes_avanza_batch_and_persists_native_source_currency() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);
    let batch_id = body["batch_id"].as_i64().expect("batch id");

    let source: String = sqlx::query_scalar("SELECT source FROM import_batches WHERE id = ?")
        .bind(batch_id)
        .fetch_one(&state.pool)
        .await
        .expect("source");
    assert_eq!(source, "AVANZA");

    let asml = db::instruments::find_by_isin(&state.pool, "NL0010273215")
        .await
        .expect("query")
        .expect("asml instrument");
    let source_currency: Option<String> = sqlx::query_scalar(
        "SELECT source_currency FROM transactions WHERE instrument_id = ? LIMIT 1",
    )
    .bind(asml.id)
    .fetch_one(&state.pool)
    .await
    .expect("source_currency");
    assert_eq!(source_currency.as_deref(), Some("EUR"));
    assert_eq!(asml.symbol, "NL0010273215");
    assert_eq!(asml.exchange, "AVANZA");
}

#[tokio::test]
async fn avanza_commit_matches_existing_instrument_by_isin() {
    let state = test_state().await;
    let existing = sqlx::query_scalar::<_, i64>(
        "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
         VALUES ('US81762P1021','AVANZA','ServiceNow','STOCK','USD','US81762P1021') RETURNING id",
    )
    .fetch_one(&state.pool)
    .await
    .expect("seed");

    let (status, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);

    let now = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("servicenow");
    assert_eq!(
        now.id, existing,
        "ISIN match must reuse the existing instrument"
    );
}

#[tokio::test]
async fn avanza_rollback_via_shared_route() {
    let state = test_state().await;
    let (_, committed) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = committed["batch_id"].as_i64().expect("batch id");

    let (status, _) = send_json(&state, "POST", &format!("/api/import/rollback/{batch_id}")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn avanza_backfills_isin_on_symbol_only_existing_instrument() {
    let state = test_state().await;
    let existing = sqlx::query_scalar::<_, i64>(
        "INSERT INTO instruments (symbol, exchange, name, type, currency, isin) \
         VALUES ('US81762P1021','AVANZA','ServiceNow','STOCK','USD',NULL) RETURNING id",
    )
    .fetch_one(&state.pool)
    .await
    .expect("seed symbol-only AVANZA row");

    let (status, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);

    let matched = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("isin should now match");
    assert_eq!(
        matched.id, existing,
        "must reuse the symbol-only row, not duplicate it"
    );
    assert_eq!(matched.isin.as_deref(), Some("US81762P1021"));

    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM instruments WHERE exchange = 'AVANZA' AND symbol = 'US81762P1021'",
    )
    .fetch_one(&state.pool)
    .await
    .expect("count");
    assert_eq!(count, 1, "no duplicate AVANZA instrument was created");
}

#[tokio::test]
async fn avanza_split_without_position_is_a_hard_error_not_a_new_instrument() {
    let state = test_state().await;
    let csv = concat!(
        "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n",
        "2026-06-02;ISK;Split värdepapper;Orphan;5;;;;;;;XS9999999999;\n",
    );

    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", csv.as_bytes()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|error| error["code"] == "split_without_position"));

    let (commit_status, _) = send_bytes(&state, "/api/import/avanza/commit", csv.as_bytes()).await;
    assert_eq!(commit_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(db::instruments::find_by_isin(&state.pool, "XS9999999999")
        .await
        .expect("query")
        .is_none());
}

#[tokio::test]
async fn commit_excludes_a_bad_asset_and_writes_the_rest() {
    let state = test_state().await;

    let (blocked, _) = send_bytes(
        &state,
        "/api/import/sharesight/commit",
        TWO_ASSETS_ONE_BAD.as_bytes(),
    )
    .await;
    assert_eq!(blocked, StatusCode::UNPROCESSABLE_ENTITY);

    let (ok, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=xetr:asml",
        TWO_ASSETS_ONE_BAD.as_bytes(),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 1);
    assert_eq!(body["counts"]["buys"], 1);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 1);
}

#[tokio::test]
async fn unknown_exclude_key_warns_but_commits() {
    let state = test_state().await;
    let (ok, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=nope:none",
        SYNTHETIC,
    )
    .await;

    assert_eq!(ok, StatusCode::OK);
    let warnings = body["warnings"].as_array().expect("warnings array");
    assert!(warnings
        .iter()
        .any(|warning| warning["code"] == "unknown_exclude_key"));
}

#[tokio::test]
async fn commit_excludes_an_asset_with_a_mapper_stage_error() {
    let state = test_state().await;
    let csv = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Buy,12/06/2026,10,\"12,50\",USD,\"0,00\",\"0,00\",SEK,\"0,100000\",\"1250,00\",All Trades,\n",
        "XETR,ASML,ASML Holding,Buy,12/06/2026,3,\"600,00\",EUR,\"0,00\",\"5,00\",EUR,\"0,100000\",\"1805,00\",All Trades,\n",
    );

    let (blocked, blocked_body) =
        send_bytes(&state, "/api/import/sharesight/commit", csv.as_bytes()).await;
    assert_eq!(blocked, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(blocked_body["error"]["code"], "non_sek_brokerage");

    let (ok, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit?exclude=xetr:asml",
        csv.as_bytes(),
    )
    .await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(body["counts"]["rows"], 1);

    let (_, holdings) = send_json(&state, "GET", "/api/holdings").await;
    assert_eq!(holdings.as_array().expect("array").len(), 1);
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
async fn standalone_imported_split_is_rejected_without_creating_an_instrument() {
    let state = test_state().await;
    let split_only = concat!(
        "P - All Trades Report between 2025-06-12 and 2026-06-12\n",
        "Market,Code,Name,Type,Date,Quantity,Price,Instrument Currency,Cost base per share (SEK),Brokerage,Brokerage Currency,Exchange Rate,Value,,Comments\n",
        "NASDAQ,MSFT,Microsoft,Split,12/06/2026,10,\"0,00\",USD,\"0,00\",\"0,00\",SEK,,\"0,00\",All Trades,\n",
    );

    let (status, body) = send_bytes(
        &state,
        "/api/import/sharesight/commit",
        split_only.as_bytes(),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["code"], "split_without_position");

    let instruments = db::instruments::list(&state.pool).await.expect("list");
    assert!(instruments.is_empty());

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

// ============================================================
// Avanza refresh (replace) tests
// ============================================================

#[tokio::test]
async fn avanza_first_import_writes_dividend() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(status, StatusCode::OK);

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE type = 'DIVIDEND'")
            .fetch_one(&state.pool)
            .await
            .expect("count");
    assert_eq!(count, 1, "one dividend transaction should be written");
    // Eligible quantity = buy 5 - sell 2 = 3
    let qty: i64 =
        sqlx::query_scalar("SELECT quantity FROM transactions WHERE type = 'DIVIDEND' LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .expect("qty");
    assert_eq!(qty, 3, "dividend quantity should be eligible share count");

    assert_eq!(body["counts"]["dividends"], 1);
}

#[tokio::test]
async fn avanza_preview_has_no_replace_candidate_before_first_import() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/avanza/preview", AVANZA).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["replace_candidate_batch_id"].is_null());
    assert!(body["replace_candidate_warning"].is_null());
}

#[tokio::test]
async fn avanza_preview_returns_replace_candidate_after_first_import() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let first_batch_id = first["batch_id"].as_i64().expect("batch id");

    let (status, preview) = send_bytes(&state, "/api/import/avanza/preview", AVANZA_V2).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(preview["replace_candidate_batch_id"], first_batch_id);
    assert!(preview["replace_candidate_warning"].is_null());
}

#[tokio::test]
async fn avanza_preview_warns_when_multiple_batches_exist() {
    let state = test_state().await;
    // Create two AVANZA batches
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let first_id = first["batch_id"].as_i64().expect("first batch id");
    let (_, second) = send_bytes(
        &state,
        "/api/import/avanza/commit?allow_duplicate=true",
        AVANZA,
    )
    .await;
    let second_id = second["batch_id"].as_i64().expect("second batch id");
    assert!(second_id > first_id);

    let (status, preview) = send_bytes(&state, "/api/import/avanza/preview", AVANZA_V2).await;
    assert_eq!(status, StatusCode::OK);
    // Should return the newest (highest id) batch
    assert_eq!(preview["replace_candidate_batch_id"], second_id);
    // Should warn about multiple batches
    assert!(
        !preview["replace_candidate_warning"].is_null(),
        "should warn about multiple batches"
    );
}

#[tokio::test]
async fn sharesight_preview_has_null_replace_candidate() {
    let state = test_state().await;
    let (status, body) = send_bytes(&state, "/api/import/sharesight/preview", SYNTHETIC).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["replace_candidate_batch_id"].is_null());
    assert!(body["replace_candidate_warning"].is_null());
}

#[tokio::test]
async fn avanza_refresh_adds_new_row_without_doubling_history() {
    let state = test_state().await;

    // First import
    let (s1, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(s1, StatusCode::OK);
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("count before");

    // Refresh with V2 (same history + one new buy)
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s2, refreshed) = send_bytes(&state, &uri, AVANZA_V2).await;
    assert_eq!(s2, StatusCode::OK, "refresh should succeed: {refreshed}");
    assert_eq!(
        refreshed["batch_id"], batch_id,
        "batch id must stay the same"
    );

    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("count after");

    // V2 has 1 extra buy, so count should be exactly 1 more
    assert_eq!(
        count_after,
        count_before + 1,
        "refresh should add exactly the one new row, not double history"
    );
    // batch count must still be 1
    let batch_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM import_batches")
        .fetch_one(&state.pool)
        .await
        .expect("batch count");
    assert_eq!(batch_count, 1);
}

#[tokio::test]
async fn avanza_refresh_idempotent_with_same_file() {
    let state = test_state().await;
    let (s1, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(s1, StatusCode::OK);
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let count_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("count before");
    let max_id_before: i64 = sqlx::query_scalar("SELECT MAX(id) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("max id before");

    // Refresh with the identical file
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s2, refreshed) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s2, StatusCode::OK, "idempotent refresh should succeed");

    let count_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("count after");
    let max_id_after: i64 = sqlx::query_scalar("SELECT MAX(id) FROM transactions")
        .fetch_one(&state.pool)
        .await
        .expect("max id after");

    assert_eq!(
        count_after, count_before,
        "COUNT(*) must be unchanged for identical file"
    );
    assert_eq!(
        max_id_after, max_id_before,
        "MAX(id) must be unchanged for identical file"
    );
    assert_eq!(refreshed["batch_id"], batch_id);
}

#[tokio::test]
async fn avanza_refresh_preserves_dividend_transaction_id() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let div_id_before: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE type = 'DIVIDEND' LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .expect("dividend id before");

    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s, _) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s, StatusCode::OK);

    let div_id_after: i64 =
        sqlx::query_scalar("SELECT id FROM transactions WHERE type = 'DIVIDEND' LIMIT 1")
            .fetch_one(&state.pool)
            .await
            .expect("dividend id after");

    assert_eq!(
        div_id_after, div_id_before,
        "dividend transaction id must be preserved on identical refresh"
    );
    let div_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE type = 'DIVIDEND'")
            .fetch_one(&state.pool)
            .await
            .expect("count");
    assert_eq!(div_count, 1, "no duplicate dividend after refresh");
}

#[tokio::test]
async fn avanza_refresh_preserves_instruments_and_price_history() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let servicenow = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("servicenow instrument");

    // Seed a price row and provider symbol for ServiceNow
    sqlx::query(
        "INSERT INTO prices (instrument_id, provider, provider_symbol, date, close, currency, fetched_at) \
         VALUES (?, 'YAHOO', 'NOW', '2026-06-01', '900.00', 'USD', '2026-06-10T00:00:00Z')",
    )
    .bind(servicenow.id)
    .execute(&state.pool)
    .await
    .expect("seed price");

    sqlx::query(
        "INSERT INTO instrument_provider_symbols (instrument_id, provider, provider_symbol, created_at, updated_at) VALUES (?, 'YAHOO', 'NOW', '2026-06-10T00:00:00Z', '2026-06-10T00:00:00Z')",
    )
    .bind(servicenow.id)
    .execute(&state.pool)
    .await
    .expect("seed provider symbol");

    // Refresh
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s, _) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s, StatusCode::OK);

    // Instrument must still exist with the same id
    let now_after = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("servicenow after refresh");
    assert_eq!(now_after.id, servicenow.id, "instrument id must not change");

    let price_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM prices WHERE instrument_id = ? AND provider = 'YAHOO'",
    )
    .bind(servicenow.id)
    .fetch_one(&state.pool)
    .await
    .expect("price count");
    assert_eq!(price_count, 1, "price row must be preserved after refresh");

    let sym_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM instrument_provider_symbols WHERE instrument_id = ? AND provider = 'YAHOO'")
            .bind(servicenow.id)
            .fetch_one(&state.pool)
            .await
            .expect("sym count");
    assert_eq!(
        sym_count, 1,
        "provider symbol must be preserved after refresh"
    );
}

#[tokio::test]
async fn avanza_refresh_preserves_manual_transaction() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let apple = db::instruments::find_by_isin(&state.pool, "US0378331005")
        .await
        .expect("query")
        .expect("apple");

    // Add a manual buy that stays valid after refresh (more Apple shares)
    let (ms, _) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": apple.id,
            "type": "Buy",
            "trade_date": "2026-06-10",
            "quantity": 2,
            "price": "220",
            "currency": "USD"
        }),
    )
    .await;
    assert_eq!(ms, StatusCode::CREATED);

    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s, _) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(
        s,
        StatusCode::OK,
        "refresh should succeed with valid manual buy"
    );

    let manual_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE import_batch_id IS NULL")
            .fetch_one(&state.pool)
            .await
            .expect("manual count");
    assert_eq!(manual_count, 1, "manual transaction must survive refresh");
}

#[tokio::test]
async fn avanza_refresh_rejected_when_excluded_asset_invalidates_manual_sell() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let apple = db::instruments::find_by_isin(&state.pool, "US0378331005")
        .await
        .expect("query")
        .expect("apple");

    // After import: Apple position = buy 5 - sell 2 = 3
    // Add a manual sell of 3 that depends on the imported Apple rows
    let (ms, _) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": apple.id,
            "type": "Sell",
            "trade_date": "2026-06-15",
            "quantity": 3,
            "price": "220",
            "currency": "USD",
            "fx_rate_to_base": "11.0"
        }),
    )
    .await;
    assert_eq!(ms, StatusCode::CREATED);

    // Refresh while excluding Apple: Apple imported rows go away → manual sell becomes invalid
    let uri = format!(
        "/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}&exclude=US0378331005"
    );
    let (s, body) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "refresh should be rejected: {body}"
    );
    assert_eq!(
        body["error"]["code"], "refresh_would_invalidate_ledger",
        "should report invalidation: {body}"
    );

    // Old imported rows must still be present (rollback on error)
    let apple_tx_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE instrument_id = ?")
            .bind(apple.id)
            .fetch_one(&state.pool)
            .await
            .expect("count");
    assert!(
        apple_tx_count > 1,
        "old imported rows must be preserved on refresh failure"
    );
}

#[tokio::test]
async fn avanza_refresh_requires_replace_batch_id() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let _batch_id = first["batch_id"].as_i64().expect("batch id");

    let (s, body) = send_bytes(&state, "/api/import/avanza/commit?mode=replace", AVANZA).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "missing_replace_batch_id");
}

#[tokio::test]
async fn avanza_refresh_returns_not_found_for_unknown_batch() {
    let state = test_state().await;
    let (s, body) = send_bytes(
        &state,
        "/api/import/avanza/commit?mode=replace&replace_batch_id=999",
        AVANZA,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(body["error"]["code"], "replace_batch_not_found");
}

#[tokio::test]
async fn avanza_refresh_returns_conflict_when_newer_batch_appeared() {
    let state = test_state().await;
    // First import gives batch 1
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let first_id = first["batch_id"].as_i64().expect("first id");

    // A second append gives batch 2 (simulates race condition after preview)
    let (_, second) = send_bytes(
        &state,
        "/api/import/avanza/commit?allow_duplicate=true",
        AVANZA,
    )
    .await;
    let second_id = second["batch_id"].as_i64().expect("second id");
    assert!(second_id > first_id);

    // Try to refresh batch 1 — but batch 2 is newer → conflict
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={first_id}");
    let (s, body) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(body["error"]["code"], "replace_candidate_changed");
}

#[tokio::test]
async fn avanza_refresh_correctly_handles_excluded_asset() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let volvo = db::instruments::find_by_isin(&state.pool, "SE0000115446")
        .await
        .expect("query")
        .expect("volvo");

    let volvo_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE instrument_id = ?")
            .bind(volvo.id)
            .fetch_one(&state.pool)
            .await
            .expect("count before");
    assert_eq!(volvo_before, 1);

    // Refresh while excluding Volvo
    let uri = format!(
        "/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}&exclude=SE0000115446"
    );
    let (s, _) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s, StatusCode::OK, "refresh without Volvo should succeed");

    let volvo_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE instrument_id = ?")
            .bind(volvo.id)
            .fetch_one(&state.pool)
            .await
            .expect("count after");
    assert_eq!(
        volvo_after, 0,
        "Volvo rows should be removed from refreshed batch"
    );
}

#[tokio::test]
async fn avanza_refresh_same_day_order_preserves_manual_row_id() {
    let state = test_state().await;
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let servicenow = db::instruments::find_by_isin(&state.pool, "US81762P1021")
        .await
        .expect("query")
        .expect("servicenow");

    // Add a manual buy on the same date as the imported buy (2026-06-01)
    let (ms, manual_body) = send_json_body(
        &state,
        "POST",
        "/api/transactions",
        serde_json::json!({
            "instrument_id": servicenow.id,
            "type": "Buy",
            "trade_date": "2026-06-01",
            "quantity": 5,
            "price": "910",
            "currency": "USD",
            "fx_rate_to_base": "10.50"
        }),
    )
    .await;
    assert_eq!(ms, StatusCode::CREATED);
    let manual_id = manual_body["id"].as_i64().expect("manual id");

    // Refresh with same file
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s, _) = send_bytes(&state, &uri, AVANZA).await;
    assert_eq!(s, StatusCode::OK);

    // Manual row must still exist with same id
    let still_there: Option<i64> = sqlx::query_scalar("SELECT id FROM transactions WHERE id = ?")
        .bind(manual_id)
        .fetch_optional(&state.pool)
        .await
        .expect("lookup");
    assert_eq!(
        still_there,
        Some(manual_id),
        "manual same-day row must be preserved"
    );
}

#[tokio::test]
async fn avanza_refresh_inserts_extra_identical_canonical_row() {
    const HEADER: &str = "Datum;Konto;Typ av transaktion;Värdepapper/beskrivning;Antal;Kurs;Belopp;Transaktionsvaluta;Courtage;Valutakurs;Instrumentvaluta;ISIN;Resultat\n";
    const BUY_ROW: &str =
        "2026-05-10;ISK;Köp;Volvo B;3;250,00;-750,00;SEK;0,00;;SEK;SE0000115446;\n";

    let state = test_state().await;

    // Initial import: one identical canonical buy row.
    let v1 = format!("{HEADER}{BUY_ROW}");
    let (_, first) = send_bytes(&state, "/api/import/avanza/commit", v1.as_bytes()).await;
    let batch_id = first["batch_id"].as_i64().expect("batch id");

    let volvo = db::instruments::find_by_isin(&state.pool, "SE0000115446")
        .await
        .expect("query")
        .expect("volvo");

    let before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE instrument_id = ?")
            .bind(volvo.id)
            .fetch_one(&state.pool)
            .await
            .expect("count before");
    assert_eq!(before, 1);

    // Refresh with two identical canonical buy rows (multiplicity 1 -> 2).
    let v3 = format!("{HEADER}{BUY_ROW}{BUY_ROW}");
    let uri = format!("/api/import/avanza/commit?mode=replace&replace_batch_id={batch_id}");
    let (s, _) = send_bytes(&state, &uri, v3.as_bytes()).await;
    assert_eq!(s, StatusCode::OK, "refresh should succeed");

    let after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transactions WHERE instrument_id = ?")
            .bind(volvo.id)
            .fetch_one(&state.pool)
            .await
            .expect("count after");
    assert_eq!(
        after, 2,
        "surplus identical canonical row must be inserted, not dropped"
    );
}

#[tokio::test]
async fn avanza_dividend_persists_eligible_shares_and_native_price() {
    let state = test_state().await;
    let (s, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(s, StatusCode::OK);

    // Eligible quantity = buy 5 (2026-05-10) - sell 2 (2026-05-15) = 3
    let (qty, source_value, source_currency): (i64, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT quantity, source_value, source_currency FROM transactions WHERE type = 'DIVIDEND' LIMIT 1",
        )
        .fetch_one(&state.pool)
        .await
        .expect("dividend row");

    assert_eq!(qty, 3, "eligible share count stored as quantity");
    // Cash amount = 120 SEK
    assert!(
        source_value.as_deref().is_some(),
        "source_value should be set"
    );
    assert_eq!(source_currency.as_deref(), Some("SEK"));
}
