//! Domain types for the pricing library.

use serde::{Deserialize, Serialize};

/// Supported exchange timeframes. The mapping to the multiplier is part of
/// the pricing contract and is versioned together with the policy file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Timeframe {
    #[serde(rename = "1m")]
    M1,
    #[serde(rename = "5m")]
    M5,
    #[serde(rename = "15m")]
    M15,
    #[serde(rename = "1h")]
    H1,
    #[serde(rename = "4h")]
    H4,
    #[serde(rename = "1d")]
    D1,
}

impl Timeframe {
    /// Multiplier applied to the per-row fee. Lower-frequency data is cheaper
    /// per row because the row density is higher and downstream value density
    /// is lower.
    pub fn multiplier(self) -> f64 {
        match self {
            Timeframe::M1 => 1.0,
            Timeframe::M5 => 0.6,
            Timeframe::M15 => 0.4,
            Timeframe::H1 => 0.25,
            Timeframe::H4 => 0.15,
            Timeframe::D1 => 0.05,
        }
    }
}

/// Market type. Futures data costs more due to extra instrument dimensions
/// (mark, index, premium, funding) even when only OHLCV is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Market {
    Spot,
    Futures,
}

impl Market {
    pub fn multiplier(self) -> f64 {
        match self {
            Market::Spot => 1.0,
            Market::Futures => 1.2,
        }
    }
}

/// Caller-supplied input for a price quote.
///
/// `rows` is the **total** rows across all pairs and timeframes in the request;
/// the caller is responsible for aggregation. For mixed-timeframe requests the
/// pricing library uses the dominant (highest-resolution) timeframe, so the
/// caller should also pick `timeframe` accordingly and may pre-average the
/// rows for cross-timeframe queries (see §3.2 worked example #4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricingRequest {
    pub rows: u64,
    pub pairs_count: usize,
    pub timeframe: Timeframe,
    pub market: Market,

    /// Free-tier credit applied as a discount in micro-USDC. 0 for paying users.
    pub free_tier_discount_usdc: u64,

    /// Extra fee charged when a request blows the Workers CPU soft cap and
    /// has to be routed through the async path. 0 for sync requests.
    pub compute_bonus_usdc: u64,
}

/// Per-component breakdown of a price quote. All fields are micro-USDC
/// (1 USDC = 1_000_000) so arithmetic is exact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceBreakdown {
    pub base_fee_usdc: u64,
    pub rows_fee_usdc: u64,
    pub pair_premium_usdc: u64,
    pub compute_bonus_usdc: u64,
    pub free_tier_discount_usdc: u64,
}

/// A complete price quote returned by `price_quote`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriceQuote {
    pub breakdown: PriceBreakdown,
    /// Sum of all positive components minus the discount, in micro-USDC.
    pub total_usdc_minor: u64,
}

impl PriceQuote {
    /// Format the total as an x402 wire string, e.g. `"0.087500"`.
    /// Always 6 decimals, always positive, never uses scientific notation.
    pub fn to_x402_string(&self) -> String {
        format!(
            "{}.{:06}",
            self.total_usdc_minor / crate::USDC_SCALE,
            self.total_usdc_minor % crate::USDC_SCALE
        )
    }
}
