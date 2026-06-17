use axum::extract::State;
use axum::Json;
use chrono::Local;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    fx_snapshot_response, load_valuation_inputs, money_string, price_snapshot_response,
    serialize_availability, serialize_valuation_reason, AvailabilityResponse, FxSnapshotResponse,
    PriceSnapshotResponse, BASE_CURRENCY,
};
use crate::db::{instruments, transactions};
use crate::domain::{derive_position, summarize_holdings, value_position};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct GainsResponse {
    pub as_of_date: String,
    pub base_currency: String,
    pub summary: SummaryResponse,
    pub rows: Vec<GainRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub market_value_base: AvailabilityResponse,
    pub cost_basis_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct GainRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub cost_basis_base: AvailabilityResponse,
    pub latest_price: Option<PriceSnapshotResponse>,
    pub previous_price: Option<PriceSnapshotResponse>,
    pub latest_fx: Option<FxSnapshotResponse>,
    pub previous_fx: Option<FxSnapshotResponse>,
    pub market_value_native: AvailabilityResponse,
    pub market_value_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub reasons: Vec<String>,
}

pub async fn list(State(state): State<AppState>) -> Result<Json<GainsResponse>, ApiError> {
    let valuation_date = Local::now().naive_local().date();
    let instruments_list = instruments::list(&state.pool).await?;
    let transaction_rows = transactions::all_for_holdings(&state.pool).await?;
    let mut ledgers = BTreeMap::new();

    for row in &transaction_rows {
        ledgers
            .entry(row.instrument_id)
            .or_insert_with(Vec::new)
            .push(row.to_ledger()?);
    }

    let mut valued_holdings = Vec::new();
    let mut gain_rows = Vec::new();

    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        let position = derive_position(&ledger).map_err(|error| {
            ApiError::internal(format!(
                "inconsistent stored ledger for instrument {}: {error:?}",
                instrument.id
            ))
        })?;
        if position.quantity == 0 {
            continue;
        }

        let valuation_inputs =
            load_valuation_inputs(&state.pool, instrument, valuation_date).await?;

        let valued_holding = value_position(
            &position,
            &instrument.currency,
            valuation_date,
            valuation_inputs.latest_price,
            valuation_inputs.previous_price,
            valuation_inputs.latest_fx,
            valuation_inputs.previous_fx,
        );

        valued_holdings.push(valued_holding.clone());

        let gain_row = GainRow {
            instrument: InstrumentResponse::from_row(instrument)?,
            quantity: valued_holding.quantity,
            cost_basis_native: money_string(valued_holding.cost_basis_native),
            cost_basis_base: serialize_availability(&valued_holding.cost_basis_base, |v| {
                money_string(*v)
            }),
            latest_price: valued_holding
                .latest_price
                .as_ref()
                .map(price_snapshot_response),
            previous_price: valued_holding
                .previous_price
                .as_ref()
                .map(price_snapshot_response),
            latest_fx: valued_holding.latest_fx.as_ref().map(fx_snapshot_response),
            previous_fx: valued_holding
                .previous_fx
                .as_ref()
                .map(fx_snapshot_response),
            market_value_native: serialize_availability(&valued_holding.market_value_native, |v| {
                money_string(*v)
            }),
            market_value_base: serialize_availability(&valued_holding.market_value_base, |v| {
                money_string(*v)
            }),
            unrealized_gain_base: serialize_availability(
                &valued_holding.unrealized_gain_base,
                |v| money_string(*v),
            ),
            unrealized_gain_percent: serialize_availability(
                &valued_holding.unrealized_gain_percent,
                |v| format!("{:.2}", v),
            ),
            day_change_base: serialize_availability(&valued_holding.day_change_base, |v| {
                money_string(*v)
            }),
            day_change_percent: serialize_availability(&valued_holding.day_change_percent, |v| {
                format!("{:.2}", v)
            }),
            reasons: valued_holding
                .reasons
                .iter()
                .map(serialize_valuation_reason)
                .collect(),
        };

        gain_rows.push(gain_row);
    }

    let summary = summarize_holdings(&valued_holdings);

    Ok(Json(GainsResponse {
        as_of_date: valuation_date.format("%Y-%m-%d").to_string(),
        base_currency: BASE_CURRENCY.to_string(),
        summary: SummaryResponse {
            market_value_base: serialize_availability(&summary.market_value_base, |v| {
                money_string(*v)
            }),
            cost_basis_base: serialize_availability(&summary.cost_basis_base, |v| money_string(*v)),
            unrealized_gain_base: serialize_availability(&summary.unrealized_gain_base, |v| {
                money_string(*v)
            }),
            unrealized_gain_percent: serialize_availability(
                &summary.unrealized_gain_percent,
                |v| format!("{:.2}", v),
            ),
            day_change_base: serialize_availability(&summary.day_change_base, |v| money_string(*v)),
            day_change_percent: serialize_availability(&summary.day_change_percent, |v| {
                format!("{:.2}", v)
            }),
            excluded_rows: summary.excluded_rows,
        },
        rows: gain_rows,
    }))
}

#[cfg(test)]
mod tests {
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
        assert_eq!(body["rows"].as_array().unwrap().len(), 0);
        assert_eq!(body["summary"]["excluded_rows"], 0);
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

        let (status, body) = send(&state, "GET", "/api/gains", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["base_currency"], BASE_CURRENCY);
        assert_eq!(body["rows"].as_array().expect("rows").len(), 1);

        let row = &body["rows"][0];
        assert_eq!(row["instrument"]["symbol"], "MSFT");
        assert_eq!(row["quantity"], 10);
        assert_eq!(row["cost_basis_native"], "1000.00");
        assert_available(&row["cost_basis_base"], "10000.00");
        assert_eq!(row["latest_price"]["close"], "120.00");
        assert_eq!(row["latest_fx"]["rate"], "11");
        assert_eq!(row["latest_fx"]["quote"], BASE_CURRENCY);
        assert_available(&row["market_value_native"], "1200.00");
        assert_available(&row["market_value_base"], "13200.00");
        assert_available(&row["unrealized_gain_base"], "3200.00");
        assert_available(&row["unrealized_gain_percent"], "32.00");
        assert_available(&row["day_change_base"], "1650.00");
        assert_available(&row["day_change_percent"], "14.28");

        assert_available(&body["summary"]["market_value_base"], "13200.00");
        assert_available(&body["summary"]["cost_basis_base"], "10000.00");
        assert_available(&body["summary"]["unrealized_gain_base"], "3200.00");
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

    fn assert_available(value: &serde_json::Value, expected: &str) {
        assert_eq!(value["status"], "available");
        assert_eq!(value["value"], expected);
    }
}
