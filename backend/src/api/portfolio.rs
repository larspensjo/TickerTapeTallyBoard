use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::valuation::{money_string, BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
use crate::db::{fx_rates, instruments, prices, provider_symbols, transactions};
use crate::domain::{
    build_value_history, FxCandidate, PriceCandidate, ValueHistoryInstrument, ValueHistoryPoint,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ValueHistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ValueHistoryResponse {
    base_currency: String,
    points: Vec<ValueHistoryPointResponse>,
}

#[derive(Debug, Serialize)]
pub struct ValueHistoryPointResponse {
    date: String,
    value_base: String,
    incomplete: bool,
    included_count: usize,
    excluded_count: usize,
}

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", format!("invalid {field}: {s}")))
}

fn point_response(point: &ValueHistoryPoint) -> ValueHistoryPointResponse {
    ValueHistoryPointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        value_base: money_string(point.value_base),
        incomplete: point.incomplete,
        included_count: point.included_count,
        excluded_count: point.excluded_count,
    }
}

pub async fn value_history(
    State(state): State<AppState>,
    Query(query): Query<ValueHistoryQuery>,
) -> Result<Json<ValueHistoryResponse>, ApiError> {
    let from = query
        .from
        .as_deref()
        .map(|s| parse_date(s, "from"))
        .transpose()?;
    let to = query
        .to
        .as_deref()
        .map(|s| parse_date(s, "to"))
        .transpose()?;
    if let (Some(from), Some(to)) = (from, to) {
        if from > to {
            return Err(ApiError::bad_request(
                "invalid_date_range",
                "from must not be after to",
            ));
        }
    }

    let instruments_list = instruments::list(&state.pool).await?;

    let transaction_rows = transactions::all_for_holdings(&state.pool).await?;
    let mut ledgers: BTreeMap<i64, Vec<_>> = BTreeMap::new();
    for row in &transaction_rows {
        ledgers
            .entry(row.instrument_id)
            .or_default()
            .push(row.to_ledger()?);
    }

    let mut inputs = Vec::new();
    for instrument in &instruments_list {
        let ledger = ledgers.remove(&instrument.id).unwrap_or_default();
        if ledger.is_empty() {
            continue;
        }

        let mapping = provider_symbols::find_by_instrument_provider(
            &state.pool,
            instrument.id,
            PRICE_PROVIDER,
        )
        .await?;
        let mapping_enabled = mapping.as_ref().is_some_and(|m| m.enabled);

        let prices: Vec<PriceCandidate> = if mapping_enabled {
            prices::list_for_instrument_in_range(
                &state.pool,
                instrument.id,
                PRICE_PROVIDER,
                None,
                None,
            )
            .await?
            .into_iter()
            .map(|row| price_candidate(instrument, row))
            .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };

        let is_base = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
        let fx_rates: Vec<FxCandidate> = if is_base {
            Vec::new()
        } else {
            fx_rates::list_for_pair(
                &state.pool,
                &instrument.currency,
                BASE_CURRENCY,
                FX_PROVIDER,
            )
            .await?
            .into_iter()
            .map(fx_candidate)
            .collect::<Result<_, _>>()?
        };

        inputs.push(ValueHistoryInstrument {
            native_currency: instrument.currency.clone(),
            ledger,
            prices,
            fx_rates,
        });
    }

    let points = build_value_history(&inputs, from, to)
        .map_err(|err| ApiError::internal(format!("value-history derivation failed: {err:?}")))?;

    Ok(Json(ValueHistoryResponse {
        base_currency: BASE_CURRENCY.to_string(),
        points: points.iter().map(point_response).collect(),
    }))
}

fn price_candidate(
    instrument: &instruments::InstrumentRow,
    row: prices::PriceRow,
) -> Result<PriceCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable date in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    let close = row.close_decimal().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable close in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    Ok(PriceCandidate {
        date,
        close,
        currency: row.currency,
    })
}

