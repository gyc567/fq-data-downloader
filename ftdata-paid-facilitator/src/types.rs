//! Wire-format types for the x402 payment flow.
//!
//! These mirror the JSON shapes in `docs/PAID_API_DESIGN.md` §2.1 and §7.1.

use serde::{Deserialize, Serialize};

/// The 402 challenge the server returns when a paid route is hit without
/// sufficient payment. The agent signs this and resubmits as `PaymentProof`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentRequired {
    pub scheme: Scheme,
    pub network: Network,
    pub asset: Asset,
    pub pay_to: String,
    /// Decimal string, 6 places, e.g. "0.087500".
    pub max_amount: String,
    pub quote_id: String,
    pub expires_at: UnixSecs,
}

/// What the agent submits in the `X-PAYMENT` header on retry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentProof {
    pub scheme: Scheme,
    pub network: Network,
    pub asset: Asset,
    /// Wallet address of the payer.
    pub payer: String,
    /// Decimal string.
    pub amount: String,
    pub quote_id: String,
    /// Base64-encoded EIP-3009 / signed authorization payload. Decoded
    /// contents are facilitator-specific; the trait verifier owns them.
    pub signature: String,
    pub nonce: String,
    pub valid_until: UnixSecs,
}

/// Outcome of a `PaymentVerifier::verify` call. Verified proofs gate access
/// to the protected route; rejected proofs map to a 402 error response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationResult {
    Verified {
        payer: String,
        amount: String,
        /// Populated by the facilitator after on-chain confirmation.
        tx_hash: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum VerificationError {
    #[error("proof amount {got} is less than required {required}")]
    InsufficientAmount { got: String, required: String },

    #[error("quote {quote_id} has expired or is unknown")]
    UnknownOrExpiredQuote { quote_id: String },

    #[error("signature failed verification: {reason}")]
    BadSignature { reason: String },

    #[error("facilitator unavailable: {reason}")]
    FacilitatorUnavailable { reason: String },
}

// --- Enums with stable serde representations ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    #[serde(rename = "exact")]
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Base,
    Polygon,
    Solana,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Asset {
    Usdc,
}

/// Unix epoch seconds. Newtype prevents accidental mixing with raw integers
/// and gives us a single place to validate range if we ever need to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixSecs(pub u64);

impl UnixSecs {
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self(secs)
    }

    pub fn from_secs(secs: u64) -> Self {
        Self(secs)
    }

    pub fn is_expired(&self) -> bool {
        Self::now().0 > self.0
    }
}
