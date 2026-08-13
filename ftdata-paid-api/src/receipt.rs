//! Receipt model + in-memory receipt store.
//!
//! Implements the Settlement & Reconciliation design from
//! `docs/PAID_API_DESIGN.md` §7. Each completed download emits a receipt
//! that the API stores locally for revenue accounting.
//!
//! Phase 2 will move this to D1 / Postgres with cross-process queries.
//! For MVP, receipts live in a `DashMap` so `/v1/reconcile` can aggregate
//! them without infrastructure.

use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// A single settled-payment record. Emitted when a download completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub receipt_id: String,
    pub job_id: String,
    pub paid_by: String,
    pub amount_usdc: String, // decimal string, 6 places
    pub tx_hash: String,
    pub network: String, // "base" | "polygon" | "solana"
    pub facilitator: String,
    pub settled_at: u64, // unix seconds
    pub policy_id: String,
    pub quote_id: String,
    pub exchange: String,
    pub pairs: Vec<String>,
    pub rows: u64,
    /// Q7: whether the downloaded data was processed through the cleaning
    /// pipeline (dedup, gap-fill, sort) before being handed to the agent.
    pub cleaned: bool,
}

/// In-memory receipt store. Thread-safe; safe to share via `Clone`.
#[derive(Debug, Clone, Default)]
pub struct ReceiptStore {
    inner: Arc<DashMap<String, Receipt>>,
}

impl ReceiptStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, receipt: Receipt) {
        self.inner.insert(receipt.receipt_id.clone(), receipt);
    }

    pub fn get(&self, receipt_id: &str) -> Option<Receipt> {
        self.inner.get(receipt_id).map(|r| r.clone())
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Return all receipts in `since..=until` (unix seconds, inclusive).
    pub fn range(&self, since: u64, until: u64) -> Vec<Receipt> {
        self.inner
            .iter()
            .map(|kv| kv.value().clone())
            .filter(|r| r.settled_at >= since && r.settled_at <= until)
            .collect()
    }
}

/// Aggregation result for `/v1/reconcile`. Mirrors the design §7.2 shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconciliationReport {
    pub period: Period,
    pub jobs_total: u64,
    pub jobs_completed: u64,
    pub jobs_failed: u64,
    pub revenue_total_usdc: String, // decimal
    pub receipts: Vec<Receipt>,
    pub facilitator_fees_usdc: String, // decimal
    pub net_revenue_usdc: String,      // decimal
    pub by_exchange: HashMap<String, String>,
    pub by_policy: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Period {
    pub since: u64,
    pub until: u64,
}

impl ReconciliationReport {
    /// Build a report by aggregating a set of receipts for `since..=until`.
    ///
    /// `facilitator_fee_bps` is the basis points charged by the facilitator
    /// (e.g. 100 bps = 1%). For Phase 1 this is constant per facilitator.
    pub fn from_receipts(
        receipts: Vec<Receipt>,
        jobs_total: u64,
        jobs_completed: u64,
        jobs_failed: u64,
        period: Period,
        facilitator_fee_bps: u64,
    ) -> Self {
        let mut revenue_minor: u128 = 0;
        let mut by_exchange: HashMap<String, u128> = HashMap::new();
        let mut by_policy: HashMap<String, u128> = HashMap::new();

        for r in &receipts {
            let minor = decimal_to_minor(&r.amount_usdc).unwrap_or(0);
            revenue_minor += minor;
            *by_exchange.entry(r.exchange.clone()).or_insert(0) += minor;
            *by_policy.entry(r.policy_id.clone()).or_insert(0) += minor;
        }

        let fee_minor = revenue_minor * facilitator_fee_bps as u128 / 10_000;
        let net_minor = revenue_minor.saturating_sub(fee_minor);

        Self {
            period,
            jobs_total,
            jobs_completed,
            jobs_failed,
            revenue_total_usdc: minor_to_decimal(revenue_minor),
            receipts,
            facilitator_fees_usdc: minor_to_decimal(fee_minor),
            net_revenue_usdc: minor_to_decimal(net_minor),
            by_exchange: by_exchange
                .into_iter()
                .map(|(k, v)| (k, minor_to_decimal(v)))
                .collect(),
            by_policy: by_policy
                .into_iter()
                .map(|(k, v)| (k, minor_to_decimal(v)))
                .collect(),
        }
    }
}

