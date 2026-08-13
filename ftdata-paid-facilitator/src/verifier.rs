//! Payment verifier trait.

use async_trait::async_trait;

use crate::types::{
    Asset, Network, PaymentProof, PaymentRequired, Scheme, UnixSecs, VerificationError,
    VerificationResult,
};

/// Async trait implemented by any x402 payment facilitator (real or mock).
///
/// The API layer calls [`PaymentVerifier::verify`] on every request to a
/// paid route. Implementations decide whether the proof is sufficient,
/// whether the underlying transfer settled on-chain, and what error to
/// surface if not.
///
/// Implementations MUST be safe to call concurrently; the API may fan out
/// requests across tasks.
#[async_trait]
pub trait PaymentVerifier: Send + Sync {
    /// Issue a 402 challenge for a quote and store it so it can be matched
    /// against a subsequent proof.
    ///
    /// `amount_usdc_minor` is the price in micro-USDC (1 USDC = 1_000_000).
    /// `ttl_seconds` is the lifetime of the challenge before it expires.
    ///
    /// Mock facilitators typically record (quote_id -> PaymentRequired) so they
    /// can later compare a proof against the original challenge. Real
    /// facilitators issue challenges tied to a specific on-chain escrow or
    /// signed authorization.
    fn prepare_challenge(
        &self,
        quote_id: String,
        amount_usdc_minor: u64,
        ttl_seconds: u64,
    ) -> PaymentRequired;

    /// Verify a payment proof. The facilitator looks up the challenge
    /// stored by `prepare_challenge` using `proof.quote_id` and validates
    /// the proof against it.
    ///
    /// Returns `UnknownOrExpiredQuote` if no matching challenge was issued
    /// or the challenge has expired.
    async fn verify(
        &self,
        proof: &PaymentProof,
    ) -> Result<VerificationResult, VerificationError>;

    /// Identifier of the underlying facilitator (for logging / receipts).
    fn facilitator_id(&self) -> &'static str;
}

/// Helper: format micro-USDC as a 6-decimal string.
pub fn format_minor(minor: u64) -> String {
    let whole = minor / 1_000_000;
    let frac = minor % 1_000_000;
    format!("{whole}.{frac:06}")
}

/// Helper: build a fallback `PaymentRequired` with default scheme/network.
/// Used by verifiers that don't override `prepare_challenge`.
pub fn default_challenge(
    quote_id: String,
    amount_usdc_minor: u64,
    ttl_seconds: u64,
    pay_to: &str,
) -> PaymentRequired {
    let now = UnixSecs::now().0;
    PaymentRequired {
        scheme: Scheme::Exact,
        network: Network::Base,
        asset: Asset::Usdc,
        pay_to: pay_to.to_string(),
        max_amount: format_minor(amount_usdc_minor),
        quote_id,
        expires_at: UnixSecs(now + ttl_seconds),
    }
}
