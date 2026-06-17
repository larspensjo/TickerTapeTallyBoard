use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow, NewInstrument};
use crate::state::AppState;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum InstrumentKindDto {
    Stock,
    Etf,
    Fund,
}

impl InstrumentKindDto {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Stock => "STOCK",
            Self::Etf => "ETF",
            Self::Fund => "FUND",
        }
    }

    fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "STOCK" => Some(Self::Stock),
            "ETF" => Some(Self::Etf),
            "FUND" => Some(Self::Fund),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateInstrument {
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InstrumentKindDto,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct InstrumentResponse {
    pub id: i64,
    pub symbol: String,
    pub exchange: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: InstrumentKindDto,
    pub currency: String,
}

impl InstrumentResponse {
    pub fn from_row(row: &InstrumentRow) -> Result<Self, ApiError> {
        let kind = InstrumentKindDto::from_db_str(&row.kind).ok_or_else(|| {
            ApiError::internal(format!("stored unknown instrument type {:?}", row.kind))
        })?;
        Ok(Self {
            id: row.id,
            symbol: row.symbol.clone(),
            exchange: row.exchange.clone(),
            name: row.name.clone(),
            kind,
            currency: row.currency.clone(),
        })
    }
}

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<InstrumentResponse>>, ApiError> {
    let rows = instruments::list(&state.pool).await?;
    let body = rows
        .iter()
        .map(InstrumentResponse::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(body))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<CreateInstrument>,
) -> Result<impl IntoResponse, ApiError> {
    let new = NewInstrument {
        symbol: body.symbol.trim().to_owned(),
        exchange: body.exchange.trim().to_owned(),
        name: body.name.trim().to_owned(),
        kind: body.kind.as_db_str().to_owned(),
        currency: body.currency.trim().to_owned(),
        isin: None,
    };
    if new.symbol.is_empty()
        || new.exchange.is_empty()
        || new.name.is_empty()
        || new.currency.is_empty()
    {
        return Err(ApiError::bad_request(
            "invalid_instrument",
            "symbol, exchange, name, and currency are required",
        ));
    }

    let (row, created) = instruments::upsert(&state.pool, &new).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(InstrumentResponse::from_row(&row)?)))
}
