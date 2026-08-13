//! POST /v1/quote — price preview (no payment required).

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[allow(unused_imports)]
use ftdata_paid_facilitator::PaymentVerifier;

use crate::error::{ApiError, ApiResult};
use crate::origin::OriginRequest;
use crate::state::AppState;

#[derive(Debug, Deserialize, Clone)]
pub struct QuoteRequest {
    pub exchange: String,
    pub pairs: Vec<String>,
    pub timeframes: Vec<String>,
    pub timerange: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct QuoteResponse {
    pub quote_id: String,
    pub estimated_rows: u64,
    pub estimated_bytes: u64,
    pub price_usdc: String,
    pub pricing_breakdown: Value,
    pub ttl_seconds: u64,
    pub payment_required: Value,
}

pub async fn handler(
    State(state): State<AppState>,
    Json(req): Json<QuoteRequest>,
) -> ApiResult<Json<QuoteResponse>> {
    validate(&req)?;

    let origin_req = OriginRequest {
        exchange: req.exchange.clone(),
        pairs: req.pairs.clone(),
        timeframes: req.timeframes.clone(),
        timerange: req.timerange.clone(),
        format: req.format.clone().unwrap_or_else(|| "feather".into()),
        market: req.market.clone().unwrap_or_else(|| "spot".into()),
    };

    let pricing_req = origin_req.to_pricing_request();
    let rows = pricing_req.rows;
    let quote = ftdata_paid_pricing::price_quote(&pricing_req)
        .map_err(|e| ApiError::Internal(format!("pricing failed: {e}")))?;

    let challenge = state.verifier.prepare_challenge(
        uuid::Uuid::new_v4().simple().to_string(),
        quote.total_usdc_minor,
        300,
    );
    let quote_id = challenge.quote_id.clone();

    Ok(Json(QuoteResponse {
        quote_id,
        estimated_rows: rows,
        estimated_bytes: rows.saturating_mul(20),
        price_usdc: quote.to_x402_string(),
        pricing_breakdown: json!({
            "base_fee": ftdata_paid_facilitator::format_minor(quote.breakdown.base_fee_usdc),
            "rows_fee": ftdata_paid_facilitator::format_minor(quote.breakdown.rows_fee_usdc),
            "pair_premium": ftdata_paid_facilitator::format_minor(quote.breakdown.pair_premium_usdc),
            "compute_bonus": ftdata_paid_facilitator::format_minor(quote.breakdown.compute_bonus_usdc),
            "free_tier_discount": ftdata_paid_facilitator::format_minor(quote.breakdown.free_tier_discount_usdc),
        }),
        ttl_seconds: 300,
        payment_required: serde_json::to_value(&challenge).unwrap(),
    }))
}

/// Shared validation used by both `/v1/quote` and `/v1/download`.
pub fn validate(req: &QuoteRequest) -> ApiResult<()> {
    if req.pairs.is_empty() {
        return Err(ApiError::BadRequest("pairs must not be empty".into()));
    }
    if req.timeframes.is_empty() {
        return Err(ApiError::BadRequest("timeframes must not be empty".into()));
    }
    // Q2: Launch is Binance-only. bybit + okx are reserved for future expansion
    // (tracked in DECISIONS.md); reject them at the API edge so agents get
    // a clear 400 instead of a 200 with a fake job.
    if req.exchange != "binance" {
        return Err(ApiError::BadRequest(format!(
            "launch is Binance-only; '{}' is not yet supported",
            req.exchange
        )));
    }
    let known_tfs = ["1m", "5m", "15m", "1h", "4h", "1d"];
    for tf in &req.timeframes {
        if !known_tfs.contains(&tf.as_str()) {
            return Err(ApiError::BadRequest(format!("unsupported timeframe: {tf}")));
        }
    }
    Ok(())
}
