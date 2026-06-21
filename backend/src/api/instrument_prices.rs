use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::valuation::{
    money_string, serialize_availability, AvailabilityResponse, BASE_CURRENCY, FX_PROVIDER,
    PRICE_PROVIDER,
};
use crate::db::{fx_rates, instruments, prices, provider_symbols};
use crate::domain::{build_price_history, FxApplied, FxCandidate, PriceCandidate, PricePoint};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct PriceHistoryQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PriceHistoryResponse {
    instrument_id: i64,
    currency: String,
    base_currency: String,
    points: Vec<PricePointResponse>,
}

#[derive(Debug, Serialize)]
pub struct PricePointResponse {
    date: String,
    close: String,
    close_base: AvailabilityResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    fx: Option<FxAppliedResponse>,
}

#[derive(Debug, Serialize)]
pub struct FxAppliedResponse {
    rate: String,
    date: String,
}

fn parse_date(s: &str, field: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("invalid_date", format!("invalid {field}: {s}")))
}

/// Full stored precision for native price and FX rate (never money_string).
fn precise_string(value: Decimal) -> String {
    value.normalize().to_string()
}

fn point_response(point: &PricePoint) -> PricePointResponse {
    PricePointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        close: precise_string(point.close),
        close_base: serialize_availability(&point.close_base, |v| money_string(*v)),
        fx: point
            .fx
            .as_ref()
            .map(|FxApplied { rate, date }| FxAppliedResponse {
                rate: precise_string(*rate),
                date: date.format("%Y-%m-%d").to_string(),
            }),
    }
}

pub async fn list(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PriceHistoryQuery>,
) -> Result<Json<PriceHistoryResponse>, ApiError> {
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

    let instrument = instruments::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;

    let mapping =
        provider_symbols::find_by_instrument_provider(&state.pool, id, PRICE_PROVIDER).await?;
    let mapping_enabled = mapping.as_ref().is_some_and(|m| m.enabled);

    let points = if mapping_enabled {
        let price_rows =
            prices::list_for_instrument_in_range(&state.pool, id, PRICE_PROVIDER, from, to).await?;
        // Decode failures are internal invariant violations, not missing data:
        // propagate them as `ApiError::internal` instead of silently dropping rows.
        let price_candidates: Vec<PriceCandidate> = price_rows
            .into_iter()
            .map(|row| price_candidate(&instrument, row))
            .collect::<Result<_, _>>()?;

        let is_base = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
        let fx_candidates: Vec<FxCandidate> = if is_base {
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

        build_price_history(&instrument.currency, &price_candidates, &fx_candidates)
    } else {
        Vec::new()
    };

    Ok(Json(PriceHistoryResponse {
        instrument_id: id,
        currency: instrument.currency.clone(),
        base_currency: BASE_CURRENCY.to_string(),
        points: points.iter().map(point_response).collect(),
    }))
}

/// Map a price row into a `PriceCandidate`. A decode failure (date or close) is
/// an internal invariant violation per `db/mod.rs`, not missing data, so it
/// propagates as `ApiError::internal` with instrument id, row id, and field
/// context rather than producing a `200` with silently missing points. A row
/// whose currency differs from the instrument's is logged here (the
/// repository-mapping boundary where the instrument is known) and kept; the
/// builder drops mismatched rows.
fn price_candidate(
    instrument: &instruments::InstrumentRow,
    row: prices::PriceRow,
) -> Result<PriceCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable date in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    let close = row.close_decimal().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable close in price row {} for instrument {}: {e}",
            row.id, instrument.id
        ))
    })?;
    if !row.currency.eq_ignore_ascii_case(&instrument.currency) {
        crate::engine_warn!(
            "price-history currency mismatch for instrument {}: row currency {:?} != instrument {:?}",
            instrument.id,
            row.currency,
            instrument.currency
        );
    }
    Ok(PriceCandidate {
        date,
        close,
        currency: row.currency,
    })
}

