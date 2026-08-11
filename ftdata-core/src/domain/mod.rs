//! Core domain types for ftdata

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// OHLCV candlestick data
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OHLCV {
    pub timestamp: i64,  // milliseconds since epoch
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl OHLCV {
    /// Validate OHLCV constraints:
    /// - high >= max(open, close)
    /// - low <= min(open, close)
    /// - volume >= 0
    /// - timestamp aligned to timeframe
    pub fn validate(&self, timeframe_ms: i64) -> Result<(), String> {
        if self.high < self.open.max(self.close) {
            return Err(format!(
                "high ({}) < max(open, close) ({})",
                self.high,
                self.open.max(self.close)
            ));
        }
        if self.low > self.open.min(self.close) {
            return Err(format!(
                "low ({}) > min(open, close) ({})",
                self.low,
                self.open.min(self.close)
            ));
        }
        if self.volume < 0.0 {
            return Err(format!("volume ({}) < 0", self.volume));
        }
        if self.timestamp % timeframe_ms != 0 {
            return Err(format!(
                "timestamp ({}) not aligned to timeframe ({})",
                self.timestamp, timeframe_ms
            ));
        }
        Ok(())
    }
}

/// Timeframe with millisecond duration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Timeframe {
    pub label: String,      // e.g., "1m", "5m", "1h"
    pub millis: i64,
}

impl Timeframe {
    pub fn m1() -> Self { Self { label: "1m".into(), millis: 60_000 } }
    pub fn m5() -> Self { Self { label: "5m".into(), millis: 300_000 } }
    pub fn m15() -> Self { Self { label: "15m".into(), millis: 900_000 } }
    pub fn m30() -> Self { Self { label: "30m".into(), millis: 1_800_000 } }
    pub fn h1() -> Self { Self { label: "1h".into(), millis: 3_600_000 } }
    pub fn h4() -> Self { Self { label: "4h".into(), millis: 14_400_000 } }
    pub fn d1() -> Self { Self { label: "1d".into(), millis: 86_400_000 } }

    pub fn new(label: &str, millis: i64) -> Self {
        Self { label: label.into(), millis }
    }
}

impl FromStr for Timeframe {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let millis = match s {
            "1m" => 60_000,
            "5m" => 300_000,
            "15m" => 900_000,
            "30m" => 1_800_000,
            "1h" => 3_600_000,
            "4h" => 14_400_000,
            "1d" => 86_400_000,
            _ => return Err(format!("unknown timeframe: {}", s)),
        };
        Ok(Self::new(s, millis))
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Time range with millisecond precision
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: i64,  // inclusive, millis
    pub end: i64,    // exclusive, millis
}

impl TimeRange {
    pub fn new(start: i64, end: i64) -> Self {
        Self { start, end }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        // Format: YYYYMMDD-YYYYMMDD or YYYYMMDD-
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() != 2 {
            return Err(format!("invalid timerange format: {}", s));
        }

        let start = parse_date(parts[0])?;
        let end = if parts[1].is_empty() {
            Utc::now().timestamp_millis()
        } else {
            parse_date(parts[1])?
        };

        Ok(Self::new(start, end))
    }

    pub fn contains(&self, ts: i64) -> bool {
        self.start <= ts && ts < self.end
    }

    pub fn overlap(&self, other: &TimeRange) -> Option<TimeRange> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        if start < end {
            Some(TimeRange::new(start, end))
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    pub fn len_ms(&self) -> i64 {
        self.end.saturating_sub(self.start)
    }
}

fn parse_date(s: &str) -> Result<i64, String> {
    if s.len() != 8 {
        return Err(format!("date must be 8 digits: {}", s));
    }
    let year: i32 = s[0..4].parse().map_err(|_| format!("invalid year"))?;
    let month: u32 = s[4..6].parse().map_err(|_| format!("invalid month"))?;
    let day: u32 = s[6..8].parse().map_err(|_| format!("invalid day"))?;

    DateTime::parse_from_rfc3339(&format!("{}-{:02}-{:02}T00:00:00Z", year, month, day))
        .map(|dt| dt.timestamp_millis())
        .map_err(|e| format!("failed to parse date: {}", e))
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

/// Market type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketType {
    Spot,
    Futures,
}

impl fmt::Display for MarketType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MarketType::Spot => write!(f, "spot"),
            MarketType::Futures => write!(f, "futures"),
        }
    }
}

