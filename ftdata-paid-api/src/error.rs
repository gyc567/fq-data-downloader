//! API error types and HTTP response mapping.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("payment required")]
    PaymentRequired {
        challenge: ftdata_paid_facilitator::PaymentRequired,
    },

    #[error("payment insufficient")]
    PaymentInsufficient { got: String, required: String },

    #[error("payment invalid: {reason}")]
    PaymentInvalid { reason: String },

    #[error("payment expired")]
    PaymentExpired { quote_id: String },

    #[error("job not found: {0}")]
    JobNotFound(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl ApiError {
    fn code(&self) -> &'static str {
        match self {
            ApiError::BadRequest(_) => "bad_request",
            ApiError::PaymentRequired { .. } => "payment_required",
            ApiError::PaymentInsufficient { .. } => "payment_insufficient",
            ApiError::PaymentInvalid { .. } => "payment_invalid",
            ApiError::PaymentExpired { .. } => "payment_expired",
            ApiError::JobNotFound(_) => "job_not_found",
            ApiError::Internal(_) => "internal_error",
        }
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::PaymentRequired { .. }
            | ApiError::PaymentInsufficient { .. }
            | ApiError::PaymentInvalid { .. }
            | ApiError::PaymentExpired { .. } => StatusCode::PAYMENT_REQUIRED,
            ApiError::JobNotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = match &self {
            ApiError::PaymentRequired { challenge } => {
                json!({
                    "error": self.code(),
                    "message": "Payment required",
                    "payment_required": challenge,
                })
            }
            ApiError::PaymentInsufficient { got, required } => {
                json!({
                    "error": self.code(),
                    "message": "Payment amount insufficient",
                    "got": got,
                    "required": required,
                })
            }
            ApiError::PaymentInvalid { reason } => {
                json!({
                    "error": self.code(),
                    "message": "Payment proof rejected",
                    "reason": reason,
                })
            }
            ApiError::PaymentExpired { quote_id } => {
                json!({
                    "error": self.code(),
                    "message": "Quote has expired",
                    "quote_id": quote_id,
                })
            }
            ApiError::JobNotFound(id) => {
                json!({
                    "error": self.code(),
                    "message": format!("Job {} not found", id),
                })
            }
            ApiError::BadRequest(msg) => {
                json!({ "error": self.code(), "message": msg })
            }
            ApiError::Internal(msg) => {
                json!({ "error": self.code(), "message": msg })
            }
        };
        (status, Json(body)).into_response()
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
