//! Gain row and attribution behavior.
use super::*;
use chrono::Duration;
use serde_json::json;

#[tokio::test]
async fn gains_open_row_percent_is_current_position_not_period_hybrid() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let latest = crate::clock::Clock::System.today();
    let previous = latest - Duration::days(1);

    // Opening buy well before the period, then an in-period partial sell, open remainder.
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-03-01",
               "quantity":4,"price":"150","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    seed_market_data(&state, instrument_id, latest, previous).await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let row = &body["rows"][0];
    assert_eq!(row["quantity"], 6);
    // total_return includes unrealized + realized + income (no income here).
    // unrealized(6 shares @120 fx11 vs @100 fx10) = 7920-6000 = 1920
    // realized(4 shares @150 fx10 vs @100 fx10) = 6000-4000 = 2000
    // total_cost = 6000+4000 = 10000; percent = 3920/10000 = 39.20
    assert_available(&row["total_return_base"], "3920.00");
    assert_available(&row["total_return_percent"], "39.20");
    // unrealized percent uses remaining-shares cost only (1920/6000 = 32.00).
    assert_available(&row["unrealized_gain_percent"], "32.00");
}

#[tokio::test]
async fn gains_can_include_closed_positions_with_realized_gain() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":10,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;

    let (default_status, default_body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(default_status, StatusCode::OK);
    assert_eq!(default_body["rows"].as_array().expect("rows").len(), 0);

    let (status, body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["include_closed_positions"], true);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1);
    assert_eq!(
        body["summary"]["market_value_base"]["status"],
        "unavailable"
    );
    // No report-end FX is cached for this USD closed position.
    assert_unavailable(&body["totals"]["capital_gain_base"], &["missing_end_fx"]);
    assert_unavailable(&body["totals"]["currency_gain_base"], &["missing_end_fx"]);
    assert_unavailable(&body["totals"]["total_return_base"], &["missing_end_fx"]);
    assert_unavailable_status(&body["totals"]["total_return_percent"]);
    assert_unavailable(&body["totals"]["income_base"], &["missing_end_fx"]);

    let row = &body["rows"][0];
    assert_eq!(row["instrument"]["symbol"], "MSFT");
    assert_eq!(row["position_status"], "closed");
    assert_eq!(row["quantity"], 0);
    assert_eq!(row["cost_basis_native"], "1000.00");
    assert_available(&row["cost_basis_base"], "10020.00");
    assert_available(&row["market_value_native"], "0.00");
    assert_available(&row["market_value_base"], "0.00");
    assert_available(&row["proceeds_native"], "1200.00");
    assert_available(&row["proceeds_base"], "13195.00");
    assert_available(&row["unrealized_price_effect_base"], "2175.00");
    assert_available(&row["unrealized_fx_effect_base"], "1000.00");
    assert_available(&row["unrealized_gain_base"], "3175.00");
    assert_available(&row["unrealized_gain_percent"], "31.68");
    assert_available(&row["price_effect_base"], "2175.00");
    assert_available(&row["fx_effect_base"], "1000.00");
    assert_available(&row["capital_gain_base"], "2175.00");
    assert_available_status(&row["capital_gain_percent"]);
    assert_available(&row["currency_gain_base"], "1000.00");
    assert_available_status(&row["currency_gain_percent"]);
    assert_available(&row["total_return_base"], "3175.00");
    assert_available(&row["total_return_percent"], "31.68");
    assert_available(&row["held_fee_component_base"], "0.00");
    assert_available(&row["realized_fee_base"], "25.00");
    assert_available(&row["realized_sell_brokerage_base"], "5.00");
    assert_available(&row["brokerage_total_base"], "25.00");
}

#[tokio::test]
async fn gains_serializes_brokerage_fields_even_when_cost_basis_is_unavailable() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":null,"brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":10,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1);

    let row = &body["rows"][0];
    assert_eq!(row["position_status"], "closed");
    assert_unavailable(&row["cost_basis_base"], &["missing_fx"]);
    assert_available(&row["proceeds_base"], "13195.00");
    assert_unavailable(&row["realized_fee_base"], &["missing_fx"]);
    assert_available(&row["held_fee_component_base"], "0.00");
    assert_available(&row["realized_sell_brokerage_base"], "5.00");
    assert_available(&row["brokerage_total_base"], "25.00");
}

#[tokio::test]
async fn gains_totals_include_closed_in_period_position_when_row_hidden() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "ERIC B", "STO", BASE_CURRENCY).await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-05",
               "quantity":100,"price":"10","currency":BASE_CURRENCY}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-20",
               "quantity":100,"price":"11","currency":BASE_CURRENCY}),
    )
    .await;

    let (status, body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
        json!({}),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 0);
    assert_available(&body["totals"]["capital_gain_base"], "100.00");
    assert_available(&body["totals"]["currency_gain_base"], "0.00");
    assert_available(&body["totals"]["total_return_base"], "100.00");
    assert_available_status(&body["totals"]["total_return_percent"]);
    assert_eq!(body["totals"]["excluded_rows"], 0);
}

