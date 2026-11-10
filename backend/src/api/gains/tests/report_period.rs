//! Report period resolution and date validation.
use super::*;
use serde_json::json;

#[tokio::test]
async fn gains_as_of_date_comes_from_the_state_clock() {
    let state = AppState::for_tests()
        .await
        .with_clock(crate::clock::Clock::fixed(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
        ));

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["as_of_date"], "2026-03-05");
    assert_eq!(body["report_period"]["end_date"], "2026-03-05");
}

#[tokio::test]
async fn a_period_preset_is_resolved_with_the_state_clock() {
    let state = AppState::for_tests()
        .await
        .with_clock(crate::clock::Clock::fixed(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
        ));

    let (status, body) = send(&state, "GET", "/api/gains?period=ytd", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["report_period"]["start_date"], "2026-01-01");
    assert_eq!(body["report_period"]["end_date"], "2026-03-05");
    assert_eq!(body["as_of_date"], "2026-03-05");
}

#[tokio::test]
async fn thirty_day_preset_resolves_calendar_days_with_the_state_clock() {
    let state = AppState::for_tests()
        .await
        .with_clock(crate::clock::Clock::fixed(
            chrono::NaiveDate::from_ymd_opt(2026, 3, 5).expect("valid date"),
        ));

    let (status, body) = send(&state, "GET", "/api/gains?period=30d", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["report_period"]["start_date"], "2026-02-03");
    assert_eq!(body["report_period"]["end_date"], "2026-03-05");
    assert_eq!(body["as_of_date"], "2026-03-05");
}

#[tokio::test]
async fn an_unknown_period_is_rejected() {
    let state = AppState::for_tests().await;

    let (status, body) = send(&state, "GET", "/api/gains?period=last-tuesday", json!({})).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_period");
}

#[tokio::test]
async fn gains_rejects_malformed_start_date() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains?start_date=not-a-date", json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "invalid_date");
}

#[tokio::test]
async fn gains_rejects_start_after_end() {
    let state = AppState::for_tests().await;
    let (status, body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-30&end_date=2026-06-01",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"]["code"], "start_date_after_end_date");
}

#[tokio::test]
async fn gains_with_end_date_uses_that_date_as_valuation_date() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-01-15", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["as_of_date"], "2026-01-15");
    assert_eq!(body["report_period"]["end_date"], "2026-01-15");
}

#[tokio::test]
async fn gains_with_no_dates_returns_inception_period() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-15",
               "quantity":10,"price":"100","currency":BASE_CURRENCY}),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["report_period"]["start_date"], "2026-01-15");
}

#[tokio::test]
async fn gains_post_end_date_transaction_excluded() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "TSLA", "NASDAQ", "USD").await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-09-01",
               "quantity":100,"price":"15","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    // Row for TSLA should show quantity 100, not 200
    let row = body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["instrument"]["symbol"] == "TSLA")
        .unwrap();
    assert_eq!(row["quantity"], 100);
}
