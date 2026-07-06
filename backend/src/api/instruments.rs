use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow, NewInstrument};
use crate::domain::ConvictionLevel;
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

/// API-facing conviction value. Serializes as `Other`/`Low`/`Medium`/`High`;
/// maps to the pure `ConvictionLevel` and the DB string at the boundary.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConvictionDto {
    Other,
    Low,
    Medium,
    High,
}

impl ConvictionDto {
    fn to_level(self) -> ConvictionLevel {
        match self {
            Self::Other => ConvictionLevel::Other,
            Self::Low => ConvictionLevel::Low,
            Self::Medium => ConvictionLevel::Medium,
            Self::High => ConvictionLevel::High,
        }
    }

    pub(crate) fn from_level(level: ConvictionLevel) -> Self {
        match level {
            ConvictionLevel::Other => Self::Other,
            ConvictionLevel::Low => Self::Low,
            ConvictionLevel::Medium => Self::Medium,
            ConvictionLevel::High => Self::High,
        }
    }

    fn from_db_str(value: &str) -> Option<Self> {
        ConvictionLevel::from_db_str(value).map(Self::from_level)
    }

    fn db_str(self) -> &'static str {
        self.to_level().db_str()
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
    pub conviction: ConvictionDto,
}

impl InstrumentResponse {
    pub fn from_row(row: &InstrumentRow) -> Result<Self, ApiError> {
        let kind = InstrumentKindDto::from_db_str(&row.kind).ok_or_else(|| {
            ApiError::internal(format!("stored unknown instrument type {:?}", row.kind))
        })?;
        let conviction = ConvictionDto::from_db_str(&row.conviction).ok_or_else(|| {
            ApiError::internal(format!("stored unknown conviction {:?}", row.conviction))
        })?;
        Ok(Self {
            id: row.id,
            symbol: row.symbol.clone(),
            exchange: row.exchange.clone(),
            name: row.name.clone(),
            kind,
            currency: row.currency.clone(),
            conviction,
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
    crate::api::reject_demo_mutation(&state)?;

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

#[derive(Debug, Deserialize)]
pub struct UpdateConviction {
    pub conviction: ConvictionDto,
}

/// `PUT /api/instruments/{id}/conviction` — set one instrument's conviction.
pub async fn update_conviction(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<UpdateConviction>,
) -> Result<Json<InstrumentResponse>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let row = instruments::update_conviction(&state.pool, id, body.conviction.db_str())
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;
    Ok(Json(InstrumentResponse::from_row(&row)?))
}

#[derive(Debug, Deserialize)]
pub struct ConvictionChange {
    pub instrument_id: i64,
    pub conviction: ConvictionDto,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConvictions {
    pub changes: Vec<ConvictionChange>,
}

/// `PUT /api/instruments/convictions` — apply several conviction changes in one
/// SQL transaction. Used by the Holdings apply-all action. Any unknown id fails
/// the whole batch with 404 so the frontend never sees a partial apply.
pub async fn update_convictions(
    State(state): State<AppState>,
    Json(body): Json<UpdateConvictions>,
) -> Result<Json<Vec<InstrumentResponse>>, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let changes: Vec<(i64, String)> = body
        .changes
        .iter()
        .map(|change| (change.instrument_id, change.conviction.db_str().to_owned()))
        .collect();

    let rows = instruments::update_convictions(&state.pool, &changes)
        .await?
        .ok_or_else(|| {
            ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                "not_found",
                "one or more instrument ids in the batch do not exist",
            )
        })?;
    let body = rows
        .iter()
        .map(InstrumentResponse::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(body))
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    async fn send(state: &AppState, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
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
            Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json body")
        };
        (status, value)
    }

    async fn create_instrument(state: &AppState, symbol: &str) -> i64 {
        let (status, body) = send(
            state,
            "POST",
            "/api/instruments",
            json!({"symbol":symbol,"exchange":"NASDAQ","name":symbol,"type":"Stock","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_i64().expect("instrument id")
    }

    #[tokio::test]
    async fn new_instrument_defaults_to_other_conviction() {
        let state = AppState::for_tests().await;
        create_instrument(&state, "MSFT").await;

        let (status, body) = send(&state, "GET", "/api/instruments", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body[0]["conviction"], "Other");
    }

    #[tokio::test]
    async fn update_conviction_changes_and_list_reflects_it() {
        let state = AppState::for_tests().await;
        let id = create_instrument(&state, "MSFT").await;

        let (status, updated) = send(
            &state,
            "PUT",
            &format!("/api/instruments/{id}/conviction"),
            json!({"conviction":"Medium"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(updated["conviction"], "Medium");

        let (_, list) = send(&state, "GET", "/api/instruments", Value::Null).await;
        assert_eq!(list[0]["conviction"], "Medium");
    }

    #[tokio::test]
    async fn update_conviction_unknown_instrument_is_404() {
        let state = AppState::for_tests().await;
        let (status, body) = send(
            &state,
            "PUT",
            "/api/instruments/9999/conviction",
            json!({"conviction":"Low"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn bulk_update_applies_all_changes() {
        let state = AppState::for_tests().await;
        let a = create_instrument(&state, "AAA").await;
        let b = create_instrument(&state, "BBB").await;

        let (status, body) = send(
            &state,
            "PUT",
            "/api/instruments/convictions",
            json!({"changes":[
                {"instrument_id":a,"conviction":"Low"},
                {"instrument_id":b,"conviction":"High"}
            ]}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("array");
        assert_eq!(rows.len(), 2);

        let (_, list) = send(&state, "GET", "/api/instruments", Value::Null).await;
        let convictions: Vec<&str> = list
            .as_array()
            .expect("array")
            .iter()
            .map(|row| row["conviction"].as_str().expect("conviction"))
            .collect();
        assert!(convictions.contains(&"Low"));
        assert!(convictions.contains(&"High"));
    }

    #[tokio::test]
    async fn bulk_update_with_unknown_id_rejects_and_rolls_back() {
        let state = AppState::for_tests().await;
        let a = create_instrument(&state, "AAA").await;

        let (status, _) = send(
            &state,
            "PUT",
            "/api/instruments/convictions",
            json!({"changes":[
                {"instrument_id":a,"conviction":"Low"},
                {"instrument_id":9999,"conviction":"High"}
            ]}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The valid id in the batch must remain unchanged after rollback.
        let (_, list) = send(&state, "GET", "/api/instruments", Value::Null).await;
        assert_eq!(list[0]["conviction"], "Other");
    }
}
