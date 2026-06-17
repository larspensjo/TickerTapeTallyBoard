use axum::extract::State;
use axum::Json;
use chrono::Local;
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::db::{fx_rates, instruments, prices, provider_symbols, transactions};
use crate::domain::{
    derive_position, summarize_holdings, value_position, Availability, DataFreshness, FxCandidate,
    PriceCandidate, ValuationReason,
};
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
    pub market_value_base: AvailabilityResponse<String>,
    pub cost_basis_base: AvailabilityResponse<String>,
    pub unrealized_gain_base: AvailabilityResponse<String>,
    pub unrealized_gain_percent: AvailabilityResponse<String>,
    pub day_change_base: AvailabilityResponse<String>,
    pub day_change_percent: AvailabilityResponse<String>,
    pub excluded_rows: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AvailabilityResponse<T> {
    Available { value: T },
    Unavailable { reasons: Vec<String> },
}

#[derive(Debug, Serialize)]
pub struct GainRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub cost_basis_native: String,
    pub cost_basis_base: AvailabilityResponse<String>,
    pub latest_price: Option<PriceSnapshotResponse>,
    pub previous_price: Option<PriceSnapshotResponse>,
    pub latest_fx: Option<FxSnapshotResponse>,
    pub previous_fx: Option<FxSnapshotResponse>,
    pub market_value_native: AvailabilityResponse<String>,
    pub market_value_base: AvailabilityResponse<String>,
    pub unrealized_gain_base: AvailabilityResponse<String>,
    pub unrealized_gain_percent: AvailabilityResponse<String>,
    pub day_change_base: AvailabilityResponse<String>,
    pub day_change_percent: AvailabilityResponse<String>,
    pub reasons: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct PriceSnapshotResponse {
    pub date: String,
    pub close: String,
    pub currency: String,
    pub freshness: String,
}

#[derive(Debug, Serialize)]
pub struct FxSnapshotResponse {
    pub date: String,
    pub rate: String,
    pub base: String,
    pub quote: String,
    pub freshness: String,
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

        // Fetch provider symbol (try YAHOO first as primary provider)
        let provider_symbol_row =
            provider_symbols::find_by_instrument_provider(&state.pool, instrument.id, "YAHOO")
                .await?;

