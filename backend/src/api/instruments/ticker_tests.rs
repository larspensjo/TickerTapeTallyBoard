use crate::api::test_support::{instrument, seed_valued, send};
use crate::db::{fundamentals_sources, provider_symbols};
use crate::domain::fundamentals::{FundamentalsSource, LinkStatus};
use crate::providers::MarketDataProvider;
use crate::state::AppState;
use axum::http::StatusCode;
use serde_json::{json, Value};

async fn mapping(
    state: &AppState,
    id: i64,
    provider: MarketDataProvider,
    symbol: &str,
    enabled: bool,
) {
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id: id,
            provider,
            provider_symbol: symbol.to_owned(),
            asset_class: None,
            currency: Some("SEK".to_owned()),
            enabled,
            created_at: "2026-11-08T12:00:00Z".to_owned(),
            updated_at: "2026-11-08T12:00:00Z".to_owned(),
        },
    )
    .await
    .expect("mapping");
}

async fn figures_link(state: &AppState, id: i64, symbol: &str) {
    fundamentals_sources::correct(
        &state.pool,
        id,
        Some(fundamentals_sources::LinkDefinition {
            source: Some(FundamentalsSource::Yahoo),
            symbol: Some(symbol.to_owned()),
            status: LinkStatus::Usable,
            ..fundamentals_sources::LinkDefinition::pending()
        }),
        None,
        "2026-11-08T12:00:00Z",
    )
    .await
    .expect("figures link");
}

#[tokio::test]
async fn ticker_contract_uses_only_enabled_yahoo_then_current_figures_then_safe_symbol() {
    let state = AppState::for_tests().await;
    let avanza = instrument(&state, "SE0012673267", "AVANZA", "SEK").await;
    let unlinked_avanza = instrument(&state, "CORN", "AVANZA", "SEK").await;
    let nasdaq_only = instrument(&state, "SE0000115446", "AVANZA", "SEK").await;
    let disabled = instrument(&state, "SE0000115420", "XSTO", "SEK").await;
    let fallback = instrument(&state, "ASML", "XAMS", "SEK").await;
    mapping(&state, avanza, MarketDataProvider::Yahoo, "EVO.ST", true).await;
    figures_link(&state, avanza, "WRONG.AS").await;
    mapping(
        &state,
        nasdaq_only,
        MarketDataProvider::NasdaqNordic,
        "12345",
        true,
    )
    .await;
    mapping(
        &state,
        disabled,
        MarketDataProvider::Yahoo,
        "GUESS.ST",
        false,
    )
    .await;
    let (_, list) = send(&state, "GET", "/api/instruments", Value::Null).await;
    for (id, ticker) in [
        (avanza, json!("EVO")),
        (unlinked_avanza, Value::Null),
        (nasdaq_only, Value::Null),
        (disabled, Value::Null),
        (fallback, json!("ASML")),
    ] {
        let row = list
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == id)
            .unwrap();
        assert!(
            row.get("ticker").is_some(),
            "ticker must serialize even when null"
        );
        assert_eq!(row["ticker"], ticker);
    }
    figures_link(&state, disabled, "ASML.AS").await;
    let (_, list) = send(&state, "GET", "/api/instruments", Value::Null).await;
    assert_eq!(
        list.as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == disabled)
            .unwrap()["ticker"],
        "ASML"
    );
}

fn assert_instrument_tickers(value: &Value) -> usize {
    match value {
        Value::Object(object) => {
            if object.contains_key("conviction")
                && object.contains_key("symbol")
                && object.contains_key("id")
            {
                assert_eq!(value["ticker"], "EVO");
                1
            } else {
                object.values().map(assert_instrument_tickers).sum()
            }
        }
        Value::Array(values) => values.iter().map(assert_instrument_tickers).sum(),
        _ => 0,
    }
}

#[tokio::test]
async fn all_instrument_response_surfaces_carry_the_same_display_ticker() {
    let state = AppState::for_tests().await;
    let id = seed_valued(&state, "EVO.ST", 10, "100", "High").await;
    for uri in [
        "/api/instruments",
        "/api/holdings",
        "/api/gains",
        "/api/opportunities",
        "/api/rebalance?amount=100",
    ] {
        let (status, body) = send(&state, "GET", uri, Value::Null).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert!(assert_instrument_tickers(&body) > 0, "{uri}: {body}");
    }
    for (uri, payload) in [
        (
            format!("/api/instruments/{id}/conviction"),
            json!({"conviction":"High"}),
        ),
        (
            "/api/instruments/convictions".to_owned(),
            json!({"changes":[{"instrument_id":id,"conviction":"High"}]}),
        ),
    ] {
        let (status, body) = send(&state, "PUT", &uri, payload).await;
        assert_eq!(status, StatusCode::OK);
        assert!(assert_instrument_tickers(&body) > 0);
    }
    let (status, created) = send(&state, "POST", "/api/instruments", json!({"symbol":"MSFT","exchange":"NASDAQ","name":"Microsoft","type":"Stock","currency":"USD"})).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["ticker"], "MSFT");
}
