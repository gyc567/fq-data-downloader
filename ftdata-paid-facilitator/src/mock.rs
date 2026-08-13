//! Mock facilitator for tests and local development.
//!
//! Accepts any proof whose amount covers the stored challenge's amount and
//! whose quote_id matches a previously issued challenge. Rejects everything
//! else. Useful for end-to-end integration tests of the 402 retry flow
//! without spinning up a real facilitator.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::types::{
    Asset, Network, PaymentProof, PaymentRequired, Scheme, UnixSecs, VerificationError,
    VerificationResult,
};
use crate::verifier::PaymentVerifier;

/// In-memory mock facilitator. Thread-safe; safe to share via `Arc`.
#[derive(Debug, Default)]
pub struct MockFacilitator {
    /// quote_id -> PaymentRequired challenge. Set on `prepare_challenge`,
    /// consumed on `verify` (looked up via `proof.quote_id`).
    challenges: Mutex<HashMap<String, PaymentRequired>>,
    /// If set, `verify` always fails with this error. Useful for failure-path tests.
    force_error: Mutex<Option<VerificationError>>,
}

impl MockFacilitator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a challenge so subsequent `verify` calls can match proofs
    /// against it. Returns the challenge; the caller typically serializes
    /// it into the 402 response.
    ///
    /// Unlike the trait method, this helper does not respect TTL — the
    /// challenge expires immediately. Tests that want TTL-aware challenges
    /// should call the trait `prepare_challenge` instead.
    pub fn prepare_challenge_pub(
        &self,
        quote_id: impl Into<String>,
        max_amount: impl Into<String>,
    ) -> PaymentRequired {
        let quote_id = quote_id.into();
        let max_amount = max_amount.into();
        let req = PaymentRequired {
            scheme: Scheme::Exact,
            network: Network::Base,
            asset: Asset::Usdc,
            pay_to: "0xMOCK_PAY_TO".to_string(),
            max_amount,
            quote_id: quote_id.clone(),
            expires_at: UnixSecs::now(),
        };
        self.challenges
            .lock()
            .unwrap()
            .insert(quote_id, req.clone());
        req
    }

    /// Test helper: replace a stored challenge (used to backdate expiry).
    #[doc(hidden)]
    pub fn replace_challenge_for_test(&self, quote_id: &str, new: PaymentRequired) {
        self.challenges
            .lock()
            .unwrap()
            .insert(quote_id.to_string(), new);
    }

    /// Force the next `verify` call to fail with the given error.
    /// Used to test error paths without crafting tricky proofs.
    pub fn force_error(&self, err: VerificationError) {
        *self.force_error.lock().unwrap() = Some(err);
    }

    /// Compare two decimal-USDC strings (e.g. "0.087500" vs "0.090000").
    /// Returns `Some(ord)` where ord is -1/0/1, or `None` on parse error.
    fn cmp_decimals(a: &str, b: &str) -> Option<i8> {
        let parse = |s: &str| -> Option<u128> {
            let (whole, frac) = s.split_once('.')?;
            let whole: u128 = whole.parse().ok()?;
            // Pad/truncate frac to 6 digits to match USDC minor units.
            let frac_padded: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
            let frac: u128 = frac_padded.parse().ok()?;
            Some(whole * 1_000_000 + frac)
        };
        let av = parse(a)?;
        let bv = parse(b)?;
        Some(av.cmp(&bv) as i8)
    }
}

#[async_trait]
impl PaymentVerifier for MockFacilitator {
    fn prepare_challenge(
        &self,
        quote_id: String,
        amount_usdc_minor: u64,
        ttl_seconds: u64,
    ) -> PaymentRequired {
        let now = UnixSecs::now().0;
        let req = PaymentRequired {
            scheme: Scheme::Exact,
            network: Network::Base,
            asset: Asset::Usdc,
            pay_to: "0xMOCK_PAY_TO".to_string(),
            max_amount: crate::verifier::format_minor(amount_usdc_minor),
            quote_id: quote_id.clone(),
            expires_at: UnixSecs(now + ttl_seconds),
        };
        self.challenges
            .lock()
            .unwrap()
            .insert(quote_id, req.clone());
        req
    }

