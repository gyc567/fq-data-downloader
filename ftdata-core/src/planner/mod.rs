//! Download planning and chunk decomposition

use crate::domain::*;
use chrono::{Datelike, TimeZone, Utc};

/// Download plan for a single symbol/timeframe
#[derive(Debug, Clone)]
pub struct DownloadPlan {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub timeframe: Timeframe,
    pub market_type: MarketType,
    pub candle_type: CandleType,
    pub time_range: TimeRange,
    pub chunks: Vec<ChunkPlan>,
    pub source: DownloadSource,
    pub estimated_size_bytes: i64,
}

impl DownloadPlan {
    /// Create a new download plan
    pub fn new(
        exchange: Exchange,
        symbol: Symbol,
        timeframe: Timeframe,
        market_type: MarketType,
        candle_type: CandleType,
        time_range: TimeRange,
        source: DownloadSource,
    ) -> Self {
        Self {
            exchange,
            symbol,
            timeframe,
            market_type,
            candle_type,
            time_range,
            chunks: vec![],
            source,
            estimated_size_bytes: 0,
        }
    }

    /// Get total number of chunks
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Get number of pending chunks
    pub fn pending_chunks(&self) -> usize {
        self.chunks.iter().filter(|c| c.status == ChunkStatus::Pending).count()
    }
}

/// Status of a chunk plan
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkStatus {
    Pending,
    Planned,
    Skipped,
}

/// A single chunk in the download plan
#[derive(Debug, Clone)]
pub struct ChunkPlan {
    pub start: i64,
    pub end: i64,
    pub status: ChunkStatus,
    pub estimated_size: i64,
    pub url: Option<String>,
    pub etag: Option<String>,
}

impl ChunkPlan {
    pub fn new(start: i64, end: i64) -> Self {
        Self {
            start,
            end,
            status: ChunkStatus::Pending,
            estimated_size: 0,
            url: None,
            etag: None,
        }
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange::new(self.start, self.end)
    }
}

/// Decompose a time range into chunks based on source type
pub struct ChunkDecomposer {
    exchange: Exchange,
    timeframe: Timeframe,
}

impl ChunkDecomposer {
    pub fn new(exchange: Exchange, timeframe: Timeframe) -> Self {
        Self { exchange, timeframe }
    }

    /// Decompose into monthly chunks (for bulk archive sources)
    pub fn decompose_monthly(&self, range: TimeRange) -> Vec<ChunkPlan> {
        let mut chunks = vec![];
        let mut current = range.start;

        while current < range.end {
            // Get first day of next month
            let dt = Utc.timestamp_millis_opt(current).unwrap();
            let year = dt.year();
            let month = dt.month();

            // Calculate first day of next month
            let (next_year, next_month) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };

            let next_month_start = Utc.with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                .unwrap()
                .timestamp_millis();

            let chunk_end = range.end.min(next_month_start);

            chunks.push(ChunkPlan::new(current, chunk_end));
            current = chunk_end;
        }

        chunks
    }

    /// Decompose into chunks based on REST API limits
    pub fn decompose_api_chunks(&self, range: TimeRange, limit_per_request: i64) -> Vec<ChunkPlan> {
        let timeframe_ms = self.timeframe.millis;
        let candles_per_chunk = limit_per_request;
        let time_per_chunk = timeframe_ms * candles_per_chunk;

        let mut chunks = vec![];
        let mut current = range.start;

        while current < range.end {
            let chunk_end = (current + time_per_chunk).min(range.end);
            chunks.push(ChunkPlan::new(current, chunk_end));
            current = chunk_end;
        }

        chunks
    }

    /// Estimate chunk size in bytes (rough approximation)
    pub fn estimate_chunk_size(&self, chunk: &ChunkPlan) -> i64 {
        let rows = (chunk.end - chunk.start) / self.timeframe.millis;
        // Rough estimate: ~50 bytes per OHLCV row in compressed form
        rows * 50
    }
}

/// Overall download plan for multiple symbol/timeframe combinations
#[derive(Debug, Clone)]
pub struct OverallPlan {
    pub plans: Vec<DownloadPlan>,
    pub total_chunks: usize,
    pub total_pending_chunks: usize,
    pub estimated_total_bytes: i64,
}

impl OverallPlan {
    pub fn new() -> Self {
        Self {
            plans: vec![],
            total_chunks: 0,
            total_pending_chunks: 0,
            estimated_total_bytes: 0,
        }
    }

    pub fn add_plan(&mut self, plan: DownloadPlan) {
        self.total_chunks += plan.chunk_count();
        self.total_pending_chunks += plan.pending_chunks();
        self.estimated_total_bytes += plan.estimated_size_bytes;
        self.plans.push(plan);
    }

