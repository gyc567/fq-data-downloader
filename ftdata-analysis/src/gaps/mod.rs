//! Gap detection module

use ftdata_core::domain::*;
use crate::statistics::{ts_to_date, format_duration};
use std::path::Path;

/// Detect gaps in OHLCV data
pub struct GapDetector {
    exchange: Exchange,
    symbol: Symbol,
    timeframe: Timeframe,
    expected_interval_ms: i64,
}

impl GapDetector {
    pub fn new(exchange: Exchange, symbol: Symbol, timeframe: Timeframe) -> Self {
        let interval_ms = timeframe.millis;
        Self {
            exchange,
            symbol,
            timeframe: timeframe.clone(),
            expected_interval_ms: interval_ms,
        }
    }

    /// Detect gaps in sorted OHLCV data
    pub fn detect_gaps(&self, timestamps: &[i64]) -> Vec<Gap> {
        let mut gaps = vec![];

        for i in 1..timestamps.len() {
            let prev = timestamps[i - 1];
            let curr = timestamps[i];
            let diff = curr - prev;

            if diff > self.expected_interval_ms {
                // Gap detected
                gaps.push(Gap {
                    exchange: self.exchange,
                    symbol: self.symbol.clone(),
                    timeframe: self.timeframe.clone(),
                    market_type: MarketType::Spot,
                    candle_type: CandleType::OHLCV,
                    from_ts: prev + self.expected_interval_ms,
                    to_ts: curr,
                    reason: format!(
                        "missing {} candles ({}ms gap)",
                        (diff / self.expected_interval_ms) - 1,
                        diff
                    ),
                    status: GapStatus::Open,
                });
            }
        }

        gaps
    }
}

/// Format gaps for display
pub fn format_gaps(gaps: &[Gap]) -> String {
    if gaps.is_empty() {
        return "No gaps detected.".to_string();
    }

    let mut output = format!("{} gap(s) detected:\n\n", gaps.len());

    for gap in gaps {
        output.push_str(&format!(
            "  {} {} {}:\n    {} → {}\n    Duration: {}\n    Reason: {}\n\n",
            gap.exchange,
            gap.symbol,
            gap.timeframe,
            ts_to_date(gap.from_ts),
            ts_to_date(gap.to_ts),
            format_duration(gap.to_ts - gap.from_ts),
            gap.reason
        ));
    }

    output
}

/// Gap report for JSON output
#[derive(serde::Serialize)]
pub struct GapReport {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub total_gaps: usize,
    pub gaps: Vec<GapDetail>,
}

#[derive(serde::Serialize)]
pub struct GapDetail {
    pub from_ts: i64,
    pub to_ts: i64,
    pub from_date: String,
    pub to_date: String,
    pub duration: String,
    pub missing_candles: i64,
    pub status: String,
}

impl GapReport {
    pub fn from_gaps(exchange: &Exchange, symbol: &Symbol, timeframe: &Timeframe, gaps: &[Gap]) -> Self {
        let interval_ms = timeframe.millis;

        Self {
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            timeframe: timeframe.label.clone(),
            total_gaps: gaps.len(),
            gaps: gaps
                .iter()
                .map(|g| GapDetail {
                    from_ts: g.from_ts,
                    to_ts: g.to_ts,
                    from_date: ts_to_date(g.from_ts),
                    to_date: ts_to_date(g.to_ts),
                    duration: format_duration(g.to_ts - g.from_ts),
                    missing_candles: (g.to_ts - g.from_ts) / interval_ms - 1,
                    status: format!("{:?}", g.status),
                })
                .collect(),
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}
