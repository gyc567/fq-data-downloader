//! HTTP routes.

pub mod download;
pub mod info;
pub mod jobs;
pub mod quote;
pub mod reconcile;

use axum::{
    routing::{get, post},
    Router,
};

use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/info", get(info::handler))
        .route("/v1/quote", post(quote::handler))
        .route("/v1/download", post(download::handler))
        .route("/v1/jobs/:id", get(jobs::handler))
        .route("/v1/reconcile", get(reconcile::handler))
        .with_state(state)
}
