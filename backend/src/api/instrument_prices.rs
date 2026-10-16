use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::api::valuation::{
    money_string, serialize_availability, AvailabilityResponse, BASE_CURRENCY,
};
use crate::db::{fx_rates, instruments, transactions};
use crate::domain::{
    build_price_history, running_average_cost, AverageCostSegment, FxApplied, FxCandidate,
    PricePoint, RunningAverageCost,
};
use crate::market_data::effective_prices;
use crate::providers::BASE_FX_PROVIDER;
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
    running_average_cost: RunningAverageCostResponse,
}

/// The instrument's running average native cost over its whole ledger,
/// independent of the requested price window.
#[derive(Debug, Serialize)]
pub struct RunningAverageCostResponse {
    segments: Vec<AverageCostSegmentResponse>,
    /// The open position's native average as Holdings formats it; null when flat.
    current_average: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AverageCostSegmentResponse {
    steps: Vec<AverageCostStepResponse>,
    closed_on: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AverageCostStepResponse {
    date: String,
    average: String,
}

#[derive(Debug, Serialize)]
pub struct PricePointResponse {
    date: String,
    close: String,
    source: String,
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

/// Full stored precision for native values: prices, FX rates and plotted
/// averages (never money_string).
fn precise_string(value: Decimal) -> String {
    value.normalize().to_string()
}

/// Decimal places kept for plotted averages, bounding repeating quotients.
const AVERAGE_DECIMAL_PLACES: u32 = 10;

fn running_average_cost_response(running: &RunningAverageCost) -> RunningAverageCostResponse {
    RunningAverageCostResponse {
        segments: running.segments.iter().map(segment_response).collect(),
        current_average: running.current_average_native.map(money_string),
    }
}

fn segment_response(segment: &AverageCostSegment) -> AverageCostSegmentResponse {
    AverageCostSegmentResponse {
        steps: segment
            .steps
            .iter()
            .map(|step| AverageCostStepResponse {
                date: step.date.format("%Y-%m-%d").to_string(),
                average: precise_string(step.average_native.round_dp(AVERAGE_DECIMAL_PLACES)),
            })
            .collect(),
        closed_on: segment
            .closed_on
            .map(|date| date.format("%Y-%m-%d").to_string()),
    }
}

fn point_response(point: &PricePoint) -> PricePointResponse {
    PricePointResponse {
        date: point.date.format("%Y-%m-%d").to_string(),
        close: precise_string(point.close),
        source: point.source.as_str().to_owned(),
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

    let price_candidates =
        effective_prices::effective_series(&state.pool, id, &instrument.currency, from, to).await?;

    let is_base = instrument.currency.eq_ignore_ascii_case(BASE_CURRENCY);
    let fx_candidates: Vec<FxCandidate> = if is_base {
        Vec::new()
    } else {
        fx_rates::list_for_pair(
            &state.pool,
            &instrument.currency,
            BASE_CURRENCY,
            BASE_FX_PROVIDER,
        )
        .await?
        .into_iter()
        .map(fx_candidate)
        .collect::<Result<_, _>>()?
    };

    let points = build_price_history(&instrument.currency, &price_candidates, &fx_candidates);

    let ledger = transactions::ledger_for_instrument(&state.pool, id).await?;
    let running = running_average_cost(&ledger).map_err(|err| {
        ApiError::internal(format!(
            "price-history: running average cost derivation failed for instrument {id}: {} at transaction {}",
            err.code(),
            err.transaction_id()
        ))
    })?;

    Ok(Json(PriceHistoryResponse {
        instrument_id: id,
        currency: instrument.currency.clone(),
        base_currency: BASE_CURRENCY.to_string(),
        points: points.iter().map(point_response).collect(),
        running_average_cost: running_average_cost_response(&running),
    }))
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
    use crate::api::valuation::BASE_CURRENCY;
    use crate::db::{fx_rates, instruments, prices, provider_symbols, transactions};
    use crate::domain::TransactionKind;
    use crate::import::now_iso8601;
    use crate::providers::{BASE_FX_PROVIDER, PRICE_PROVIDER_PRECEDENCE};
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
                provider: PRICE_PROVIDER_PRECEDENCE[0],
                provider_symbol: "SYM".to_owned(),
                asset_class: None,
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
                provider: PRICE_PROVIDER_PRECEDENCE[0],
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
                provider: BASE_FX_PROVIDER,
                fetched_at: now_iso8601(),
            },
        )
        .await
        .expect("fx upsert should succeed");
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).expect("valid date")
    }