fn fx_candidate(row: fx_rates::FxRateRow) -> Result<FxCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable date in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    let rate = row.rate_decimal().map_err(|e| {
        ApiError::internal(format!(
            "value-history: undecodable rate in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    Ok(FxCandidate {
        date,
        rate,
        base: row.base,
        quote: row.quote,
    })
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::api::valuation::{BASE_CURRENCY, FX_PROVIDER, PRICE_PROVIDER};
    use crate::db::{fx_rates, instruments, prices, provider_symbols};
    use crate::import::now_iso8601;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
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
            .expect("completes");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let value = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json")
        };
        (status, value)
    }

    async fn instrument(state: &AppState, symbol: &str, currency: &str) -> i64 {
        let (row, _) = instruments::upsert(
            &state.pool,
            &instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: "STO".to_owned(),
                name: symbol.to_owned(),
                kind: "STOCK".to_owned(),
                currency: currency.to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument");
        row.id
    }

    async fn enable_mapping(state: &AppState, instrument_id: i64, enabled: bool) {
        let now = now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "SYM".to_owned(),
                currency: None,
                enabled,
                created_at: now.clone(),
                updated_at: now,
            },
        )
        .await
        .expect("mapping");
    }

    async fn seed_price(
        state: &AppState,
        instrument_id: i64,
        date: NaiveDate,
        close: rust_decimal::Decimal,
        currency: &str,
    ) {
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id,
                provider: PRICE_PROVIDER.to_owned(),
                provider_symbol: "SYM".to_owned(),
                date,
                close,
                currency: currency.to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("price");
    }

    async fn seed_fx(state: &AppState, date: NaiveDate, rate: rust_decimal::Decimal) {
        fx_rates::upsert(
            &state.pool,
            &fx_rates::NewFxRate {
                base: "USD".to_owned(),
                quote: BASE_CURRENCY.to_owned(),
                date,
                rate,
                provider: FX_PROVIDER.to_owned(),
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("fx");
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("date")
    }

    #[tokio::test]
    async fn empty_portfolio_returns_empty_points() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["base_currency"], "SEK");
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn from_after_to_is_invalid_date_range() {
        let state = AppState::for_tests().await;
        let (status, body) = send(
            &state,
            "GET",
            "/api/portfolio/value-history?from=2026-06-12&to=2026-06-11",
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date_range");
    }

    #[tokio::test]
    async fn sek_holding_produces_monotonic_value_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, true).await;
        let (create_status, _) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-01-02","quantity":10,"price":"100","currency":BASE_CURRENCY}),
        )
        .await;
        assert_eq!(create_status, StatusCode::CREATED);
        seed_price(&state, id, d(2026, 1, 2), dec!(100), BASE_CURRENCY).await;
        seed_price(&state, id, d(2026, 1, 5), dec!(110), BASE_CURRENCY).await;

        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let points = body["points"].as_array().expect("points");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["date"], "2026-01-02");
        assert_eq!(points[0]["value_base"], "1000.00");
        assert_eq!(points[0]["incomplete"], false);
        assert_eq!(points[0]["included_count"], 1);
        assert_eq!(points[1]["value_base"], "1100.00");
    }

    #[tokio::test]
    async fn disabled_mapping_excludes_cached_prices() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, false).await;
        let (create_status, _) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-01-02","quantity":10,"price":"100","currency":BASE_CURRENCY}),
        )
        .await;
        assert_eq!(create_status, StatusCode::CREATED);
        seed_price(&state, id, d(2026, 1, 2), dec!(100), BASE_CURRENCY).await;

        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn usd_holding_uses_fx_carry_forward() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, true).await;
        let (create_status, _) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":id,"type":"Buy","trade_date":"2026-01-02","quantity":10,"price":"100","currency":"USD","fx_rate_to_base":"10"}),
        )
        .await;
        assert_eq!(create_status, StatusCode::CREATED);
        seed_price(&state, id, d(2026, 1, 2), dec!(100), "USD").await;
        seed_fx(&state, d(2026, 1, 2), dec!(10)).await;
        seed_fx(&state, d(2026, 1, 6), dec!(11)).await;

        let (status, body) = send(&state, "GET", "/api/portfolio/value-history", json!({})).await;
        assert_eq!(status, StatusCode::OK);
        let points = body["points"].as_array().expect("points");
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["value_base"], "10000.00");
        assert_eq!(points[1]["date"], "2026-01-06");
        assert_eq!(points[1]["value_base"], "11000.00");
    }
}