impl FromStr for MarketType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "spot" => Ok(MarketType::Spot),
            "futures" => Ok(MarketType::Futures),
            _ => Err(format!("unknown market type: {}", s)),
        }
    }
}

/// Candle type for futures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CandleType {
    OHLCV,
    Mark,
    Index,
    Premium,
    Funding,
}

impl fmt::Display for CandleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CandleType::OHLCV => write!(f, "ohlcv"),
            CandleType::Mark => write!(f, "mark"),
            CandleType::Index => write!(f, "index"),
            CandleType::Premium => write!(f, "premium"),
            CandleType::Funding => write!(f, "funding"),
        }
    }
}

impl FromStr for CandleType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ohlcv" => Ok(CandleType::OHLCV),
            "mark" => Ok(CandleType::Mark),
            "index" => Ok(CandleType::Index),
            "premium" => Ok(CandleType::Premium),
            "funding" => Ok(CandleType::Funding),
            _ => Err(format!("unknown candle type: {}", s)),
        }
    }
}

/// Download status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadStatus {
    Pending,
    Downloading,
    Verified,
    Failed,
}

impl fmt::Display for DownloadStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadStatus::Pending => write!(f, "pending"),
            DownloadStatus::Downloading => write!(f, "downloading"),
            DownloadStatus::Verified => write!(f, "verified"),
            DownloadStatus::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for DownloadStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(DownloadStatus::Pending),
            "downloading" => Ok(DownloadStatus::Downloading),
            "verified" => Ok(DownloadStatus::Verified),
            "failed" => Ok(DownloadStatus::Failed),
            _ => Err(format!("unknown status: {}", s)),
        }
    }
}

/// Download source type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadSource {
    Bulk,
    API,
}

impl fmt::Display for DownloadSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadSource::Bulk => write!(f, "bulk"),
            DownloadSource::API => write!(f, "api"),
        }
    }
}

impl FromStr for DownloadSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bulk" => Ok(DownloadSource::Bulk),
            "api" => Ok(DownloadSource::API),
            _ => Err(format!("unknown download source: {}", s)),
        }
    }
}

/// Symbol representation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol {
    pub base: String,       // e.g., "BTC"
    pub quote: String,      // e.g., "USDT"
}

impl Symbol {
    pub fn new(base: &str, quote: &str) -> Self {
        Self { base: base.into(), quote: quote.into() }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        // Format: BTC/USDT or BTCUSDT
        let parts: Vec<&str> = if s.contains('/') {
            s.split('/').collect()
        } else if s.len() >= 6 {
            let (base, quote) = s.split_at(s.len() - 4);
            vec![base, quote]
        } else {
            return Err(format!("invalid symbol format: {}", s));
        };

        if parts.len() != 2 {
            return Err(format!("invalid symbol: {}", s));
        }

        Ok(Self::new(parts[0], parts[1]))
    }

    /// Returns Freqtrade-compatible format: BTC_USDT
    pub fn freqtrade_format(&self) -> String {
        format!("{}_{}", self.base, self.quote)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.base, self.quote)
    }
}

impl FromStr for Symbol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Symbol::parse(s)
    }
}

/// Exchange identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Binance,
    Bybit,
    OKX,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Binance => write!(f, "binance"),
            Exchange::Bybit => write!(f, "bybit"),
            Exchange::OKX => write!(f, "okx"),
        }
    }
}

impl FromStr for Exchange {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "binance" => Ok(Exchange::Binance),
            "bybit" => Ok(Exchange::Bybit),
            "okx" => Ok(Exchange::OKX),
            _ => Err(format!("unknown exchange: {}", s)),
        }
    }
}

/// Gap in data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub timeframe: Timeframe,
    pub market_type: MarketType,
    pub candle_type: CandleType,
    pub from_ts: i64,
    pub to_ts: i64,
    pub reason: String,
    pub status: GapStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapStatus {
    Open,
    Repaired,
    Ignored,
}

impl fmt::Display for GapStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GapStatus::Open => write!(f, "open"),
            GapStatus::Repaired => write!(f, "repaired"),
            GapStatus::Ignored => write!(f, "ignored"),
        }
    }
}

/// Download chunk descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: i64,
    pub download_id: i64,
    pub start: i64,
    pub end: i64,
    pub status: DownloadStatus,
    pub size: Option<i64>,
    pub checksum: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub retry_count: u32,
    pub error: Option<String>,
}

