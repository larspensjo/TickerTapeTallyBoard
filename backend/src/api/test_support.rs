use std::collections::HashMap;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::{Local, NaiveDate};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::str::FromStr;
use tower::ServiceExt;

use crate::api::router;
use crate::db::{fx_rates, prices, provider_symbols};
use crate::state::AppState;

pub(crate) async fn send(
    state: &AppState,
    method: &str,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
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
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json body")
    };
    (status, value)
}

pub(crate) async fn instrument(
    state: &AppState,
    symbol: &str,
    exchange: &str,
    currency: &str,
) -> i64 {
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

pub(crate) struct SeededHoldingSpec<'a> {
    pub(crate) symbol: &'a str,
    pub(crate) exchange: &'a str,
    pub(crate) currency: &'a str,
    pub(crate) quantity: i64,
    pub(crate) price: &'a str,
    pub(crate) conviction: &'a str,
    pub(crate) price_date: NaiveDate,
    pub(crate) fx_date: Option<NaiveDate>,
    pub(crate) fx_rate: Option<&'a str>,
}

pub(crate) async fn seed_valued(
    state: &AppState,
    symbol: &str,
    quantity: i64,
    price: &str,
    conviction: &str,
) -> i64 {
    seed_valued_at(
        state,
        SeededHoldingSpec {
            symbol,
            exchange: "STO",
            currency: "SEK",
            quantity,
            price,
            conviction,
            price_date: Local::now().naive_local().date(),
            fx_date: None,
            fx_rate: None,
        },
    )
    .await
}

pub(crate) async fn seed_valued_at(state: &AppState, spec: SeededHoldingSpec<'_>) -> i64 {
    let id = instrument(state, spec.symbol, spec.exchange, spec.currency).await;
    let now = crate::import::now_iso8601();
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: id,
            provider: crate::api::valuation::PRICE_PROVIDER.to_owned(),
            provider_symbol: spec.symbol.to_owned(),
            currency: Some(spec.currency.to_owned()),
            enabled: true,
            created_at: now.clone(),
            updated_at: now.clone(),
        },
    )
    .await
    .expect("provider symbol");
    prices::upsert(
        &state.pool,
        &prices::NewPrice {
            instrument_id: id,
            provider: crate::api::valuation::PRICE_PROVIDER.to_owned(),
            provider_symbol: spec.symbol.to_owned(),
            date: spec.price_date,
            close: Decimal::from_str(spec.price).expect("price"),
            currency: spec.currency.to_owned(),
            fetched_at: now.clone(),
        },
    )
    .await
    .expect("price");

    if !spec.currency.eq_ignore_ascii_case("SEK") {
        let fx_date = spec.fx_date.expect("fx date");
        let fx_rate = spec.fx_rate.expect("fx rate");
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: spec.currency.to_owned(),
                quote: "SEK".to_owned(),
                date: fx_date,
                rate: Decimal::from_str(fx_rate).expect("fx rate"),
                provider: crate::api::valuation::FX_PROVIDER.to_owned(),
                fetched_at: now.clone(),
            },
        )
        .await
        .expect("fx rate");
    }

    send(
        state,
        "POST",
        "/api/transactions",
        json!({
            "instrument_id": id,
            "type": "Buy",
            "trade_date": "2026-06-01",
            "quantity": spec.quantity,
            "price": spec.price,
            "currency": spec.currency,
            "fx_rate_to_base": spec.fx_rate.unwrap_or("1"),
        }),
    )
    .await;
    set_conviction(state, id, spec.conviction).await;
    id
}

pub(crate) async fn set_conviction(state: &AppState, id: i64, conviction: &str) {
    let (status, _) = send(
        state,
        "PUT",
        &format!("/api/instruments/{id}/conviction"),
        json!({ "conviction": conviction }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

pub(crate) async fn holdings_by_symbol(state: &AppState) -> HashMap<String, Value> {
    let (status, holdings) = send(state, "GET", "/api/holdings", Value::Null).await;
    assert_eq!(status, StatusCode::OK);
    holdings
        .as_array()
        .expect("array")
        .iter()
        .map(|holding| {
            (
                holding["instrument"]["symbol"]
                    .as_str()
                    .expect("symbol")
                    .to_owned(),
                holding.clone(),
            )
        })
        .collect()
}
