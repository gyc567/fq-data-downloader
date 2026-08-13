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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Timeframe ----

    #[test]
    fn timeframe_multiplier_table_matches_design() {
        // Pin the §3.1 multiplier table as a closed-loop test.
        assert_eq!(Timeframe::M1.multiplier(), 1.0);
        assert_eq!(Timeframe::M5.multiplier(), 0.6);
        assert_eq!(Timeframe::M15.multiplier(), 0.4);
        assert_eq!(Timeframe::H1.multiplier(), 0.25);
        assert_eq!(Timeframe::H4.multiplier(), 0.15);
        assert_eq!(Timeframe::D1.multiplier(), 0.05);
    }

    #[test]
    fn timeframe_serde_roundtrip_lowercase() {
        for tf in [
            Timeframe::M1,
            Timeframe::M5,
            Timeframe::M15,
            Timeframe::H1,
            Timeframe::H4,
            Timeframe::D1,
        ] {
            let s = serde_json::to_string(&tf).unwrap();
            let back: Timeframe = serde_json::from_str(&s).unwrap();
            assert_eq!(back, tf);
        }
    }

    #[test]
    fn timeframe_serde_uses_canonical_names() {
        assert_eq!(serde_json::to_string(&Timeframe::M1).unwrap(), "\"1m\"");
        assert_eq!(serde_json::to_string(&Timeframe::M5).unwrap(), "\"5m\"");
        assert_eq!(serde_json::to_string(&Timeframe::M15).unwrap(), "\"15m\"");
        assert_eq!(serde_json::to_string(&Timeframe::H1).unwrap(), "\"1h\"");
        assert_eq!(serde_json::to_string(&Timeframe::H4).unwrap(), "\"4h\"");
        assert_eq!(serde_json::to_string(&Timeframe::D1).unwrap(), "\"1d\"");
    }

    // ---- Market ----

    #[test]
    fn market_multiplier_table_matches_design() {
        assert_eq!(Market::Spot.multiplier(), 1.0);
        assert_eq!(Market::Futures.multiplier(), 1.2);
    }

    #[test]
    fn market_serde_roundtrip_lowercase() {
        for m in [Market::Spot, Market::Futures] {
            let s = serde_json::to_string(&m).unwrap();
            let back: Market = serde_json::from_str(&s).unwrap();
            assert_eq!(back, m);
        }
    }

    // ---- PriceQuote::to_x402_string ----

    #[test]
    fn x402_string_format_zero_padded() {
        let q = PriceQuote {
            breakdown: PriceBreakdown {
                base_fee_usdc: 0,
                rows_fee_usdc: 0,
                pair_premium_usdc: 0,
                compute_bonus_usdc: 0,
                free_tier_discount_usdc: 0,
            },
            total_usdc_minor: 1, // smallest non-zero
        };
        assert_eq!(q.to_x402_string(), "0.000001");
    }

    #[test]
    fn x402_string_format_no_scientific_for_large() {
        let q = PriceQuote {
            breakdown: PriceBreakdown {
                base_fee_usdc: 0,
                rows_fee_usdc: 0,
                pair_premium_usdc: 0,
                compute_bonus_usdc: 0,
                free_tier_discount_usdc: 0,
            },
            total_usdc_minor: 100_000_000, // $100
        };
        assert_eq!(q.to_x402_string(), "100.000000");
    }

    #[test]
    fn x402_string_format_one_dollar() {
        let q = PriceQuote {
            breakdown: PriceBreakdown {
                base_fee_usdc: 0,
                rows_fee_usdc: 0,
                pair_premium_usdc: 0,
                compute_bonus_usdc: 0,
                free_tier_discount_usdc: 0,
            },
            total_usdc_minor: 1_000_000,
        };
        assert_eq!(q.to_x402_string(), "1.000000");
    }

    // ---- PricingRequest ----

    #[test]
    fn pricing_request_constructs_with_all_fields() {
        let r = PricingRequest {
            rows: 100,
            pairs_count: 2,
            timeframe: Timeframe::H1,
            market: Market::Futures,
            free_tier_discount_usdc: 50,
            compute_bonus_usdc: 25,
        };
        assert_eq!(r.rows, 100);
        assert_eq!(r.pairs_count, 2);
        assert_eq!(r.timeframe, Timeframe::H1);
        assert_eq!(r.market, Market::Futures);
        assert_eq!(r.free_tier_discount_usdc, 50);
        assert_eq!(r.compute_bonus_usdc, 25);
    }
}

