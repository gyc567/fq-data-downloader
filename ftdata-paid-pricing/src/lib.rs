//! ftdata-paid-pricing
//!
//! Pure-function pricing library for the ftdata-paid x402 data service.
//!
//! **No network. No facilitator. No I/O. No side effects.**
//!
//! Implements the dynamic pricing formula from `docs/PAID_API_DESIGN.md` §3.1
//! and is the closed feedback loop target for the 5 worked examples in §3.2.
//!
//! ## Quick start
//!
//! ```
//! use ftdata_paid_pricing::{PricingRequest, Timeframe, Market, price_quote};
//!
//! let req = PricingRequest {
//!     rows: 43_200,                      // BTC/USDT 1 month 1m
//!     pairs_count: 1,
//!     timeframe: Timeframe::M1,
//!     market: Market::Spot,
//!     free_tier_discount_usdc: 0,
//!     compute_bonus_usdc: 0,
//! };
//!
//! let quote = price_quote(&req).unwrap();
//! // 0.010432 USDC = 10432 micro-USDC
//! assert_eq!(quote.total_usdc_minor, 10_432);
//! ```

pub mod error;
pub mod formula;
pub mod types;

pub use error::PricingError;
pub use formula::price_quote;
pub use types::{Market, PriceBreakdown, PriceQuote, PricingRequest, Timeframe};

// Re-export the well-known constants so callers don't have to redefine them.
pub use formula::{BASE_FEE_USDC, PAIR_PREMIUM_USDC, PER_MILLION_ROWS_USDC};

/// USDC minor unit scale (1 USDC = 1_000_000 micro-USDC).
/// Matches the x402 wire-format precision (6 decimals).
pub const USDC_SCALE: u64 = 1_000_000;