        let (latest_price, previous_price) = if let Some(ps_row) = provider_symbol_row {
            if ps_row.enabled {
                let latest = prices::find_latest_on_or_before(
                    &state.pool,
                    instrument.id,
                    "YAHOO",
                    valuation_date,
                )
                .await?
                .and_then(|row| {
                    let date = row.date_value().ok()?;
                    let close = row.close_decimal().ok()?;
                    Some(PriceCandidate {
                        date,
                        close,
                        currency: row.currency,
                    })
                });

                let previous = if let Some(ref latest) = latest {
                    prices::find_previous_before(&state.pool, instrument.id, "YAHOO", latest.date)
                        .await?
                        .and_then(|row| {
                            let date = row.date_value().ok()?;
                            let close = row.close_decimal().ok()?;
                            Some(PriceCandidate {
                                date,
                                close,
                                currency: row.currency,
                            })
                        })
                } else {
                    None
                };

                (latest, previous)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        // Fetch FX rates for non-SEK instruments
        let (latest_fx, previous_fx) = if instrument.currency.eq_ignore_ascii_case("SEK") {
            (None, None)
        } else {
            let latest = fx_rates::find_latest_on_or_before(
                &state.pool,
                &instrument.currency,
                "SEK",
                "YAHOO",
                valuation_date,
            )
            .await?
            .and_then(|row| {
                let date = row.date_value().ok()?;
                let rate = row.rate_decimal().ok()?;
                Some(FxCandidate {
                    date,
                    rate,
                    base: row.base,
                    quote: row.quote,
                })
            });

            let previous = if let Some(ref latest) = latest {
                fx_rates::find_previous_before(
                    &state.pool,
                    &instrument.currency,
                    "SEK",
                    "YAHOO",
                    latest.date,
                )
                .await?
                .and_then(|row| {
                    let date = row.date_value().ok()?;
                    let rate = row.rate_decimal().ok()?;
                    Some(FxCandidate {
                        date,
                        rate,
                        base: row.base,
                        quote: row.quote,
                    })
                })
            } else {
                None
            };

            (latest, previous)
        };

        let valued_holding = value_position(
            &position,
            &instrument.currency,
            valuation_date,
            latest_price.clone(),
            previous_price.clone(),
            latest_fx.clone(),
            previous_fx.clone(),
        );

        valued_holdings.push(valued_holding.clone());

        let gain_row = GainRow {
            instrument: InstrumentResponse::from_row(instrument)?,
            quantity: valued_holding.quantity,
            cost_basis_native: money_string(valued_holding.cost_basis_native),
            cost_basis_base: serialize_availability(&valued_holding.cost_basis_base, |v| {
                money_string(*v)
            }),
            latest_price: valued_holding.latest_price.as_ref().map(|snapshot| {
                PriceSnapshotResponse {
                    date: snapshot.date.format("%Y-%m-%d").to_string(),
                    close: money_string(snapshot.close),
                    currency: snapshot.currency.clone(),
                    freshness: serialize_freshness(snapshot.freshness),
                }
            }),
            previous_price: valued_holding.previous_price.as_ref().map(|snapshot| {
                PriceSnapshotResponse {
                    date: snapshot.date.format("%Y-%m-%d").to_string(),
                    close: money_string(snapshot.close),
                    currency: snapshot.currency.clone(),
                    freshness: serialize_freshness(snapshot.freshness),
                }
            }),
            latest_fx: valued_holding
                .latest_fx
                .as_ref()
                .map(|snapshot| FxSnapshotResponse {
                    date: snapshot.date.format("%Y-%m-%d").to_string(),
                    rate: snapshot.rate.to_string(),
                    base: snapshot.base.clone(),
                    quote: snapshot.quote.clone(),
                    freshness: serialize_freshness(snapshot.freshness),
                }),
            previous_fx: valued_holding
                .previous_fx
                .as_ref()
                .map(|snapshot| FxSnapshotResponse {
                    date: snapshot.date.format("%Y-%m-%d").to_string(),
                    rate: snapshot.rate.to_string(),
                    base: snapshot.base.clone(),
                    quote: snapshot.quote.clone(),
                    freshness: serialize_freshness(snapshot.freshness),
                }),
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
        base_currency: "SEK".to_string(),
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

fn serialize_availability<T, F>(value: &Availability<T>, f: F) -> AvailabilityResponse<String>
where
    F: Fn(&T) -> String,
{
    match value {
        Availability::Available(v) => AvailabilityResponse::Available { value: f(v) },
        Availability::Unavailable { reasons } => AvailabilityResponse::Unavailable {
            reasons: reasons.iter().map(serialize_valuation_reason).collect(),
        },
    }
}

fn serialize_valuation_reason(reason: &ValuationReason) -> String {
    match reason {
        ValuationReason::MissingPrice => "missing_price".to_string(),
        ValuationReason::MissingFx => "missing_fx".to_string(),
        ValuationReason::MissingPreviousClose => "missing_previous_close".to_string(),
        ValuationReason::MissingPreviousFx => "missing_previous_fx".to_string(),
        ValuationReason::StalePrice { trading_days } => {
            format!("stale_price_{}_days", trading_days)
        }
        ValuationReason::StaleFx { trading_days } => {
            format!("stale_fx_{}_days", trading_days)
        }
        ValuationReason::ZeroCostBasis => "zero_cost_basis".to_string(),
        ValuationReason::ZeroPreviousMarketValue => "zero_previous_market_value".to_string(),
        ValuationReason::BaseCostBasisUnavailable { .. } => {
            "base_cost_basis_unavailable".to_string()
        }
    }
}

fn serialize_freshness(freshness: DataFreshness) -> String {
    match freshness {
        DataFreshness::Fresh => "fresh".to_string(),
        DataFreshness::MinorStale { trading_days } => {
            format!("minor_stale_{}_days", trading_days)
        }
        DataFreshness::WarningStale { trading_days } => {
            format!("warning_stale_{}_days", trading_days)
        }
    }
}

fn money_string(value: Decimal) -> String {
    let raw = value.round_dp(2).to_string();
    match raw.split_once('.') {
        Some((whole, fractional)) => {
            let two_digits = match fractional.len() {
                0 => "00".to_owned(),
                1 => format!("{fractional}0"),
                _ => fractional[..2].to_owned(),
            };
            format!("{whole}.{two_digits}")
        }
        None => format!("{raw}.00"),
    }
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
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
}
