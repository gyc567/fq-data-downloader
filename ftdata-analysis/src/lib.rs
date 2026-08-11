//! Analysis module for gap detection, statistics, and data inspection

pub mod gaps;
pub mod statistics;
pub mod duplicates;

use ftdata_core::domain::*;
use polars::prelude::ChunkAgg;
use std::path::Path;
use std::str::FromStr;

/// Dataset statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct DatasetStats {
    pub exchange: String,
    pub symbol: String,
    pub timeframe: String,
    pub market_type: String,
    pub candle_type: String,
    pub format: String,
    pub from_ts: Option<i64>,
    pub to_ts: Option<i64>,
    pub rows: i64,
    pub size_bytes: u64,
    pub date_range: Option<String>,
    pub gaps: Vec<GapInfo>,
    pub duplicates: u64,
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GapInfo {
    pub from_ts: i64,
    pub to_ts: i64,
    pub from_date: String,
    pub to_date: String,
    pub duration: String,
}

impl DatasetStats {
    pub fn new(
        exchange: Exchange,
        symbol: &Symbol,
        timeframe: Timeframe,
        market_type: MarketType,
        candle_type: CandleType,
        format: DataFormat,
    ) -> Self {
        Self {
            exchange: exchange.to_string(),
            symbol: symbol.to_string(),
            timeframe: timeframe.label.clone(),
            market_type: market_type.to_string(),
            candle_type: candle_type.to_string(),
            format: format.to_string(),
            from_ts: None,
            to_ts: None,
            rows: 0,
            size_bytes: 0,
            date_range: None,
            gaps: vec![],
            duplicates: 0,
            checksum: None,
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

/// Convert timestamp to human-readable date
pub fn ts_to_date(ts: i64) -> String {
    use chrono::{DateTime, TimeZone, Utc};
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

/// Inspect a dataset file
pub fn inspect_file(path: &Path) -> Result<DatasetStats, String> {
    use ftdata_storage::feather;
    use ftdata_storage::parquet;

    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .ok_or("invalid filename")?;

    // Parse filename: BTC_USDT-1m.feather
    let parts: Vec<&str> = filename.rsplitn(2, '.').collect();
    if parts.len() != 2 {
        return Err("invalid filename format".into());
    }

    let name_parts: Vec<&str> = parts[1].split('-').collect();
    if name_parts.len() < 2 {
        return Err("invalid dataset name format".into());
    }

    let symbol = Symbol::parse(name_parts[0]).map_err(|e| e.to_string())?;
    let timeframe = Timeframe::from_str(name_parts[1]).map_err(|e| e.to_string())?;

    let format = DataFormat::from_str(parts[0]).unwrap_or(DataFormat::Feather);

    // Read file
    let df = if path.extension().map(|e| e == "feather").unwrap_or(false) {
        feather::read_feather(path).map_err(|e| e.to_string())?
    } else if path.extension().map(|e| e == "parquet").unwrap_or(false) {
        parquet::read_parquet(path).map_err(|e| e.to_string())?
    } else {
        return Err("unsupported format".into());
    };

    let rows = df.height() as i64;
    let size_bytes = std::fs::metadata(path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Get timestamp range
    let (from_ts, to_ts) = if let Ok(ts_col) = df.column("timestamp") {
        let ts_i64 = ts_col.i64().map_err(|e| e.to_string())?;
        let min = ts_i64.min();
        let max = ts_i64.max();
        (min, max)
    } else {
        (None, None)
    };

    let date_range = match (from_ts, to_ts) {
        (Some(from), Some(to)) => {
            Some(format!("{} → {}", ts_to_date(from), ts_to_date(to)))
        }
        _ => None,
    };

    let mut stats = DatasetStats::new(
        Exchange::Binance, // Would need to parse from path
        &symbol,
        timeframe,
        MarketType::Spot,
        CandleType::OHLCV,
        format,
    );
    stats.rows = rows;
    stats.size_bytes = size_bytes;
    stats.from_ts = from_ts;
    stats.to_ts = to_ts;
    stats.date_range = date_range;

    Ok(stats)
}
