//! GET /v1/jobs/{id} — job status query.

use axum::{
    extract::{Path, State},
    Json,
};

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::jobs::Job>> {
    state
        .jobs
        .get(&id)
        .map(Json)
        .ok_or(ApiError::JobNotFound(id))
}
