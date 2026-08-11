//! Duplicate detection module

use ftdata_core::domain::OHLCV;
use std::collections::HashSet;

/// Find duplicate timestamps in OHLCV data
pub fn find_duplicates(ohlcv: &[OHLCV]) -> Vec<i64> {
    let mut seen = HashSet::new();
    let mut duplicates = vec![];

    for row in ohlcv {
        if !seen.insert(row.timestamp) {
            duplicates.push(row.timestamp);
        }
    }

    duplicates
}

/// Remove duplicates, keeping the first occurrence
pub fn remove_duplicates(ohlcv: &mut [OHLCV]) -> usize {
    if ohlcv.is_empty() {
        return 0;
    }

    let mut vec = ohlcv.to_vec();
    vec.sort_by_key(|r| r.timestamp);

    let initial_len = vec.len();
    let mut write_idx = 0;

    for read_idx in 1..vec.len() {
        if vec[read_idx].timestamp != vec[write_idx].timestamp {
            write_idx += 1;
            vec[write_idx] = vec[read_idx].clone();
        }
    }

    vec.truncate(write_idx + 1);

    let removed = initial_len - vec.len();
    ohlcv.iter_mut().zip(vec.iter()).for_each(|(dst, src)| *dst = src.clone());
    removed
}

/// Duplicate report
#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicateReport {
    pub total_duplicates: usize,
    pub duplicate_timestamps: Vec<i64>,
    pub rows_after_dedup: usize,
}

impl DuplicateReport {
    pub fn new(ohlcv: &mut [OHLCV]) -> Self {
        let duplicates = find_duplicates(ohlcv);
        let before_count = ohlcv.len();
        let removed = remove_duplicates(ohlcv);

        Self {
            total_duplicates: duplicates.len(),
            duplicate_timestamps: duplicates,
            rows_after_dedup: before_count - removed,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}
