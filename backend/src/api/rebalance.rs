use std::collections::BTreeMap;

use axum::extract::{Query, State};
use axum::Json;
use chrono::Local;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{money_string, serialize_freshness, staler_freshness, BASE_CURRENCY};
use crate::api::valued_holdings::{load_valued_open_holdings, ValuedOpenHolding};
use crate::domain::{
    build_ladder, pool_membership, DataFreshness, PlannedTrade, RebalanceCandidate,
    RebalanceLadder, RebalanceRung, TradeSide, UntradedCandidate,
};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RebalanceQuery {
    pub amount: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RebalanceResponse {
    pub amount_base: String,
    pub base_currency: &'static str,
    pub plan: RebalancePlanResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebalancePlanResponse {
    Available {
        pool_value_base: String,
        candidate_count: usize,
        rungs: Vec<RebalanceRungResponse>,
    },
    Unavailable {
        reasons: Vec<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct RebalanceRungResponse {
    pub selected_count: usize,
    pub effective_trade_count: usize,
    pub trades: Vec<RebalanceTradeResponse>,
    pub untraded: Vec<RebalanceUntradedResponse>,
    pub achieved_net_base: String,
    pub residual_base: String,
    pub coverage_percent: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RebalanceTradeResponse {
    pub instrument: InstrumentResponse,
    pub side: RebalanceTradeSideResponse,
    pub shares: i64,
    pub price_base: String,
    pub amount_base: String,
    pub freshness: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RebalanceTradeSideResponse {
    Buy,
    Sell,
}

#[derive(Debug, Serialize)]
pub struct RebalanceUntradedResponse {
    pub instrument: InstrumentResponse,
    pub reason: String,
}

#[derive(Debug)]
struct PreparedCandidate {
    instrument: crate::db::instruments::InstrumentRow,
    candidate: RebalanceCandidate,
    price_freshness: DataFreshness,
    fx_freshness: DataFreshness,
}

pub async fn handler(
    State(state): State<AppState>,
    Query(query): Query<RebalanceQuery>,
) -> Result<Json<RebalanceResponse>, ApiError> {
    let amount = parse_amount(query.amount.as_deref())?;
    let valuation_date = Local::now().naive_local().date();
    let valued_holdings = load_valued_open_holdings(&state.pool, valuation_date).await?;
    let prepared = assemble_candidates(valued_holdings)?;
    let candidates: Vec<RebalanceCandidate> = prepared
        .iter()
        .map(|entry| entry.candidate.clone())
        .collect();

    let ladder = match build_ladder(&candidates, amount) {
        Ok(ladder) => ladder,
        Err(reason) => {
            return Ok(Json(RebalanceResponse {
                amount_base: money_string(amount),
                base_currency: BASE_CURRENCY,
                plan: RebalancePlanResponse::Unavailable {
                    reasons: vec![reason.as_str().to_owned()],
                },
            }));
        }
    };

    let plan = serialize_available_plan(&prepared, ladder)?;

    Ok(Json(RebalanceResponse {
        amount_base: money_string(amount),
        base_currency: BASE_CURRENCY,
        plan,
    }))
}

fn parse_amount(amount: Option<&str>) -> Result<Decimal, ApiError> {
    let amount = amount.ok_or_else(|| {
        ApiError::bad_request("invalid_amount", "amount query parameter is required")
    })?;
    Decimal::from_str(amount.trim())
        .map_err(|_| ApiError::bad_request("invalid_amount", "amount must be a decimal"))
}

fn assemble_candidates(
    valued_holdings: Vec<ValuedOpenHolding>,
) -> Result<Vec<PreparedCandidate>, ApiError> {
    let mut prepared = Vec::new();

    for holding in valued_holdings {
        let market_value_state = holding.market_value_state();
        let Some((weight, market_value_base)) =
            pool_membership(holding.conviction, market_value_state)
        else {
            continue;
        };

        let (price_base, price_freshness, fx_freshness) = {
            holding.valuation.as_ref().ok_or_else(|| {
                log_internal_inconsistency(
                    &holding,
                    "candidate had eligible pool membership without valuation",
                )
            })?;
            let price_snapshot = holding.latest_price_snapshot().ok_or_else(|| {
                log_internal_inconsistency(
                    &holding,
                    "candidate had eligible pool membership without latest price",
                )
            })?;
            let fx_snapshot = holding.latest_fx_snapshot().ok_or_else(|| {
                log_internal_inconsistency(
                    &holding,
                    "candidate had eligible pool membership without latest FX",
                )
            })?;

            (
                price_snapshot.close * fx_snapshot.rate,
                price_snapshot.freshness,
                fx_snapshot.freshness,
            )
        };
        let candidate = RebalanceCandidate {
            instrument_id: holding.instrument.id,
            weight,
            market_value_base,
            price_base,
            held_quantity: holding.position.quantity,
        };

        prepared.push(PreparedCandidate {
            instrument: holding.instrument,
            candidate,
            price_freshness,
            fx_freshness,
        });
    }

    Ok(prepared)
}

fn log_internal_inconsistency(holding: &ValuedOpenHolding, reason: &str) -> ApiError {
    crate::engine_error!(
        "rebalance candidate inconsistency for instrument {} ({} / {}): {}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        reason
    );
    ApiError::internal(format!(
        "rebalance candidate inconsistency for instrument {}",
        holding.instrument.id
    ))
}

fn serialize_available_plan(
    prepared: &[PreparedCandidate],
    ladder: RebalanceLadder,
) -> Result<RebalancePlanResponse, ApiError> {
    let prepared_by_id: BTreeMap<i64, &PreparedCandidate> = prepared
        .iter()
        .map(|entry| (entry.candidate.instrument_id, entry))
        .collect();

    let rungs = ladder
        .rungs
        .iter()
        .map(|rung| serialize_rung(rung, &prepared_by_id))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RebalancePlanResponse::Available {
        pool_value_base: money_string(ladder.pool_value_base),
        candidate_count: ladder.candidate_count,
        rungs,
    })
}

fn serialize_rung(
    rung: &RebalanceRung,
    prepared_by_id: &BTreeMap<i64, &PreparedCandidate>,
) -> Result<RebalanceRungResponse, ApiError> {
    let trades = rung
        .trades
        .iter()
        .map(|trade| serialize_trade(trade, prepared_by_id))
        .collect::<Result<Vec<_>, _>>()?;
    let untraded = rung
        .untraded
        .iter()
        .map(|candidate| serialize_untraded(candidate, prepared_by_id))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RebalanceRungResponse {
        selected_count: rung.selected_count,
        effective_trade_count: rung.effective_trade_count,
        trades,
        untraded,
        achieved_net_base: money_string(rung.achieved_net_base),
        residual_base: money_string(rung.residual_base),
        coverage_percent: rung.coverage_percent.map(|value| format!("{value:.2}")),
    })
}

fn serialize_trade(
    trade: &PlannedTrade,
    prepared_by_id: &BTreeMap<i64, &PreparedCandidate>,
) -> Result<RebalanceTradeResponse, ApiError> {
    let prepared = lookup_prepared(prepared_by_id, trade.instrument_id)?;
    let freshness = serialize_freshness(staler_freshness(
        prepared.price_freshness,
        prepared.fx_freshness,
    ));

    Ok(RebalanceTradeResponse {
        instrument: InstrumentResponse::from_row(&prepared.instrument)?,
        side: trade.side.into(),
        shares: trade.shares,
        price_base: money_string(trade.price_base),
        amount_base: money_string(trade.amount_base),
        freshness,
    })
}

fn serialize_untraded(
    untraded: &UntradedCandidate,
    prepared_by_id: &BTreeMap<i64, &PreparedCandidate>,
) -> Result<RebalanceUntradedResponse, ApiError> {
    let prepared = lookup_prepared(prepared_by_id, untraded.instrument_id)?;
    Ok(RebalanceUntradedResponse {
        instrument: InstrumentResponse::from_row(&prepared.instrument)?,
        reason: untraded.reason.as_str().to_owned(),
    })
}

fn lookup_prepared<'a>(
    prepared_by_id: &'a BTreeMap<i64, &PreparedCandidate>,
    instrument_id: i64,
) -> Result<&'a PreparedCandidate, ApiError> {
    prepared_by_id.get(&instrument_id).copied().ok_or_else(|| {
        crate::engine_error!(
            "rebalance response lookup missing prepared candidate for instrument {}",
            instrument_id
        );
        ApiError::internal(format!(
            "rebalance response lookup missing candidate for instrument {}",
            instrument_id
        ))
    })
}

impl From<TradeSide> for RebalanceTradeSideResponse {
    fn from(side: TradeSide) -> Self {
        match side {
            TradeSide::Buy => Self::Buy,
            TradeSide::Sell => Self::Sell,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::api::test_support::{
        holdings_by_symbol, instrument, seed_valued, seed_valued_at, send, set_conviction,
        SeededHoldingSpec,
    };
    use crate::db::provider_symbols;
    use crate::state::AppState;
    use axum::http::StatusCode;
    use chrono::{Duration, Local};
    use rust_decimal::Decimal;
    use serde_json::{json, Value};
    use std::str::FromStr;

    fn assert_plan_status(body: &Value, status: &str) {
        assert_eq!(body["plan"]["status"], status);
    }

    #[tokio::test]
    async fn happy_path_builds_the_expected_ladder_and_supports_positive_and_negative_offsets() {
        let state = AppState::for_tests().await;
        seed_valued(&state, "AAA", 100, "1000", "Low").await;
        seed_valued(&state, "BBB", 300, "1000", "Medium").await;
        seed_valued(&state, "CCC", 300, "1000", "High").await;

        let (status, body) = send(&state, "GET", "/api/rebalance?amount=0", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["amount_base"], "0.00");
        assert_eq!(body["base_currency"], "SEK");
        assert_plan_status(&body, "available");
        assert_eq!(body["plan"]["pool_value_base"], "700000.00");
        assert_eq!(body["plan"]["candidate_count"], 3);

        let rungs = body["plan"]["rungs"].as_array().expect("rungs");
        assert_eq!(rungs.len(), 3);

        assert_eq!(rungs[0]["selected_count"], 1);
        assert_eq!(rungs[0]["effective_trade_count"], 1);
        assert_eq!(rungs[0]["trades"][0]["instrument"]["symbol"], "BBB");
        assert_eq!(rungs[0]["trades"][0]["side"], "sell");
        assert_eq!(rungs[0]["trades"][0]["shares"], 100);
        assert_eq!(rungs[0]["trades"][0]["price_base"], "1000.00");
        assert_eq!(rungs[0]["trades"][0]["amount_base"], "100000.00");
        assert_eq!(rungs[0]["trades"][0]["freshness"], "fresh");
        assert_eq!(rungs[0]["achieved_net_base"], "-100000.00");
        assert_eq!(rungs[0]["residual_base"], "100000.00");
        assert_eq!(rungs[0]["coverage_percent"], "50.00");

        assert_eq!(rungs[1]["selected_count"], 2);
        assert_eq!(rungs[1]["effective_trade_count"], 2);
        assert_eq!(rungs[1]["trades"].as_array().expect("trades").len(), 2);
        assert_eq!(rungs[1]["achieved_net_base"], "0.00");
        assert_eq!(rungs[1]["residual_base"], "0.00");
        assert_eq!(rungs[1]["coverage_percent"], "100.00");

        assert_eq!(rungs[2]["selected_count"], 3);
        assert_eq!(rungs[2]["effective_trade_count"], 2);
        assert_eq!(rungs[2]["trades"].as_array().expect("trades").len(), 2);
        assert_eq!(rungs[2]["untraded"][0]["instrument"]["symbol"], "AAA");
        assert_eq!(rungs[2]["untraded"][0]["reason"], "on_target");
        assert_eq!(rungs[2]["achieved_net_base"], "0.00");
        assert_eq!(rungs[2]["residual_base"], "0.00");
        assert_eq!(rungs[2]["coverage_percent"], "100.00");

        let (status, positive) =
            send(&state, "GET", "/api/rebalance?amount=50000", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&positive, "available");
        assert_eq!(positive["plan"]["rungs"][0]["trades"][0]["side"], "buy");
        let positive_achieved = Decimal::from_str(
            positive["plan"]["rungs"][0]["achieved_net_base"]
                .as_str()
                .expect("achieved net base"),
        )
        .expect("achieved net base decimal");
        assert!(positive_achieved > Decimal::ZERO);

        let (status, negative) =
            send(&state, "GET", "/api/rebalance?amount=-50000", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&negative, "available");
        assert_eq!(negative["plan"]["rungs"][0]["trades"][0]["side"], "sell");
        let negative_achieved = Decimal::from_str(
            negative["plan"]["rungs"][0]["achieved_net_base"]
                .as_str()
                .expect("achieved net base"),
        )
        .expect("achieved net base decimal");
        assert!(negative_achieved < Decimal::ZERO);
    }

    #[tokio::test]
    async fn ineligible_holdings_stay_out_of_the_candidate_pool() {
        let state = AppState::for_tests().await;
        seed_valued(&state, "AAA", 100, "1000", "Low").await;
        seed_valued(&state, "BBB", 300, "1000", "Medium").await;
        seed_valued(&state, "CCC", 300, "1000", "High").await;

        seed_valued(&state, "OTHER", 100, "1000", "Other").await;

        let missing_id = instrument(&state, "MISS", "STO", "SEK").await;
        let now = crate::import::now_iso8601();
        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: missing_id,
                provider: crate::api::valuation::PRICE_PROVIDER.to_owned(),
                provider_symbol: "MISS".to_owned(),
                currency: Some("SEK".to_owned()),
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )
        .await
        .expect("provider symbol");
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":missing_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"1000","currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;
        set_conviction(&state, missing_id, "High").await;

        let no_map = instrument(&state, "NOMAP", "STO", "SEK").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":no_map,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"1000","currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;
        set_conviction(&state, no_map, "Medium").await;

        let closed = seed_valued(&state, "CLOSED", 100, "1000", "Low").await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":closed,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":100,"price":"1000","currency":"SEK","fx_rate_to_base":"1"}),
        )
        .await;

        let holdings = holdings_by_symbol(&state).await;
        let eligible_count = holdings
            .values()
            .filter(|holding| {
                matches!(
                    holding["conviction_target"]["status"].as_str(),
                    Some("above") | Some("below") | Some("on_target")
                )
            })
            .count();
        assert_eq!(eligible_count, 3);
        assert_eq!(
            holdings["OTHER"]["conviction_target"]["status"],
            "no_target"
        );
        assert_eq!(
            holdings["MISS"]["conviction_target"]["status"],
            "excluded_unavailable"
        );
        assert_eq!(
            holdings["NOMAP"]["conviction_target"]["status"],
            "excluded_unavailable"
        );
        assert!(!holdings.contains_key("CLOSED"));

        let (status, body) = send(&state, "GET", "/api/rebalance?amount=0", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&body, "available");
        assert_eq!(body["plan"]["candidate_count"], eligible_count);
    }

    #[tokio::test]
    async fn invalid_amount_returns_a_bad_request() {
        let state = AppState::for_tests().await;

        for uri in ["/api/rebalance", "/api/rebalance?amount=abc"] {
            let (status, body) = send(&state, "GET", uri, Value::Null).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["error"]["code"], "invalid_amount", "{uri}");
        }
    }

    #[tokio::test]
    async fn empty_pool_and_offset_exceeds_pool_are_reported_as_unavailable() {
        let state = AppState::for_tests().await;
        let (status, empty) = send(&state, "GET", "/api/rebalance?amount=0", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&empty, "unavailable");
        assert_eq!(empty["plan"]["reasons"], json!(["empty_pool"]));

        let state = AppState::for_tests().await;
        seed_valued(&state, "AAA", 100, "1000", "Low").await;
        let (status, too_negative) =
            send(&state, "GET", "/api/rebalance?amount=-100000", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&too_negative, "unavailable");
        assert_eq!(
            too_negative["plan"]["reasons"],
            json!(["offset_exceeds_pool"])
        );
    }

    #[tokio::test]
    async fn freshness_uses_the_staler_of_price_and_fx() {
        let stale_price_state = AppState::for_tests().await;
        let today = Local::now().naive_local().date();
        let stale = today - Duration::days(10);
        seed_valued_at(
            &stale_price_state,
            SeededHoldingSpec {
                symbol: "STALE",
                exchange: "STO",
                currency: "SEK",
                quantity: 100,
                price: "1000",
                conviction: "Low",
                price_date: stale,
                fx_date: None,
                fx_rate: None,
            },
        )
        .await;

        let (status, stale_body) = send(
            &stale_price_state,
            "GET",
            "/api/rebalance?amount=1000",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&stale_body, "available");
        let freshness = stale_body["plan"]["rungs"][0]["trades"][0]["freshness"]
            .as_str()
            .expect("freshness");
        assert!(freshness.starts_with("warning_stale_"), "{freshness}");

        let foreign_state = AppState::for_tests().await;
        seed_valued_at(
            &foreign_state,
            SeededHoldingSpec {
                symbol: "FXSTALE",
                exchange: "NASDAQ",
                currency: "USD",
                quantity: 100,
                price: "1000",
                conviction: "Low",
                price_date: today,
                fx_date: Some(stale),
                fx_rate: Some("10"),
            },
        )
        .await;

        let (status, fx_body) = send(
            &foreign_state,
            "GET",
            "/api/rebalance?amount=10000",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&fx_body, "available");
        let freshness = fx_body["plan"]["rungs"][0]["trades"][0]["freshness"]
            .as_str()
            .expect("freshness");
        assert!(freshness.starts_with("warning_stale_"), "{freshness}");
    }

    #[tokio::test]
    async fn demo_mode_allows_rebalance_get() {
        let state = AppState::for_tests().await.with_demo_mode(true);
        let (status, body) = send(&state, "GET", "/api/rebalance?amount=0", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_plan_status(&body, "unavailable");
    }
}
