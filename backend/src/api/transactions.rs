use std::str::FromStr;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqlitePool;

use crate::api::error::ApiError;
use crate::db::instruments::{self, InstrumentRow};
use crate::db::transactions::{self, NewTransaction, TransactionRow};
use crate::domain::{self, LedgerTransaction, ProposedTransaction, TransactionKind};
use crate::state::AppState;

const PROSPECTIVE_TRANSACTION_ID: i64 = i64::MAX;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionKindDto {
    Buy,
    Sell,
    Split,
    Dividend,
}

impl From<TransactionKindDto> for TransactionKind {
    fn from(value: TransactionKindDto) -> Self {
        match value {
            TransactionKindDto::Buy => TransactionKind::Buy,
            TransactionKindDto::Sell => TransactionKind::Sell,
            TransactionKindDto::Split => TransactionKind::Split,
            TransactionKindDto::Dividend => TransactionKind::Dividend,
        }
    }
}

impl From<TransactionKind> for TransactionKindDto {
    fn from(value: TransactionKind) -> Self {
        match value {
            TransactionKind::Buy => TransactionKindDto::Buy,
            TransactionKind::Sell => TransactionKindDto::Sell,
            TransactionKind::Split => TransactionKindDto::Split,
            TransactionKind::Dividend => TransactionKindDto::Dividend,
        }
    }
}

/// Request body for both POST (create) and PUT (full replacement).
#[derive(Debug, Deserialize)]
pub struct TransactionInput {
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: TransactionKindDto,
    pub trade_date: String,
    pub quantity: i64,
    #[serde(default)]
    pub price: Option<String>,
    #[serde(default)]
    pub dividend_per_share: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub fx_rate_to_base: Option<String>,
    #[serde(default)]
    pub brokerage: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

impl TransactionInput {
    fn proposed(&self) -> Result<ProposedTransaction, ApiError> {
        let trade_date =
            NaiveDate::parse_from_str(self.trade_date.trim(), "%Y-%m-%d").map_err(|_| {
                ApiError::bad_request(
                    "invalid_date",
                    format!("trade_date must be YYYY-MM-DD: {:?}", self.trade_date),
                )
            })?;
        Ok(ProposedTransaction {
            kind: self.kind.into(),
            trade_date,
            quantity: self.quantity,
            price: parse_decimal("price", &self.price)?,
            dividend_per_share: parse_decimal("dividend_per_share", &self.dividend_per_share)?,
            currency: normalize(&self.currency),
            fx_rate_to_base: parse_decimal("fx_rate_to_base", &self.fx_rate_to_base)?,
            brokerage_base: parse_decimal("brokerage", &self.brokerage)?,
        })
    }

    fn note(&self) -> Option<String> {
        normalize(&self.note)
    }
}

fn normalize(value: &Option<String>) -> Option<String> {
    value
        .as_ref()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
}

fn parse_decimal(label: &str, value: &Option<String>) -> Result<Option<Decimal>, ApiError> {
    match normalize(value) {
        None => Ok(None),
        Some(raw) => Decimal::from_str(&raw).map(Some).map_err(|_| {
            ApiError::bad_request(
                "invalid_decimal",
                format!("{label} is not a valid decimal: {raw:?}"),
            )
        }),
    }
}

#[derive(Debug, Serialize)]
pub struct TransactionResponse {
    pub id: i64,
    pub instrument_id: i64,
    #[serde(rename = "type")]
    pub kind: TransactionKindDto,
    pub trade_date: String,
    pub quantity: i64,
    pub price: Option<String>,
    pub dividend_per_share: Option<String>,
    pub currency: Option<String>,
    pub fx_rate_to_base: Option<String>,
    pub brokerage: Option<String>,
    pub brokerage_currency: Option<String>,
    pub source_value: Option<String>,
    pub source_currency: Option<String>,
    pub note: Option<String>,
    pub import_batch_id: Option<i64>,
}

impl TransactionResponse {
    pub fn from_row(row: &TransactionRow) -> Result<Self, ApiError> {
        let kind = TransactionKind::from_db_str(&row.kind)
            .ok_or_else(|| ApiError::internal(format!("stored unknown type {:?}", row.kind)))?;
        Ok(Self {
            id: row.id,
            instrument_id: row.instrument_id,
            kind: kind.into(),
            trade_date: row.trade_date.clone(),
            quantity: row.quantity,
            price: row.price.clone(),
            dividend_per_share: row.dividend_per_share.clone(),
            currency: row.currency.clone(),
            fx_rate_to_base: row.fx_rate_to_base.clone(),
            brokerage: row.brokerage.clone(),
            brokerage_currency: row.brokerage_currency.clone(),
            source_value: row.source_value.clone(),
            source_currency: row.source_currency.clone(),
            note: row.note.clone(),
            import_batch_id: row.import_batch_id,
        })
    }
}

pub async fn list(
    State(state): State<AppState>,
) -> Result<Json<Vec<TransactionResponse>>, ApiError> {
    let rows = transactions::list(&state.pool).await?;
    let body = rows
        .iter()
        .map(TransactionResponse::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(body))
}

pub async fn create(
    State(state): State<AppState>,
    Json(body): Json<TransactionInput>,
) -> Result<impl IntoResponse, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let proposed = body.proposed()?;
    let signed_quantity = domain::validate(&proposed)?;

