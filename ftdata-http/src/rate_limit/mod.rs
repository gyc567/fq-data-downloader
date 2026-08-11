//! Rate limiting module

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Token bucket rate limiter
pub struct TokenBucketLimiter {
    /// Tokens per second
    rate: f64,
    /// Maximum tokens in bucket
    capacity: f64,
    /// Current tokens available
    tokens: parking_lot::Mutex<f64>,
    /// Last refill time
    last_refill: parking_lot::Mutex<Instant>,
    /// Semaphore for concurrency control
    semaphore: Semaphore,
}

impl TokenBucketLimiter {
    pub fn new(requests_per_second: f64, max_concurrent: usize) -> Arc<Self> {
        Arc::new(Self {
            rate: requests_per_second,
            capacity: requests_per_second,
            tokens: parking_lot::Mutex::new(requests_per_second),
            last_refill: parking_lot::Mutex::new(Instant::now()),
            semaphore: Semaphore::new(max_concurrent),
        })
    }

    /// Refill tokens based on elapsed time
    fn refill(&self) {
        let mut tokens = self.tokens.lock();
        let mut last_refill = self.last_refill.lock();
        let elapsed = last_refill.elapsed().as_secs_f64();
        let current_tokens = *tokens;
        let new_tokens = (current_tokens + elapsed * self.rate).min(self.capacity);
        *tokens = new_tokens;
        *last_refill = Instant::now();
    }

    /// Acquire permission to make a request
    pub async fn acquire(&self) -> Result<(), RateLimitError> {
        // Wait for permit
        let _permit = self.semaphore.acquire().await.map_err(|_| RateLimitError::LimitReached)?;

        // Refill and check tokens
        self.refill();

        let tokens = *self.tokens.lock();
        if tokens >= 1.0 {
            *self.tokens.lock() = tokens - 1.0;
            Ok(())
        } else {
            // Wait for tokens
            let wait_time = Duration::from_secs_f64((1.0 - tokens) / self.rate);
            drop(_permit);

            tokio::time::sleep(wait_time).await;

            // Try again without semaphore (we already have it)
            self.refill();
            let tokens = *self.tokens.lock();
            if tokens >= 1.0 {
                *self.tokens.lock() = tokens - 1.0;
                Ok(())
            } else {
                Err(RateLimitError::LimitReached)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum RateLimitError {
    LimitReached,
    WaitFailed,
}

/// Per-exchange rate limiters
pub struct ExchangeRateLimiters {
    binance: Arc<TokenBucketLimiter>,
    bybit: Arc<TokenBucketLimiter>,
    okx: Arc<TokenBucketLimiter>,
}

impl ExchangeRateLimiters {
    pub fn new() -> Self {
        // Binance: 1200 requests per minute = 20 req/s
        // Bybit: 10 requests per second
        // OKX: 20 requests per 2 seconds = 10 req/s
        Self {
            binance: TokenBucketLimiter::new(20.0, 8),
            bybit: TokenBucketLimiter::new(10.0, 8),
            okx: TokenBucketLimiter::new(10.0, 8),
        }
    }

    pub fn for_exchange(&self, exchange: &str) -> Arc<TokenBucketLimiter> {
        match exchange.to_lowercase().as_str() {
            "binance" => self.binance.clone(),
            "bybit" => self.bybit.clone(),
            "okx" => self.okx.clone(),
            _ => self.binance.clone(),
        }
    }
}

impl Default for ExchangeRateLimiters {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket() {
        let limiter = TokenBucketLimiter::new(10.0, 5);
        // Should succeed immediately
        limiter.acquire().await.unwrap();
    }
}
