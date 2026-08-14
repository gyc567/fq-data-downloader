//! In-memory job tracker.
//!
//! Phase 2 will move this to a D1/Postgres-backed queue with proper
//! persistence. For now, jobs live in a `DashMap` so the API can demonstrate
//! the `POST /v1/download` → `GET /v1/jobs/{id}` flow without infra.

use std::sync::Arc;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub progress: f64, // 0.0 .. 1.0
    pub quote_id: String,
    pub amount_paid_usdc: String,
    pub tx_hash: Option<String>,
    pub payer: Option<String>,
    pub result: Option<JobResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    /// Signed URL — for now this is just a synthetic placeholder; real
    /// implementation will mint an R2 presigned URL via the storage layer.
    pub download_url: String,
    pub expires_at: u64,
}

/// Job store. Cheap to clone (Arc inside).
#[derive(Debug, Clone, Default)]
pub struct JobStore {
    inner: Arc<DashMap<String, Job>>,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, job: Job) -> String {
        let id = job.id.clone();
        self.inner.insert(id.clone(), job);
        id
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.inner.get(id).map(|j| j.clone())
    }

    pub fn update<F>(&self, id: &str, f: F) -> Option<Job>
    where
        F: FnOnce(&mut Job),
    {
        let mut entry = self.inner.get_mut(id)?;
        f(entry.value_mut());
        Some(entry.value().clone())
    }

    /// Iterate over all jobs (newest first).
    pub fn iter_rev(&self) -> impl Iterator<Item = Job> + '_ {
        self.inner.iter().map(|j| j.clone())
    }

    /// Total job count.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

pub fn new_job_id() -> String {
    format!("job_{}", Uuid::new_v4().simple())
}