    let instrument = instruments::find(&state.pool, body.instrument_id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", body.instrument_id))?;
    assert_currency_matches(&proposed, &instrument)?;

    let prospective = ledger_transaction(PROSPECTIVE_TRANSACTION_ID, signed_quantity, &proposed);
    assert_ledger_valid(&state.pool, body.instrument_id, None, Some(prospective)).await?;

    let row = transactions::insert(
        &state.pool,
        &new_transaction(
            body.instrument_id,
            signed_quantity,
            &proposed,
            transaction_currency(&proposed, &instrument),
            body.note(),
        ),
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(TransactionResponse::from_row(&row)?),
    ))
}

pub async fn replace(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<TransactionInput>,
) -> Result<impl IntoResponse, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let proposed = body.proposed()?;
    let signed_quantity = domain::validate(&proposed)?;

    let existing = transactions::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("transaction", id))?;
    let instrument = instruments::find(&state.pool, body.instrument_id)
        .await?
        .ok_or_else(|| ApiError::not_found("instrument", body.instrument_id))?;
    assert_currency_matches(&proposed, &instrument)?;

    let edited = ledger_transaction(id, signed_quantity, &proposed);
    assert_ledger_valid(&state.pool, body.instrument_id, Some(id), Some(edited)).await?;
    if existing.instrument_id != body.instrument_id {
        assert_ledger_valid(&state.pool, existing.instrument_id, Some(id), None).await?;
    }

    let row = transactions::replace(
        &state.pool,
        id,
        &new_transaction(
            body.instrument_id,
            signed_quantity,
            &proposed,
            transaction_currency(&proposed, &instrument),
            body.note(),
        ),
    )
    .await?;

    Ok(Json(TransactionResponse::from_row(&row)?))
}

