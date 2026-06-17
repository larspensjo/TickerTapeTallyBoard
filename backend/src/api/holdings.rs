use axum::extract::State;
use axum::Json;
use chrono::Local;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    load_valuation_inputs, money_string, serialize_availability, AvailabilityResponse,
};
use crate::db::{instruments, transactions};
use crate::domain::{derive_position, value_position, BaseCostBasis, Position, UnavailableReason};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct HoldingResponse {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub average_cost_native: String,
    pub base: BaseResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valuation: Option<ValuationField>,
}

#[derive(Debug, Serialize)]
pub struct ValuationField {
    pub market_value_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BaseResponse {
    Available {
        cost_basis_base: String,
        average_cost_base: String,
        fee_component_base: String,
    },
    Unavailable {
        reasons: Vec<ReasonResponse>,
    },
}

#[derive(Debug, Serialize)]
pub struct ReasonResponse {
    pub code: &'static str,
    pub transaction_id: i64,
}

impl HoldingResponse {
    fn build(
        instrument: &instruments::InstrumentRow,
        position: &Position,
        valuation: Option<ValuationField>,
    ) -> Result<Self, ApiError> {
        let average_cost_native = position
            .average_cost_native()
            .ok_or_else(|| ApiError::internal("holding with non-positive quantity"))
            .map(money_string)?;

        let base = match &position.base {
            BaseCostBasis::Available {
                cost_basis_base,
                fee_component_base,
            } => BaseResponse::Available {
                cost_basis_base: money_string(*cost_basis_base),
                average_cost_base: position
                    .average_cost_base()
                    .ok_or_else(|| ApiError::internal("available base without average"))
                    .map(money_string)?,
                fee_component_base: money_string(*fee_component_base),
            },
            BaseCostBasis::Unavailable { reasons } => BaseResponse::Unavailable {
                reasons: reasons
                    .iter()
                    .map(|reason| match reason {
                        UnavailableReason::MissingFx { transaction_id } => ReasonResponse {
                            code: "missing_fx",
                            transaction_id: *transaction_id,
                        },
                    })
                    .collect(),
            },
        };

        Ok(Self {
            instrument: InstrumentResponse::from_row(instrument)?,
            quantity: position.quantity,
            cost_basis_native: money_string(position.cost_basis_native),
            average_cost_native,
            base,
            valuation,
        })
    }
}

pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<HoldingResponse>>, ApiError> {
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

    let mut holdings = Vec::new();

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
        let valuation = if valuation_inputs.price_mapping_enabled {
            let valued_holding = value_position(
                &position,
                &instrument.currency,
                valuation_date,
                valuation_inputs.latest_price,
                valuation_inputs.previous_price,
                valuation_inputs.latest_fx,
                valuation_inputs.previous_fx,
            );

            Some(ValuationField {
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
            })
        } else {
            None
        };

        holdings.push(HoldingResponse::build(instrument, &position, valuation)?);
    }

    Ok(Json(holdings))
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn send(state: &AppState, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
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

    #[tokio::test]
    async fn holding_reports_weighted_average_and_base_cost() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-12",
                   "quantity":10,"price":"12.50","currency":"USD","fx_rate_to_base":"10.0","brokerage":"9.60"}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["quantity"], 10);
        assert_eq!(holding["average_cost_native"], "12.50");
        assert_eq!(holding["cost_basis_native"], "125.00");
        assert_eq!(holding["base"]["status"], "available");
        assert_eq!(holding["base"]["cost_basis_base"], "1259.60");
        assert_eq!(holding["base"]["average_cost_base"], "125.96");
        assert_eq!(holding["base"]["fee_component_base"], "9.60");
    }

    #[tokio::test]
    async fn split_rescales_average_in_holdings() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "NOW", "NYSE", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"120","currency":"USD","fx_rate_to_base":"1"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Split","trade_date":"2026-06-02","quantity":10}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["quantity"], 20);
        assert_eq!(holding["average_cost_native"], "60.00");
    }

    #[tokio::test]
    async fn missing_fx_reports_unavailable_base_with_reason() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ASML", "EURONEXT", "EUR").await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":5,"price":"600","currency":"EUR"}),
        )
        .await;
        let tx_id = created["id"].as_i64().expect("id");

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["average_cost_native"], "600.00");
        assert_eq!(holding["base"]["status"], "unavailable");
        assert_eq!(holding["base"]["reasons"][0]["code"], "missing_fx");
        assert_eq!(holding["base"]["reasons"][0]["transaction_id"], tx_id);
    }

    #[tokio::test]
    async fn closed_position_is_omitted() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(holdings.as_array().expect("array").len(), 0);
    }

    #[tokio::test]
    async fn partial_sell_formats_derived_money_to_two_decimals() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "NASDAQ", "USD").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":3,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":1,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings.as_array().expect("array")[0];
        assert_eq!(holding["quantity"], 2);
        assert_eq!(holding["cost_basis_native"], "200.00");
        assert_eq!(holding["average_cost_native"], "100.00");
        assert_eq!(holding["base"]["cost_basis_base"], "2000.00");
        assert_eq!(holding["base"]["average_cost_base"], "1000.00");
    }

    #[tokio::test]
    async fn holdings_follow_instrument_exchange_symbol_order() {
        let state = AppState::for_tests().await;
        let zzz = instrument(&state, "ZZZ", "NYSE", "USD").await;
        let aaa = instrument(&state, "AAA", "NASDAQ", "USD").await;
        for id in [zzz, aaa] {
            send(
                &state,
                "POST",
                "/api/transactions",
                json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                       "quantity":1,"price":"10","currency":"USD","fx_rate_to_base":"10"}),
            )
            .await;
        }

        let (status, holdings) = send(&state, "GET", "/api/holdings", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        let holdings = holdings.as_array().expect("array");
        assert_eq!(holdings.len(), 2);
        assert_eq!(holdings[0]["instrument"]["exchange"], "NASDAQ");
        assert_eq!(holdings[0]["instrument"]["symbol"], "AAA");
        assert_eq!(holdings[1]["instrument"]["exchange"], "NYSE");
        assert_eq!(holdings[1]["instrument"]["symbol"], "ZZZ");
    }
}
