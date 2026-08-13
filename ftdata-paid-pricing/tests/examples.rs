//! Worked-example tests for the pricing formula.
//!
//! Each test case is taken verbatim from `docs/PAID_API_DESIGN.md` §3.2.
//! The expected `total_usdc_minor` value is the design-doc total expressed in
//! micro-USDC (1 USDC = 1_000_000).
//!
//! The closed feedback loop for the L2 implementation is: every cell in the
//! §3.2 price table must produce exactly the value the design specifies.

use ftdata_paid_pricing::{price_quote, Market, PricingRequest, Timeframe};

/// Helper: convert a design-doc USDC string like "0.087500" to micro-USDC u64.
fn usdc(s: &str) -> u64 {
    let (whole, frac) = s.split_once('.').expect("design doc uses fixed decimals");
    let whole: u64 = whole.parse().expect("whole part");
    // Always pad/truncate frac to 6 digits to match x402 wire format.
    let frac_padded: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
    let frac: u64 = frac_padded.parse().expect("frac part");
    whole * 1_000_000 + frac
}

fn req(rows: u64, pairs: usize, tf: Timeframe, mkt: Market) -> PricingRequest {
    PricingRequest {
        rows,
        pairs_count: pairs,
        timeframe: tf,
        market: mkt,
        free_tier_discount_usdc: 0,
        compute_bonus_usdc: 0,
    }
}

// ------------------------------------------------------------------
// §3.2 worked examples (table rows 1-5)
// ------------------------------------------------------------------

