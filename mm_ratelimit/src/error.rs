use std::fmt;

/// Result type for rate limiting operations
pub type Result<T> = std::result::Result<T, RateLimitError>;

/// Errors that can occur during rate limiting operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitError {
    /// Rate limit exceeded - no tokens/quota available
    Exceeded,

    /// Invalid configuration
    InvalidConfig(&'static str),

    /// System time error
    TimeError,
}

impl fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RateLimitError::Exceeded => write!(f, "Rate limit exceeded"),
            RateLimitError::InvalidConfig(msg) => write!(f, "Invalid rate limiter configuration: {}", msg),
            RateLimitError::TimeError => write!(f, "System time error"),
        }
    }
}

impl std::error::Error for RateLimitError {}