    /// Insert a ledger row directly through the repository, bypassing write
    /// validation. `quantity` is the signed position effect.
    async fn seed_transaction(
        state: &AppState,
        instrument_id: i64,
        kind: TransactionKind,
        trade_date: NaiveDate,
        quantity: i64,
        price: Option<rust_decimal::Decimal>,
    ) -> i64 {
        transactions::insert(
            &state.pool,
            &transactions::NewTransaction {
                instrument_id,
                kind,
                trade_date,
                quantity,
                price,
                dividend_per_share: None,
                currency: Some("USD".to_owned()),
                fx_rate_to_base: Some(dec!(10)),
                brokerage: None,
                note: None,
            },
        )
        .await
        .expect("transaction insert should succeed")
        .id
    }

    async fn buy(
        state: &AppState,
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: rust_decimal::Decimal,
    ) {
        seed_transaction(state, id, TransactionKind::Buy, date, qty, Some(price)).await;
    }

    async fn sell(
        state: &AppState,
        id: i64,
        date: NaiveDate,
        qty: i64,
        price: rust_decimal::Decimal,
    ) -> i64 {
        seed_transaction(state, id, TransactionKind::Sell, date, -qty, Some(price)).await
    }

    async fn running_average(state: &AppState, id: i64) -> serde_json::Value {
        let (status, body) = send(state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::OK);
        body["running_average_cost"].clone()
    }

    fn segments(block: &serde_json::Value) -> &Vec<serde_json::Value> {
        block["segments"].as_array().expect("segments array")
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
        assert_eq!(point["source"], "YAHOO");
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
        assert_eq!(point["source"], "YAHOO");
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

    #[tokio::test]
    async fn open_position_with_two_buys_has_one_open_segment() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;
        buy(&state, id, d(2025, 3, 2), 10, dec!(125)).await;

        let block = running_average(&state, id).await;
        assert_eq!(
            block,
            serde_json::json!({
                "segments": [{
                    "steps": [
                        { "date": "2025-01-10", "average": "100" },
                        { "date": "2025-03-02", "average": "112.5" }
                    ],
                    "closed_on": null
                }],
                "current_average": "112.50"
            })
        );
    }

