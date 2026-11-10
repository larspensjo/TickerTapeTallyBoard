//! Percentage method behavior and regression fixtures.
use super::*;
use rust_decimal_macros::dec;
use serde_json::json;

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
                provider: PRICE_PROVIDER_PRECEDENCE[0],
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
                provider: BASE_FX_PROVIDER,
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
            provider: PRICE_PROVIDER_PRECEDENCE[0],
            provider_symbol: "MSFT".to_owned(),
            asset_class: None,
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
                provider: PRICE_PROVIDER_PRECEDENCE[0],
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
                provider: BASE_FX_PROVIDER,
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
            provider: PRICE_PROVIDER_PRECEDENCE[0],
            provider_symbol: "MSFT".to_owned(),
            asset_class: None,
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
                provider: PRICE_PROVIDER_PRECEDENCE[0],
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
                provider: BASE_FX_PROVIDER,
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
            provider: PRICE_PROVIDER_PRECEDENCE[0],
            provider_symbol: "TSLA".to_owned(),
            asset_class: None,
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
