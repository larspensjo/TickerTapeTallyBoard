//! Portfolio waterfall reconciliation.
use super::super::waterfall::{IncomeInput, PortfolioWaterfallAccumulator};
use super::*;
use crate::api::valuation::{AvailabilityResponse, BASE_CURRENCY};
use crate::domain::{Availability, BaseAmount, RealizedGain, ValuedHolding};
use chrono::Duration;
use rust_decimal_macros::dec;
use serde_json::json;

#[tokio::test]
async fn gains_portfolio_waterfall_reconciles_across_open_and_closed_positions() {
    let state = AppState::for_tests().await;
    let open_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let closed_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":4,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Buy","trade_date":"2026-06-03",
               "quantity":5,"price":"20","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Sell","trade_date":"2026-06-04",
               "quantity":5,"price":"30","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;

    seed_market_data(
        &state,
        open_id,
        crate::clock::Clock::System.today(),
        crate::clock::Clock::System.today() - Duration::days(1),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1);
    assert_available(&body["portfolio_waterfall"]["cost_basis_base"], "6012.00");
    assert_available(
        &body["portfolio_waterfall"]["held_fee_component_base"],
        "12.00",
    );
    assert_available(&body["portfolio_waterfall"]["price_effect_base"], "1308.00");
    assert_available(&body["portfolio_waterfall"]["fx_effect_base"], "600.00");
    assert_available(&body["portfolio_waterfall"]["market_value_base"], "7920.00");
    assert_available(
        &body["portfolio_waterfall"]["realized_gain_base"],
        "1317.00",
    );
    assert_available(&body["portfolio_waterfall"]["realized_fee_base"], "13.00");
    assert_available(
        &body["portfolio_waterfall"]["realized_cost_basis_base"],
        "4108.00",
    );
    assert_available(
        &body["portfolio_waterfall"]["brokerage_total_base"],
        "25.00",
    );
    assert_available(&body["portfolio_waterfall"]["income_base"], "0.00");
    assert_available(
        &body["portfolio_waterfall"]["unrealized_gain_base"],
        "1908.00",
    );
    assert_available(&body["portfolio_waterfall"]["total_return_base"], "3225.00");
    assert_eq!(body["portfolio_waterfall"]["income_not_tracked"], false);
    assert_eq!(body["portfolio_waterfall"]["excluded_rows"], 0);
}

#[tokio::test]
async fn gains_portfolio_waterfall_excludes_rows_with_missing_price_inputs() {
    let state = AppState::for_tests().await;
    let open_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let closed_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;
    let incomplete_id = instrument(&state, "AAPL", "NASDAQ", "USD").await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":4,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Buy","trade_date":"2026-06-03",
               "quantity":5,"price":"20","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Sell","trade_date":"2026-06-04",
               "quantity":5,"price":"30","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":incomplete_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;

    seed_market_data(
        &state,
        open_id,
        crate::clock::Clock::System.today(),
        crate::clock::Clock::System.today() - Duration::days(1),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 2);
    assert_available(&body["portfolio_waterfall"]["total_return_base"], "3225.00");
    assert_eq!(body["portfolio_waterfall"]["excluded_rows"], 1);
}

#[tokio::test]
async fn gains_portfolio_waterfall_is_independent_of_include_closed() {
    let state = AppState::for_tests().await;
    let open_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let closed_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":open_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":4,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Buy","trade_date":"2026-06-03",
               "quantity":5,"price":"20","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":closed_id,"type":"Sell","trade_date":"2026-06-04",
               "quantity":5,"price":"30","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;

    seed_market_data(
        &state,
        open_id,
        crate::clock::Clock::System.today(),
        crate::clock::Clock::System.today() - Duration::days(1),
    )
    .await;

    let (_, default_body) = send(&state, "GET", "/api/gains", json!({})).await;
    let (_, closed_body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;

    assert_eq!(
        default_body["portfolio_waterfall"],
        closed_body["portfolio_waterfall"]
    );
    assert_eq!(default_body["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(closed_body["rows"].as_array().expect("rows").len(), 2);
}

#[test]
fn gains_portfolio_waterfall_income_not_tracked_keeps_zero_step() {
    let mut accum = PortfolioWaterfallAccumulator::default();
    accum.add_open(
        &portfolio_holding("100.00", "10.00", "0.00", "110.00", "10.00"),
        &portfolio_realized("5.00", "20.00", "1.00"),
        dec!(2),
        IncomeInput::NotTracked,
    );

    let response = accum.into_response();
    match response.income_base {
        AvailabilityResponse::Available { value } => assert_eq!(value, "0.00"),
        other => panic!("unexpected income base: {other:?}"),
    }
    assert!(response.income_not_tracked);
    assert_eq!(response.excluded_rows, 0);
}

fn portfolio_holding(
    cost_basis: &str,
    price_effect: &str,
    fx_effect: &str,
    market_value: &str,
    unrealized_gain: &str,
) -> ValuedHolding {
    ValuedHolding {
        quantity: 10,
        cost_basis_native: dec!(0),
        cost_basis_base: Availability::Available(cost_basis.parse().expect("decimal")),
        fee_component_base: Availability::Available(dec!(0)),
        price_effect_base: Availability::Available(price_effect.parse().expect("decimal")),
        fx_effect_base: Availability::Available(fx_effect.parse().expect("decimal")),
        latest_price: Availability::Unavailable { reasons: vec![] },
        previous_price: Availability::Unavailable { reasons: vec![] },
        latest_fx: Availability::Unavailable { reasons: vec![] },
        previous_fx: Availability::Unavailable { reasons: vec![] },
        market_value_native: Availability::Available(dec!(0)),
        market_value_base: Availability::Available(market_value.parse().expect("decimal")),
        unrealized_gain_base: Availability::Available(unrealized_gain.parse().expect("decimal")),
        unrealized_gain_percent: Availability::Available(dec!(0)),
        day_change_base: Availability::Available(dec!(0)),
        day_change_percent: Availability::Available(dec!(0)),
        reasons: vec![],
    }
}

fn portfolio_realized(gain: &str, cost_basis: &str, fee: &str) -> RealizedGain {
    RealizedGain {
        sold_quantity: 10,
        proceeds_native: dec!(0),
        cost_basis_native: dec!(0),
        proceeds_base: BaseAmount::Available(dec!(0)),
        cost_basis_base: BaseAmount::Available(cost_basis.parse().expect("decimal")),
        price_effect_base: BaseAmount::Available(dec!(0)),
        fx_effect_base: BaseAmount::Available(dec!(0)),
        gain_base: BaseAmount::Available(gain.parse().expect("decimal")),
        fee_base: BaseAmount::Available(fee.parse().expect("decimal")),
        sell_brokerage_base: dec!(0),
    }
}
