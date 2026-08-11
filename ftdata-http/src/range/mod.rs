//! HTTP Range header support for resumable downloads

use http::HeaderMap;
use http::header::{HeaderValue, RANGE};
use reqwest::Response;
use std::fmt;

/// Represents a byte range for HTTP Range requests
#[derive(Debug, Clone, Copy)]
pub struct ByteRange {
    pub start: u64,
    pub end: Option<u64>, // None means to the end
}

impl fmt::Display for ByteRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.end {
            Some(end) => write!(f, "bytes={}-{}", self.start, end),
            None => write!(f, "bytes={}-", self.start),
        }
    }
}

impl ByteRange {
    /// Create a range from `start` to the end
    pub fn from_start(start: u64) -> Self {
        Self { start, end: None }
    }

    /// Create a range from `start` to `end` (inclusive)
    pub fn from_start_to_end(start: u64, end: u64) -> Self {
        Self { start: start, end: Some(end) }
    }

    /// Create a range for the last `length` bytes
    pub fn suffix(length: u64) -> Self {
        Self { start: 0, end: Some(length - 1) }
    }

    /// Get Content-Range header value
    pub fn content_range_header(&self, total_size: Option<u64>) -> String {
        match (self.end, total_size) {
            (Some(end), Some(total)) => {
                format!("bytes {}-{}/{}", self.start, end, total)
            }
            _ => format!("bytes {}-*", self.start),
        }
    }
}

/// Range support check result
#[derive(Debug)]
pub struct RangeSupport {
    /// Whether the server supports Range requests
    pub supports_range: bool,
    /// Accept-Ranges header value
    pub accept_ranges: Option<String>,
    /// Content-Length of the full resource
    pub content_length: Option<u64>,
    /// ETag of the resource
    pub etag: Option<String>,
    /// Last-Modified date
    pub last_modified: Option<String>,
}

impl RangeSupport {
    /// Parse Accept-Ranges header
    pub fn parse_accept_ranges(headers: &HeaderMap) -> Option<String> {
        headers
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Parse Content-Length header
    pub fn parse_content_length(headers: &HeaderMap) -> Option<u64> {
        headers
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok())
    }

    /// Parse ETag header
    pub fn parse_etag(headers: &HeaderMap) -> Option<String> {
        headers
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }

    /// Parse Last-Modified header
    pub fn parse_last_modified(headers: &HeaderMap) -> Option<String> {
        headers
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }
}

/// Build Range header
pub fn range_header(range: &ByteRange) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        RANGE,
        HeaderValue::from_str(&range.to_string()).unwrap(),
    );
    headers
}

/// Check if response is a range response (206)
pub fn is_partial_content(response: &Response) -> bool {
    response.status() == reqwest::StatusCode::PARTIAL_CONTENT
}

/// Parse Content-Range header
pub fn parse_content_range(headers: &HeaderMap) -> Option<(u64, u64, u64)> {
    // Format: "bytes start-end/length" or "bytes */length"
    headers
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            let parts: Vec<&str> = s.split('/').collect();
            if parts.len() != 2 {
                return None;
            }
            let range_part = parts[0].strip_prefix("bytes ")?;
            let length: u64 = parts[1].parse().ok()?;
            let range_parts: Vec<&str> = range_part.split('-').collect();
            if range_parts.len() != 2 {
                return None;
            }
            let start: u64 = range_parts[0].parse().ok()?;
            let end: u64 = range_parts[1].parse().ok()?;
            Some((start, end, length))
        })
}
