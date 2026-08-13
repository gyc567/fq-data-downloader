//! GET /v1/info — service metadata + free-tier policy.

use axum::{extract::State, Json};
use serde_json::{json, Value};

use crate::state::AppState;

pub async fn handler(State(_state): State<AppState>) -> Json<Value> {
    Json(json!({
        "service": "ftdata-paid",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
            "POST /v1/quote",
            "POST /v1/download",
            "GET  /v1/jobs/{id}",
            "GET  /v1/info"
        ],
        "free_tier": {
            "max_rows_per_request": 50_000,
            "rate_limit_per_hour": 10,
            "freshness_delay_hours": 24,
            "formats": ["feather"]
        },
        "pricing": {
            "currency": "USDC",
            "decimals": 6,
            "base_fee_usdc": "0.010000"
        },
        "x402": {
            "scheme": "exact",
            "network": "base",
            "asset": "USDC"
        }
    }))
}
