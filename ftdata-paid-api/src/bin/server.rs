//! ftdata-paid-api server binary.
//!
//! Spins up the Axum HTTP server with the configured verifier. For Phase 1
//! MVP this only wires `MockFacilitator`; real facilitators (Cloudflare /
//! Coinbase / self-hosted, Q4) will plug into the same `AppState` slot.

use std::net::SocketAddr;
use std::sync::Arc;

use ftdata_paid_api::{routes::router, AppState};
use ftdata_paid_facilitator::{MockFacilitator, PaymentVerifier};
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

    // Build state. Phase 1 only ships MockFacilitator; Q4 will gate this on env.
    let verifier: Arc<dyn PaymentVerifier> = Arc::new(MockFacilitator::new());
    let state = AppState::new(verifier);
    let app = router(state);

    tracing::info!(%bind, "ftdata-paid-api listening (mock facilitator)");

    let listener = TcpListener::bind(bind).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
