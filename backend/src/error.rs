use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use regex::Regex;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Authentication required or token invalid")]
    Unauthorized,

    #[error("Access denied: {0}")]
    Forbidden(String),

    #[error("Invalid request: {0}")]
    BadRequest(String),

    #[error("Resource not found: {0}")]
    NotFound(String),

    #[error("SQL validation rejected: {0}")]
    SqlValidationRejected(String),

    #[error("Adapter unavailable: {0}")]
    AdapterUnavailable(String),

    #[error("Database error: {0}")]
    DatabaseError(String),

    #[error("LLM provider error: {0}")]
    LlmError(String),

    #[error("Query execution timed out")]
    Timeout,

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
    code: &'static str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, msg) = match &self {
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized", self.to_string()),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden", self.to_string()),
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request", self.to_string()),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "not_found", self.to_string()),
            AppError::SqlValidationRejected(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "sql_validation_rejected",
                self.to_string(),
            ),
            AppError::AdapterUnavailable(_) => (
                StatusCode::NOT_IMPLEMENTED,
                "adapter_unavailable",
                self.to_string(),
            ),
            AppError::DatabaseError(_) => (
                StatusCode::BAD_GATEWAY,
                "database_error",
                sanitize_error_message(&self.to_string()),
            ),
            AppError::LlmError(_) => (
                StatusCode::BAD_GATEWAY,
                "llm_error",
                sanitize_error_message(&self.to_string()),
            ),
            AppError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                "execution_timeout",
                self.to_string(),
            ),
            AppError::Internal(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "An unexpected internal error occurred.".to_string(),
            ),
        };

        (status, Json(ErrorBody { error: msg, code })).into_response()
    }
}

/// Strips potential connection URLs, passwords, or Bearer tokens before errors leave the backend.
pub fn sanitize_error_message(raw: &str) -> String {
    let url_re = Regex::new(r"(?i)(postgres(?:ql)?://[^\s@]+@[^\s]+|bearer\s+[a-z0-9._-]+)")
        .expect("valid regex");
    let redacted = url_re.replace_all(raw, "[REDACTED_CREDENTIAL]").to_string();
    let pw_re = Regex::new(r"(?i)(password|secret|api[_-]?key|token)\s*[:=]\s*\S+")
        .expect("valid regex");
    pw_re.replace_all(&redacted, "$1=[REDACTED]").to_string()
}
