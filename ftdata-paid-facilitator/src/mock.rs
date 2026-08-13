//! Mock facilitator for tests and local development.
//!
//! Accepts any proof whose amount covers the required amount and whose
//! quote_id matches the challenge. Rejects everything else. Useful for
//! end-to-end integration tests of the 402 retry flow without spinning up
//! a real facilitator.

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
    /// quote_id -> required amount (decimal string). Set on `prepare_challenge`,
    /// consumed on `verify`.
    quotes: Mutex<HashMap<String, String>>,
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
    pub fn prepare_challenge(
        &self,
        quote_id: impl Into<String>,
        max_amount: impl Into<String>,
    ) -> PaymentRequired {
        let quote_id = quote_id.into();
        let max_amount = max_amount.into();
        self.quotes
            .lock()
            .unwrap()
            .insert(quote_id.clone(), max_amount.clone());
        PaymentRequired {
            scheme: Scheme::Exact,
            network: Network::Base,
            asset: Asset::Usdc,
            pay_to: "0xMOCK_PAY_TO".to_string(),
            max_amount,
            quote_id,
            // Default expiry is now + 5 minutes. Real impls should use TTL from quote.
            expires_at: UnixSecs::now(),
        }
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
    async fn verify(
        &self,
        required: &PaymentRequired,
        proof: &PaymentProof,
    ) -> Result<VerificationResult, VerificationError> {
        // Honor forced errors first so tests can short-circuit.
        if let Some(err) = self.force_error.lock().unwrap().take() {
            return Err(err);
        }

        // 1. Quote must match what we issued.
        if proof.quote_id != required.quote_id {
            return Err(VerificationError::UnknownOrExpiredQuote {
                quote_id: proof.quote_id.clone(),
            });
        }

        // 2. Quote must not be expired.
        if required.expires_at.is_expired() {
            return Err(VerificationError::UnknownOrExpiredQuote {
                quote_id: required.quote_id.clone(),
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
        // Sub-micro-USDC rounding should still compare.
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
}

