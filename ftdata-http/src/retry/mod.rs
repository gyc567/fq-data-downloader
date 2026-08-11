//! Retry logic with exponential backoff and jitter

use std::time::Duration;
use tokio::time::sleep;

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Initial backoff duration
    pub initial_delay: Duration,
    /// Maximum backoff duration
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub multiplier: f64,
    /// Jitter factor (0.0 to 1.0)
    pub jitter: f64,
    /// Maximum number of retries
    pub max_retries: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(300), // 5 minutes
            multiplier: 2.0,
            jitter: 0.1,
            max_retries: 5,
        }
    }
}

/// Errors that should trigger a retry
#[derive(Debug, Clone)]
pub enum RetryableError {
    Timeout,
    RateLimited(u64), // retry-after seconds
    ConnectionReset,
    ServerError(u16), // 502, 503, 504
    NetworkError(String),
}

impl RetryableError {
    /// Classify an error as retryable or not
    pub fn classify(error: &reqwest::Error) -> Option<Self> {
        if error.is_timeout() {
            Some(RetryableError::Timeout)
        } else if error.is_connect() {
            Some(RetryableError::NetworkError("connection error".into()))
        } else if let Some(status) = error.status() {
            match status.as_u16() {
                429 => {
                    // Retry-After header not easily accessible from Error
                    Some(RetryableError::RateLimited(1))
                }
                502 | 503 | 504 => Some(RetryableError::ServerError(status.as_u16())),
                _ => None,
            }
        } else {
            None
        }
    }
}

/// Calculate the next delay with exponential backoff and jitter
pub fn calculate_delay(
    attempt: u32,
    config: &RetryConfig,
    retry_after: Option<u64>,
) -> Duration {
    // If we have a Retry-After header, use it
    if let Some(seconds) = retry_after {
        return Duration::from_secs(seconds);
    }

    // Exponential backoff
    let base_delay = config.initial_delay.as_secs_f64()
        * config.multiplier.powf(attempt as f64);

    // Cap at max delay
    let delay = base_delay.min(config.max_delay.as_secs_f64());

    // Add jitter
    let jitter_range = delay * config.jitter;
    let jitter = (rand_simple() - 0.5) * 2.0 * jitter_range;

    Duration::from_secs_f64(delay + jitter)
}

/// Simple pseudo-random for jitter (avoiding external deps)
fn rand_simple() -> f64 {
    use std::time::Instant;
    let now = Instant::now();
    let ns = now.elapsed().as_nanos();
    ((ns % 1000) as f64) / 1000.0
}

/// Retry a future with exponential backoff
pub async fn retry_with_backoff<F, Fut, T, E>(
    config: RetryConfig,
    mut f: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut attempt = 0;

    loop {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if attempt >= config.max_retries {
                    return Err(e);
                }

                let delay = calculate_delay(attempt, &config, None);
                tracing::debug!("retry attempt {} after {:?}: {:?}", attempt, delay, e);
                sleep(delay).await;
                attempt += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        let config = RetryConfig::default();
        // Attempt 0: ~1s * 2^0 = 1s (with jitter could be slightly less)
        // Attempt 1: ~1s * 2^1 = 2s
        // Attempt 2: ~1s * 2^2 = 4s
        assert!(calculate_delay(0, &config, None).as_millis() >= 500);
    }
}
