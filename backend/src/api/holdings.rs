use axum::extract::State;
use axum::Json;
use chrono::Local;
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::{ConvictionDto, InstrumentResponse};
use crate::api::valuation::{
    load_valuation_inputs, money_string, serialize_availability, serialize_valuation_reason,
    AvailabilityResponse,
};
use crate::db::{instruments, transactions};
use crate::domain::{
    derive_position, derive_targets, value_position, Availability, BaseCostBasis, ConvictionLevel,
    ConvictionTargetInput, ConvictionTargetOutput, MarketValueState, Position, TargetField,
    UnavailableReason, ValuationReason,
};
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
    pub conviction_target: ConvictionTargetResponse,
}

#[derive(Debug, Serialize)]
pub struct ValuationField {
    pub market_value_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
}

/// Conviction target derived for the full eligible pool. The `status` and the
/// availability of each field come from the pure `domain::conviction` module;
/// only formatting happens here. Current value is intentionally not duplicated —
/// consumers read the same holding's `valuation.market_value_base`.
#[derive(Debug, Serialize)]
pub struct ConvictionTargetResponse {
    pub conviction: ConvictionDto,
    pub status: &'static str,
    pub target_value_base: AvailabilityResponse,
    pub target_gap_base: AvailabilityResponse,
    pub target_gap_percent: AvailabilityResponse,
}

fn target_field_response(
    field: &TargetField,
    valuation_reasons: &[String],
    format: impl Fn(Decimal) -> String,
) -> AvailabilityResponse {
    match field {
        TargetField::Available(value) => AvailabilityResponse::Available {
            value: format(*value),
        },
        TargetField::Unavailable(reasons) => {
            // Retain the target-specific reason (e.g. `valuation_unavailable`)
            // and append the underlying market-value reasons so the target
            // tooltip is as actionable as the valuation field beside it.
            let mut codes: Vec<String> = reasons
                .iter()
                .map(|reason| reason.as_str().to_owned())
                .collect();
            codes.extend(valuation_reasons.iter().cloned());
            AvailabilityResponse::Unavailable { reasons: codes }
        }
    }
}

fn build_conviction_target(
    output: &ConvictionTargetOutput,
    market_value_reasons: &[ValuationReason],
) -> ConvictionTargetResponse {
    // Only present when the market value is present-but-unavailable, which the
    // domain maps to `TargetReason::ValuationUnavailable`; empty otherwise.
    let valuation_reasons: Vec<String> = market_value_reasons
        .iter()
        .map(serialize_valuation_reason)
        .collect();
    ConvictionTargetResponse {
        conviction: ConvictionDto::from_level(output.conviction),
        status: output.status.as_str(),
        target_value_base: target_field_response(
            &output.target_value,
            &valuation_reasons,
            money_string,
        ),
        target_gap_base: target_field_response(
            &output.target_gap,
            &valuation_reasons,
            money_string,
        ),
        target_gap_percent: target_field_response(
            &output.target_gap_percent,
            &valuation_reasons,
            |value| format!("{value:.2}"),
        ),
    }
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
        conviction_target: ConvictionTargetResponse,
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
            conviction_target,
        })
    }
}