/// Example 1: BTC/USDT 1 month 1m spot → $0.010432
#[test]
fn example_1_btc_1m_one_month() {
    let q = price_quote(&req(43_200, 1, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q.total_usdc_minor, usdc("0.010432"));
    assert_eq!(q.breakdown.base_fee_usdc, 10_000);
    assert_eq!(q.breakdown.pair_premium_usdc, 0);
    // rows_fee should be tiny (43200 / 1M = 0.0432 → rounds to 432 micro-USDC)
    assert_eq!(q.breakdown.rows_fee_usdc, 432);
}

/// Example 2: BTC/USDT 1 year 1m spot → $0.015256
#[test]
fn example_2_btc_1m_one_year() {
    let q = price_quote(&req(525_600, 1, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q.total_usdc_minor, usdc("0.015256"));
    // rows_fee = 525600 / 1M * 10000 = 5256 micro-USDC
    assert_eq!(q.breakdown.rows_fee_usdc, 5_256);
}

/// Example 3: 5 majors 1 year 1m spot → $0.07628
#[test]
fn example_3_five_majors_one_year_1m() {
    let q = price_quote(&req(2_628_000, 5, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q.total_usdc_minor, usdc("0.076280"));
    // rows_fee = 2628000 / 1M * 10000 = 26280
    assert_eq!(q.breakdown.rows_fee_usdc, 26_280);
    // pair_premium = (5-1) * 10000 = 40000
    assert_eq!(q.breakdown.pair_premium_usdc, 40_000);
}

/// Example 4: 10 majors 1m+5m 1 year spot → $0.13153
/// (rows pre-aggregated per the design's worked-example arithmetic)
#[test]
fn example_4_ten_majors_mixed_timeframes_one_year() {
    let q = price_quote(&req(3_153_600, 10, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q.total_usdc_minor, usdc("0.131536"));
    // rows_fee = 3153600 / 1M * 10000 = 31536
    assert_eq!(q.breakdown.rows_fee_usdc, 31_536);
    // pair_premium = (10-1) * 10000 = 90000
    assert_eq!(q.breakdown.pair_premium_usdc, 90_000);
}

/// Example 5: All coins 5 years 1m spot (extreme) → design says $1.46.
/// The design's $1.46 implies 96 pairs (pair_premium = 95 * $0.01 = $0.95).
/// We pin both the design value (with 96 pairs) and the arithmetic value
/// (with 100 pairs → $1.50) to make the discrepancy explicit and testable.
#[test]
fn example_5_all_coins_five_years_1m_extreme() {
    // Design-doc value: $1.46 with 96 pairs.
    let q96 = price_quote(&req(50_000_000, 96, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q96.total_usdc_minor, usdc("1.460000"));
    assert_eq!(q96.breakdown.rows_fee_usdc, 500_000);
    // pair_premium = (96-1) * 10000 = 950_000
    assert_eq!(q96.breakdown.pair_premium_usdc, 950_000);

    // Pure arithmetic with 100 pairs: base + rows + pair = 0.01 + 0.50 + 0.99 = $1.50.
    let q100 = price_quote(&req(50_000_000, 100, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q100.total_usdc_minor, usdc("1.500000"));
}

// ------------------------------------------------------------------
// Multiplier behavior (not covered by §3.2 directly)
// ------------------------------------------------------------------

/// Timeframe multiplier: same row count but 1d instead of 1m should be much cheaper.
/// The 1m rows_fee * 0.05 may round by ±1 micro-USDC vs 1d, so check ratio
/// within tolerance rather than exact equality.
#[test]
fn multiplier_timeframe_1d_cheaper_than_1m() {
    let one_minute = price_quote(&req(525_600, 1, Timeframe::M1, Market::Spot)).unwrap();
    let one_day = price_quote(&req(525_600, 1, Timeframe::D1, Market::Spot)).unwrap();

    // Ratio should be very close to 0.05 (the 1d multiplier).
    let ratio = one_day.breakdown.rows_fee_usdc as f64
        / one_minute.breakdown.rows_fee_usdc as f64;
    assert!(
        (ratio - 0.05).abs() < 0.001,
        "1d/1m ratio should be ~0.05, got {}",
        ratio
    );
    assert!(one_day.total_usdc_minor < one_minute.total_usdc_minor);
}

/// Market multiplier: futures should be 1.2x spot.
#[test]
fn multiplier_futures_more_expensive_than_spot() {
    let spot = price_quote(&req(2_628_000, 1, Timeframe::M1, Market::Spot)).unwrap();
    let fut = price_quote(&req(2_628_000, 1, Timeframe::M1, Market::Futures)).unwrap();
    // rows_fee * 1.2 in spot should equal rows_fee in futures (within 1 micro).
    let diff = fut.breakdown.rows_fee_usdc.abs_diff(spot.breakdown.rows_fee_usdc * 6 / 5);
    assert!(diff <= 1, "futures rows_fee should be ~1.2x spot, diff={}", diff);
}

// ------------------------------------------------------------------
// Edge cases & error handling
// ------------------------------------------------------------------

/// Empty rows: still pay the base fee.
#[test]
fn edge_zero_rows_still_pays_base() {
    let q = price_quote(&req(0, 1, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q.total_usdc_minor, 10_000);
    assert_eq!(q.breakdown.rows_fee_usdc, 0);
}

/// Free tier discount caps at zero, never goes negative.
#[test]
fn edge_free_tier_discount_caps_at_zero() {
    let r = PricingRequest {
        rows: 43_200,
        pairs_count: 1,
        timeframe: Timeframe::M1,
        market: Market::Spot,
        free_tier_discount_usdc: 1_000_000, // absurdly large
        compute_bonus_usdc: 0,
    };
    let q = price_quote(&r).unwrap();
    assert_eq!(q.total_usdc_minor, 0);
}

/// Free tier discount exactly cancels base fee.
#[test]
fn edge_free_tier_discount_exact() {
    let r = PricingRequest {
        rows: 0,
        pairs_count: 1,
        timeframe: Timeframe::M1,
        market: Market::Spot,
        free_tier_discount_usdc: 10_000, // exactly base fee
        compute_bonus_usdc: 0,
    };
    let q = price_quote(&r).unwrap();
    assert_eq!(q.total_usdc_minor, 0);
}

/// compute_bonus for async (CPU overage) path adds to total.
#[test]
fn compute_bonus_added_for_async() {
    let r = PricingRequest {
        rows: 525_600,
        pairs_count: 1,
        timeframe: Timeframe::M1,
        market: Market::Spot,
        free_tier_discount_usdc: 0,
        compute_bonus_usdc: 5_000,
    };
    let q = price_quote(&r).unwrap();
    // Example 2 was 0.015256; with +5000 micro-USDC = 0.020256.
    assert_eq!(q.total_usdc_minor, usdc("0.020256"));
}

/// Zero pairs is an error.
#[test]
fn error_zero_pairs_rejected() {
    let r = PricingRequest {
        rows: 1000,
        pairs_count: 0,
        timeframe: Timeframe::M1,
        market: Market::Spot,
        free_tier_discount_usdc: 0,
        compute_bonus_usdc: 0,
    };
    assert!(price_quote(&r).is_err());
}

// ------------------------------------------------------------------
// x402 wire format
// ------------------------------------------------------------------

/// to_x402_string produces 6-decimal output matching x402 wire format.
#[test]
fn x402_wire_format_examples() {
    let q1 = price_quote(&req(43_200, 1, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q1.to_x402_string(), "0.010432");

    let q5 = price_quote(&req(50_000_000, 96, Timeframe::M1, Market::Spot)).unwrap();
    assert_eq!(q5.to_x402_string(), "1.460000");

    // Large value past the unit boundary.
    let qbig = price_quote(&req(50_000_000_000, 1000, Timeframe::M1, Market::Spot)).unwrap();
    // 50000 base * 10000 micro per pair ... we just check the format.
    assert!(qbig.to_x402_string().contains('.'));
    assert_eq!(qbig.to_x402_string().split('.').nth(1).unwrap().len(), 6);
}
