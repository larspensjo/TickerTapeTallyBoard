use crate::{
    api::{error::ApiError, extract::ApiJson},
    db::{
        fundamentals_sources::{
            self, Classification, CorrectionError, FundamentalsSource, LinkDefinition, LinkStatus,
            PreviousLink,
        },
        instruments,
    },
    market_data::FundamentalsPreview,
    state::AppState,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviousLinkInput {
    SameCompany,
    WrongCompany,
}
impl From<PreviousLinkInput> for PreviousLink {
    fn from(value: PreviousLinkInput) -> Self {
        match value {
            PreviousLinkInput::SameCompany => Self::SameCompany,
            PreviousLinkInput::WrongCompany => Self::WrongCompany,
        }
    }
}
#[derive(Debug, Deserialize)]
pub struct Correction {
    pub provider: Option<String>,
    pub provider_symbol: Option<String>,
    #[serde(default)]
    pub not_a_company: bool,
    pub previous_link: Option<PreviousLinkInput>,
}
#[derive(Debug, Deserialize)]
pub struct Automatic {
    pub previous_link: Option<PreviousLinkInput>,
}
#[derive(Debug, Deserialize)]
pub struct PreviewQuery {
    pub symbol: String,
}
#[derive(Serialize)]
pub struct PreviewResponse {
    #[serde(flatten)]
    pub preview: FundamentalsPreview,
    pub data_revision: String,
}
fn symbol(raw: &str) -> Result<String, ApiError> {
    let symbol = raw.trim();
    if symbol.is_empty()
        || symbol.len() > 100
        || symbol.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ApiError::bad_request(
            "invalid_provider_symbol",
            "A provider symbol is required, without spaces",
        ));
    }
    Ok(symbol.to_ascii_uppercase())
}
async fn require_instrument(state: &AppState, id: i64) -> Result<(), ApiError> {
    instruments::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;
    Ok(())
}
async fn correct(
    state: &AppState,
    id: i64,
    definition: Option<LinkDefinition>,
    previous: Option<PreviousLinkInput>,
) -> Result<StatusCode, ApiError> {
    require_instrument(state, id).await?;
    let symbol = definition
        .as_ref()
        .and_then(|d| d.symbol.clone())
        .unwrap_or_else(|| "none".into());
    let link = fundamentals_sources::correct(
        &state.pool, id, definition, previous.map(Into::into),
        &state.clock.now_utc().to_rfc3339(),
    ).await.map_err(|error| match error {
        CorrectionError::PreviousLinkRequired => ApiError::bad_request(
            "previous_link_required",
            "Choose whether the previous link was the same company or the wrong company",
        ),
        CorrectionError::Repo(error) => ApiError::internal(format!(
            "fundamentals correction instrument_id={id} symbol={symbol} endpoint=fundamentals-source reason=repository_error: {error}"
        )),
    })?;
    crate::engine_info!("fundamentals correction instrument_id={} symbol={} endpoint=fundamentals-source link_generation={} previous_link={:?} reason=source_corrected", id, symbol, link.token.generation, previous);
    Ok(StatusCode::NO_CONTENT)
}
pub async fn put(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(body): ApiJson<Correction>,
) -> Result<StatusCode, ApiError> {
    let definition =
        if body.not_a_company && body.provider.is_none() && body.provider_symbol.is_none() {
            LinkDefinition {
                classification: Classification::NotCompany,
                status: LinkStatus::NotApplicable,
                ..LinkDefinition::pending()
            }
        } else {
            let provider = body
                .provider
                .as_deref()
                .and_then(FundamentalsSource::from_db_str)
                .ok_or_else(|| {
                    ApiError::bad_request(
                        "invalid_provider",
                        "provider must be YAHOO, or null with not_a_company",
                    )
                })?;
            if body.not_a_company {
                return Err(ApiError::bad_request(
                    "invalid_provider",
                    "not_a_company cannot have a provider",
                ));
            }
            LinkDefinition {
                source: Some(provider),
                symbol: Some(symbol(body.provider_symbol.as_deref().unwrap_or(""))?),
                ..LinkDefinition::pending()
            }
        };
    correct(&state, id, Some(definition), body.previous_link).await
}
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    query: Result<Query<Automatic>, axum::extract::rejection::QueryRejection>,
) -> Result<StatusCode, ApiError> {
    let Query(query) = query.map_err(|_| {
        ApiError::bad_request(
            "invalid_json",
            "previous_link must be same_company or wrong_company",
        )
    })?;
    correct(&state, id, None, query.previous_link).await
}
pub async fn preview(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<PreviewQuery>,
) -> Result<Json<PreviewResponse>, ApiError> {
    require_instrument(&state, id).await?;
    let symbol = symbol(&query.symbol)?;
    let preview = if state.is_demo() {
        FundamentalsPreview::unavailable(symbol)
    } else {
        state
            .market_data
            .preview_fundamentals(id, symbol, state.clock.today())
            .await
    };
    Ok(Json(PreviewResponse {
        preview,
        data_revision: state
            .revision
            .current()
            .await
            .map_err(|e| ApiError::internal(e.to_string()))?,
    }))
}

#[cfg(test)]
mod tests;
