mod loading;
mod response;
use response::{CompanyFiguresResponse, FigureResponse, ReportDateResponse};

use crate::{
    api::{error::ApiError, valuation::load_valuation_inputs},
    db::{fundamental_values, fundamentals_sources, fx_rates, instruments, transactions},
    domain::{
        company_figures::{
            self, CompanyFigures, FxRate, Inputs, Price, ReportedValue, SourceState,
        },
        fundamentals::{AttemptStatus, Classification, LinkStatus, Metric, PeriodType},
        report_dates::{self, Observation, ReportDate},
        DataFreshness, FxSnapshot, PriceSnapshot, TransactionKind,
    },
    market_data::effective_prices,
    providers::BASE_FX_PROVIDER,
    state::AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize)]
pub struct HoldingsFundamentals {
    pub pe: FigureResponse,
    pub ps: FigureResponse,
    pub next_report: ReportDateResponse,
}
#[derive(Debug, Serialize)]
pub struct FundamentalsResponse {
    #[serde(flatten)]
    pub figures: CompanyFiguresResponse,
    pub next_report: ReportDateResponse,
    pub last_report: ReportDateResponse,
    pub source: SourcePanel,
    pub data_revision: String,
}
#[derive(Debug, Serialize)]
pub struct SourcePanel {
    pub symbol: Option<String>,
    pub origin: Option<crate::domain::fundamentals::LinkOrigin>,
    pub security_type: Option<String>,
    pub classification: Classification,
    pub status: LinkStatus,
    pub candidates: serde_json::Value,
    pub last_success: Option<String>,
    pub last_attempt_at: Option<String>,
    pub last_attempt_outcome: Option<AttemptStatus>,
    pub last_attempt_reason: Option<String>,
    pub awaiting_figures: bool,
}

pub async fn get(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<FundamentalsResponse>, ApiError> {
    let instrument = instruments::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;
    let today = state.clock.today();
    let inputs = load_valuation_inputs(&state.pool, &instrument, today).await?;
    let price = inputs.latest_price.map(|p| PriceSnapshot {
        date: p.date,
        close: p.close,
        currency: p.currency,
        source: p.source,
        freshness: crate::domain::data_freshness(today, p.date),
    });
    let fx = inputs.latest_fx.map(|f| FxSnapshot {
        date: f.date,
        rate: f.rate,
        base: f.base,
        quote: f.quote,
        freshness: DataFreshness::Fresh,
    });
    let mut response = load(
        &state.pool,
        &instrument,
        today,
        &state.clock,
        price.as_ref(),
        fx.as_ref(),
    )
    .await?;
    response.data_revision = state
        .revision
        .current()
        .await
        .map_err(|e| ApiError::internal(format!("could not read data revision: {e}")))?;
    Ok(Json(response))
}

/// Price and listing FX are explicit inputs so Holdings never reloads its valuation
/// price. Only historical evidence may require a distinct, earlier close.
pub(super) async fn load(
    pool: &SqlitePool,
    instrument: &instruments::InstrumentRow,
    today: NaiveDate,
    clock: &crate::clock::Clock,
    loaded_price: Option<&PriceSnapshot>,
    loaded_fx: Option<&FxSnapshot>,
) -> Result<FundamentalsResponse, ApiError> {
    let loading::Loaded {
        figures,
        next_report,
        source,
        link,
        latest,
        annual,
    } = loading::load_inputs(
        pool,
        instrument,
        today,
        clock,
        loaded_price,
        loaded_fx,
        None,
    )
    .await?;
    let id = instrument.id;
    let awaiting = source.awaiting_figures;
    let observations = fundamental_values::active_observations(pool, id)
        .await?
        .into_iter()
        .filter(|o| o.period_type == PeriodType::Quarter)
        .map(|o| Observation {
            period_end: o.period_end,
            first_seen: o.first_seen_on,
            absent_on: o.absent_fetch_on,
        })
        .collect::<Vec<_>>();
    let (_, last_report) =
        report_dates::report_dates(today, latest, &annual, &observations, &source);
    let panel = SourcePanel {
        symbol: link.as_ref().and_then(|s| s.definition.symbol.clone()),
        origin: link.as_ref().map(|s| s.definition.origin),
        security_type: link
            .as_ref()
            .and_then(|s| s.definition.security_type.clone()),
        classification: source.classification,
        status: source.status,
        candidates: link
            .as_ref()
            .map(|s| serde_json::from_str(&s.definition.candidates))
            .transpose()
            .map_err(|e| {
                ApiError::internal(format!(
                    "invalid fundamentals candidates instrument_id={id}: {e}"
                ))
            })?
            .unwrap_or_else(|| serde_json::json!([])),
        last_success: link.as_ref().and_then(|s| s.last_success_at.clone()),
        last_attempt_at: link.as_ref().and_then(|s| s.last_attempt_at.clone()),
        last_attempt_outcome: link.as_ref().and_then(|s| s.last_attempt_status),
        last_attempt_reason: link.as_ref().and_then(|s| s.last_attempt_reason.clone()),
        awaiting_figures: awaiting,
    };
    Ok(FundamentalsResponse {
        figures: figures.into(),
        next_report: next_report.into(),
        last_report: last_report.into(),
        source: panel,
        data_revision: String::new(),
    })
}

pub(super) async fn load_holdings(
    pool: &SqlitePool,
    holding: &super::valued_holdings::ValuedOpenHolding,
    today: NaiveDate,
    clock: &crate::clock::Clock,
) -> Result<HoldingsFundamentals, ApiError> {
    let loaded = loading::load_inputs(
        pool,
        &holding.instrument,
        today,
        clock,
        holding.latest_price_snapshot(),
        holding.latest_fx_snapshot(),
        Some(&holding.ledger),
    )
    .await?;
    Ok(HoldingsFundamentals {
        pe: loaded.figures.pe.into(),
        ps: loaded.figures.ps.into(),
        next_report: loaded.next_report.into(),
    })
}

#[cfg(test)]
mod tests;
