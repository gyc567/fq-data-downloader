//! Payment verifier trait.

use async_trait::async_trait;

use crate::types::{PaymentProof, PaymentRequired, VerificationError, VerificationResult};

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
    /// Verify a payment proof against the challenge it claims to satisfy.
    async fn verify(
        &self,
        required: &PaymentRequired,
        proof: &PaymentProof,
    ) -> Result<VerificationResult, VerificationError>;

    /// Identifier of the underlying facilitator (for logging / receipts).
    fn facilitator_id(&self) -> &'static str;
}
