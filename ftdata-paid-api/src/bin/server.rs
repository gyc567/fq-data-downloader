//! ftdata-paid-api server binary.
//!
//! Spins up the Axum HTTP server with the configured facilitator.
//!
//! Facilitator selection (Q4):
//!   - If `CLOUDFLARE_MGW_URL` + `CLOUDFLARE_MGW_API_KEY` are set, use
//!     `CloudflareFacilitator` (production path against CF's MGW).
//!   - Otherwise fall back to `MockFacilitator` (dev / tests / no MGW yet).
//!
//! The `AppState` slot accepts any `Arc<dyn PaymentVerifier>`, so swapping
//! the facilitator does not touch the route handlers.

use std::net::SocketAddr;
use std::sync::Arc;

use ftdata_paid_api::{routes::router, AppState};
use ftdata_paid_facilitator::{CloudflareFacilitator, MockFacilitator, PaymentVerifier};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tracing via RUST_LOG; default to info.
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // Bind address — defaults to 0.0.0.0:8080, override with FTDATA_BIND.
    let bind: SocketAddr = std::env::var("FTDATA_BIND")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
        .parse()?;

    // Q4: pick facilitator. CloudflareFacilitator when env is set, else Mock.
    let verifier: Arc<dyn PaymentVerifier> =
        if let Some(cf) = CloudflareFacilitator::from_env() {
            tracing::info!(
                mgw = %cf_network_label(&cf),
                "ftdata-paid-api using CloudflareFacilitator"
            );
            Arc::new(cf)
        } else {
            tracing::warn!(
                "ftdata-paid-api using MockFacilitator (set CLOUDFLARE_MGW_URL + CLOUDFLARE_MGW_API_KEY to use Cloudflare MGW)"
            );
            Arc::new(MockFacilitator::new())
        };
    let state = AppState::new(verifier);
    let app = router(state);

    tracing::info!(%bind, "ftdata-paid-api listening");

    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Tiny helper to log the MGW network label without exposing the whole struct.
fn cf_network_label(cf: &CloudflareFacilitator) -> String {
    format!("{:?}", cf_network(cf))
}
fn cf_network(_cf: &CloudflareFacilitator) -> ftdata_paid_facilitator::Network {
    // We don't expose network publicly; use env to read it.
    match std::env::var("CLOUDFLARE_MGW_NETWORK").as_deref() {
        Ok("polygon") => ftdata_paid_facilitator::Network::Polygon,
        Ok("solana") => ftdata_paid_facilitator::Network::Solana,
        _ => ftdata_paid_facilitator::Network::Base,
    }
}
