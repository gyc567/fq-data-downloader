//! Statistics module

use ftdata_core::domain::*;
use chrono::{DateTime, TimeZone, Utc};

/// Convert timestamp to human-readable date
pub fn ts_to_date(ts: i64) -> String {
    Utc.timestamp_millis_opt(ts)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "invalid".to_string())
}

/// Format duration in human-readable form
pub fn format_duration(ms: i64) -> String {
    let seconds = ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d {}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds % 60)
    } else {
        format!("{}s", seconds)
    }
}

/// Calculate data coverage percentage
pub fn calculate_coverage(
    expected_start: i64,
    expected_end: i64,
    actual_timestamps: &[i64],
    timeframe_ms: i64,
) -> f64 {
    if actual_timestamps.is_empty() {
        return 0.0;
    }

    let expected_count = (expected_end - expected_start) / timeframe_ms;
    if expected_count == 0 {
        return 100.0;
    }

    // Count unique timestamps
    let unique_count = actual_timestamps.iter().collect::<std::collections::HashSet<_>>().len();
    (unique_count as f64 / expected_count as f64) * 100.0
}

/// Summary statistics for OHLCV data
#[derive(Debug, Clone, serde::Serialize)]
pub struct SummaryStats {
    pub row_count: i64,
    pub date_range: Option<String>,
    pub expected_candles: i64,
    pub actual_candles: i64,
    pub coverage_percent: f64,
    pub first_candle: Option<String>,
    pub last_candle: Option<String>,
    pub volume_sum: Option<f64>,
    pub price_high: Option<f64>,
    pub price_low: Option<f64>,
}

impl SummaryStats {
    pub fn from_ohlcv(ohlcv: &[OHLCV], timeframe_ms: i64) -> Self {
        let row_count = ohlcv.len() as i64;

        let (first_ts, last_ts) = if let (Some(first), Some(last)) = (ohlcv.first(), ohlcv.last()) {
            (Some(first.timestamp), Some(last.timestamp))
        } else {
            (None, None)
        };

        let date_range = match (first_ts, last_ts) {
            (Some(from), Some(to)) => Some(format!("{} → {}", ts_to_date(from), ts_to_date(to))),
            _ => None,
        };

        let expected_candles = match (first_ts, last_ts) {
            (Some(from), Some(to)) => (to - from) / timeframe_ms,
            _ => 0,
        };

        let coverage_percent = if expected_candles > 0 {
            (row_count as f64 / expected_candles as f64) * 100.0
        } else {
            100.0
        };

        let volume_sum = ohlcv.iter().map(|o| o.volume).reduce(|a, b| a + b);
        let price_high = ohlcv.iter().map(|o| o.high).reduce(|a, b| a.max(b));
        let price_low = ohlcv.iter().map(|o| o.low).reduce(|a, b| a.min(b));

        Self {
            row_count,
            date_range,
            expected_candles,
            actual_candles: row_count,
            coverage_percent,
            first_candle: first_ts.map(ts_to_date),
            last_candle: last_ts.map(ts_to_date),
            volume_sum,
            price_high,
            price_low,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}