/// Parse a 6-decimal USDC string into micro-USDC (u128 to avoid u64 overflow).
fn decimal_to_minor(s: &str) -> Option<u128> {
    let (whole, frac) = s.split_once('.')?;
    let whole: u128 = whole.parse().ok()?;
    let frac_padded: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
    let frac: u128 = frac_padded.parse().ok()?;
    Some(whole * 1_000_000 + frac)
}

/// Format micro-USDC as a 6-decimal string.
fn minor_to_decimal(minor: u128) -> String {
    let whole = minor / 1_000_000;
    let frac = minor % 1_000_000;
    format!("{whole}.{frac:06}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_receipt(id: &str, amount: &str, exchange: &str, policy: &str, settled_at: u64) -> Receipt {
        Receipt {
            receipt_id: id.to_string(),
            job_id: format!("job_{id}"),
            paid_by: "0xAGENT".into(),
            amount_usdc: amount.into(),
            tx_hash: format!("0xHASH_{id}"),
            network: "base".into(),
            facilitator: "mock".into(),
            settled_at,
            policy_id: policy.into(),
            quote_id: format!("qt_{id}"),
            exchange: exchange.into(),
            pairs: vec!["BTC/USDT".into()],
            rows: 525_600,
            cleaned: false,
        }
    }

    #[test]
    fn receipt_store_insert_and_get() {
        let s = ReceiptStore::new();
        let r = sample_receipt("r1", "0.050000", "binance", "pol_default_v1", 1_000);
        s.insert(r.clone());
        assert_eq!(s.get("r1"), Some(r));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn receipt_store_range_filters_by_settled_at() {
        let s = ReceiptStore::new();
        s.insert(sample_receipt("r1", "0.050000", "binance", "p1", 100));
        s.insert(sample_receipt("r2", "0.100000", "binance", "p1", 200));
        s.insert(sample_receipt("r3", "0.200000", "okx", "p1", 300));
        let in_range = s.range(150, 250);
        assert_eq!(in_range.len(), 1);
        assert_eq!(in_range[0].receipt_id, "r2");
    }

    #[test]
    fn decimal_to_minor_parses_six_decimal_strings() {
        assert_eq!(decimal_to_minor("0.050000"), Some(50_000));
        assert_eq!(decimal_to_minor("1.000000"), Some(1_000_000));
        assert_eq!(decimal_to_minor("1.46"), Some(1_460_000));
        assert_eq!(decimal_to_minor("0.000001"), Some(1));
        assert_eq!(decimal_to_minor("invalid"), None);
    }

    #[test]
    fn minor_to_decimal_round_trip() {
        for minor in [0u128, 1, 50_000, 1_000_000, 1_460_000, u128::MAX / 2] {
            let s = minor_to_decimal(minor);
            assert_eq!(decimal_to_minor(&s), Some(minor), "round-trip failed for {minor}");
        }
    }

    #[test]
    fn report_aggregates_revenue_by_exchange_and_policy() {
        let receipts = vec![
            sample_receipt("r1", "0.050000", "binance", "p1", 100),
            sample_receipt("r2", "0.100000", "binance", "p1", 200),
            sample_receipt("r3", "0.200000", "okx", "p2", 300),
        ];
        let report = ReconciliationReport::from_receipts(
            receipts,
            3,
            3,
            0,
            Period { since: 0, until: 1000 },
            100, // 1% fee
        );
        assert_eq!(report.revenue_total_usdc, "0.350000");
        assert_eq!(report.facilitator_fees_usdc, "0.003500");
        assert_eq!(report.net_revenue_usdc, "0.346500");
        assert_eq!(report.by_exchange.get("binance"), Some(&"0.150000".to_string()));
        assert_eq!(report.by_exchange.get("okx"), Some(&"0.200000".to_string()));
        assert_eq!(report.by_policy.get("p1"), Some(&"0.150000".to_string()));
        assert_eq!(report.by_policy.get("p2"), Some(&"0.200000".to_string()));
    }

    #[test]
    fn report_zero_receipts_yields_zero_totals() {
        let report = ReconciliationReport::from_receipts(
            vec![],
            0,
            0,
            0,
            Period { since: 0, until: 0 },
            0,
        );
        assert_eq!(report.revenue_total_usdc, "0.000000");
        assert_eq!(report.net_revenue_usdc, "0.000000");
        assert!(report.by_exchange.is_empty());
    }
}