    #[tokio::test]
    async fn midpoint_average_keeps_full_precision_and_matches_holdings_text() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 1, dec!(112.00)).await;
        buy(&state, id, d(2025, 1, 11), 1, dec!(112.25)).await;

        let block = running_average(&state, id).await;
        let steps = segments(&block)[0]["steps"].as_array().expect("steps");
        assert_eq!(steps.last().expect("last step")["average"], "112.125");
        // Midpoint to even, as Holdings formats it; half-up would give 112.13.
        assert_eq!(block["current_average"], "112.12");

        let (status, holdings) = send(&state, "/api/holdings").await;
        assert_eq!(status, StatusCode::OK);
        let holding = &holdings["holdings"].as_array().expect("holdings")[0];
        assert_eq!(holding["instrument"]["id"], id);
        assert_eq!(block["current_average"], holding["average_cost_native"]);
    }

    #[tokio::test]
    async fn close_and_reopen_starts_a_fresh_segment() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;
        buy(&state, id, d(2025, 2, 10), 10, dec!(120)).await;
        sell(&state, id, d(2025, 6, 20), 20, dec!(130)).await;
        buy(&state, id, d(2025, 9, 1), 5, dec!(95.1)).await;

        let block = running_average(&state, id).await;
        let segs = segments(&block);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0]["closed_on"], "2025-06-20");
        assert_eq!(segs[0]["steps"][1]["average"], "110");
        assert!(segs[1]["closed_on"].is_null());
        assert_eq!(
            segs[1]["steps"],
            serde_json::json!([{ "date": "2025-09-01", "average": "95.1" }])
        );
        // From the reopened position only, not blended with the first period.
        assert_eq!(block["current_average"], "95.10");
    }

    #[tokio::test]
    async fn sold_out_instrument_has_only_closed_segments_and_no_current_average() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;
        sell(&state, id, d(2025, 3, 1), 4, dec!(110)).await;
        sell(&state, id, d(2025, 4, 1), 6, dec!(120)).await;

        let block = running_average(&state, id).await;
        let segs = segments(&block);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0]["closed_on"], "2025-04-01");
        assert!(block["current_average"].is_null());
    }

    #[tokio::test]
    async fn split_adjusts_earlier_steps() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(300)).await;
        // 4:1 split: 10 shares become 40.
        seed_transaction(&state, id, TransactionKind::Split, d(2025, 2, 1), 30, None).await;
        buy(&state, id, d(2025, 3, 1), 40, dec!(85)).await;

        let block = running_average(&state, id).await;
        let steps = segments(&block)[0]["steps"].as_array().expect("steps");
        assert_eq!(steps[0]["average"], "75"); // 300 / 4
        assert_eq!(steps[1]["average"], "80"); // (3000 + 3400) / 80
        assert_eq!(block["current_average"], "80.00");
    }

    #[tokio::test]
    async fn from_to_window_does_not_trim_segments() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;
        sell(&state, id, d(2025, 2, 10), 10, dec!(110)).await;
        buy(&state, id, d(2025, 9, 1), 10, dec!(90)).await;

        let full = running_average(&state, id).await;
        let (status, body) = send(
            &state,
            &format!("/api/instruments/{id}/prices?from=2025-08-01&to=2025-08-31"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["running_average_cost"], full);
        assert_eq!(segments(&full).len(), 2);
    }

    #[tokio::test]
    async fn instrument_without_transactions_has_no_segments() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;

        let block = running_average(&state, id).await;
        assert_eq!(
            block,
            serde_json::json!({ "segments": [], "current_average": null })
        );
    }

    #[tokio::test]
    async fn missing_or_disabled_mapping_still_serves_segments() {
        let state = AppState::for_tests().await;
        let unmapped = instrument(&state, "MSFT", "USD").await;
        let disabled = instrument(&state, "AAPL", "USD").await;
        enable_mapping(&state, disabled, false).await;
        for id in [unmapped, disabled] {
            seed_price(&state, id, d(2025, 1, 10), dec!(100), "USD").await;
            buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;

            let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["points"].as_array().expect("points").len(), 0);
            assert_eq!(segments(&body["running_average_cost"]).len(), 1);
            assert_eq!(body["running_average_cost"]["current_average"], "100.00");
        }
    }

    #[tokio::test]
    async fn invalid_stored_ledger_is_internal_error_naming_instrument_and_transaction() {
        let state = AppState::for_tests().await;
        let id = instrument(&state, "MSFT", "USD").await;
        buy(&state, id, d(2025, 1, 10), 10, dec!(100)).await;
        let bad = sell(&state, id, d(2025, 2, 10), 11, dec!(110)).await;

        let (status, body) = send(&state, &format!("/api/instruments/{id}/prices")).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "internal_error");
        assert_eq!(
            body["error"]["message"],
            format!(
                "price-history: running average cost derivation failed for instrument {id}: \
                 sell_exceeds_position at transaction {bad}"
            )
        );
    }
}