/// A collected open holding awaiting its pool-wide conviction target. Targets
/// depend on every eligible holding, so they are derived once after the whole
/// pool is gathered rather than row by row.
struct PendingHolding<'a> {
    instrument: &'a instruments::InstrumentRow,
    position: Position,
    valuation: Option<ValuationField>,
    /// Underlying market-value reasons when the valuation is present-but-
    /// unavailable, carried into the target response; empty otherwise.
    market_value_reasons: Vec<ValuationReason>,
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

    let mut pending = Vec::new();
    let mut target_inputs = Vec::new();

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
        // `valuation: None` means price mapping is disabled, which is distinct
        // from a present-but-unavailable valuation; the target module treats
        // them as different exclusion reasons and never as zero value.
        let (valuation, market_value, market_value_reasons) = if valuation_inputs
            .price_mapping_enabled
        {
            let valued_holding = value_position(
                &position,
                &instrument.currency,
                valuation_date,
                valuation_inputs.latest_price,
                valuation_inputs.previous_price,
                valuation_inputs.latest_fx,
                valuation_inputs.previous_fx,
            );

            let (market_value, market_value_reasons) = match &valued_holding.market_value_base {
                Availability::Available(value) => (MarketValueState::Available(*value), Vec::new()),
                Availability::Unavailable { reasons } => {
                    (MarketValueState::Unavailable, reasons.clone())
                }
            };

            let field = ValuationField {
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
            };

            (Some(field), market_value, market_value_reasons)
        } else {
            (None, MarketValueState::MappingDisabled, Vec::new())
        };

        let conviction = ConvictionLevel::from_db_str(&instrument.conviction).ok_or_else(|| {
            ApiError::internal(format!(
                "stored unknown conviction {:?} for instrument {}",
                instrument.conviction, instrument.id
            ))
        })?;

        target_inputs.push(ConvictionTargetInput {
            instrument_id: instrument.id,
            conviction,
            market_value,
        });
        pending.push(PendingHolding {
            instrument,
            position,
            valuation,
            market_value_reasons,
        });
    }

    // Derive targets once for the whole eligible pool, then attach in order.
    let targets = derive_targets(&target_inputs);
    let holdings = pending
        .into_iter()
        .zip(targets.iter())
        .map(|(holding, output)| {
            HoldingResponse::build(
                holding.instrument,
                &holding.position,
                holding.valuation,
                build_conviction_target(output, &holding.market_value_reasons),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(holdings))
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::Local;
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

    /// Seed one SEK holding (fx = 1) with an enabled price mapping and a current
    /// price, so its available market value equals `quantity * price`.
    async fn seed_valued(
        state: &AppState,
        symbol: &str,
        quantity: i64,
        price: &str,
        conviction: &str,
    ) -> i64 {
        use crate::db::{prices, provider_symbols};
        use rust_decimal::Decimal;
        use std::str::FromStr;

        let id = instrument(state, symbol, "STO", "SEK").await;
        let now = crate::import::now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: id,
                provider: super::super::valuation::PRICE_PROVIDER.to_owned(),
                provider_symbol: symbol.to_owned(),
                currency: Some("SEK".to_owned()),
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
                provider: super::super::valuation::PRICE_PROVIDER.to_owned(),
                provider_symbol: symbol.to_owned(),
                date: Local::now().naive_local().date(),
                close: Decimal::from_str(price).expect("price"),
                currency: "SEK".to_owned(),
                fetched_at: now,
            },
        )
        .await
        .expect("price");
        send(
            state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":quantity,"price":price,"currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;
        set_conviction(state, id, conviction).await;
        id
    }

    async fn set_conviction(state: &AppState, id: i64, conviction: &str) {
        let (status, _) = send(
            state,
            "PUT",
            &format!("/api/instruments/{id}/conviction"),
            json!({ "conviction": conviction }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn holdings_by_symbol(state: &AppState) -> std::collections::HashMap<String, Value> {
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

    #[tokio::test]
    async fn conviction_targets_match_design_example() {
        let state = AppState::for_tests().await;
        seed_valued(&state, "AAA", 1000, "100", "Low").await;
        seed_valued(&state, "BBB", 1000, "300", "Medium").await;
        seed_valued(&state, "CCC", 1000, "300", "High").await;
        seed_valued(&state, "DDD", 1000, "500", "Other").await;

        let holdings = holdings_by_symbol(&state).await;

        let a = &holdings["AAA"]["conviction_target"];
        assert_eq!(a["conviction"], "Low");
        assert_eq!(a["status"], "on_target");
        assert_eq!(a["target_value_base"]["value"], "100000.00");
        assert_eq!(a["target_gap_base"]["value"], "0.00");

        let b = &holdings["BBB"]["conviction_target"];
        assert_eq!(b["status"], "above");
        assert_eq!(b["target_value_base"]["value"], "200000.00");
        assert_eq!(b["target_gap_base"]["value"], "100000.00");

        let c = &holdings["CCC"]["conviction_target"];
        assert_eq!(c["status"], "below");
        assert_eq!(c["target_value_base"]["value"], "400000.00");
        assert_eq!(c["target_gap_base"]["value"], "-100000.00");

        let d = &holdings["DDD"]["conviction_target"];
        assert_eq!(d["conviction"], "Other");
        assert_eq!(d["status"], "no_target");
        assert_eq!(d["target_value_base"]["status"], "unavailable");
        assert_eq!(d["target_value_base"]["reasons"][0], "no_target");
    }

    #[tokio::test]
    async fn editing_one_conviction_reprices_every_eligible_target() {
        let state = AppState::for_tests().await;
        let a = seed_valued(&state, "AAA", 1000, "100", "Low").await;
        let b = seed_valued(&state, "BBB", 1000, "100", "Low").await;

        // Two equal Low holdings each target the pool average of 100000.
        let before = holdings_by_symbol(&state).await;
        assert_eq!(
            before["AAA"]["conviction_target"]["target_value_base"]["value"],
            "100000.00"
        );
        assert_eq!(
            before["BBB"]["conviction_target"]["target_value_base"]["value"],
            "100000.00"
        );

        // Raising B to High moves A's target too (pool 200000, weights 1 and 4).
        set_conviction(&state, b, "High").await;
        let after = holdings_by_symbol(&state).await;
        assert_eq!(
            after["AAA"]["conviction_target"]["target_value_base"]["value"],
            "40000.00"
        );
        assert_eq!(after["AAA"]["conviction_target"]["status"], "above");
        assert_eq!(
            after["BBB"]["conviction_target"]["target_value_base"]["value"],
            "160000.00"
        );
        assert_eq!(after["BBB"]["conviction_target"]["status"], "below");
        let _ = a;
    }

    #[tokio::test]
    async fn convicted_holding_without_price_mapping_is_excluded_unavailable() {
        let state = AppState::for_tests().await;
        // No provider symbol/price mapping: valuation is absent (disabled).
        let id = instrument(&state, "NOPX", "STO", "SEK").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;
        set_conviction(&state, id, "High").await;

        let holdings = holdings_by_symbol(&state).await;
        let target = &holdings["NOPX"]["conviction_target"];
        assert_eq!(target["conviction"], "High");
        assert_eq!(target["status"], "excluded_unavailable");
        assert_eq!(target["target_value_base"]["status"], "unavailable");
        assert_eq!(
            target["target_value_base"]["reasons"][0],
            "price_mapping_disabled"
        );
    }

    #[tokio::test]
    async fn convicted_holding_with_missing_price_is_excluded_unavailable() {
        let state = AppState::for_tests().await;
        use crate::db::provider_symbols;
        // Enabled price mapping but no price row → valuation unavailable.
        let id = instrument(&state, "MISS", "STO", "SEK").await;
        let now = crate::import::now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: id,
                provider: super::super::valuation::PRICE_PROVIDER.to_owned(),
                provider_symbol: "MISS".to_owned(),
                currency: Some("SEK".to_owned()),
                enabled: true,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("provider symbol");
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;
        set_conviction(&state, id, "Medium").await;

        let holdings = holdings_by_symbol(&state).await;
        let target = &holdings["MISS"]["conviction_target"];
        assert_eq!(target["status"], "excluded_unavailable");
        // Retains the target-specific reason and carries the underlying
        // market-value reason so the tooltip matches the valuation field.
        let reasons: Vec<&str> = target["target_value_base"]["reasons"]
            .as_array()
            .expect("reasons array")
            .iter()
            .map(|reason| reason.as_str().expect("reason string"))
            .collect();
        assert_eq!(reasons[0], "valuation_unavailable");
        assert!(
            reasons.contains(&"missing_price"),
            "expected underlying valuation reason, got {reasons:?}"
        );
        // The same underlying reason appears in the valuation field beside it.
        let valuation_reasons: Vec<&str> = holdings["MISS"]["valuation"]["market_value_base"]
            ["reasons"]
            .as_array()
            .expect("valuation reasons array")
            .iter()
            .map(|reason| reason.as_str().expect("reason string"))
            .collect();
        assert!(valuation_reasons.contains(&"missing_price"));
    }

    #[tokio::test]
    async fn all_other_holdings_report_no_target_without_treating_values_as_zero() {
        let state = AppState::for_tests().await;
        seed_valued(&state, "AAA", 1000, "100", "Other").await;
        seed_valued(&state, "BBB", 1000, "300", "Other").await;

        let holdings = holdings_by_symbol(&state).await;
        for symbol in ["AAA", "BBB"] {
            let target = &holdings[symbol]["conviction_target"];
            assert_eq!(target["status"], "no_target");
            assert_eq!(target["target_value_base"]["status"], "unavailable");
            assert_eq!(target["target_value_base"]["reasons"][0], "no_target");
        }
    }
}
