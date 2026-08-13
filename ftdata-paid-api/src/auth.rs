//! API key + wallet auth chain (Q8, Q5, Q10 dependency).
//!
//! Phase 1 ships with two auth modes for paid routes:
//!
//! 1. **x402 wallet** — Agent sends `X-PAYMENT` header; verifier looks up
//!    the challenge by `proof.quote_id` and the wallet is recorded as
//!    `proof.payer`. This is the primary path for AI agents.
//!
//! 2. **API key** (Q8) — Enterprise customer sends `Authorization: Bearer
//!    fta_live_...` header. The API key resolves to a customer identity
//!    (also a wallet address) and rate limit + audit are per-customer.
//!
//! Both paths produce the same `CallerIdentity` which downstream code
//! (rate limit, reconcile, audit) uses uniformly.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Identity of whoever is making the call. The address field is the
/// canonical key for rate limiting, audit, and reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerIdentity {
    /// Wallet address (x402) or the API-key-bound address (auth path).
    pub address: String,
    /// How this identity was established.
    pub method: AuthMethod,
    /// Free-form label for logging (e.g. "agent:ftdata-bot-v1" or "customer:acme").
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMethod {
    /// x402 wallet signature.
    X402,
    /// API key.
    ApiKey,
}

/// In-memory API key store. Phase 1 keeps keys in-process; Phase 2
/// moves to a DB-backed store with rotation and scoping.
///
/// Keys follow the format `fta_live_<32 hex>` and resolve to a
/// `CallerIdentity`. Key generation is out of scope for Phase 1
/// (admin-issued via DB seed or environment variable in dev).
#[derive(Debug, Clone, Default)]
pub struct ApiKeyStore {
    /// key -> identity
    keys: Arc<DashMap<String, CallerIdentity>>,
}

impl ApiKeyStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a key. Returns the key string so the caller can give it
    /// to the customer. Keys are stored in plaintext here for MVP
    /// (hashing is a Phase 2 concern).
    pub fn issue(&self, label: impl Into<String>, address: impl Into<String>) -> String {
        let key = format!("fta_live_{}", generate_key_suffix());
        self.keys.insert(
            key.clone(),
            CallerIdentity {
                address: address.into(),
                method: AuthMethod::ApiKey,
                label: label.into(),
            },
        );
        key
    }

    /// Look up a key. Returns the identity if valid.
    pub fn resolve(&self, key: &str) -> Option<CallerIdentity> {
        self.keys.get(key).map(|i| i.clone())
    }

    /// Number of registered keys.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Bulk-load keys from a JSON config (env var in dev, file in prod).
    /// Format: `[{"key": "fta_live_...", "label": "...", "address": "0x..."}, ...]`
    pub fn load_from_json(&self, json: &str) -> Result<usize, serde_json::Error> {
        let entries: Vec<ApiKeyEntry> = serde_json::from_str(json)?;
        let n = entries.len();
        for e in entries {
            self.keys.insert(
                e.key,
                CallerIdentity {
                    address: e.address,
                    method: AuthMethod::ApiKey,
                    label: e.label,
                },
            );
        }
        Ok(n)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ApiKeyEntry {
    key: String,
    label: String,
    address: String,
}

/// Generate a 32-character hex key suffix. Uses the system RNG.
fn generate_key_suffix() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    hex_encode(&bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

/// Extract an API key from an `Authorization: Bearer fta_live_...` header.
/// Per RFC 7235 the auth scheme is case-insensitive, so `bearer`, `BEARER`,
/// and `Bearer` are all accepted.
pub fn parse_bearer(header: &str) -> Option<&str> {
    let trimmed = header.trim_start();
    // Find the scheme name (first whitespace-separated token).
    let scheme_end = trimmed.find(|c: char| c.is_whitespace())?;
    let scheme = &trimmed[..scheme_end];
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return None;
    }
    // Skip whitespace, take the rest as the credential.
    let rest = trimmed[scheme_end..].trim_start();
    if rest.starts_with("fta_live_") {
        Some(rest)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_and_resolve_round_trip() {
        let s = ApiKeyStore::new();
        let key = s.issue("agent:bot", "0xABC");
        let id = s.resolve(&key).unwrap();
        assert_eq!(id.address, "0xABC");
        assert_eq!(id.label, "agent:bot");
        assert_eq!(id.method, AuthMethod::ApiKey);
    }

    #[test]
    fn resolve_unknown_key_returns_none() {
        let s = ApiKeyStore::new();
        assert!(s.resolve("fta_live_doesnotexist").is_none());
    }

    #[test]
    fn generated_keys_have_correct_prefix() {
        let s = ApiKeyStore::new();
        let k = s.issue("x", "0xX");
        assert!(k.starts_with("fta_live_"));
        assert_eq!(k.len(), "fta_live_".len() + 32);
    }

    #[test]
    fn parse_bearer_extracts_key() {
        assert_eq!(
            parse_bearer("Bearer fta_live_abc123"),
            Some("fta_live_abc123")
        );
        assert_eq!(parse_bearer("bearer fta_live_abc"), Some("fta_live_abc"));
    }

    #[test]
    fn parse_bearer_rejects_non_bearer() {
        assert_eq!(parse_bearer("Basic dXNlcjpwYXNz"), None);
        assert_eq!(parse_bearer("fta_live_no_prefix"), None);
        assert_eq!(parse_bearer(""), None);
    }

    #[test]
    fn load_from_json_bulk_loads() {
        let s = ApiKeyStore::new();
        let json = r#"[
            {"key": "fta_live_aaaa", "label": "agent:bot1", "address": "0xA"},
            {"key": "fta_live_bbbb", "label": "agent:bot2", "address": "0xB"}
        ]"#;
        let n = s.load_from_json(json).unwrap();
        assert_eq!(n, 2);
        assert_eq!(s.len(), 2);
        assert!(s.resolve("fta_live_aaaa").is_some());
    }

    #[test]
    fn hex_encode_produces_lowercase_hex() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0xab, 0xcd]), "00ffabcd");
    }
}
