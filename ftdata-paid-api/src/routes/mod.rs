//! HTTP routes.

pub mod download;
pub mod info;
pub mod jobs;
pub mod quote;
pub mod reconcile;

use crate::web;

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
        // web dashboard routes
        .route("/dashboard/", get(web::dashboard::home))
        .route("/dashboard/quote", get(web::quote::form))
        .route("/dashboard/jobs", get(web::jobs::list))
        .route("/dashboard/jobs/:id", get(web::jobs::detail))
        .with_state(state)
}
