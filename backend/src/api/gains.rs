use axum::extract::{Query, State};
use axum::Json;
use chrono::Local;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    fx_snapshot_response, load_valuation_inputs, money_string, price_snapshot_response,
    serialize_availability, serialize_valuation_reason, AvailabilityResponse, FxSnapshotResponse,
    PriceSnapshotResponse, BASE_CURRENCY,
};
use crate::db::{instruments, transactions};
use crate::domain::{
    derive_position_performance, summarize_holdings, value_position, Availability, BaseAmount,
    RealizedGain, ValuationReason, ValuedHolding,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct GainsQuery {
    #[serde(default)]
    include_closed: bool,
}

#[derive(Debug, Serialize)]
pub struct GainsResponse {
    pub as_of_date: String,
    pub base_currency: String,
    pub include_closed_positions: bool,
    pub summary: SummaryResponse,
    pub totals: TotalsResponse,
    pub rows: Vec<GainRow>,
}

#[derive(Debug, Serialize)]
pub struct SummaryResponse {
    pub market_value_base: AvailabilityResponse,
    pub cost_basis_base: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
    pub unrealized_gain_base: AvailabilityResponse,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub day_change_base: AvailabilityResponse,
    pub day_change_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct TotalsResponse {
    pub capital_gain_base: AvailabilityResponse,
    pub capital_gain_percent: AvailabilityResponse,
    pub income_base: AvailabilityResponse,
    pub income_percent: AvailabilityResponse,
    pub currency_gain_base: AvailabilityResponse,
    pub currency_gain_percent: AvailabilityResponse,
    pub total_return_base: AvailabilityResponse,
    pub total_return_percent: AvailabilityResponse,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
pub struct GainRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub cost_basis_base: AvailabilityResponse,
    pub price_effect_base: AvailabilityResponse,
    pub fx_effect_base: AvailabilityResponse,
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
    pub position_status: GainPositionStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GainPositionStatus {
    Open,
    Closed,
}

pub async fn list(
    State(state): State<AppState>,
    Query(query): Query<GainsQuery>,
) -> Result<Json<GainsResponse>, ApiError> {
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
    let mut totals = TotalsAccumulator::default();

    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        let performance = derive_position_performance(&ledger).map_err(|error| {
            ApiError::internal(format!(
                "inconsistent stored ledger for instrument {}: {error:?}",
                instrument.id
            ))
        })?;
        if performance.position.quantity == 0 {
            if query.include_closed && performance.realized.sold_quantity > 0 {
                totals.add_closed(&performance.realized);
                gain_rows.push(closed_gain_row(instrument, &performance.realized)?);
            }
            continue;
        }

        let valuation_inputs =
            load_valuation_inputs(&state.pool, instrument, valuation_date).await?;

        let valued_holding = value_position(
            &performance.position,
            &instrument.currency,
            valuation_date,
            valuation_inputs.latest_price,
            valuation_inputs.previous_price,
            valuation_inputs.latest_fx,
            valuation_inputs.previous_fx,
        );

        valued_holdings.push(valued_holding.clone());
        totals.add_open(&valued_holding);

        gain_rows.push(open_gain_row(instrument, &valued_holding)?);
        if query.include_closed && performance.realized.sold_quantity > 0 {
            totals.add_closed(&performance.realized);
            gain_rows.push(closed_gain_row(instrument, &performance.realized)?);
        }
    }

    let summary = summarize_holdings(&valued_holdings);

    Ok(Json(GainsResponse {
        as_of_date: valuation_date.format("%Y-%m-%d").to_string(),
        base_currency: BASE_CURRENCY.to_string(),
        include_closed_positions: query.include_closed,
        summary: SummaryResponse {
            market_value_base: serialize_availability(&summary.market_value_base, |v| {
                money_string(*v)
            }),
            cost_basis_base: serialize_availability(&summary.cost_basis_base, |v| money_string(*v)),
            price_effect_base: serialize_availability(&summary.price_effect_base, |v| {
                money_string(*v)
            }),
            fx_effect_base: serialize_availability(&summary.fx_effect_base, |v| money_string(*v)),
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
        totals: totals.into_response(),
        rows: gain_rows,
    }))
}

#[derive(Default)]
struct TotalsAccumulator {
    cost_basis_base: Decimal,
    capital_gain_base: Decimal,
    currency_gain_base: Decimal,
    total_return_base: Decimal,
    included_rows: usize,
    excluded_rows: usize,
}

impl TotalsAccumulator {
    fn add_open(&mut self, value: &ValuedHolding) {
        self.add_values(
            value.cost_basis_base.clone(),
            value.price_effect_base.clone(),
            value.fx_effect_base.clone(),
            value.unrealized_gain_base.clone(),
        );
    }

    fn add_closed(&mut self, value: &RealizedGain) {
        self.add_values(
            base_amount_availability(&value.cost_basis_base),
            base_amount_availability(&value.price_effect_base),
            base_amount_availability(&value.fx_effect_base),
            base_amount_availability(&value.gain_base),
        );
    }

    fn add_values(
        &mut self,
        cost_basis_base: Availability<Decimal>,
        capital_gain_base: Availability<Decimal>,
        currency_gain_base: Availability<Decimal>,
        total_return_base: Availability<Decimal>,
    ) {
        match (
            cost_basis_base.as_ref(),
            capital_gain_base.as_ref(),
            currency_gain_base.as_ref(),
            total_return_base.as_ref(),
        ) {
            (Some(cost_basis), Some(capital_gain), Some(currency_gain), Some(total_return)) => {
                self.cost_basis_base += *cost_basis;
                self.capital_gain_base += *capital_gain;
                self.currency_gain_base += *currency_gain;
                self.total_return_base += *total_return;
                self.included_rows += 1;
            }
            _ => self.excluded_rows += 1,
        }
    }

    fn into_response(self) -> TotalsResponse {
        let has_totals = self.included_rows > 0;
        let capital_gain_base = totals_money(has_totals, self.capital_gain_base);
        let currency_gain_base = totals_money(has_totals, self.currency_gain_base);
        let total_return_base = totals_money(has_totals, self.total_return_base);

        TotalsResponse {
            capital_gain_base,
            capital_gain_percent: totals_percent(
                has_totals,
                self.capital_gain_base,
                self.cost_basis_base,
            ),
            income_base: AvailabilityResponse::Unavailable {
                reasons: vec!["income_not_tracked".to_string()],
            },
            income_percent: AvailabilityResponse::Unavailable {
                reasons: vec!["income_not_tracked".to_string()],
            },
            currency_gain_base,
            currency_gain_percent: totals_percent(
                has_totals,
                self.currency_gain_base,
                self.cost_basis_base,
            ),
            total_return_base,
            total_return_percent: totals_percent(
                has_totals,
                self.total_return_base,
                self.cost_basis_base,
            ),
            excluded_rows: self.excluded_rows,
        }
    }
}

fn totals_money(has_totals: bool, value: Decimal) -> AvailabilityResponse {
    if has_totals {
        AvailabilityResponse::Available {
            value: money_string(value),
        }
    } else {
        AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        }
    }
}

fn totals_percent(
    has_totals: bool,
    numerator: Decimal,
    cost_basis_base: Decimal,
) -> AvailabilityResponse {
    if !has_totals {
        return AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        };
    }

    if cost_basis_base == Decimal::ZERO {
        return AvailabilityResponse::Unavailable {
            reasons: vec!["zero_cost_basis".to_string()],
        };
    }

    AvailabilityResponse::Available {
        value: format!("{:.2}", (numerator / cost_basis_base) * Decimal::from(100)),
    }
}

fn open_gain_row(
    instrument: &instruments::InstrumentRow,
    valued_holding: &ValuedHolding,
) -> Result<GainRow, ApiError> {
    Ok(GainRow {
        instrument: InstrumentResponse::from_row(instrument)?,
        quantity: valued_holding.quantity,
        cost_basis_native: money_string(valued_holding.cost_basis_native),
        cost_basis_base: serialize_availability(&valued_holding.cost_basis_base, |v| {
            money_string(*v)
        }),
        price_effect_base: serialize_availability(&valued_holding.price_effect_base, |v| {
            money_string(*v)
        }),
        fx_effect_base: serialize_availability(&valued_holding.fx_effect_base, |v| {
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
        unrealized_gain_base: serialize_availability(&valued_holding.unrealized_gain_base, |v| {
            money_string(*v)
        }),
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
        position_status: GainPositionStatus::Open,
    })
}

fn closed_gain_row(
    instrument: &instruments::InstrumentRow,
    realized: &RealizedGain,
) -> Result<GainRow, ApiError> {
    let cost_basis_base = base_amount_availability(&realized.cost_basis_base);
    let gain_base = base_amount_availability(&realized.gain_base);
    let gain_percent = match (gain_base.as_ref(), cost_basis_base.as_ref()) {
        (Some(gain), Some(cost_basis)) if *cost_basis != Decimal::ZERO => {
            Availability::available((*gain / *cost_basis) * Decimal::from(100))
        }
        (Some(_), Some(_)) => Availability::unavailable(ValuationReason::ZeroCostBasis),
        _ => Availability::Unavailable {
            reasons: merge_closed_reasons(&[gain_base.reasons(), cost_basis_base.reasons()]),
        },
    };
    let mut reasons = merge_closed_reasons(&[
        cost_basis_base.reasons(),
        base_amount_availability(&realized.proceeds_base).reasons(),
        base_amount_availability(&realized.price_effect_base).reasons(),
        base_amount_availability(&realized.fx_effect_base).reasons(),
        gain_base.reasons(),
        gain_percent.reasons(),
    ]);

    dedup_valuation_reasons(&mut reasons);

    Ok(GainRow {
        instrument: InstrumentResponse::from_row(instrument)?,
        quantity: 0,
        cost_basis_native: money_string(realized.cost_basis_native),
        cost_basis_base: serialize_availability(&cost_basis_base, |v| money_string(*v)),
        price_effect_base: serialize_base_amount(&realized.price_effect_base),
        fx_effect_base: serialize_base_amount(&realized.fx_effect_base),
        latest_price: None,
        previous_price: None,
        latest_fx: None,
        previous_fx: None,
        market_value_native: AvailabilityResponse::Available {
            value: money_string(realized.proceeds_native),
        },
        market_value_base: serialize_base_amount(&realized.proceeds_base),
        unrealized_gain_base: serialize_availability(&gain_base, |v| money_string(*v)),
        unrealized_gain_percent: serialize_availability(&gain_percent, |v| format!("{:.2}", v)),
        day_change_base: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        day_change_percent: AvailabilityResponse::Unavailable {
            reasons: Vec::new(),
        },
        reasons: reasons.iter().map(serialize_valuation_reason).collect(),
        position_status: GainPositionStatus::Closed,
    })
}

fn serialize_base_amount(value: &BaseAmount) -> AvailabilityResponse {
    let availability = base_amount_availability(value);
    serialize_availability(&availability, |v| money_string(*v))
}

fn base_amount_availability(value: &BaseAmount) -> Availability<Decimal> {
    match value {
        BaseAmount::Available(value) => Availability::available(*value),
        BaseAmount::Unavailable { .. } => Availability::unavailable(ValuationReason::MissingFx),
    }
}

fn merge_closed_reasons(sources: &[Vec<ValuationReason>]) -> Vec<ValuationReason> {
    let mut reasons = Vec::new();
    for source in sources {
        reasons.extend_from_slice(source);
    }
    dedup_valuation_reasons(&mut reasons);
    reasons
}

fn dedup_valuation_reasons(reasons: &mut Vec<ValuationReason>) {
    let mut deduped = Vec::new();
    for reason in reasons.drain(..) {
        if !deduped.contains(&reason) {
            deduped.push(reason);
        }
    }
    *reasons = deduped;
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
        assert_eq!(body["include_closed_positions"], false);
        assert_eq!(body["rows"].as_array().unwrap().len(), 0);
        assert_eq!(body["summary"]["excluded_rows"], 0);
        assert_eq!(body["totals"]["excluded_rows"], 0);
        assert_unavailable(&body["totals"]["capital_gain_base"], &[]);
        assert_unavailable(&body["totals"]["income_base"], &["income_not_tracked"]);
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
        assert_available(&body["totals"]["capital_gain_base"], "2175.00");
        assert_available(&body["totals"]["capital_gain_percent"], "21.70");
        assert_available(&body["totals"]["currency_gain_base"], "1000.00");
        assert_available(&body["totals"]["currency_gain_percent"], "9.98");
        assert_available(&body["totals"]["total_return_base"], "3175.00");
        assert_available(&body["totals"]["total_return_percent"], "31.68");
        assert_unavailable(&body["totals"]["income_base"], &["income_not_tracked"]);

        let row = &body["rows"][0];
        assert_eq!(row["instrument"]["symbol"], "MSFT");
        assert_eq!(row["position_status"], "closed");
        assert_eq!(row["quantity"], 0);
        assert_eq!(row["cost_basis_native"], "1000.00");
        assert_available(&row["cost_basis_base"], "10020.00");
        assert_available(&row["market_value_native"], "1200.00");
        assert_available(&row["market_value_base"], "13195.00");
        assert_available(&row["unrealized_gain_base"], "3175.00");
        assert_available(&row["unrealized_gain_percent"], "31.68");
        assert_available(&row["price_effect_base"], "2175.00");
        assert_available(&row["fx_effect_base"], "1000.00");
    }

    #[tokio::test]
    async fn gains_include_closed_positions_counts_partial_sells_from_open_positions() {
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
        assert_eq!(body["rows"].as_array().expect("rows").len(), 2);
        assert_available(&body["totals"]["capital_gain_base"], "2175.00");
        assert_available(&body["totals"]["capital_gain_percent"], "21.70");
        assert_available(&body["totals"]["currency_gain_base"], "1000.00");
        assert_available(&body["totals"]["currency_gain_percent"], "9.98");
        assert_available(&body["totals"]["total_return_base"], "3175.00");
        assert_available(&body["totals"]["total_return_percent"], "31.68");

        let rows = body["rows"].as_array().expect("rows");
        let open_row = rows
            .iter()
            .find(|row| row["position_status"] == "open")
            .expect("open row");
        assert_eq!(open_row["quantity"], 6);
        assert_available(&open_row["unrealized_gain_base"], "1908.00");

        let closed_row = rows
            .iter()
            .find(|row| row["position_status"] == "closed")
            .expect("closed row");
        assert_eq!(closed_row["quantity"], 0);
        assert_available(&closed_row["unrealized_gain_base"], "1267.00");
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
        assert_available(&row["price_effect_base"], "2200.00");
        assert_available(&row["fx_effect_base"], "1000.00");
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
        assert_available(&body["summary"]["price_effect_base"], "2200.00");
        assert_available(&body["summary"]["fx_effect_base"], "1000.00");
        assert_available(&body["summary"]["unrealized_gain_base"], "3200.00");
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
}