pub async fn remove(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    crate::api::reject_demo_mutation(&state)?;

    let existing = transactions::find(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("transaction", id))?;

    assert_ledger_valid(&state.pool, existing.instrument_id, Some(id), None).await?;
    let deleted = transactions::delete(&state.pool, id).await?;
    if deleted == 0 {
        return Err(ApiError::not_found("transaction", id));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn ledger_transaction(
    id: i64,
    signed_quantity: i64,
    proposed: &ProposedTransaction,
) -> LedgerTransaction {
    LedgerTransaction {
        id,
        trade_date: proposed.trade_date,
        kind: proposed.kind,
        quantity: signed_quantity,
        price: proposed.price,
        dividend_per_share: proposed.dividend_per_share,
        fx_rate_to_base: proposed.fx_rate_to_base,
        brokerage_base: proposed.brokerage_base.unwrap_or(Decimal::ZERO),
    }
}

fn new_transaction(
    instrument_id: i64,
    signed_quantity: i64,
    proposed: &ProposedTransaction,
    currency: Option<String>,
    note: Option<String>,
) -> NewTransaction {
    // Dividends store the shares-eligible count (proposed.quantity), not the position
    // effect (signed_quantity == 0), so that income calculations can use the stored qty.
    let db_quantity = if proposed.kind == TransactionKind::Dividend {
        proposed.quantity
    } else {
        signed_quantity
    };
    NewTransaction {
        instrument_id,
        kind: proposed.kind,
        trade_date: proposed.trade_date,
        quantity: db_quantity,
        price: proposed.price,
        dividend_per_share: proposed.dividend_per_share,
        currency,
        fx_rate_to_base: proposed.fx_rate_to_base,
        brokerage: proposed.brokerage_base,
        note,
    }
}

/// Reject a Buy/Sell whose native currency differs from the instrument's currency.
/// Holdings label native cost basis with the instrument currency, so a mismatch
/// would present mixed-currency totals as if they were all one currency. Split
/// rows carry no currency and are always accepted here.
fn assert_currency_matches(
    proposed: &ProposedTransaction,
    instrument: &InstrumentRow,
) -> Result<(), ApiError> {
    if !matches!(
        proposed.kind,
        TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend
    ) {
        return Ok(());
    }
    if let Some(currency) = proposed.currency.as_deref() {
        if !currency.eq_ignore_ascii_case(&instrument.currency) {
            return Err(ApiError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "currency_mismatch",
                format!(
                    "transaction currency {currency} does not match instrument currency {}",
                    instrument.currency
                ),
            ));
        }
    }
    Ok(())
}

fn transaction_currency(
    proposed: &ProposedTransaction,
    instrument: &InstrumentRow,
) -> Option<String> {
    if matches!(
        proposed.kind,
        TransactionKind::Buy | TransactionKind::Sell | TransactionKind::Dividend
    ) {
        Some(instrument.currency.clone())
    } else {
        proposed.currency.clone()
    }
}

/// Re-derive the instrument's ledger with the proposed change applied; reject if
/// any step is invalid (decision: every write must leave the ledger derivable).
async fn assert_ledger_valid(
    pool: &SqlitePool,
    instrument_id: i64,
    exclude_id: Option<i64>,
    extra: Option<LedgerTransaction>,
) -> Result<(), ApiError> {
    let mut ledger: Vec<LedgerTransaction> =
        transactions::ledger_for_instrument(pool, instrument_id).await?;
    if let Some(excluded) = exclude_id {
        ledger.retain(|tx| tx.id != excluded);
    }
    if let Some(extra) = extra {
        ledger.push(extra);
    }
    ledger.sort_by_key(|tx| (tx.trade_date, tx.id));
    domain::derive_position(&ledger).map_err(|error| {
        let transaction_id = (error.transaction_id() != PROSPECTIVE_TRANSACTION_ID)
            .then_some(error.transaction_id());
        ApiError::from_ledger_error(error, transaction_id)
    })?;
    Ok(())
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

    async fn create_instrument(state: &AppState) -> i64 {
        create_instrument_with(state, "MSFT", "NASDAQ", "Microsoft", "USD").await
    }

    async fn create_instrument_with(
        state: &AppState,
        symbol: &str,
        exchange: &str,
        name: &str,
        currency: &str,
    ) -> i64 {
        let (status, body) = send(
            state,
            "POST",
            "/api/instruments",
            json!({"symbol":symbol,"exchange":exchange,"name":name,"type":"Stock","currency":currency}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        body["id"].as_i64().expect("instrument id")
    }

    #[tokio::test]
    async fn duplicate_instrument_returns_existing() {
        let state = AppState::for_tests().await;
        let first = create_instrument(&state).await;
        let (status, body) = send(
            &state,
            "POST",
            "/api/instruments",
            json!({"symbol":"MSFT","exchange":"NASDAQ","name":"Microsoft Corp","type":"Stock","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"].as_i64(), Some(first));
        assert_eq!(body["name"], "Microsoft");
    }

    #[tokio::test]
    async fn buy_round_trips_through_list() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;

        let (status, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-12",
                   "quantity":10,"price":"12.50","currency":"USD","fx_rate_to_base":"10.0","brokerage":"9.60"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["quantity"], 10);
        assert_eq!(created["price"], "12.50");
        assert_eq!(created["brokerage_currency"], "SEK");

        let (status, list) = send(&state, "GET", "/api/transactions", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list.as_array().expect("array").len(), 1);
    }

    #[tokio::test]
    async fn sell_stores_negative_quantity() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, sold) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":4,"price":"110","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(sold["quantity"], -4);
    }

    #[tokio::test]
    async fn oversell_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":3,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":4,"price":"110","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "sell_exceeds_position");
        assert_eq!(error["error"]["details"], Value::Null);
    }

    #[tokio::test]
    async fn buy_missing_price_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":3,"currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "price_required");
    }

    #[tokio::test]
    async fn split_without_position_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Split","trade_date":"2026-06-01","quantity":8}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "split_without_position");
        assert_eq!(error["error"]["details"], Value::Null);
    }

    #[tokio::test]
    async fn valid_split_after_buy_is_created() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;

        let (status, split) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Split","trade_date":"2026-06-02","quantity":10}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(split["type"], "Split");
        assert_eq!(split["quantity"], 10);
    }

    #[tokio::test]
    async fn dividend_round_trips_through_list() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        // First add some shares so the dividend makes sense (not required by API but realistic)
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":100,"price":"12.50","currency":"USD","fx_rate_to_base":"10.5"}),
        )
        .await;

        let (status, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
                   "quantity":100,"dividend_per_share":"0.25","currency":"USD","fx_rate_to_base":"10.5"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["type"], "Dividend");
        assert_eq!(created["quantity"], 100);
        assert_eq!(created["dividend_per_share"], "0.25");
        assert_eq!(created["currency"], "USD");

        let (status, list) = send(&state, "GET", "/api/transactions", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list.as_array().expect("array").len(), 2);
    }

    #[tokio::test]
    async fn dividend_with_brokerage_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Dividend","trade_date":"2026-06-15",
                   "quantity":100,"dividend_per_share":"0.25","currency":"USD","brokerage":"5.00"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "dividend_must_not_carry_brokerage");
    }

    #[tokio::test]
    async fn put_replaces_fields() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, replaced) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{id}"),
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":12,"price":"105","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(replaced["quantity"], 12);
        assert_eq!(replaced["price"], "105");
    }

    #[tokio::test]
    async fn put_unknown_transaction_is_not_found() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;

        let (status, error) = send(
            &state,
            "PUT",
            "/api/transactions/999",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":12,"price":"105","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn put_can_move_transaction_to_another_instrument() {
        let state = AppState::for_tests().await;
        let source_id = create_instrument(&state).await;
        let target_id = create_instrument_with(&state, "AAPL", "NASDAQ", "Apple", "USD").await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":source_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, moved) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{id}"),
            json!({"instrument_id":target_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"usd"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(moved["instrument_id"], target_id);
        assert_eq!(moved["currency"], "USD");
    }

    #[tokio::test]
    async fn put_move_that_breaks_source_ledger_is_rejected() {
        let state = AppState::for_tests().await;
        let source_id = create_instrument(&state).await;
        let target_id = create_instrument_with(&state, "AAPL", "NASDAQ", "Apple", "USD").await;
        let (_, buy) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":source_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let buy_id = buy["id"].as_i64().expect("id");
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":source_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":6,"price":"110","currency":"USD"}),
        )
        .await;

        let (status, error) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{buy_id}"),
            json!({"instrument_id":target_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "sell_exceeds_position");
    }

    #[tokio::test]
    async fn delete_that_would_break_ledger_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, buy) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let buy_id = buy["id"].as_i64().expect("id");
        send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Sell","trade_date":"2026-06-02",
                   "quantity":6,"price":"110","currency":"USD"}),
        )
        .await;

        let (status, error) = send(
            &state,
            "DELETE",
            &format!("/api/transactions/{buy_id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "sell_exceeds_position");
    }

    #[tokio::test]
    async fn delete_removes_transaction() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, body) = send(
            &state,
            "DELETE",
            &format!("/api/transactions/{id}"),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(body, Value::Null);

        let (status, list) = send(&state, "GET", "/api/transactions", Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(list.as_array().expect("array").len(), 0);
    }

    #[tokio::test]
    async fn transaction_for_unknown_instrument_is_not_found() {
        let state = AppState::for_tests().await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":999,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":1,"price":"100","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error["error"]["code"], "not_found");
    }

    #[tokio::test]
    async fn currency_mismatch_on_create_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":1,"price":"100","currency":"EUR"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "currency_mismatch");
    }

    #[tokio::test]
    async fn transaction_currency_is_normalized_to_instrument_currency() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (status, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":1,"price":"100","currency":"usd"}),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(created["currency"], "USD");
    }

    #[tokio::test]
    async fn currency_mismatch_on_replace_is_rejected() {
        let state = AppState::for_tests().await;
        let instrument_id = create_instrument(&state).await;
        let (_, created) = send(
            &state,
            "POST",
            "/api/transactions",
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"USD"}),
        )
        .await;
        let id = created["id"].as_i64().expect("id");

        let (status, error) = send(
            &state,
            "PUT",
            &format!("/api/transactions/{id}"),
            json!({"instrument_id":instrument_id,"type":"Buy","trade_date":"2026-06-01",
                   "quantity":10,"price":"100","currency":"EUR"}),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(error["error"]["code"], "currency_mismatch");
    }

    #[tokio::test]
    async fn invalid_instrument_fields_are_rejected() {
        let state = AppState::for_tests().await;
        let (status, error) = send(
            &state,
            "POST",
            "/api/instruments",
            json!({"symbol":" ","exchange":"NASDAQ","name":"Microsoft","type":"Stock","currency":"USD"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error["error"]["code"], "invalid_instrument");
    }
}
