//! POST /v1/download — x402-protected data download.
//!
//! Without `X-PAYMENT` header: returns 402 with PaymentRequired challenge.
//! With valid payment proof: enqueues a job and returns 202 with poll URL.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[allow(unused_imports)]
use ftdata_paid_facilitator::{PaymentProof, PaymentVerifier, UnixSecs, VerificationError};

use crate::error::{ApiError, ApiResult};
use crate::jobs::{new_job_id, Job, JobResult, JobStatus};
use crate::origin::OriginRequest;
use crate::routes::quote::{validate as validate_quote, QuoteRequest};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct DownloadBody {
    pub exchange: String,
    pub pairs: Vec<String>,
    pub timeframes: Vec<String>,
    pub timerange: String,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
}

pub async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<DownloadBody>,
) -> ApiResult<axum::response::Response> {
    // 1. Validate the body the same way as /v1/quote.
    let qr = QuoteRequest {
        exchange: body.exchange.clone(),
        pairs: body.pairs.clone(),
        timeframes: body.timeframes.clone(),
        timerange: body.timerange.clone(),
        format: body.format.clone(),
        market: body.market.clone(),
    };
    validate_quote(&qr)?;

    let origin_req = OriginRequest {
        exchange: body.exchange.clone(),
        pairs: body.pairs.clone(),
        timeframes: body.timeframes.clone(),
        timerange: body.timerange.clone(),
        format: body.format.clone().unwrap_or_else(|| "feather".into()),
        market: body.market.clone().unwrap_or_else(|| "spot".into()),
    };
    let pricing_req = origin_req.to_pricing_request();
    let quote = ftdata_paid_pricing::price_quote(&pricing_req)
        .map_err(|e| ApiError::Internal(format!("pricing failed: {e}")))?;
    let price_str = quote.to_x402_string();

    // 2. Issue a challenge and look for X-PAYMENT header.
    let challenge = state.verifier.prepare_challenge(
        uuid::Uuid::new_v4().simple().to_string(),
        quote.total_usdc_minor,
        300,
    );
    let quote_id = challenge.quote_id.clone();

    let payment_header = headers.get("x-payment").and_then(|v| v.to_str().ok());

    let proof = match payment_header {
        Some(h) => match parse_proof(h) {
            Ok(p) => p,
            Err(e) => return Ok(ApiError::PaymentInvalid { reason: e }.into_response()),
        },
        None => {
            return Ok(ApiError::PaymentRequired { challenge }.into_response());
        }
    };

    // 3. Validate the proof. The facilitator looks up the stored challenge
    //    by `proof.quote_id` and validates against it.
    match state.verifier.verify(&proof).await {
        Ok(verified) => {
            let (payer, amount, tx_hash) = match verified {
                ftdata_paid_facilitator::VerificationResult::Verified {
                    payer,
                    amount,
                    tx_hash,
                } => (payer, amount, tx_hash),
            };

            // 4. Enqueue a job and return 202.
            let job_id = new_job_id();
            let payer_for_bg = payer.clone();
            let tx_hash_for_bg = tx_hash.clone();
            let job = Job {
                id: job_id.clone(),
                status: JobStatus::Queued,
                progress: 0.0,
                quote_id: quote_id.clone(),
                amount_paid_usdc: amount.clone(),
                tx_hash: Some(tx_hash),
                payer: Some(payer),
                result: None,
                error: None,
            };
            state.jobs.insert(job);

            // Spawn the origin work in the background so the response is fast.
            let jobs = state.jobs.clone();
            let receipts = state.receipts.clone();
            let policy_id = state.pricing.policy_id.clone();
            let job_id_bg = job_id.clone();
            let req_bg = origin_req.clone();
            let payer_bg = payer_for_bg;
            let amount_bg = amount.clone();
            let tx_hash_bg = tx_hash_for_bg;
            let quote_id_bg = quote_id.clone();
            let rows_bg = pricing_req.rows;
            tokio::spawn(async move {
                jobs.update(&job_id_bg, |j| {
                    j.status = JobStatus::Running;
                    j.progress = 0.1;
                });
                match crate::origin::run(&req_bg).await {
                    Ok(path) => {
                        let path_str = path.to_string_lossy().to_string();
                        let bytes = tokio::fs::metadata(&path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        let hash = crate::origin::blake3_of_file(&path_str);
                        let expires = UnixSecs::now().0 + 300;
                        let url = format!(
                            "https://r2.example.com/signed/{job_id_bg}?expires={expires}"
                        );
                        jobs.update(&job_id_bg, |j| {
                            j.status = JobStatus::Completed;
                            j.progress = 1.0;
                            j.result = Some(JobResult {
                                files: vec![crate::jobs::FileEntry {
                                    name: format!("{}.feather", req_bg.pairs.join("_")),
                                    bytes,
                                    sha256: hash,
                                    download_url: url,
                                    expires_at: expires,
                                }],
                            });
                        });
                        // Emit a receipt per design §7.
                        let receipt = crate::receipt::Receipt {
                            receipt_id: format!("rcpt_{job_id_bg}"),
                            job_id: job_id_bg.clone(),
                            paid_by: payer_bg.clone(),
                            amount_usdc: amount_bg.clone(),
                            tx_hash: tx_hash_bg.clone(),
                            network: "base".into(),
                            facilitator: "mock".into(),
                            settled_at: UnixSecs::now().0,
                            policy_id: policy_id.clone(),
                            quote_id: quote_id_bg.clone(),
                            exchange: req_bg.exchange.clone(),
                            pairs: req_bg.pairs.clone(),
                            rows: rows_bg,
                        };
                        receipts.insert(receipt);
                    }
                    Err(e) => {
                        jobs.update(&job_id_bg, |j| {
                            j.status = JobStatus::Failed;
                            j.error = Some(format!("{e}"));
                        });
                    }
                }
            });

            Ok((
                StatusCode::ACCEPTED,
                Json(json!({
                    "job_id": job_id,
                    "status": "queued",
                    "estimated_completion_s": 45,
                    "poll_url": format!("/v1/jobs/{job_id}"),
                    "stream_url": format!("/v1/jobs/{job_id}/stream"),
                    "payment_settled": true,
                    "amount_paid_usdc": price_str,
                })),
            )
                .into_response())
        }
        Err(e) => {
            let err = match e {
                VerificationError::InsufficientAmount { got, required } => {
                    ApiError::PaymentInsufficient { got, required }
                }
                VerificationError::UnknownOrExpiredQuote { quote_id } => {
                    ApiError::PaymentExpired { quote_id }
                }
                VerificationError::BadSignature { reason } => {
                    ApiError::PaymentInvalid { reason }
                }
                VerificationError::FacilitatorUnavailable { reason } => {
                    ApiError::PaymentInvalid { reason }
                }
            };
            Ok(err.into_response())
        }
    }
}

/// Parse the X-PAYMENT header. Accepts either raw JSON or base64-url(JSON).
fn parse_proof(h: &str) -> Result<PaymentProof, String> {
    if let Ok(p) = serde_json::from_str::<PaymentProof>(h) {
        return Ok(p);
    }
    if let Ok(bytes) = base64_light::decode_url_safe(h) {
        if let Ok(p) = serde_json::from_slice::<PaymentProof>(&bytes) {
            return Ok(p);
        }
    }
    Err("X-PAYMENT header must be JSON or base64-url(JSON)".into())
}

mod base64_light {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    pub fn decode_url_safe(s: &str) -> Result<Vec<u8>, String> {
        let mut out = Vec::with_capacity(s.len() * 3 / 4);
        let mut buf: u32 = 0;
        let mut bits: u32 = 0;
        for c in s.bytes() {
            let v = ALPHABET.iter().position(|x| *x == c).ok_or("bad char")? as u32;
            buf = (buf << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push(((buf >> bits) & 0xff) as u8);
            }
        }
        Ok(out)
    }
}