impl Chunk {
    pub fn new(download_id: i64, start: i64, end: i64) -> Self {
        Self {
            id: 0,
            download_id,
            start,
            end,
            status: DownloadStatus::Pending,
            size: None,
            checksum: None,
            etag: None,
            last_modified: None,
            retry_count: 0,
            error: None,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.status == DownloadStatus::Pending
    }

    pub fn time_range(&self) -> TimeRange {
        TimeRange::new(self.start, self.end)
    }
}

/// Download descriptor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    pub id: i64,
    pub exchange: Exchange,
    pub market_type: MarketType,
    pub symbol: Symbol,
    pub timeframe: Timeframe,
    pub candle_type: CandleType,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub status: DownloadStatus,
    pub source: DownloadSource,
    pub bytes_total: Option<i64>,
    pub bytes_downloaded: Option<i64>,
    pub checksum: Option<String>,
    pub retry_count: u32,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Download {
    pub fn new(
        exchange: Exchange,
        market_type: MarketType,
        symbol: Symbol,
        timeframe: Timeframe,
        candle_type: CandleType,
        time_range: TimeRange,
        source: DownloadSource,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: 0,
            exchange,
            market_type,
            symbol,
            timeframe,
            candle_type,
            start_ts: Some(time_range.start),
            end_ts: Some(time_range.end),
            status: DownloadStatus::Pending,
            source,
            bytes_total: None,
            bytes_downloaded: None,
            checksum: None,
            retry_count: 0,
            error: None,
            created_at: now,
            updated_at: now,
        }
    }
}

/// Data format for output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataFormat {
    Feather,
    Parquet,
    Json,
    JsonGz,
}

impl Default for DataFormat {
    fn default() -> Self {
        DataFormat::Feather
    }
}

impl fmt::Display for DataFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataFormat::Feather => write!(f, "feather"),
            DataFormat::Parquet => write!(f, "parquet"),
            DataFormat::Json => write!(f, "json"),
            DataFormat::JsonGz => write!(f, "jsongz"),
        }
    }
}

impl DataFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            DataFormat::Feather => "feather",
            DataFormat::Parquet => "parquet",
            DataFormat::Json => "json",
            DataFormat::JsonGz => "json.gz",
        }
    }
}

impl FromStr for DataFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "feather" => Ok(DataFormat::Feather),
            "parquet" => Ok(DataFormat::Parquet),
            "json" => Ok(DataFormat::Json),
            "jsongz" | "json.gz" => Ok(DataFormat::JsonGz),
            _ => Err(format!("unknown format: {}", s)),
        }
    }
}

/// File metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub exchange: Exchange,
    pub symbol: Symbol,
    pub timeframe: Timeframe,
    pub market_type: MarketType,
    pub candle_type: CandleType,
    pub from_ts: Option<i64>,
    pub to_ts: Option<i64>,
    pub rows: Option<i64>,
    pub size: Option<i64>,
    pub checksum: Option<String>,
    pub format: DataFormat,
    pub verified: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

/// Download URL with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadUrl {
    pub url: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_length: Option<i64>,
    pub time_range: TimeRange,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_parse() {
        assert_eq!(Symbol::parse("BTC/USDT").unwrap().base, "BTC");
        assert_eq!(Symbol::parse("BTCUSDT").unwrap().base, "BTC");
        assert_eq!(Symbol::parse("BTCUSDT").unwrap().quote, "USDT");
    }

    #[test]
    fn test_symbol_freqtrade_format() {
        assert_eq!(Symbol::parse("BTC/USDT").unwrap().freqtrade_format(), "BTC_USDT");
    }

    #[test]
    fn test_timeframe_parse() {
        assert_eq!(Timeframe::from_str("1m").unwrap().millis, 60_000);
        assert_eq!(Timeframe::from_str("1h").unwrap().millis, 3_600_000);
    }

    #[test]
    fn test_timerange_parse() {
        let tr = TimeRange::parse("20200101-20200102").unwrap();
        assert!(tr.start < tr.end);
    }

    #[test]
    fn test_ohlcv_validate() {
        let valid = OHLCV {
            timestamp: 60000,
            open: 100.0,
            high: 105.0,
            low: 98.0,
            close: 102.0,
            volume: 1000.0,
        };
        assert!(valid.validate(60000).is_ok());

        let invalid = OHLCV {
            timestamp: 60000,
            open: 100.0,
            high: 99.0,  // high < open
            low: 98.0,
            close: 102.0,
            volume: 1000.0,
        };
        assert!(invalid.validate(60000).is_err());
    }
}