    async fn verify(
        &self,
        proof: &PaymentProof,
    ) -> Result<VerificationResult, VerificationError> {
        // Honor forced errors first so tests can short-circuit.
        if let Some(err) = self.force_error.lock().unwrap().take() {
            return Err(err);
        }

        // 1. Look up the challenge by proof.quote_id.
        let required = self
            .challenges
            .lock()
            .unwrap()
            .get(&proof.quote_id)
            .cloned()
            .ok_or_else(|| VerificationError::UnknownOrExpiredQuote {
                quote_id: proof.quote_id.clone(),
            })?;

        // 2. Quote must not be expired.
        if required.expires_at.is_expired() {
            return Err(VerificationError::UnknownOrExpiredQuote {
                quote_id: proof.quote_id.clone(),
            });
        }

        // 3. Amount must cover the required amount.
        match Self::cmp_decimals(&proof.amount, &required.max_amount) {
            Some(ord) if ord >= 0 => {}
            _ => {
                return Err(VerificationError::InsufficientAmount {
                    got: proof.amount.clone(),
                    required: required.max_amount.clone(),
                });
            }
        }

        // 4. Simulate an on-chain settlement hash.
        let tx_hash = format!("0xMOCK_{}", uuid_like(&proof.signature));

        Ok(VerificationResult::Verified {
            payer: proof.payer.clone(),
            amount: proof.amount.clone(),
            tx_hash,
        })
    }

    fn facilitator_id(&self) -> &'static str {
        "mock"
    }
}

/// Tiny deterministic pseudo-uuid for mock tx hashes. Not cryptographic.
fn uuid_like(input: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in input.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_decimals_orders_correctly() {
        assert_eq!(MockFacilitator::cmp_decimals("0.050000", "0.050000"), Some(0));
        assert_eq!(MockFacilitator::cmp_decimals("0.060000", "0.050000"), Some(1));
        assert_eq!(MockFacilitator::cmp_decimals("0.040000", "0.050000"), Some(-1));
        assert_eq!(MockFacilitator::cmp_decimals("0.000001", "0.0000009"), Some(1));
    }

    #[test]
    fn cmp_decimals_handles_invalid() {
        assert_eq!(MockFacilitator::cmp_decimals("abc", "0.1"), None);
    }

    #[test]
    fn uuid_like_is_deterministic() {
        assert_eq!(uuid_like("foo"), uuid_like("foo"));
        assert_ne!(uuid_like("foo"), uuid_like("bar"));
    }

    #[test]
    fn new_is_default_and_zero_state() {
        let m = MockFacilitator::new();
        assert!(m.challenges.lock().unwrap().is_empty());
        assert!(m.force_error.lock().unwrap().is_none());
    }

    #[test]
    fn prepare_challenge_pub_registers_and_returns() {
        let m = MockFacilitator::new();
        let req = m.prepare_challenge_pub("qt_1", "0.050000");

        assert_eq!(req.quote_id, "qt_1");
        assert_eq!(req.max_amount, "0.050000");
        assert_eq!(req.pay_to, "0xMOCK_PAY_TO");

        // Stored challenge is retrievable.
        let stored = m.challenges.lock().unwrap();
        let entry = stored.get("qt_1").expect("challenge stored");
        assert_eq!(entry.max_amount, "0.050000");
        assert_eq!(entry.quote_id, "qt_1");
    }

    #[test]
    fn force_error_sets_then_clears_on_take() {
        let m = MockFacilitator::new();
        m.force_error(VerificationError::BadSignature {
            reason: "test".to_string(),
        });
        assert!(m.force_error.lock().unwrap().is_some());
        let _ = m.force_error.lock().unwrap().take();
        assert!(m.force_error.lock().unwrap().is_none());
    }

    #[test]
    fn unix_secs_now_is_monotonic_and_recent() {
        let a = UnixSecs::now().0;
        let b = UnixSecs::now().0;
        assert!(b >= a);
        assert!(a > 1_704_067_200);
        assert!(a < 4_102_444_800);
    }

    #[test]
    fn unix_secs_is_expired_compare_correctly() {
        assert!(!UnixSecs::from_secs(u64::MAX).is_expired());
        assert!(UnixSecs::from_secs(0).is_expired());
    }
}
