//! Multi-layer validation pipeline

use crate::domain::{CandleType, Gap, GapStatus, OHLCV, Timeframe, TimeRange};
use std::collections::HashSet;

/// Validation result with details
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
    pub gaps: Vec<Gap>,
    pub duplicates_removed: u64,
}

impl ValidationResult {
    pub fn ok() -> Self {
        Self {
            valid: true,
            errors: vec![],
            warnings: vec![],
            gaps: vec![],
            duplicates_removed: 0,
        }
    }

    pub fn with_error(mut self, error: ValidationError) -> Self {
        self.valid = false;
        self.errors.push(error);
        self
    }

    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    pub fn with_gap(mut self, gap: Gap) -> Self {
        self.gaps.push(gap);
        self
    }

    pub fn merge(&mut self, other: ValidationResult) {
        if !other.valid {
            self.valid = false;
        }
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
        self.gaps.extend(other.gaps);
        self.duplicates_removed += other.duplicates_removed;
    }
}

#[derive(Debug, Clone)]
pub enum ValidationError {
    Schema { row: u64, message: String },
    Timestamp { row: u64, message: String },
    OHLC { row: u64, message: String },
    Checksum { message: String },
}

/// Validate downloaded OHLCV data
pub struct Validator {
    exchange: crate::domain::Exchange,
    symbol: crate::domain::Symbol,
    timeframe: Timeframe,
    time_range: TimeRange,
    candle_type: CandleType,
}

impl Validator {
    pub fn new(
        exchange: crate::domain::Exchange,
        symbol: crate::domain::Symbol,
        timeframe: Timeframe,
        time_range: TimeRange,
        candle_type: CandleType,
    ) -> Self {
        Self {
            exchange,
            symbol,
            timeframe,
            time_range,
            candle_type,
        }
    }

    /// Validate a batch of OHLCV rows
    pub fn validate(&self, rows: &[OHLCV]) -> ValidationResult {
        let mut result = ValidationResult::ok();

        // 1. Schema validation (all required fields present - done by parsing)

        // 2. Timestamp validation
        let (ts_errors, gaps, duplicates) = self.validate_timestamps(rows);
        result.errors.extend(ts_errors);
        result.gaps.extend(gaps);
        result.duplicates_removed = duplicates;

        // 3. OHLC constraints
        for (i, row) in rows.iter().enumerate() {
            if let Err(e) = row.validate(self.timeframe.millis) {
                result.errors.push(ValidationError::OHLC {
                    row: i as u64,
                    message: e,
                });
            }
        }

        if !result.errors.is_empty() {
            result.valid = false;
        }

        result
    }

    /// Validate timestamps: ascending order, no duplicates, aligned to timeframe
    fn validate_timestamps(
        &self,
        rows: &[OHLCV],
    ) -> (Vec<ValidationError>, Vec<Gap>, u64) {
        let mut errors = vec![];
        let mut gaps = vec![];
        let mut seen = HashSet::new();
        let mut duplicates = 0u64;

        let expected_interval = self.timeframe.millis;

        for (i, row) in rows.iter().enumerate() {
            let row_num = i as u64;

            // Check alignment
            if row.timestamp % expected_interval != 0 {
                errors.push(ValidationError::Timestamp {
                    row: row_num,
                    message: format!(
                        "timestamp {} not aligned to {} interval",
                        row.timestamp, expected_interval
                    ),
                });
            }

            // Check duplicate
            if !seen.insert(row.timestamp) {
                duplicates += 1;
                // Don't add error for duplicates, just count them
                // They'll be removed in dedup step
            }

            // Check ascending order (except for first row)
            if i > 0 {
                let prev = rows[i - 1].timestamp;
                let curr = row.timestamp;

                if curr < prev {
                    errors.push(ValidationError::Timestamp {
                        row: row_num,
                        message: format!(
                            "timestamp {} < previous {} (not ascending)",
                            curr, prev
                        ),
                    });
                } else if curr - prev > expected_interval {
                    // Gap detected
                    gaps.push(Gap {
                        exchange: self.exchange,
                        symbol: self.symbol.clone(),
                        timeframe: self.timeframe.clone(),
                        market_type: crate::domain::MarketType::Spot,
                        candle_type: self.candle_type,
                        from_ts: prev + expected_interval,
                        to_ts: curr,
                        reason: "missing candles between consecutive rows".into(),
                        status: GapStatus::Open,
                    });
                }
            }
        }

        (errors, gaps, duplicates)
    }

    /// Check for gaps at the boundaries of the time range
    pub fn check_boundary_gaps(
        &self,
        rows: &[OHLCV],
        local_range: TimeRange,
    ) -> Vec<Gap> {
        let mut gaps = vec![];
        let expected_interval = self.timeframe.millis;

        if let Some(first) = rows.first() {
            if local_range.start < first.timestamp {
                gaps.push(Gap {
                    exchange: self.exchange,
                    symbol: self.symbol.clone(),
                    timeframe: self.timeframe.clone(),
                    market_type: crate::domain::MarketType::Spot,
                    candle_type: self.candle_type,
                    from_ts: local_range.start,
                    to_ts: first.timestamp,
                    reason: "gap at beginning of data".into(),
                    status: GapStatus::Open,
                });
            }
        }

        if let Some(last) = rows.last() {
            let expected_last = local_range.end - expected_interval;
            if last.timestamp < expected_last {
                gaps.push(Gap {
                    exchange: self.exchange,
                    symbol: self.symbol.clone(),
                    timeframe: self.timeframe.clone(),
                    market_type: crate::domain::MarketType::Spot,
                    candle_type: self.candle_type,
                    from_ts: last.timestamp + expected_interval,
                    to_ts: local_range.end,
                    reason: "gap at end of data".into(),
                    status: GapStatus::Open,
                });
            }
        }

        gaps
    }
}

/// Sort and deduplicate OHLCV rows by timestamp
pub fn sort_and_dedup(rows: &mut Vec<OHLCV>) -> u64 {
    let initial_len = rows.len() as u64;

    // Sort by timestamp
    rows.sort_by_key(|r| r.timestamp);

    // Remove duplicates (keep first occurrence)
    let mut write_idx = 0;
    for read_idx in 1..rows.len() {
        if rows[read_idx].timestamp != rows[write_idx].timestamp {
            write_idx += 1;
            rows[write_idx] = rows[read_idx].clone();
        }
    }

    rows.truncate(write_idx + 1);

    initial_len - rows.len() as u64
}

/// Calculate BLAKE3 checksum for data
pub fn calculate_checksum(data: &[u8]) -> String {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_and_dedup() {
        let mut rows = vec![
            OHLCV { timestamp: 100, open: 1.0, high: 2.0, low: 0.5, close: 1.5, volume: 100.0 },
            OHLCV { timestamp: 200, open: 1.5, high: 2.5, low: 1.0, close: 2.0, volume: 150.0 },
            OHLCV { timestamp: 100, open: 1.0, high: 2.0, low: 0.5, close: 1.6, volume: 120.0 }, // duplicate
            OHLCV { timestamp: 300, open: 2.0, high: 3.0, low: 1.5, close: 2.5, volume: 200.0 },
        ];

        let removed = sort_and_dedup(&mut rows);
        assert_eq!(removed, 1);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].timestamp, 100);
        assert_eq!(rows[0].close, 1.5); // First occurrence kept
    }

    #[test]
    fn test_checksum() {
        let data = b"hello world";
        let checksum = calculate_checksum(data);
        assert_eq!(checksum.len(), 64); // BLAKE3 hex is 64 chars
    }
}
