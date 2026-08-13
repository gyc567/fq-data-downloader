//! Shared application state.

use std::sync::Arc;

use ftdata_paid_facilitator::PaymentVerifier;
use ftdata_paid_pricing::{Market, PricingRequest, Timeframe};

use crate::jobs::JobStore;
use crate::receipt::ReceiptStore;

/// Configuration for the pricing layer. Holds defaults so the API can answer
/// `/v1/quote` without any per-call policy lookup.
#[derive(Debug, Clone)]
pub struct PricingConfig {
    pub free_tier_discount_usdc: u64,
    pub compute_bonus_usdc: u64,
    pub policy_id: String,
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 0,
            policy_id: "pol_default_v1".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub verifier: Arc<dyn PaymentVerifier>,
    pub jobs: JobStore,
    pub receipts: ReceiptStore,
    pub pricing: PricingConfig,
}

impl AppState {
    pub fn new(verifier: Arc<dyn PaymentVerifier>) -> Self {
        Self {
            verifier,
            jobs: JobStore::new(),
            receipts: ReceiptStore::new(),
            pricing: PricingConfig::default(),
        }
    }

    pub fn with_pricing(mut self, cfg: PricingConfig) -> Self {
        self.pricing = cfg;
        self
    }

    /// Build a `PricingRequest` from a CLI-style request.
    pub fn pricing_request(
        &self,
        pairs_count: usize,
        rows: u64,
        timeframe: Timeframe,
        market: Market,
    ) -> PricingRequest {
        PricingRequest {
            rows,
            pairs_count,
            timeframe,
            market,
            free_tier_discount_usdc: self.pricing.free_tier_discount_usdc,
            compute_bonus_usdc: self.pricing.compute_bonus_usdc,
        }
    }
}
