//! Pricing policy loaded from a key-value store (Q11).
//!
//! DECISIONS.md Q11: **Workers KV** — simple, low latency.
//!
//! Phase 1 ships a `MemoryKv` impl that mimics the Cloudflare Workers KV
//! interface. When deployed to CF Workers, the real `worker::kv::KvStore`
//! is bound via `wrangler` config and plugged in via this same trait.
//!
//! Policy layout (per the audit summary in PAID_API_DESIGN.md §0a):
//! - `policy:<id>` -> PricingPolicy JSON
//! - `policy:active` -> the currently-active policy id (string)
//!
//! The pricing crate currently embeds constants (BASE_FEE_USDC = 10_000, etc.)
//! via code. Moving them to KV is what makes the policy tunable without
//! a deploy. For Phase 1 the defaults still match the code constants, so
//! the policy is the single source of truth.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Minimal KV abstraction. `get` and `put`; no list, no expiry, no CAS.
/// Matches the surface Cloudflare Workers KV exposes (minus namespace).
pub trait Kv: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn put(&self, key: &str, value: String);
}

/// In-memory KV. Used for tests, dev, and the Phase 1 server binary.
#[derive(Debug, Default, Clone)]
pub struct MemoryKv {
    inner: Arc<std::sync::Mutex<HashMap<String, String>>>,
}

impl MemoryKv {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed with a starting set of key=value pairs.
    pub fn seeded(pairs: &[(&str, &str)]) -> Self {
        let s = Self::new();
        for (k, v) in pairs {
            s.put(k, (*v).to_string());
        }
        s
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().is_empty()
    }
}

impl Kv for MemoryKv {
    fn get(&self, key: &str) -> Option<String> {
        self.inner.lock().unwrap().get(key).cloned()
    }
    fn put(&self, key: &str, value: String) {
        self.inner.lock().unwrap().insert(key.to_string(), value);
    }
}

/// Pricing policy as it lives in KV. All fields override the hard-coded
/// constants in `ftdata-paid-pricing` if present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PricingPolicy {
    pub policy_id: String,
    /// Micro-USDC base fee (1 USDC = 1_000_000).
    pub base_fee_usdc: u64,
    /// Micro-USDC per million K-lines.
    pub per_million_rows_usdc: u64,
    /// Micro-USDC per extra pair (after the first).
    pub per_extra_pair_usdc: u64,
    /// Timeframe multipliers: 1m=1.0, 1d=0.05, etc. Match ftdata-paid-pricing::Timeframe::multiplier.
    pub timeframe_multiplier: HashMap<String, f64>,
    /// Market multipliers: spot=1.0, futures=1.2.
    pub market_multiplier: HashMap<String, f64>,
}

impl Default for PricingPolicy {
    fn default() -> Self {
        let mut tfm = HashMap::new();
        tfm.insert("1m".into(), 1.0);
        tfm.insert("5m".into(), 0.6);
        tfm.insert("15m".into(), 0.4);
        tfm.insert("1h".into(), 0.25);
        tfm.insert("4h".into(), 0.15);
        tfm.insert("1d".into(), 0.05);
        let mut mm = HashMap::new();
        mm.insert("spot".into(), 1.0);
        mm.insert("futures".into(), 1.2);
        Self {
            policy_id: "pol_default_v1".into(),
            base_fee_usdc: 10_000,        // $0.01
            per_million_rows_usdc: 10_000, // $0.01 per 1M rows
            per_extra_pair_usdc: 10_000,  // $0.01 per extra pair
            timeframe_multiplier: tfm,
            market_multiplier: mm,
        }
    }
}

impl PricingPolicy {
    /// Seed the KV with the default policy under `policy:<id>` and set
    /// `policy:active` to it. Returns the active id.
    pub fn seed_default(kv: &dyn Kv) -> String {
        let policy = PricingPolicy::default();
        let id = policy.policy_id.clone();
        let json = serde_json::to_string(&policy).expect("PricingPolicy serializes");
        kv.put(&format!("policy:{}", id), json);
        kv.put("policy:active", id.clone());
        id
    }

    /// Load the active policy. Falls back to the default if KV is empty
    /// or the stored JSON is unparseable (e.g. mid-deploy).
    pub fn load_active(kv: &dyn Kv) -> Self {
        let active_id = kv.get("policy:active");
        if let Some(id) = active_id {
            if let Some(json) = kv.get(&format!("policy:{}", id)) {
                if let Ok(p) = serde_json::from_str::<PricingPolicy>(&json) {
                    return p;
                }
            }
        }
        PricingPolicy::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_kv_round_trip() {
        let kv = MemoryKv::new();
        assert_eq!(kv.get("foo"), None);
        kv.put("foo", "bar".into());
        assert_eq!(kv.get("foo"), Some("bar".into()));
        assert_eq!(kv.len(), 1);
    }

    #[test]
    fn default_policy_seeds_and_loads() {
        let kv = MemoryKv::new();
        let id = PricingPolicy::seed_default(&kv);
        assert_eq!(id, "pol_default_v1");
        let loaded = PricingPolicy::load_active(&kv);
        assert_eq!(loaded, PricingPolicy::default());
    }

    #[test]
    fn load_active_falls_back_when_kv_empty() {
        let kv = MemoryKv::new();
        let loaded = PricingPolicy::load_active(&kv);
        assert_eq!(loaded, PricingPolicy::default());
    }

    #[test]
    fn load_active_falls_back_on_corrupt_json() {
        let kv = MemoryKv::new();
        kv.put("policy:active", "pol_x".into());
        kv.put("policy:pol_x", "not json".into());
        let loaded = PricingPolicy::load_active(&kv);
        assert_eq!(loaded, PricingPolicy::default());
    }

    #[test]
    fn custom_policy_round_trip() {
        let kv = MemoryKv::new();
        let mut p = PricingPolicy::default();
        p.policy_id = "pol_v2".into();
        p.base_fee_usdc = 20_000; // $0.02
        kv.put("policy:pol_v2", serde_json::to_string(&p).unwrap());
        kv.put("policy:active", "pol_v2".into());
        let loaded = PricingPolicy::load_active(&kv);
        assert_eq!(loaded.base_fee_usdc, 20_000);
    }

    #[test]
    fn seeded_helper_loads_initial_pairs() {
        let kv = MemoryKv::seeded(&[("a", "1"), ("b", "2")]);
        assert_eq!(kv.get("a"), Some("1".into()));
        assert_eq!(kv.get("b"), Some("2".into()));
        assert_eq!(kv.len(), 2);
    }
}
