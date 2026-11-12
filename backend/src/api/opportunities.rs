use axum::extract::{Query, State};
use axum::Json;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::api::conviction_target::{build_conviction_target, ConvictionTargetResponse};
use crate::api::error::ApiError;
use crate::api::instruments::InstrumentResponse;
use crate::api::valuation::{
    gain_percent_string, money_string, serialize_availability, serialize_freshness,
    staler_freshness, AvailabilityResponse, PriceSnapshotResponse,
};
use crate::api::valued_holdings::{load_valued_holdings, ValuedOpenHolding};
use crate::domain::{
    derive_targets, price_versus_average, recent_price_change, Availability,
    ConvictionTargetOutput, PriceVersusAverage, RecentChangeReason, RecentPriceChange,
    RecentWindow, TransactionKind, ValuationReason,
};
use crate::market_data::effective_prices;
use crate::state::AppState;

mod evidence;

#[derive(Debug, Deserialize)]
pub struct OpportunitiesQuery {
    pub window: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OpportunitiesResponse {
    pub period: OpportunityPeriod,
    pub rows: Vec<OpportunityRow>,
    pub data_revision: String,
}

#[derive(Debug, Serialize)]
pub struct OpportunityPeriod {
    pub window: &'static str,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Serialize)]
pub struct OpportunityRow {
    pub instrument: InstrumentResponse,
    pub quantity: i64,
    pub latest_close: Option<PriceSnapshotResponse>,
    pub price_change: PriceChangeResponse,
    pub average_cost_native: String,
    pub price_vs_average_percent: AvailabilityResponse,
    pub below_average: Option<bool>,
    pub unrealized_gain_percent: AvailabilityResponse,
    pub unrealized_gain_freshness: Option<String>,
    pub conviction_target: ConvictionTargetResponse,
    pub evidence: Option<evidence::EvidenceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PriceChangeResponse {
    Available {
        start_date: String,
        start_close: String,
        end_date: String,
        end_close: String,
        change_percent: String,
    },
    Unavailable {
        reasons: Vec<&'static str>,
    },
}

impl From<RecentPriceChange> for PriceChangeResponse {
    fn from(change: RecentPriceChange) -> Self {
        match change {
            RecentPriceChange::Available {
                start_date,
                start_close,
                end_date,
                end_close,
                change_percent,
            } => Self::Available {
                start_date: start_date.to_string(),
                start_close: start_close.to_string(),
                end_date: end_date.to_string(),
                end_close: end_close.to_string(),
                change_percent: money_string(change_percent),
            },
            RecentPriceChange::Unavailable(reasons) => Self::Unavailable {
                reasons: reasons
                    .into_iter()
                    .map(RecentChangeReason::as_str)
                    .collect(),
            },
        }
    }
}

pub async fn handler(
    State(state): State<AppState>,
    Query(query): Query<OpportunitiesQuery>,
) -> Result<Json<OpportunitiesResponse>, ApiError> {
    let window = match query.window.as_deref() {
        None => RecentWindow::OneWeek,
        Some(value) => RecentWindow::from_query(value).ok_or_else(|| {
            ApiError::bad_request("invalid_window", "window must be 7d, 14d or 30d")
        })?,
    };
    let today = state.clock.today();
    let start_date = window.start_date(today);
    let tickers = crate::db::instrument_tickers::list(&state.pool).await?;
    let holdings = load_valued_holdings(&state.pool, today).await?;
    let inputs: Vec<_> = holdings
        .iter()
        .map(ValuedOpenHolding::conviction_target_input)
        .collect();
    let targets = derive_targets(&inputs);
    let mut pending = Vec::new();

    for (holding, target) in holdings
        .iter()
        .zip(&targets)
        .filter(|(holding, _)| holding.position.quantity > 0)
    {
        let end = holding.latest_price_snapshot();
        let start = if end.is_some_and(|end| end.date > start_date) {
            effective_prices::effective_latest_on_or_before(
                &state.pool,
                holding.instrument.id,
                start_date,
            )
            .await
            .map_err(|error| log_start_close_failure(holding, window, start_date, error))?
        } else {
            None
        };
        let split_dates: Vec<_> = holding
            .ledger
            .iter()
            .filter(|transaction| transaction.kind == TransactionKind::Split)
            .map(|transaction| transaction.trade_date)
            .collect();
        let change = if holding.valuation.is_some() {
            recent_price_change(start_date, end, start.as_ref(), &split_dates)
        } else {
            RecentPriceChange::Unavailable(vec![RecentChangeReason::NoPriceSource])
        };
        pending.push((holding, target, change));
    }
    let held_changes: Vec<_> = pending
        .iter()
        .map(|(holding, _, change)| {
            (
                holding.instrument.id,
                match change {
                    RecentPriceChange::Available { change_percent, .. } => Some(*change_percent),
                    RecentPriceChange::Unavailable(_) => None,
                },
            )
        })
        .collect();
    let mut rows = Vec::with_capacity(pending.len());
    for (holding, target, change) in pending {
        let evidence = evidence::load(
            &state,
            today,
            window,
            holding,
            target,
            &change,
            &held_changes,
        )
        .await?;
        let mut row = build_opportunity_row(holding, target, change, window, start_date, &tickers)?;
        row.evidence = evidence;
        rows.push(row);
    }

    Ok(Json(OpportunitiesResponse {
        period: OpportunityPeriod {
            window: window.as_str(),
            start_date: start_date.to_string(),
            end_date: today.to_string(),
        },
        rows,
        data_revision: state.revision.current().await.map_err(|error| {
            ApiError::internal(format!("could not read data revision: {error}"))
        })?,
    }))
}

fn build_opportunity_row(
    holding: &ValuedOpenHolding,
    target: &ConvictionTargetOutput,
    change: RecentPriceChange,
    window: RecentWindow,
    start_date: NaiveDate,
    tickers: &crate::db::instrument_tickers::TickerMap,
) -> Result<OpportunityRow, ApiError> {
    let average = holding.position.average_cost_native().ok_or_else(|| {
        log_internal_inconsistency(
            holding,
            window,
            start_date,
            "held position without average cost",
        )
    })?;
    let end = holding.latest_price_snapshot();
    let (price_vs_average_percent, below_average) =
        match price_versus_average(end, &holding.instrument.currency, Some(average)) {
            PriceVersusAverage::Available {
                percent,
                below_average,
            } => (
                AvailabilityResponse::Available {
                    value: money_string(percent),
                },
                Some(below_average),
            ),
            PriceVersusAverage::Unavailable(reasons) => (
                AvailabilityResponse::Unavailable {
                    reasons: reasons
                        .into_iter()
                        .map(|reason| reason.as_str().to_owned())
                        .collect(),
                },
                None,
            ),
        };
    let unrealized_gain_percent = match &holding.valuation {
        Some(valuation) => {
            serialize_availability(&valuation.unrealized_gain_percent, gain_percent_string)
        }
        None => serialize_availability(
            &Availability::<rust_decimal::Decimal>::unavailable(ValuationReason::MissingPrice),
            gain_percent_string,
        ),
    };
    // Missing snapshots carry no freshness claim; the gain's availability
    // already explains the missing price or FX.
    let unrealized_gain_freshness = holding
        .latest_price_snapshot()
        .zip(holding.latest_fx_snapshot())
        .map(|(price, fx)| serialize_freshness(staler_freshness(price.freshness, fx.freshness)));
    Ok(OpportunityRow {
        instrument: InstrumentResponse::from_row(&holding.instrument, tickers).map_err(
            |error| {
                log_internal_inconsistency(
                    holding,
                    window,
                    start_date,
                    &format!("invalid instrument response: {error:?}"),
                )
            },
        )?,
        quantity: holding.position.quantity,
        latest_close: end.map(|snapshot| PriceSnapshotResponse {
            date: snapshot.date.to_string(),
            close: snapshot.close.to_string(),
            currency: snapshot.currency.clone(),
            source: snapshot.source.as_str().to_owned(),
            freshness: serialize_freshness(snapshot.freshness),
        }),
        price_change: change.into(),
        average_cost_native: money_string(average),
        price_vs_average_percent,
        below_average,
        unrealized_gain_percent,
        unrealized_gain_freshness,
        conviction_target: build_conviction_target(target, &holding.market_value_reasons()),
        evidence: None,
    })
}

fn log_start_close_failure(
    holding: &ValuedOpenHolding,
    window: RecentWindow,
    start_date: NaiveDate,
    error: crate::db::RepoError,
) -> ApiError {
    crate::engine_error!(
        "opportunities start-close lookup failed for instrument {} ({} / {}), window={} start_date={}: {}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        window.as_str(),
        start_date,
        error
    );
    ApiError::from(error)
}

fn log_internal_inconsistency(
    holding: &ValuedOpenHolding,
    window: RecentWindow,
    start_date: NaiveDate,
    reason: &str,
) -> ApiError {
    crate::engine_error!(
        "opportunities row inconsistency for instrument {} ({} / {}), window={} start_date={}: {}",
        holding.instrument.id,
        holding.instrument.exchange,
        holding.instrument.symbol,
        window.as_str(),
        start_date,
        reason
    );
    ApiError::internal(format!(
        "opportunities row inconsistency for instrument {}",
        holding.instrument.id
    ))
}

#[cfg(test)]
mod tests;
