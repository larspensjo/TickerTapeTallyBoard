use crate::api::router;
use crate::api::valuation::BASE_CURRENCY;
use crate::db::{fx_rates, prices, provider_symbols};
use crate::import::now_iso8601;
use crate::providers::{BASE_FX_PROVIDER, PRICE_PROVIDER_PRECEDENCE};
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use rust_decimal_macros::dec;
use serde_json::json;
use tower::ServiceExt;

mod gain_rows;
mod percentage_method;
mod portfolio_waterfall;
mod report_period;

async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builds");
    let response = router(state.clone())
        .oneshot(request)
        .await
        .expect("request completes");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body readable");
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json body")
    };
    (status, value)
}

#[tokio::test]
async fn gains_empty_portfolio() {
    let state = AppState::for_tests().await;
    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["base_currency"], "SEK");
    assert_eq!(body["include_closed_positions"], false);
    assert_eq!(body["rows"].as_array().unwrap().len(), 0);
    assert_eq!(body["summary"]["excluded_rows"], 0);
    assert_eq!(body["totals"]["excluded_rows"], 0);
    assert_unavailable(&body["totals"]["capital_gain_base"], &[]);
    assert_unavailable(&body["totals"]["income_base"], &[]);
}

async fn instrument(state: &AppState, symbol: &str, exchange: &str, currency: &str) -> i64 {
    let (status, body) = send(
        state,
        "POST",
        "/api/instruments",
        json!({"symbol":symbol,"exchange":exchange,"name":symbol,"type":"Stock","currency":currency}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    body["id"].as_i64().expect("instrument id")
}

async fn seed_market_data(
    state: &AppState,
    instrument_id: i64,
    latest: chrono::NaiveDate,
    previous: chrono::NaiveDate,
) {
    let fetched_at = now_iso8601();
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: PRICE_PROVIDER_PRECEDENCE[0],
            provider_symbol: "MSFT".to_owned(),
            asset_class: None,
            currency: Some("USD".to_owned()),
            enabled: true,
            created_at: fetched_at.clone(),
            updated_at: fetched_at.clone(),
        },
    )
    .await
    .expect("provider symbol inserted");

    for (date, close) in [(previous, dec!(110)), (latest, dec!(120))] {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER_PRECEDENCE[0],
                provider_symbol: "MSFT".to_owned(),
                date,
                close,
                currency: "USD".to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .expect("price inserted");
    }

    for (date, rate) in [(previous, dec!(10.5)), (latest, dec!(11))] {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate,
                provider: BASE_FX_PROVIDER,
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .expect("fx rate inserted");
    }
}

async fn seed_sek_prices(state: &AppState, instrument_id: i64, symbol: &str) {
    let fetched_at = now_iso8601();
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: PRICE_PROVIDER_PRECEDENCE[0],
            provider_symbol: symbol.to_owned(),
            asset_class: None,
            currency: Some(BASE_CURRENCY.to_owned()),
            enabled: true,
            created_at: fetched_at.clone(),
            updated_at: fetched_at.clone(),
        },
    )
    .await
    .expect("provider symbol inserted");

    for date in [
        chrono::NaiveDate::from_ymd_opt(2026, 6, 29).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
    ] {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER_PRECEDENCE[0],
                provider_symbol: symbol.to_owned(),
                date,
                close: dec!(12),
                currency: BASE_CURRENCY.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .expect("price inserted");
    }
}

fn assert_available(value: &serde_json::Value, expected: &str) {
    assert_eq!(value["status"], "available");
    assert_eq!(value["value"], expected);
}

fn assert_unavailable(value: &serde_json::Value, expected: &[&str]) {
    assert_eq!(value["status"], "unavailable");
    let reasons = value["reasons"]
        .as_array()
        .expect("unavailable reasons array")
        .iter()
        .map(|reason| reason.as_str().expect("reason string"))
        .collect::<Vec<_>>();
    assert_eq!(reasons, expected);
}

fn assert_unavailable_status(value: &serde_json::Value) {
    assert_eq!(value["status"], "unavailable");
}

fn assert_available_status(value: &serde_json::Value) {
    assert_eq!(value["status"], "available");
}

#[tokio::test]
async fn gains_response_names_the_data_revision_it_was_computed_from() {
    let state = AppState::for_tests().await;
    let before = state.revision.current().await.expect("revision reads");

    instrument(&state, "STAMP", "STO", BASE_CURRENCY).await;

    assert_ne!(
        state.revision.current().await.expect("revision reads"),
        before
    );

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["data_revision"],
        state.revision.current().await.expect("revision reads")
    );
}