#[tokio::test]
async fn gains_include_closed_positions_keeps_partial_sells_in_one_open_row() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let latest = crate::clock::Clock::System.today();
    let previous = latest - Duration::days(1);

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10","brokerage":"20"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
               "quantity":4,"price":"120","currency":"USD","fx_rate_to_base":"11","brokerage":"5"}),
    )
    .await;

    seed_market_data(&state, instrument_id, latest, previous).await;

    let (status, body) = send(&state, "GET", "/api/gains?include_closed=true", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["include_closed_positions"], true);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1);
    assert_available(&body["totals"]["capital_gain_base"], "2175.00");
    assert_available(&body["totals"]["currency_gain_base"], "1000.00");
    assert_available(&body["totals"]["total_return_base"], "3175.00");
    // Percent values vary based on today's date (days since historical buy); just check available.
    assert_available_status(&body["totals"]["capital_gain_percent"]);
    assert_available_status(&body["totals"]["currency_gain_percent"]);
    assert_available_status(&body["totals"]["total_return_percent"]);

    let rows = body["rows"].as_array().expect("rows");
    let open_row = &rows[0];
    assert_eq!(open_row["position_status"], "open");
    assert_eq!(open_row["quantity"], 6);
    assert_available_status(&open_row["performance_denominator_base"]);
    assert_available(&open_row["unrealized_gain_base"], "1908.00");
    // Breakdown columns include realized gains (unrealized price 1308 + realized price 867 = 2175).
    assert_available(&open_row["capital_gain_base"], "2175.00");
    assert_available(&open_row["currency_gain_base"], "1000.00");
    assert_available(&open_row["unrealized_price_effect_base"], "1308.00");
    assert_available(&open_row["unrealized_fx_effect_base"], "600.00");
    assert_available(&open_row["total_return_base"], "3175.00");
    assert_available(&open_row["held_fee_component_base"], "12.00");
    assert_available(&open_row["realized_fee_base"], "13.00");
    assert_available(&open_row["realized_sell_brokerage_base"], "5.00");
    assert_available(&open_row["brokerage_total_base"], "25.00");
}

#[tokio::test]
async fn gains_populated_portfolio_uses_cached_price_and_frankfurter_fx() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let latest = crate::clock::Clock::System.today();
    let previous = latest - Duration::days(1);
    let trade_date = (latest - Duration::days(10)).format("%Y-%m-%d").to_string();

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":trade_date,
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;

    seed_market_data(&state, instrument_id, latest, previous).await;

    let (status, body) = send(&state, "GET", "/api/gains?method=modified_dietz", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["base_currency"], BASE_CURRENCY);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 1);

    let row = &body["rows"][0];
    assert_eq!(row["instrument"]["symbol"], "MSFT");
    assert_eq!(row["quantity"], 10);
    assert_eq!(row["performance_start_date"], trade_date);
    assert_available(&row["performance_denominator_base"], "10000.00");
    assert_eq!(row["cost_basis_native"], "1000.00");
    assert_available(&row["cost_basis_base"], "10000.00");
    assert_available(&row["price_effect_base"], "2200.00");
    assert_available(&row["fx_effect_base"], "1000.00");
    assert_eq!(row["latest_price"]["close"], "120.00");
    assert_eq!(row["latest_price"]["source"], "YAHOO");
    assert_eq!(row["latest_fx"]["rate"], "11");
    assert_eq!(row["latest_fx"]["quote"], BASE_CURRENCY);
    assert_available(&row["market_value_native"], "1200.00");
    assert_available(&row["market_value_base"], "13200.00");
    assert_unavailable(&row["proceeds_native"], &[]);
    assert_unavailable(&row["proceeds_base"], &[]);
    assert_available(&row["unrealized_gain_base"], "3200.00");
    assert_available(&row["unrealized_gain_percent"], "32.00");
    assert_available(&row["capital_gain_base"], "2200.00");
    assert_available(&row["capital_gain_percent"], "22.00");
    assert_available(&row["currency_gain_base"], "1000.00");
    assert_available(&row["currency_gain_percent"], "10.00");
    assert_available(&row["total_return_base"], "3200.00");
    assert_available(&row["total_return_percent"], "32.00");
    assert_available(&row["day_change_base"], "1650.00");
    assert_available(&row["day_change_percent"], "14.28");
    assert_available(&row["held_fee_component_base"], "0.00");
    assert_available(&row["realized_fee_base"], "0.00");
    assert_available(&row["realized_sell_brokerage_base"], "0.00");
    assert_available(&row["brokerage_total_base"], "0.00");

    assert_available(&body["summary"]["market_value_base"], "13200.00");
    assert_available(&body["summary"]["cost_basis_base"], "10000.00");
    assert_available(&body["summary"]["price_effect_base"], "2200.00");
    assert_available(&body["summary"]["fx_effect_base"], "1000.00");
    assert_available(&body["summary"]["unrealized_gain_base"], "3200.00");
    // Modified Dietz inception mode: buy 10d ago, weight=10/10=1, denom=10000
    // capital=2200, currency=1000, total=3200 → same as cost-basis results
    assert_available(&body["totals"]["capital_gain_base"], "2200.00");
    assert_available(&body["totals"]["capital_gain_percent"], "22.00");
    assert_available(&body["totals"]["currency_gain_base"], "1000.00");
    assert_available(&body["totals"]["currency_gain_percent"], "10.00");
    assert_available(&body["totals"]["total_return_base"], "3200.00");
    assert_available(&body["totals"]["total_return_percent"], "32.00");
}