    /// Convert to JSON for MCP output
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "plans": self.plans.iter().map(|p| {
                serde_json::json!({
                    "exchange": p.exchange.to_string(),
                    "symbol": p.symbol.to_string(),
                    "timeframe": p.timeframe.label,
                    "market_type": p.market_type.to_string(),
                    "candle_type": p.candle_type.to_string(),
                    "time_range": {
                        "from": p.time_range.start,
                        "to": p.time_range.end,
                    },
                    "chunks": p.chunks.len(),
                    "pending_chunks": p.pending_chunks(),
                    "source": p.source.to_string(),
                    "estimated_size_bytes": p.estimated_size_bytes,
                })
            }).collect::<Vec<_>>(),
            "total_chunks": self.total_chunks,
            "total_pending_chunks": self.total_pending_chunks,
            "estimated_total_bytes": self.estimated_total_bytes,
        })
    }
}

impl Default for OverallPlan {
    fn default() -> Self {
        Self::new()
    }
}

/// Source resolver determines whether to use bulk or REST API
pub struct SourceResolver {
    exchange: Exchange,
}

impl SourceResolver {
    pub fn new(exchange: Exchange) -> Self {
        Self { exchange }
    }

    /// Check if bulk archive is available for this exchange
    pub fn supports_bulk(&self) -> bool {
        matches!(self.exchange, Exchange::Binance | Exchange::Bybit)
    }

    /// Get the preferred download source
    pub fn resolve_source(&self, for_historical: bool) -> DownloadSource {
        if for_historical && self.supports_bulk() {
            DownloadSource::Bulk
        } else {
            DownloadSource::API
        }
    }

    /// Get bulk download URLs for Binance
    pub fn get_binance_bulk_urls(
        symbol: &Symbol,
        timeframe: &Timeframe,
        time_range: &TimeRange,
    ) -> Vec<(String, TimeRange)> {
        let mut urls = vec![];

        // Binance bulk format: monthly zip files
        // URL: https://data.binance.vision/data/spot/monthly/klines/{symbol}/1m/{symbol}-1m-{YYYY}-{MM}.zip
        let mut current = time_range.start;
        while current < time_range.end {
            let dt = Utc.timestamp_millis_opt(current).unwrap();
            let year = dt.year();
            let month = dt.month();

            let url = format!(
                "https://data.binance.vision/data/spot/monthly/klines/{}/{}/{}-{}-{}-{:02}.zip",
                symbol.freqtrade_format(),
                timeframe.label,
                symbol.freqtrade_format(),
                timeframe.label,
                year,
                month
            );

            // Calculate end of this month
            let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
            let next_month_start = Utc.with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
                .unwrap()
                .timestamp_millis();
            let chunk_end = time_range.end.min(next_month_start);

            urls.push((url, TimeRange::new(current, chunk_end)));
            current = chunk_end;
        }

        urls
    }

    /// Get bulk download URLs for Bybit
    pub fn get_bybit_bulk_urls(
        symbol: &Symbol,
        timeframe: &Timeframe,
        time_range: &TimeRange,
    ) -> Vec<(String, TimeRange)> {
        let mut urls = vec![];

        // Bybit uses daily zip files
        let mut current = time_range.start;
        while current < time_range.end {
            let dt = Utc.timestamp_millis_opt(current).unwrap();
            let year = dt.year();
            let month = dt.month();
            let day = dt.day();

            let url = format!(
                "https://raw.githubusercontent.com/bybit-exchange/bybit-archive/main/spot/1m/{}/{}-{}-{}-{}-{:02}.zip",
                symbol.freqtrade_format(),
                symbol.freqtrade_format(),
                timeframe.label,
                year,
                month,
                day
            );

            // Next day
            let next_day_start = current + 86_400_000; // 1 day in ms
            let chunk_end = time_range.end.min(next_day_start);

            urls.push((url, TimeRange::new(current, chunk_end)));
            current = chunk_end;
        }

        urls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_monthly_decomposition() {
        let decomposer = ChunkDecomposer::new(Exchange::Binance, Timeframe::m1());
        let range = TimeRange::new(0, 86_400_000 * 60); // ~2 months
        let chunks = decomposer.decompose_monthly(range);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn test_api_chunks() {
        let decomposer = ChunkDecomposer::new(Exchange::Binance, Timeframe::m1());
        let range = TimeRange::new(0, 86_400_000 * 60); // ~2 months
        let chunks = decomposer.decompose_api_chunks(range, 1000);
        // 2 months of 1m data = ~60 days * 1440 candles = ~86,400 candles
        // At 1000 per request = ~87 chunks
        assert!(chunks.len() > 80);
    }
}
