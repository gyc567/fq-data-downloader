//! Domain errors for ftdata

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("rate limited, retry after {0}s")]
    RateLimited(u64),

    #[error("exchange ban (418), cooling down")]
    ExchangeBan,

    #[error("invalid response from exchange: {0}")]
    InvalidResponse(String),

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("data gap detected: {from} → {to}")]
    DataGap { from: i64, to: i64 },

    #[error("invalid OHLCV at row {row}: {reason}")]
    InvalidOHLCV { row: u64, reason: String },

    #[error("unsupported exchange: {0}")]
    UnsupportedExchange(String),

    #[error("unsupported timeframe: {0}")]
    UnsupportedTimeframe(String),

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("resume state corrupted: {0}")]
    ResumeStateCorrupted(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("validation error: {0}")]
    Validation(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("rate limiter error: {0}")]
    RateLimiter(String),

    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("max retries exceeded")]
    MaxRetriesExceeded,
}

impl DownloadError {
    /// Whether this error should trigger a retry
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            DownloadError::Network(_)
                | DownloadError::RateLimited(_)
                | DownloadError::Timeout(_)
                | DownloadError::InvalidResponse(_)
        )
    }

    /// Whether this error indicates a permanent failure (no retry)
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            DownloadError::ChecksumMismatch { .. }
                | DownloadError::UnsupportedExchange(_)
                | DownloadError::UnsupportedTimeframe(_)
                | DownloadError::ExchangeBan
        )
    }

    /// Serialize to JSON for MCP responses
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "error": self.to_string(),
            "type": self.type_name(),
            "retryable": self.is_retryable(),
            "fatal": self.is_fatal(),
        })
    }

    fn type_name(&self) -> &'static str {
        match self {
            DownloadError::Network(_) => "network",
            DownloadError::RateLimited(_) => "rate_limited",
            DownloadError::ExchangeBan => "exchange_ban",
            DownloadError::InvalidResponse(_) => "invalid_response",
            DownloadError::ChecksumMismatch { .. } => "checksum_mismatch",
            DownloadError::DataGap { .. } => "data_gap",
            DownloadError::InvalidOHLCV { .. } => "invalid_ohlcv",
            DownloadError::UnsupportedExchange(_) => "unsupported_exchange",
            DownloadError::UnsupportedTimeframe(_) => "unsupported_timeframe",
            DownloadError::Storage(_) => "storage",
            DownloadError::ResumeStateCorrupted(_) => "resume_state_corrupted",
            DownloadError::Parse(_) => "parse",
            DownloadError::Validation(_) => "validation",
            DownloadError::Database(_) => "database",
            DownloadError::RateLimiter(_) => "rate_limiter",
            DownloadError::Timeout(_) => "timeout",
            DownloadError::MaxRetriesExceeded => "max_retries_exceeded",
        }
    }
}

impl From<rusqlite::Error> for DownloadError {
    fn from(e: rusqlite::Error) -> Self {
        DownloadError::Database(e.to_string())
    }
}

impl From<serde_json::Error> for DownloadError {
    fn from(e: serde_json::Error) -> Self {
        DownloadError::Parse(e.to_string())
    }
}

impl From<csv::Error> for DownloadError {
    fn from(e: csv::Error) -> Self {
        DownloadError::Parse(e.to_string())
    }
}

impl From<zip::result::ZipError> for DownloadError {
    fn from(e: zip::result::ZipError) -> Self {
        DownloadError::Parse(format!("zip error: {}", e))
    }
}

/// Result type alias for download operations
pub type DownloadResult<T> = Result<T, DownloadError>;
