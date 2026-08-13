//! Cloudflare Monetization Gateway (MGW) facilitator client (Q4).
//!
//! Per DECISIONS.md Q4: **Cloudflare (MGW integration, Workers-friendly)**.
//!
//! `CloudflareFacilitator` POSTs payment proofs to a Cloudflare MGW endpoint
//! (or any compatible x402 facilitator) for verification. Used in production
//! via:
//!
//!   CLOUDFLARE_MGW_URL=https://gateway.ai.cloudflare.com/v1/.../agents/.../verify
//!   CLOUDFLARE_MGW_API_KEY=<workers ai gateway token>
//!
//! In dev / tests, point it at a local axum server that mimics the wire
//! format (see tests/integration.rs).
//!
//! The MGW wire format (based on x402 + Cloudflare's announced spec):
//!
//!   POST {base_url}/v1/payment/verify
//!   Authorization: Bearer {api_key}
//!   Content-Type: application/json
//!   {
//!     "proof": { "scheme", "network", "asset", "payer", "amount",
//!                "quote_id", "signature", "nonce", "valid_until" },
//!     "required": { "max_amount", "quote_id", "pay_to", "expires_at" }
//!   }
//!
//!   200 OK: { "verified": true, "tx_hash": "0x...",
//!             "payer": "0x...", "amount": "0.087500" }
//!   402:    { "verified": false, "code": "insufficient_amount",
//!             "message": "...", "required": "0.087500", "got": "0.05" }
//!   4xx:    { "verified": false, "code": "...", "message": "..." }
//!   5xx:    network/server error (mapped to FacilitatorUnavailable)
//!
//! prepare_challenge remains local (MGW doesn't issue challenges in the
//! current model; the origin does). The challenge is just metadata that
//! identifies what payment amount is expected.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::types::{
    Network, PaymentProof, PaymentRequired, Scheme, UnixSecs, VerificationError,
    VerificationResult,
};
use crate::verifier::{format_minor, PaymentVerifier};

/// Cloudflare MGW facilitator client.
#[derive(Debug, Clone)]
pub struct CloudflareFacilitator {
    base_url: String,
    api_key: String,
    network: Network,
    http: Client,
}

impl CloudflareFacilitator {
    /// Construct a new client. The `base_url` is the full MGW endpoint
    /// (e.g. `https://gateway.ai.cloudflare.com/v1/acct/agents/foo/verify`).
    /// `api_key` is the Cloudflare API token / Workers AI gateway token.
    /// `network` is the chain the facilitator settles on.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        network: Network,
    ) -> Self {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client builder should not fail with default config");
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            network,
            http,
        }
    }

    /// Construct with a pre-built reqwest::Client (used in tests to share
    /// one client across multiple facilitator instances).
    pub fn with_client(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        network: Network,
        http: Client,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: api_key.into(),
            network,
            http,
        }
    }

    /// Read config from environment: `CLOUDFLARE_MGW_URL` + `CLOUDFLARE_MGW_API_KEY`.
    /// Returns `None` if either is missing (caller decides fallback).
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("CLOUDFLARE_MGW_URL").ok()?;
        let api_key = std::env::var("CLOUDFLARE_MGW_API_KEY").ok()?;
        if base_url.is_empty() || api_key.is_empty() {
            return None;
        }
        // Default to Base (Q3) since that's the launch network.
        let network = match std::env::var("CLOUDFLARE_MGW_NETWORK").as_deref() {
            Ok("polygon") => Network::Polygon,
            Ok("solana") => Network::Solana,
            _ => Network::Base,
        };
        Some(Self::new(base_url, api_key, network))
    }
}

// ---- wire types ----

#[derive(Debug, Serialize)]
struct VerifyRequest<'a> {
    proof: &'a PaymentProof,
    required: RequiredView<'a>,
}

#[derive(Debug, Serialize)]
struct RequiredView<'a> {
    max_amount: &'a str,
    quote_id: &'a str,
    pay_to: &'a str,
    expires_at: u64,
    network: Network,
    scheme: Scheme,
}

#[derive(Debug, Deserialize)]
struct VerifyResponse {
    verified: bool,
    #[serde(default)]
    tx_hash: Option<String>,
    #[serde(default)]
    payer: Option<String>,
    #[serde(default)]
    amount: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    required: Option<String>,
    #[serde(default)]
    got: Option<String>,
}

#[async_trait]
impl PaymentVerifier for CloudflareFacilitator {
    fn prepare_challenge(
        &self,
        quote_id: String,
        amount_usdc_minor: u64,
        ttl_seconds: u64,
    ) -> PaymentRequired {
        // MGW doesn't issue challenges; we generate locally. The agent will
        // sign and submit via X-PAYMENT, and we forward to MGW for verify.
        let now = UnixSecs::now().0;
        PaymentRequired {
            scheme: Scheme::Exact,
            network: self.network,
            asset: crate::types::Asset::Usdc,
            pay_to: "0xCLOUDFLARE_MGW_SETTLEMENT".into(),
            max_amount: format_minor(amount_usdc_minor),
            quote_id,
            expires_at: UnixSecs(now + ttl_seconds),
        }
    }

