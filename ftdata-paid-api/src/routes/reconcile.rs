//! GET /v1/reconcile — settlement reconciliation report.
//!
//! Admin-only endpoint (no external auth in MVP). Returns aggregated
//! revenue stats over a time window. Mirrors `docs/PAID_API_DESIGN.md` §7.2.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::ApiResult;
use crate::receipt::{Period, ReconciliationReport};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ReconcileQuery {
    pub since: u64, // unix seconds (inclusive)
    pub until: u64, // unix seconds (inclusive)
}

pub async fn handler(
    State(state): State<AppState>,
    Query(q): Query<ReconcileQuery>,
) -> ApiResult<Json<ReconciliationReport>> {
    let receipts = state.receipts.range(q.since, q.until);
    // For Phase 1 we don't have per-job status counters separated from
    // receipts; jobs_total/failed mirror the count of completed receipts.
    // Real Phase 2 will count from the JobStore.
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
        // Default facilitator fee: 1% (100 bps). Real Phase 2 will read
        // this from the facilitator's policy.
        100,
    );
    Ok(Json(report))
}