#[tokio::test]
async fn gains_unavailable_attribution_serializes_reason_arrays() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let trade_date = (crate::clock::Clock::System.today() - Duration::days(10))
        .format("%Y-%m-%d")
        .to_string();

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":trade_date,
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);

    let row = &body["rows"][0];
    assert_unavailable(&row["price_effect_base"], &["missing_price", "missing_fx"]);
    assert_unavailable(&row["fx_effect_base"], &["missing_price", "missing_fx"]);
}

#[tokio::test]
async fn gains_totals_remain_available_when_one_instrument_is_incomplete() {
    let state = AppState::for_tests().await;
    let available_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let incomplete_id = instrument(&state, "AAPL", "NASDAQ", "USD").await;
    let latest = crate::clock::Clock::System.today();
    let previous = latest - Duration::days(1);
    let trade_date = (latest - Duration::days(10)).format("%Y-%m-%d").to_string();

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":available_id,"type":"Buy","trade_date":trade_date,
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":incomplete_id,"type":"Buy","trade_date":trade_date,
               "quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;

    seed_market_data(&state, available_id, latest, previous).await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rows"].as_array().expect("rows").len(), 2);
    assert_available(&body["totals"]["capital_gain_base"], "2200.00");
    assert_available(&body["totals"]["currency_gain_base"], "1000.00");
    assert_available(&body["totals"]["total_return_base"], "3200.00");
    assert_available_status(&body["totals"]["total_return_percent"]);
    assert_eq!(body["totals"]["excluded_rows"], 1);
}

#[tokio::test]
async fn dividend_income_appears_in_gain_row_and_totals() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "ERICB", "STO", BASE_CURRENCY).await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10.00","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
               "quantity":100,"dividend_per_share":"0.50","currency":BASE_CURRENCY}),
    )
    .await;
    seed_sek_prices(&state, instrument_id, "ERICB").await;

    // income = 100 * 0.50 = 50 SEK; unrealized = 100*12 - 100*10 = 200 SEK
    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let row = &body["rows"][0];
    assert_available(&row["income_base"], "50.00");
    assert_available(&body["totals"]["income_base"], "50.00");
    // total_return_base must include income so row column sums match the header total.
    assert_available(&row["total_return_base"], "250.00");
}

#[tokio::test]
async fn gains_open_row_exposes_realized_gain_base() {
    let state = AppState::for_tests().await;
    let sold_id = instrument(&state, "SELLER", "STO", BASE_CURRENCY).await;
    let never_id = instrument(&state, "HOLDER", "STO", BASE_CURRENCY).await;

    // SELLER: buy 10 @100, sell 4 @150 (SEK, no fees) -> realized (150-100)*4 = 200, 6 open.
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":sold_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":10,"price":"100","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":sold_id,"type":"Sell","trade_date":"2026-06-05",
               "quantity":4,"price":"150","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;
    // HOLDER: buy only, never sold -> realized 0.
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":never_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":5,"price":"100","currency":BASE_CURRENCY,"fx_rate_to_base":"1"}),
    )
    .await;

    let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
    assert_eq!(status, StatusCode::OK);

    let rows = body["rows"].as_array().expect("rows");
    let sold = rows
        .iter()
        .find(|r| r["instrument"]["symbol"] == "SELLER")
        .expect("seller row");
    let never = rows
        .iter()
        .find(|r| r["instrument"]["symbol"] == "HOLDER")
        .expect("holder row");

    assert_eq!(sold["position_status"], "open");
    assert_eq!(sold["quantity"], 6);
    assert_available(&sold["realized_gain_base"], "200.00");
    // Sold 4 @ cost 100 -> sold cost basis 400.00.
    assert_available(&sold["realized_cost_basis_base"], "400.00");
    assert_eq!(never["position_status"], "open");
    assert_available(&never["realized_gain_base"], "0.00");
    assert_available(&never["realized_cost_basis_base"], "0.00");
}
