use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow, NewInstrument};
use crate::db::{prices, provider_symbols, transactions};
use crate::domain::derive_position;
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
    #[serde(default)]
    pub isin: Option<String>,
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
        exchange: body.exchange.trim().to_ascii_uppercase(),
        name: body.name.trim().to_owned(),
        kind: body.kind.as_db_str().to_owned(),
        currency: body.currency.trim().to_owned(),
        isin: normalize_isin(body.isin),
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

pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let instrument = instruments::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", id))?;
    let mut tx = state.pool.begin().await.map_err(|error| {
        ApiError::internal(format!(
            "instrument delete: failed to begin transaction for instrument {}: {error}",
            id
        ))
    })?;
    let ledger = transactions::ledger_for_instrument_in_tx(&mut tx, id).await?;
    let position = derive_position(&ledger).map_err(|error| {
        ApiError::internal(format!(
            "instrument delete: inconsistent stored ledger for instrument {}: {error:?}",
            id
        ))
    })?;

    if !ledger.is_empty() || position.quantity != 0 {
        let reason = if !ledger.is_empty() {
            "has_transactions"
        } else {
            "nonzero_quantity"
        };
        crate::engine_warn!(
            "instrument delete rejected instrument_id={} symbol={} exchange={} transactions={} quantity={} reason={}",
            instrument.id,
            instrument.symbol,
            instrument.exchange,
            ledger.len(),
            position.quantity,
            reason
        );
        if let Err(error) = tx.rollback().await {
            crate::engine_error!(
                "instrument delete rollback failed instrument_id={} reason={} error={}",
                instrument.id,
                reason,
                error
            );
        }
        return Err(ApiError::conflict(
            "instrument_not_deletable",
            "Instrument can only be deleted when it has no transactions and zero quantity.",
        )
        .with_details(json!({
            "instrument_id": instrument.id,
            "reason": reason,
            "transactions": ledger.len(),
            "quantity": position.quantity,
        })));
    }

    let provider_symbol_rows =
        provider_symbols::delete_by_instrument_id_in_tx(&mut tx, instrument.id).await?;
    let price_rows = prices::delete_by_instrument_id_in_tx(&mut tx, instrument.id).await?;
    let deleted = instruments::delete_in_tx(&mut tx, instrument.id).await?;
    if deleted == 0 {
        if let Err(error) = tx.rollback().await {
            crate::engine_error!(
                "instrument delete rollback failed instrument_id={} error={}",
                instrument.id,
                error
            );
        }
        return Err(ApiError::not_found("instrument", id));
    }
    tx.commit().await.map_err(|error| {
        ApiError::internal(format!(
            "instrument delete: failed to commit deletion for instrument {}: {error}",
            id
        ))
    })?;

    crate::engine_info!(
        "instrument deleted instrument_id={} symbol={} exchange={} provider_symbol_rows={} price_rows={}",
        instrument.id,
        instrument.symbol,
        instrument.exchange,
        provider_symbol_rows,
        price_rows
    );

    Ok(StatusCode::NO_CONTENT)
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

fn normalize_isin(value: Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use crate::api::router;
    use crate::db::{instruments as instruments_db, prices, provider_symbols};
    use crate::import::now_iso8601;
    use crate::state::AppState;
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use chrono::NaiveDate;
    use rust_decimal::Decimal;
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
        create_instrument_with(state, symbol, "NASDAQ", None).await
    }

    async fn create_instrument_with(
        state: &AppState,
        symbol: &str,
        exchange: &str,
        isin: Option<&str>,
    ) -> i64 {
        let mut payload = json!({
            "symbol": symbol,
            "exchange": exchange,
            "name": symbol,
            "type": "Stock",
            "currency": "USD",
        });
        if let Some(isin) = isin {
            payload["isin"] = json!(isin);
        }
        let (status, body) = send(state, "POST", "/api/instruments", payload).await;
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

    #[tokio::test]
    async fn create_normalizes_exchange_and_isin_before_persisting() {
        let state = AppState::for_tests().await;

        let (status, body) = send(
            &state,
            "POST",
            "/api/instruments",
            json!({
                "symbol": "CORN",
                "exchange": " Avanza ",
                "name": "Corning",
                "type": "Stock",
                "currency": "usd",
                "isin": " us2193501051 ",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(body["exchange"], "AVANZA");

        let row = instruments_db::find(&state.pool, body["id"].as_i64().expect("id"))
            .await
            .expect("find instrument")
            .expect("instrument exists");
        assert_eq!(row.exchange, "AVANZA");
        assert_eq!(row.isin.as_deref(), Some("US2193501051"));
    }

    #[tokio::test]
    async fn delete_removes_never_traded_instrument_and_dependents() {
        let state = AppState::for_tests().await;
        let id = create_instrument_with(&state, "CORN", "Avanza", Some("us2193501051")).await;
        let now = now_iso8601();

        provider_symbols::upsert(
            &state.pool,
            &provider_symbols::NewProviderSymbol {
                instrument_id: id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "CORN".to_owned(),
                currency: Some("USD".to_owned()),
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )
        .await
        .expect("provider symbol");
        prices::upsert(
            &state.pool,
            &prices::NewPrice {
                instrument_id: id,
                provider: "YAHOO".to_owned(),
                provider_symbol: "CORN".to_owned(),
                date: NaiveDate::from_ymd_opt(2026, 6, 1).expect("date"),
                close: Decimal::new(12345, 2),
                currency: "USD".to_owned(),
                fetched_at: now,
            },
        )
        .await
        .expect("price");

        let (status, _) = send(
            &state,
            "DELETE",
            &format!("/api/instruments/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        assert!(instruments_db::find(&state.pool, id)
            .await
            .expect("find")
            .is_none());
        assert!(
            provider_symbols::find_by_instrument_provider(&state.pool, id, "YAHOO")
                .await
                .expect("provider symbol lookup")
                .is_none()
        );
        assert!(prices::find_by_key(
            &state.pool,
            id,
            "YAHOO",
            NaiveDate::from_ymd_opt(2026, 6, 1).expect("date"),
        )
        .await
        .expect("price lookup")
        .is_none());
    }

    #[tokio::test]
    async fn delete_rejects_instrument_with_transaction_history() {
        let state = AppState::for_tests().await;
        let id = create_instrument_with(&state, "CORN", "Avanza", Some("US2193501051")).await;

        let (status, _) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({
                "instrument_id": id,
                "type": "Buy",
                "trade_date": "2026-06-01",
                "quantity": 1,
                "price": "10",
                "currency": "USD",
                "fx_rate_to_base": "10"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);

        let (status, body) = send(
            &state,
            "DELETE",
            &format!("/api/instruments/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(body["error"]["code"], "instrument_not_deletable");
        assert!(instruments_db::find(&state.pool, id)
            .await
            .expect("find")
            .is_some());
    }
}
