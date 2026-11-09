//! Avanza refresh integration tests.
use super::*;

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
async fn avanza_dividend_persists_eligible_shares_and_dividend_per_share() {
    let state = test_state().await;
    let (s, _) = send_bytes(&state, "/api/import/avanza/commit", AVANZA).await;
    assert_eq!(s, StatusCode::OK);

    // Eligible quantity = buy 5 (2026-05-10) - sell 2 (2026-05-15) = 3
    let (qty, price, dividend_per_share, source_value, source_currency): (
        i64,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) =
        sqlx::query_as(
            "SELECT quantity, price, dividend_per_share, source_value, source_currency FROM transactions WHERE type = 'DIVIDEND' LIMIT 1",
        )
        .fetch_one(&state.pool)
        .await
        .expect("dividend row");

    assert_eq!(qty, 3, "eligible share count stored as quantity");
    assert_eq!(price, None, "dividend must not store market price");
    assert!(
        dividend_per_share.as_deref().is_some(),
        "dividend_per_share should be set"
    );
    // Cash amount = 120 SEK
    assert!(
        source_value.as_deref().is_some(),
        "source_value should be set"
    );
    assert_eq!(source_currency.as_deref(), Some("SEK"));
}