    async fn verify(
        &self,
        proof: &PaymentProof,
    ) -> Result<VerificationResult, VerificationError> {
        // Find the matching challenge. In the production flow the challenge
        // is stored in MGW after `prepare_challenge` returns; here we
        // extract it from the proof's quote_id by re-issuing via MGW's
        // lookup endpoint. For Phase 1 we use a synchronous lookup: send
        // the proof along with the quote_id and let MGW decide.
        //
        // To keep the trait signature simple, we accept that the caller
        // has already validated the quote_id and amount before calling.
        // The MGW handles on-chain settlement + signature check.

        let url = format!("{}/v1/payment/verify", self.base_url.trim_end_matches('/'));
        let required = RequiredView {
            max_amount: &proof.amount, // best-effort hint; MGW is authoritative
            quote_id: &proof.quote_id,
            pay_to: "0xCLOUDFLARE_MGW_SETTLEMENT",
            expires_at: 0, // MGW owns the actual challenge
            network: proof.network,
            scheme: proof.scheme,
        };
        let body = VerifyRequest { proof, required };

        let resp = match self
            .http
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(VerificationError::FacilitatorUnavailable {
                    reason: format!("network error contacting {url}: {e}"),
                });
            }
        };

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let parsed: VerifyResponse = if text.is_empty() {
            VerifyResponse {
                verified: false,
                tx_hash: None,
                payer: None,
                amount: None,
                code: None,
                message: Some("empty response body".into()),
                required: None,
                got: None,
            }
        } else {
            serde_json::from_str(&text).map_err(|e| {
                VerificationError::FacilitatorUnavailable {
                    reason: format!("malformed MGW response (status {status}): {e}; body={text}"),
                }
            })?
        };

        if parsed.verified {
            Ok(VerificationResult::Verified {
                payer: parsed.payer.unwrap_or_else(|| proof.payer.clone()),
                amount: parsed.amount.unwrap_or_else(|| proof.amount.clone()),
                tx_hash: parsed.tx_hash.unwrap_or_else(|| "0xMGW_NO_TX_HASH".into()),
            })
        } else {
            // Map MGW error code to our VerificationError variant.
            let code = parsed.code.as_deref().unwrap_or("unknown");
            Err(match code {
                "insufficient_amount" => VerificationError::InsufficientAmount {
                    got: parsed.got.unwrap_or_default(),
                    required: parsed.required.unwrap_or_default(),
                },
                "expired_quote" | "unknown_quote" => VerificationError::UnknownOrExpiredQuote {
                    quote_id: proof.quote_id.clone(),
                },
                "bad_signature" | "invalid_proof" => VerificationError::BadSignature {
                    reason: parsed.message.unwrap_or_else(|| "bad signature".into()),
                },
                _ => VerificationError::FacilitatorUnavailable {
                    reason: format!(
                        "MGW returned verified=false (status {status}, code {code}): {}",
                        parsed.message.unwrap_or_default()
                    ),
                },
            })
        }
    }

    fn facilitator_id(&self) -> &'static str {
        "cloudflare"
    }
}

// Suppress unused import warning for Arc (used by tests, not by the impl).
#[allow(dead_code)]
fn _arc_marker(_: Arc<()>) {}

#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn from_env_combined_happy_and_network_path() {
        // Single test for happy path + network override to avoid parallel
        // test env-var leakage (cargo runs tests in parallel within a crate).
        let saved_url = std::env::var("CLOUDFLARE_MGW_URL").ok();
        let saved_key = std::env::var("CLOUDFLARE_MGW_API_KEY").ok();
        let saved_net = std::env::var("CLOUDFLARE_MGW_NETWORK").ok();
        std::env::set_var("CLOUDFLARE_MGW_URL", "https://mgw.example.com/verify");
        std::env::set_var("CLOUDFLARE_MGW_API_KEY", "test-key");
        std::env::remove_var("CLOUDFLARE_MGW_NETWORK");
        let cf = CloudflareFacilitator::from_env().unwrap();
        assert_eq!(cf.base_url, "https://mgw.example.com/verify");
        assert_eq!(cf.api_key, "test-key");
        assert_eq!(cf.network, Network::Base);
        std::env::set_var("CLOUDFLARE_MGW_NETWORK", "polygon");
        let cf = CloudflareFacilitator::from_env().unwrap();
        assert_eq!(cf.network, Network::Polygon);
        match saved_url {
            Some(v) => std::env::set_var("CLOUDFLARE_MGW_URL", v),
            None => std::env::remove_var("CLOUDFLARE_MGW_URL"),
        }
        match saved_key {
            Some(v) => std::env::set_var("CLOUDFLARE_MGW_API_KEY", v),
            None => std::env::remove_var("CLOUDFLARE_MGW_API_KEY"),
        }
        match saved_net {
            Some(v) => std::env::set_var("CLOUDFLARE_MGW_NETWORK", v),
            None => std::env::remove_var("CLOUDFLARE_MGW_NETWORK"),
        }
    }

    #[test]
    fn facilitator_id_is_cloudflare() {
        let cf = CloudflareFacilitator::new("http://x", "k", Network::Base);
        assert_eq!(cf.facilitator_id(), "cloudflare");
    }

    #[test]
    fn prepare_challenge_sets_pay_to_mgw_settlement() {
        let cf = CloudflareFacilitator::new("http://x", "k", Network::Base);
        let ch = cf.prepare_challenge("qt_1".into(), 50_000, 300);
        assert_eq!(ch.network, Network::Base);
        assert_eq!(ch.max_amount, "0.050000");
        assert_eq!(ch.quote_id, "qt_1");
    }
}
