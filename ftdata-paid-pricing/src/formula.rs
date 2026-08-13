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
