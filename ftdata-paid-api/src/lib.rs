//! ftdata-paid-api
//!
//! HTTP API for the ftdata-paid x402 paid data service. Implements the routes
//! described in `docs/PAID_API_DESIGN.md` §2:
//!
//! - `GET  /v1/info`     — service metadata + free-tier policy
//! - `POST /v1/quote`    — price preview (no payment required)
//! - `POST /v1/download` — paid data download (x402 middleware)
//! - `GET  /v1/jobs/{id}`— job status (queued / running / completed / failed)
//!
//! ## Status
//!
//! - All 4 routes implemented against the pricing + facilitator crates.
//! - Real R2/S3 upload, Workers edge enforcement, and KV caching are
//!   **pending** infrastructure work (Q1, Q11).
//! - The download "origin" is an in-process stub that writes a fake feather
//!   file. Real download calls into the ftdata-core crates (Q4, Q9).
//!
//! ## x402 flow
//!
//! 1. Agent sends `POST /v1/download` without payment header.
//! 2. Server returns `402 Payment Required` with `PaymentRequired` body.
//! 3. Agent retries with `X-PAYMENT: <base64-json>` header.
//! 4. Server validates via `PaymentVerifier::verify`.
//! 5. On success, server enqueues a job and returns `202 Accepted` with
//!    `poll_url`. On failure, returns `402` with the verification error.

pub mod auth;
pub mod error;
pub mod jobs;
pub mod origin;
pub mod policy;
pub mod rate_limit;
pub mod receipt;
pub mod routes;
pub mod state;

pub use auth::{ApiKeyStore, AuthMethod, CallerIdentity};
pub use error::ApiError;
pub use jobs::{Job, JobStatus};
pub use policy::{Kv, MemoryKv, PricingPolicy};
pub use rate_limit::{RateLimitHit, RateLimitScope, RateLimiter, RateLimits};
pub use receipt::{Receipt, ReceiptStore, ReconciliationReport};
pub use state::AppState;
