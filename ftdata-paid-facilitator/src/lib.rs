//! ftdata-paid-facilitator
//!
//! x402 payment facilitator abstraction for ftdata-paid.
//!
//! Defines the [`PaymentVerifier`] trait that any x402 facilitator
//! (Cloudflare, Coinbase, self-hosted, or mock for testing) implements,
//! plus the wire-format types [`PaymentRequired`], [`PaymentProof`], and
//! [`VerificationResult`].
//!
//! ## Status
//!
//! - `PaymentVerifier` trait: stable
//! - `MockFacilitator` (in [`mock`]): ready for tests + local dev
//! - Real `HttpFacilitator`: **pending** — depends on Q4 (facilitator choice)
//!
//! ## Why the trait
//!
//! The 402 retry flow described in `docs/PAID_API_DESIGN.md` §2.1 requires
//! the API to (1) issue a `PaymentRequired` challenge, (2) accept a
//! `PaymentProof` from the agent in the `X-PAYMENT` header on retry, and
//! (3) verify that proof via a facilitator. By depending on the trait
//! instead of a concrete facilitator, the API crate can run end-to-end
//! against `MockFacilitator` today and swap in a real facilitator later
//! without touching route handlers.

pub mod mock;
pub mod types;
pub mod verifier;

pub use mock::MockFacilitator;
pub use types::{
    Asset, Network, PaymentProof, PaymentRequired, Scheme, UnixSecs, VerificationError,
    VerificationResult,
};
pub use verifier::{default_challenge, format_minor, PaymentVerifier};
