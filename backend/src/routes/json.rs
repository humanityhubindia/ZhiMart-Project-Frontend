//! JSON body extractor whose rejections use the API error shape
//! (`400 validation_error`) instead of axum's plain-text rejections.

use axum::{
    Json,
    extract::{FromRequest, Request, rejection::JsonRejection},
    http::StatusCode,
};
use serde::de::DeserializeOwned;

use crate::error::AppError;

pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(map_rejection(&rejection)),
        }
    }
}

fn map_rejection(rejection: &JsonRejection) -> AppError {
    // The rejection text may quote request content; log only its category.
    let msg = match rejection {
        JsonRejection::MissingJsonContentType(_) => {
            "expected request with `Content-Type: application/json`"
        }
        JsonRejection::JsonSyntaxError(_) => "request body is not valid JSON",
        JsonRejection::JsonDataError(_) => "request body has missing, unknown or invalid fields",
        JsonRejection::BytesRejection(_) if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            "request body is too large"
        }
        _ => "could not read request body",
    };
    tracing::debug!(status = %rejection.status(), reason = msg, "rejected JSON body");
    AppError::Validation(msg)
}
