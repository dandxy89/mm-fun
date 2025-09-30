use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("HTTP request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Circuit breaker open")]
    CircuitBreakerOpen,

    #[error("Timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("JSON parsing error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("API error: {code} - {message}")]
    ApiError { code: i64, message: String },
}

pub type Result<T> = std::result::Result<T, HttpError>;