fn fx_candidate(row: fx_rates::FxRateRow) -> Result<FxCandidate, ApiError> {
    let date = row.date_value().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable date in fx row {} ({}->{}): {e}",
            row.id, row.base, row.quote
        ))
    })?;
    let rate = row.rate_decimal().map_err(|e| {
        ApiError::internal(format!(
            "price-history: undecodable rate in fx row {} ({}->{}): {e}",
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
    use tower::ServiceExt;

    async fn send(state: &AppState, uri: &str) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
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

    async fn instrument(state: &AppState, symbol: &str, currency: &str) -> i64 {
        let (row, _) = instruments::upsert(
            &state.pool,
            &instruments::NewInstrument {
                symbol: symbol.to_owned(),
                exchange: "NASDAQ".to_owned(),
                name: symbol.to_owned(),
                kind: "STOCK".to_owned(),
                currency: currency.to_owned(),
                isin: None,
            },
        )
        .await
        .expect("instrument upsert should succeed");
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
        .expect("mapping upsert should succeed");
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
        .expect("price upsert should succeed");
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
        .expect("fx upsert should succeed");
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    #[tokio::test]
    async fn non_sek_happy_path_converts_with_full_precision_close() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, true).await;
        seed_fx(&state, d(2026, 6, 10), dec!(10.4731)).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(110.5034), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["currency"], "USD");
        assert_eq!(body["base_currency"], "SEK");
        let point = &body["points"][0];
        assert_eq!(point["date"], "2026-06-10");
        assert_eq!(point["close"], "110.5034"); // full precision, not money_string
        assert_eq!(point["close_base"]["status"], "available");
        // 110.5034 * 10.4731 = 1157.31315854, serialized through money_string (2 dp).
        assert_eq!(point["close_base"]["value"], "1157.31");
        assert_eq!(point["fx"]["rate"], "10.4731");
        assert_eq!(point["fx"]["date"], "2026-06-10");
    }

    #[tokio::test]
    async fn sek_instrument_uses_identity_and_omits_fx() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "ERICB", BASE_CURRENCY).await;
        enable_mapping(&state, id, true).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(42.5), BASE_CURRENCY).await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        let point = &body["points"][0];
        assert_eq!(point["close_base"]["value"], "42.50");
        assert!(point.get("fx").is_none());
    }

    #[tokio::test]
    async fn unknown_instrument_is_404() {
        let state = AppState::for_tests().await;
        let (status, body) = send(&state, "/api/instruments/999/prices").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn from_after_to_is_invalid_date_range() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=2026-06-12&to=2026-06-11"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date_range");
    }

    #[tokio::test]
    async fn malformed_from_returns_invalid_date() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;

        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=not-a-date"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "invalid_date");
    }

    #[tokio::test]
    async fn missing_mapping_returns_empty_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        // No mapping at all; seed a price that must NOT be served.
        seed_price(&state, id, d(2026, 6, 10), dec!(100), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn disabled_mapping_returns_empty_points() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, false).await;
        seed_price(&state, id, d(2026, 6, 10), dec!(100), "USD").await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["points"].as_array().expect("points").len(), 0);
    }

    #[tokio::test]
    async fn carry_forward_when_from_is_after_only_fx_rate() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        enable_mapping(&state, id, true).await;
        // Only FX rate is dated before `from`; the full FX set must still be loaded.
        seed_fx(&state, d(2026, 6, 1), dec!(10)).await;
        seed_price(&state, id, d(2026, 6, 20), dec!(100), "USD").await;

        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=2026-06-15&to=2026-06-25"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let point = &body["points"][0];
        assert_eq!(point["date"], "2026-06-20");
        assert_eq!(point["close_base"]["value"], "1000.00");
        assert_eq!(point["fx"]["date"], "2026-06-01"); // prior rate's date proves carry-forward
    }
}
