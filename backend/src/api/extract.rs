use axum::{
    body::Bytes,
    extract::{rejection::*, FromRequest},
    http::{Request, StatusCode},
    Json,
};
use serde::de::DeserializeOwned;

use crate::api::{
    body_limits::{API_BODY_LIMIT_BYTES, BYTES_PER_MIB},
    ApiError,
};

pub struct ApiJson<T>(pub T);

pub struct ImportBody(pub Bytes);

enum BodyRejection {
    Json(JsonRejection),
    Bytes(BytesRejection),
}

impl<T, S> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(
        request: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|rejection| {
                map_body_rejection(BodyRejection::Json(rejection), API_BODY_LIMIT_BYTES)
            })
    }
}

impl<S> FromRequest<S> for ImportBody
where
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(
        request: Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        Bytes::from_request(request, state)
            .await
            .map(Self)
            .map_err(|rejection| {
                map_body_rejection(
                    BodyRejection::Bytes(rejection),
                    crate::api::body_limits::IMPORT_BODY_LIMIT_BYTES,
                )
            })
    }
}

fn map_body_rejection(rejection: BodyRejection, limit_bytes: usize) -> ApiError {
    match rejection {
        BodyRejection::Json(JsonRejection::JsonDataError(_))
        | BodyRejection::Json(JsonRejection::JsonSyntaxError(_)) => {
            ApiError::bad_request("invalid_json", "Request body contains invalid JSON.")
        }
        BodyRejection::Json(JsonRejection::MissingJsonContentType(_)) => ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "unsupported_media_type",
            "Request body must use Content-Type: application/json.",
        ),
        BodyRejection::Json(JsonRejection::BytesRejection(rejection))
        | BodyRejection::Bytes(rejection) => map_bytes_rejection(rejection, limit_bytes),
        BodyRejection::Json(_) => {
            ApiError::bad_request("invalid_json", "Request body contains invalid JSON.")
        }
    }
}

fn map_bytes_rejection(rejection: BytesRejection, limit_bytes: usize) -> ApiError {
    match rejection {
        BytesRejection::FailedToBufferBody(FailedToBufferBody::LengthLimitError(_)) => {
            let limit_mb = limit_bytes / BYTES_PER_MIB;
            ApiError::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "payload_too_large",
                format!("Request body exceeds the {limit_mb} MB limit."),
            )
        }
        _ => ApiError::bad_request("missing_body", "Request body could not be read."),
    }
}
