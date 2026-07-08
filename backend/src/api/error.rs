use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use serde_json::Value;

use crate::db::RepoError;
use crate::domain::{LedgerError, ValidationError};

/// One error shape for the whole API: `{ "error": { code, message, details? } }`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Value>,
}

#[derive(Serialize)]
struct ApiErrorBody {
    error: ApiErrorPayload,
}

#[derive(Serialize)]
struct ApiErrorPayload {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<Value>,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }

    pub fn not_found(resource: &str, id: i64) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{resource} {id} not found"),
        )
    }

    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn demo_read_only() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "demo_read_only",
            "Demo mode is read-only.",
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }

    pub fn from_ledger_error(error: LedgerError, transaction_id: Option<i64>) -> Self {
        let api_error = ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            error.code(),
            ledger_message(error),
        );
        match transaction_id {
            Some(id) => api_error.with_details(serde_json::json!({ "transaction_id": id })),
            None => api_error,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            crate::engine_error!("api error [{}]: {}", self.code, self.message);
        }
        let body = ApiErrorBody {
            error: ApiErrorPayload {
                code: self.code,
                message: self.message,
                details: self.details,
            },
        };
        (self.status, Json(body)).into_response()
    }
}

impl From<ValidationError> for ApiError {
    fn from(error: ValidationError) -> Self {
        ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            error.code(),
            error.message(),
        )
    }
}

impl From<LedgerError> for ApiError {
    fn from(error: LedgerError) -> Self {
        ApiError::from_ledger_error(error, Some(error.transaction_id()))
    }
}

impl From<RepoError> for ApiError {
    fn from(error: RepoError) -> Self {
        ApiError::internal(error.to_string())
    }
}

fn ledger_message(error: LedgerError) -> String {
    match error {
        LedgerError::SellExceedsPosition {
            available,
            requested,
            ..
        } => format!("Sell of {requested} exceeds the available position of {available}."),
        LedgerError::SplitWithoutPosition { .. } => {
            "A split requires an existing position.".to_owned()
        }
        LedgerError::SplitDrivesNonPositive {
            resulting_quantity, ..
        } => {
            format!("Split would drive the position to {resulting_quantity} (must stay positive).")
        }
        LedgerError::BuyMissingPrice { .. } => "A buy requires a native price.".to_owned(),
        LedgerError::SellMissingPrice { .. } => "A sell requires a native price.".to_owned(),
    }
}
