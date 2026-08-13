//! Pricing formula.
//!
//! Implements `docs/PAID_API_DESIGN.md` §3.1:
//!
//! ```text
//! price = base_fee
//!       + rows_fee * timeframe_multiplier * market_multiplier
//!       + pair_premium
//!       + compute_bonus
//!       - free_tier_discount
//! ```
//!
//! Arithmetic strategy:
//! - Integer micro-USDC (`u64`) for base_fee, pair_premium, compute_bonus, free_tier_discount.
//! - `rows_fee` is computed in `u128` to avoid overflow on `rows * PER_MILLION_ROWS_USDC`,
//!   then converted to `u64` after the `f64` multiplier (bounded to [0.05, 1.2]) is
//!   applied with `round()`. This avoids silent u64 overflow while keeping float
//!   usage to a single bounded multiplication.

use crate::error::PricingError;
use crate::types::{PriceBreakdown, PriceQuote, PricingRequest};

/// Base fee per request, micro-USDC. $0.01.
pub const BASE_FEE_USDC: u64 = 10_000;

/// Per-million-rows fee, micro-USDC. $0.01 per 1M K-lines.
pub const PER_MILLION_ROWS_USDC: u64 = 10_000;

/// Per-extra-pair surcharge, micro-USDC. $0.01 per pair after the first.
pub const PAIR_PREMIUM_USDC: u64 = 10_000;

const ROWS_PER_MILLION: u128 = 1_000_000;

/// Compute a price quote.
///
/// # Errors
///
/// - `PricingError::InvalidPairCount` if `pairs_count == 0`.
/// - `PricingError::InternalOverflow` if the pre-discount sum would overflow
///   `u64` (effectively impossible for realistic inputs).
pub fn price_quote(req: &PricingRequest) -> Result<PriceQuote, PricingError> {
    if req.pairs_count == 0 {
        return Err(PricingError::InvalidPairCount {
            got: req.pairs_count,
        });
    }

    let base_fee_usdc = BASE_FEE_USDC;

    // rows_fee_base = rows * PER_MILLION_ROWS_USDC / ROWS_PER_MILLION
    //              = rows / 100  (in micro-USDC, at multiplier = 1.0)
    //
    // Done in u128 to stay safe even at u64::MAX rows.
    let rows = req.rows as u128;
    let rows_fee_base_u128 = rows
        .checked_mul(PER_MILLION_ROWS_USDC as u128)
        .ok_or(PricingError::InternalOverflow {
            context: "rows * PER_MILLION_ROWS_USDC",
        })?
        / ROWS_PER_MILLION;

    let tf_mult = req.timeframe.multiplier();
    let mkt_mult = req.market.multiplier();
    let combined_mult = tf_mult * mkt_mult;

    // Apply the multiplier in f64 (bounded to ~[0.05, 1.2]) and round back.
    // The result fits in u64: rows_fee_base is at most ~1.8e14 and the
    // multiplier caps it at ~2.2e14 micro-USDC ≈ $220k per request.
    let rows_fee_usdc = (rows_fee_base_u128 as f64 * combined_mult).round() as u64;

    let pair_premium_usdc = (req.pairs_count as u64 - 1) * PAIR_PREMIUM_USDC;
    let compute_bonus_usdc = req.compute_bonus_usdc;

    let pre_discount = base_fee_usdc
        .checked_add(rows_fee_usdc)
        .and_then(|v| v.checked_add(pair_premium_usdc))
        .and_then(|v| v.checked_add(compute_bonus_usdc))
        .ok_or(PricingError::InternalOverflow {
            context: "pre-discount sum",
        })?;

    let total_usdc_minor = pre_discount.saturating_sub(req.free_tier_discount_usdc);

    Ok(PriceQuote {
        breakdown: PriceBreakdown {
            base_fee_usdc,
            rows_fee_usdc,
            pair_premium_usdc,
            compute_bonus_usdc,
            free_tier_discount_usdc: req.free_tier_discount_usdc,
        },
        total_usdc_minor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Market, Timeframe};

    fn r(rows: u64, pairs: usize, tf: Timeframe, mkt: Market) -> PricingRequest {
        PricingRequest {
            rows,
            pairs_count: pairs,
            timeframe: tf,
            market: mkt,
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 0,
        }
    }

    // Direct unit coverage of the formula's invariants. Integration tests in
    // tests/examples.rs cover the design-doc worked examples end-to-end.

    #[test]
    fn base_fee_is_minimum_charge_even_at_zero_rows() {
        let q = price_quote(&r(0, 1, Timeframe::M1, Market::Spot)).unwrap();
        assert_eq!(q.total_usdc_minor, BASE_FEE_USDC);
    }

    #[test]
    fn pair_premium_increments_per_extra_pair() {
        let q1 = price_quote(&r(0, 1, Timeframe::M1, Market::Spot)).unwrap();
        let q2 = price_quote(&r(0, 2, Timeframe::M1, Market::Spot)).unwrap();
        let q5 = price_quote(&r(0, 5, Timeframe::M1, Market::Spot)).unwrap();
        assert_eq!(q2.total_usdc_minor - q1.total_usdc_minor, PAIR_PREMIUM_USDC);
        assert_eq!(q5.total_usdc_minor - q1.total_usdc_minor, 4 * PAIR_PREMIUM_USDC);
    }

    #[test]
    fn rows_fee_scales_linearly_with_rows() {
        let a = price_quote(&r(100_000, 1, Timeframe::M1, Market::Spot)).unwrap();
        let b = price_quote(&r(200_000, 1, Timeframe::M1, Market::Spot)).unwrap();
        // Doubling rows should double rows_fee (with at most 1 micro-USDC rounding).
        let expected = 2 * a.breakdown.rows_fee_usdc;
        let diff = b.breakdown.rows_fee_usdc.abs_diff(expected);
        assert!(diff <= 1, "rows_fee should scale linearly, got diff={}", diff);
    }

    #[test]
    fn saturating_sub_keeps_total_non_negative() {
        let req = PricingRequest {
            rows: 0,
            pairs_count: 1,
            timeframe: Timeframe::M1,
            market: Market::Spot,
            free_tier_discount_usdc: u64::MAX,
            compute_bonus_usdc: 0,
        };
        let q = price_quote(&req).unwrap();
        assert_eq!(q.total_usdc_minor, 0);
    }

    #[test]
    fn compute_bonus_adds_directly_to_total() {
        let baseline = price_quote(&r(0, 1, Timeframe::M1, Market::Spot)).unwrap();
        let with_bonus = price_quote(&PricingRequest {
            rows: 0,
            pairs_count: 1,
            timeframe: Timeframe::M1,
            market: Market::Spot,
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 7_777,
        })
        .unwrap();
        assert_eq!(
            with_bonus.total_usdc_minor - baseline.total_usdc_minor,
            7_777
        );
    }

    #[test]
    fn internal_overflow_unreachable_for_realistic_rows() {
        // Sanity: even u64::MAX rows should not overflow the internal u128 step.
        // (We test that it doesn't error, not that it returns a specific value.)
        let req = PricingRequest {
            rows: u64::MAX,
            pairs_count: 1,
            timeframe: Timeframe::M1,
            market: Market::Spot,
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 0,
        };
        let q = price_quote(&req);
        assert!(q.is_ok(), "u64::MAX rows should not overflow");
    }
}

