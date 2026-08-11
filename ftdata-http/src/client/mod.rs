//! HTTP client with range support, retry, and rate limiting

use crate::range::{ByteRange, RangeSupport, is_partial_content};
use crate::retry::RetryConfig;
use crate::rate_limit::TokenBucketLimiter;
use futures::StreamExt;
use reqwest::{Client, Version};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// HTTP client for downloading data
pub struct HttpClient {
    client: Client,
    rate_limiter: Arc<TokenBucketLimiter>,
    retry_config: RetryConfig,
}

impl HttpClient {
    pub fn new(rate_limiter: Arc<TokenBucketLimiter>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .expect("failed to create HTTP client");

        Self {
            client,
            rate_limiter,
            retry_config: RetryConfig::default(),
        }
    }

    /// Set custom retry config
    pub fn with_retry_config(mut self, config: RetryConfig) -> Self {
        self.retry_config = config;
        self
    }

    /// Check if server supports Range requests
    pub async fn check_range_support(&self, url: &str) -> Result<RangeSupport, reqwest::Error> {
        let response = self.client.head(url).send().await?;
        let headers = response.headers();

        Ok(RangeSupport {
            supports_range: RangeSupport::parse_accept_ranges(headers)
                .map(|v| v != "none")
                .unwrap_or(false),
            accept_ranges: RangeSupport::parse_accept_ranges(headers),
            content_length: RangeSupport::parse_content_length(headers),
            etag: RangeSupport::parse_etag(headers),
            last_modified: RangeSupport::parse_last_modified(headers),
        })
    }

    /// Download a full file with streaming
    pub async fn download_file(&self, url: &str) -> Result<Vec<u8>, DownloadError> {
        self.rate_limiter.acquire().await.map_err(|_| {
            DownloadError::RateLimiter("rate limiter error".into())
        })?;

        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(DownloadError::Network(response.error_for_status().unwrap_err()));
        }

        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }

    /// Download with range support (resume)
    pub async fn download_range(
        &self,
        url: &str,
        range: ByteRange,
        expected_length: Option<u64>,
    ) -> Result<(Vec<u8>, RangeSupport), DownloadError> {
        self.rate_limiter.acquire().await.map_err(|_| {
            DownloadError::RateLimiter("rate limiter error".into())
        })?;

        let mut request = self.client.get(url);
        request = request
            .header("Range", range.to_string())
            .version(Version::HTTP_11);

        let response = request.send().await?;

        // Check for 416 Range Not Satisfiable
        if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(DownloadError::InvalidResponse(
                "Range not satisfiable (416)".into(),
            ));
        }

        let headers = response.headers().clone();
        let range_support = RangeSupport {
            supports_range: true,
            accept_ranges: RangeSupport::parse_accept_ranges(&headers),
            content_length: RangeSupport::parse_content_length(&headers),
            etag: RangeSupport::parse_etag(&headers),
            last_modified: RangeSupport::parse_last_modified(&headers),
        };

        if !is_partial_content(&response) {
            // Server doesn't support Range, download entire file
            let bytes = response.bytes().await?;
            return Ok((bytes.to_vec(), range_support));
        }

        let bytes = response.bytes().await?;
        Ok((bytes.to_vec(), range_support))
    }

    /// Download to a file with resume support
    pub async fn download_to_file(
        &self,
        url: &str,
        path: &std::path::Path,
        resume_from: u64,
    ) -> Result<u64, DownloadError> {
        // Check server support
        let support = self.check_range_support(url).await?;

        let (mut file, total_size) = if resume_from > 0 && support.supports_range {
            // Resume: open existing file and seek to end
            let file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await?;

            let metadata = file.metadata().await?;
            let current_size = metadata.len();

            info!(
                "Resuming download from byte {}, server supports range: {}",
                current_size,
                support.supports_range
            );

            (file, support.content_length)
        } else {
            // Fresh download
            let file = tokio::fs::File::create(path).await?;
            (file, support.content_length)
        };

        let start_byte = file.metadata().await.map(|m| m.len()).unwrap_or(0);

        // Build request with Range header if resuming
        let mut request = self.client.get(url);
        if start_byte > 0 {
            let range = ByteRange::from_start(start_byte);
            request = request.header("Range", range.to_string());
            debug!("Requesting range: {}", range);
        }

        let response = request.send().await?;

        if !response.status().is_success() && !is_partial_content(&response) {
            return Err(DownloadError::Network(
                response.error_for_status().unwrap_err(),
            ));
        }

        let mut stream = response.bytes_stream();
        let mut bytes_written = start_byte;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DownloadError::Network(e))?;
            file.write_all(&chunk).await?;
            bytes_written += chunk.len() as u64;
        }

        file.flush().await?;

        Ok(bytes_written)
    }

    /// Fetch JSON API response
    pub async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, DownloadError> {
        self.rate_limiter.acquire().await.map_err(|_| {
            DownloadError::RateLimiter("rate limiter error".into())
        })?;

        let response = self.client.get(url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            if status.as_u16() == 429 || status.as_u16() == 502 || status.as_u16() == 503 || status.as_u16() == 504 {
                warn!("Retryable HTTP error: {}", status);
                return Err(DownloadError::RateLimiter("rate limited".into()));
            }
            return Err(DownloadError::Network(
                response.error_for_status().unwrap_err(),
            ));
        }

        let json = response.json::<T>().await?;
        Ok(json)
    }
}

#[derive(Debug)]
pub enum DownloadError {
    Network(reqwest::Error),
    RateLimiter(String),
    InvalidResponse(String),
    Io(std::io::Error),
}

impl From<reqwest::Error> for DownloadError {
    fn from(e: reqwest::Error) -> Self {
        DownloadError::Network(e)
    }
}

impl From<std::io::Error> for DownloadError {
    fn from(e: std::io::Error) -> Self {
        DownloadError::Io(e)
    }
}
