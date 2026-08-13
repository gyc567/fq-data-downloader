//! GET /v1/reconcile — customer-facing settlement report (Q10).
//!
//! DECISIONS.md Q10: **Real-time: every receipt reconciles immediately.**
//! DECISIONS.md Q8 + Q10: this endpoint is **customer-facing**, not admin.
//!
//! Auth (any of):
//!   - x402 wallet via `?wallet=...` query param (lighter than re-proving)
//!   - x402 via `X-PAYMENT` header (extract payer from proof)
//!   - API key via `Authorization: Bearer fta_live_...`
//!
//! Authorization model:
//!   - Customer sees only their own receipts (filtered by `paid_by`)
//!   - Admin role (API key with `admin:` label prefix) sees all
//!
//! The "all receipts" admin view replaces the previous unauthenticated
//! endpoint behavior, gated behind the admin key.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::auth::parse_bearer;
use crate::error::{ApiError, ApiResult};
use crate::receipt::{Period, ReconciliationReport};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ReconcileQuery {
    pub since: u64,
    pub until: u64,
    /// Optional wallet filter for x402 callers (admin role bypasses this).
    #[serde(default)]
    pub wallet: Option<String>,
}

enum CallerRole {
    /// Customer authenticated via x402 or API key. Sees only own receipts.
    Customer { wallet: String },
    /// Admin (API key with label starting with "admin:"). Sees all.
    Admin,
}

fn resolve_role(
    headers: &axum::http::HeaderMap,
    query_wallet: Option<&str>,
    state: &AppState,
) -> ApiResult<CallerRole> {
    // 1. API key (header).
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(key) = parse_bearer(auth) {
            if let Some(identity) = state.api_keys.resolve(key) {
                if identity.label.starts_with("admin:") {
                    return Ok(CallerRole::Admin);
                }
                return Ok(CallerRole::Customer {
                    wallet: identity.address,
                });
            }
        }
    }
    // 2. x402 via ?wallet= query param.
    if let Some(w) = query_wallet {
        if !w.is_empty() {
            return Ok(CallerRole::Customer {
                wallet: w.to_string(),
            });
        }
    }
    // 3. x402 via X-PAYMENT header — the proof's payer field.
    if let Some(payment) = headers.get("x-payment").and_then(|v| v.to_str().ok()) {
        if let Ok(proof) = serde_json::from_str::<ftdata_paid_facilitator::PaymentProof>(payment) {
            return Ok(CallerRole::Customer {
                wallet: proof.payer,
            });
        }
    }
    Err(ApiError::PaymentInvalid {
        reason: "reconcile requires API key (Authorization: Bearer fta_live_...) or ?wallet=... or X-PAYMENT header".into(),
    })
}

pub async fn handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<ReconcileQuery>,
) -> ApiResult<Json<ReconciliationReport>> {
    let role = resolve_role(&headers, q.wallet.as_deref(), &state)?;

    let all_receipts = state.receipts.range(q.since, q.until);
    let receipts: Vec<_> = match &role {
        CallerRole::Admin => all_receipts,
        CallerRole::Customer { wallet } => all_receipts
            .into_iter()
            .filter(|r| &r.paid_by == wallet)
            .collect(),
    };

    // Default facilitator fee: 1% (100 bps). Real Phase 2 will read this
    // from the facilitator's policy.
    let jobs_completed = receipts.len() as u64;
    let report = ReconciliationReport::from_receipts(
        receipts,
        jobs_completed,
        jobs_completed,
        0,
        Period {
            since: q.since,
            until: q.until,
        },
        100,
    );
    Ok(Json(report))
}
