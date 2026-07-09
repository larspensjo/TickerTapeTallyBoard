use chrono::{Duration, Local};

use crate::api::router;
use crate::api::valuation::{BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
use crate::db::{fx_rates, prices, provider_symbols};
use crate::import::now_iso8601;
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use rust_decimal_macros::dec;
use serde_json::json;
use tower::ServiceExt;

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

#[tokio::test]
async fn gains_open_row_percent_is_current_position_not_period_hybrid() {
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
    let latest = Local::now().naive_local().date();
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
    let latest = Local::now().naive_local().date();
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
    let latest = Local::now().naive_local().date();
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
    let trade_date = (Local::now().naive_local().date() - Duration::days(10))
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
    let latest = Local::now().naive_local().date();
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
async fn gains_all_mode_uses_one_report_start_for_row_denominators() {
    let state = AppState::for_tests().await;
    let early_id = instrument(&state, "EARLY", "STO", BASE_CURRENCY).await;
    let later_id = instrument(&state, "LATER", "STO", BASE_CURRENCY).await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":early_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10","currency":BASE_CURRENCY}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":later_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":100,"price":"10","currency":BASE_CURRENCY}),
    )
    .await;

    seed_sek_prices(&state, early_id, "EARLY").await;
    seed_sek_prices(&state, later_id, "LATER").await;

    let (status, body) = send(&state, "GET", "/api/gains?end_date=2026-06-30", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["report_period"]["start_date"], "2026-01-01");

    let rows = body["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    let early_row = rows
        .iter()
        .find(|row| row["instrument"]["symbol"] == "EARLY")
        .expect("early row");
    let later_row = rows
        .iter()
        .find(|row| row["instrument"]["symbol"] == "LATER")
        .expect("later row");

    // Both rows share the same report_start_date regardless of when each was first bought.
    assert_eq!(early_row["performance_start_date"], "2026-01-01");
    assert_eq!(later_row["performance_start_date"], "2026-01-01");
    // performance_denominator_base = cost_basis_base; SEK buys with no fx_rate_to_base are unavailable.
    assert_unavailable_status(&early_row["performance_denominator_base"]);
    assert_unavailable_status(&later_row["performance_denominator_base"]);
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

async fn seed_june_fixture(state: &AppState) -> i64 {
    let instrument_id = instrument(state, "MSFT", "NASDAQ", "USD").await;
    send(
        state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-01-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    let fetched_at = crate::import::now_iso8601();
    for (date, close) in [
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            dec!(10),
        ),
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            dec!(12),
        ),
    ] {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "MSFT".to_owned(),
                date,
                close,
                currency: "USD".to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    for date in [
        chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
    ] {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate: dec!(10),
                provider: FX_PROVIDER.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: PRICE_PROVIDER.to_owned(),
            provider_symbol: "MSFT".to_owned(),
            currency: Some("USD".to_owned()),
            enabled: true,
            created_at: fetched_at.clone(),
            updated_at: fetched_at,
        },
    )
    .await
    .unwrap();
    instrument_id
}

#[tokio::test]
async fn gains_with_date_range_selectable_method() {
    let state = AppState::for_tests().await;
    seed_june_fixture(&state).await;

    // default = xirr; buy on day 0 of 29-day period: cumulative = (1+annualized)^(29/365.25)-1 = 20.00%
    let (_, body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
        json!({}),
    )
    .await;
    assert_eq!(body["percentage_method"], "money_weighted");
    assert_available(&body["totals"]["total_return_percent"], "20.00");

    // simple: 2000 / (0 + 10000) = 20.00%
    let (_, s) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=simple",
        json!({}),
    )
    .await;
    assert_eq!(s["percentage_method"], "simple");
    assert_available(&s["totals"]["total_return_percent"], "20.00");

    // modified_dietz still available (legacy path retained)
    let (_, md) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=modified_dietz",
        json!({}),
    )
    .await;
    assert_eq!(md["percentage_method"], "modified_dietz");
    assert_available_status(&md["totals"]["total_return_percent"]);

    // unknown method -> 400
    let (st, _) = send(&state, "GET", "/api/gains?method=bogus", json!({})).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn gains_xirr_zero_total_return_with_nonzero_components() {
    // Capital gain and currency gain offset each other to zero total return.
    // Components must be unavailable (ZeroOrInvalidPerformanceDenominator), not "0.00".
    // total_return_percent must be "0.00".
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "MSFT", "NASDAQ", "USD").await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;

    // end_mv = 100 * 10 (price) * 10 (FX) = 10000; begin_mv = 0; net_flows = 10000
    // total_return = 0; capital_gain = (10-10)*100*10 = 0 + fx contribution = (10-10)*100*10 - 10000 + 10000 = 0
    // Actually set price drop to $9 and FX rise such that total_return = 0:
    // end_mv = 100 * 9 * (10000/9/100) = 10000 — but that is hard to engineer exactly.
    // Easier: SEK instrument, buy at 10, end at 10 → total=0, capital=0, currency=0 → all zero → returns "0.00".
    // For the interesting case (nonzero components, zero total), use FX gain offsetting price loss:
    // 100 shares, buy price $10, FX 10 → begin_mv=0, net_flows=10000
    // end price $8, end FX 12.5 → end_mv = 100*8*12.5 = 10000 → total_return = 0
    // capital = (100*8 - 100*10)*12.5 - 0 = -200*12.5 = -2500
    // currency = total - capital = 0 - (-2500) = 2500

    let fetched_at = crate::import::now_iso8601();
    for (date, close) in [
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            dec!(10),
        ),
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            dec!(8),
        ),
    ] {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "MSFT".to_owned(),
                date,
                close,
                currency: "USD".to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    for (date, rate) in [
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            dec!(10),
        ),
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            dec!(12.5),
        ),
    ] {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate,
                provider: FX_PROVIDER.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: PRICE_PROVIDER.to_owned(),
            provider_symbol: "MSFT".to_owned(),
            currency: Some("USD".to_owned()),
            enabled: true,
            created_at: fetched_at.clone(),
            updated_at: fetched_at,
        },
    )
    .await
    .unwrap();

    let (status, body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30",
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["percentage_method"], "money_weighted");
    assert_available(&body["totals"]["total_return_base"], "0.00");
    assert_available(&body["totals"]["total_return_percent"], "0.00");
    assert_unavailable_status(&body["totals"]["capital_gain_percent"]);
    assert_unavailable_status(&body["totals"]["currency_gain_percent"]);
}

#[tokio::test]
async fn gains_split_neutrality_regression() {
    // Regression: Simple and XIRR use actual_period_cash_flows (not split-adjusted).
    // reconstruct_period now receives the full ledger, so post_period_split_factor correctly
    // reflects post-end_date splits. actual_period_cash_flows is unaffected by the split
    // factor (by design), while period_cash_flows (used by Modified Dietz) is adjusted.
    // The key invariant is verified at the unit level in
    // performance::tests::actual_period_cash_flows_unaffected_by_post_period_split.
    //
    // This test confirms that when a split is recorded after end_date, both Simple and
    // Modified Dietz give the same result as when no split is recorded — because
    // actual_period_cash_flows intentionally ignores the post-period split factor.
    //
    // Setup: buy 100 shares at $10, FX=10 on Jun 1 (period start); 2:1 split on Aug 1.
    // Querying Jun 1 - Jun 30 (end_date before split): split excluded from ledger.
    // post_period_split_factor = 1; actual_cash_flows = period_cash_flows = [10000].
    // total_return = 12000 - 0 - 10000 = 2000; denom = 10000; percent = 20.00%.
    let state = AppState::for_tests().await;
    let instrument_id = instrument(&state, "TSLA", "NASDAQ", "USD").await;

    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
               "quantity":100,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
    )
    .await;
    send(
        &state,
        "POST",
        "/api/transactions",
        json!({"instrument_id":instrument_id,"type":"Split","trade_date":"2026-08-01",
               "quantity":100,"currency":"USD"}),
    )
    .await;

    let fetched_at = crate::import::now_iso8601();
    for (date, close) in [
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            dec!(10),
        ),
        (
            chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            dec!(12),
        ),
    ] {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "TSLA".to_owned(),
                date,
                close,
                currency: "USD".to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    for date in [
        chrono::NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
    ] {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate: dec!(10),
                provider: FX_PROVIDER.to_owned(),
                fetched_at: fetched_at.clone(),
            },
        )
        .await
        .unwrap();
    }
    provider_symbols::upsert(
        &state.pool,
        &provider_symbols::NewProviderSymbol {
            instrument_id,
            provider: PRICE_PROVIDER.to_owned(),
            provider_symbol: "TSLA".to_owned(),
            currency: Some("USD".to_owned()),
            enabled: true,
            created_at: fetched_at.clone(),
            updated_at: fetched_at,
        },
    )
    .await
    .unwrap();

    let (_, simple_body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=simple",
        json!({}),
    )
    .await;
    let (_, md_body) = send(
        &state,
        "GET",
        "/api/gains?start_date=2026-06-01&end_date=2026-06-30&method=modified_dietz",
        json!({}),
    )
    .await;

    // actual_period_cash_flows does not apply the post-period split factor, so for Simple and
    // XIRR the result is unaffected by the Aug 1 split. Modified Dietz uses period_cash_flows
    // which does apply the factor, but reconstruct_period is called with the full ledger so the
    // factor is set correctly (2 here). However, the denominator scaling and the end_mv scaling
    // cancel out, giving the same total_return_percent = 2000/10000 = 20.00%.
    // The behavioural difference is verified at the unit level in
    // performance::tests::actual_period_cash_flows_unaffected_by_post_period_split.
    assert_available(&simple_body["totals"]["total_return_percent"], "20.00");
    assert_available(&md_body["totals"]["total_return_percent"], "20.00");
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
            provider: PRICE_PROVIDER.to_owned(),
            provider_symbol: "MSFT".to_owned(),
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
                provider: PRICE_PROVIDER.to_owned(),
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
                provider: FX_PROVIDER.to_owned(),
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
            provider: PRICE_PROVIDER.to_owned(),
            provider_symbol: symbol.to_owned(),
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
                provider: PRICE_PROVIDER.to_owned(),
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
