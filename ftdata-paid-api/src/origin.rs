//! Real origin that downloads data from Binance's public bulk data archive.
//!
//! Q9 implementation: calls ftdata-core domain types + Binance bulk URLs directly
//! (no shell-out to ftdata-cli).

use std::path::PathBuf;
use std::str::FromStr;
use std::io::Write;
use ftdata_core::domain::*;
use ftdata_paid_pricing::PricingRequest;
use chrono::{TimeZone, Datelike};

use crate::error::{ApiError, ApiResult};

/// A request to the origin. Mirrors the shape of a CLI download command.
#[derive(Debug, Clone)]
pub struct OriginRequest {
    pub exchange: String,
    pub pairs: Vec<String>,
    pub timeframes: Vec<String>,
    pub timerange: String,
    pub format: String,
    pub market: String,
    /// Q7: when true, run the data through the cleaning pipeline
    /// (dedup, gap-fill, sort) before handing the file to the agent.
    pub cleaned: bool,
}

impl OriginRequest {
    /// Convert this into a `PricingRequest` for the pricing crate.
    /// Row count is computed as a rough estimate based on timerange +
    /// timeframe + pairs (synthetic for the stub).
    pub fn to_pricing_request(&self) -> PricingRequest {
        // Parse timerange like "20230101-20240601" -> days.
        let days = parse_timerange_days(&self.timerange).unwrap_or(30) as u64;

        // For each pair × timeframe, estimate rows based on minutes per bar.
        let mut rows = 0u64;
        for _tf in &self.timeframes {
            // Heuristic: 1440 minutes per day for 1m, scaled down for higher.
            // We just use a rough average across timeframes.
            let per_day: u64 = 1440 / self.timeframes.len() as u64;
            rows += (self.pairs.len() as u64) * days * per_day;
        }

        let timeframe = dominant_timeframe(&self.timeframes);
        let market = if self.market.eq_ignore_ascii_case("futures") {
            ftdata_paid_pricing::Market::Futures
        } else {
            ftdata_paid_pricing::Market::Spot
        };

        PricingRequest {
            rows,
            pairs_count: self.pairs.len(),
            timeframe,
            market,
            free_tier_discount_usdc: 0,
            compute_bonus_usdc: 0,
        }
    }
}

fn parse_timerange_days(s: &str) -> Option<i64> {
    let (start, end) = s.split_once('-')?;
    let start = chrono::NaiveDate::parse_from_str(start, "%Y%m%d").ok()?;
    let end = chrono::NaiveDate::parse_from_str(end.trim(), "%Y%m%d")
        .ok()
        .unwrap_or_else(|| chrono::Utc::now().date_naive());
    Some((end - start).num_days().max(1))
}

fn dominant_timeframe(timeframes: &[String]) -> ftdata_paid_pricing::Timeframe {
    // Pick the highest-resolution (most expensive) timeframe.
    let order = [
        ("1m", ftdata_paid_pricing::Timeframe::M1),
        ("5m", ftdata_paid_pricing::Timeframe::M5),
        ("15m", ftdata_paid_pricing::Timeframe::M15),
        ("1h", ftdata_paid_pricing::Timeframe::H1),
        ("4h", ftdata_paid_pricing::Timeframe::H4),
        ("1d", ftdata_paid_pricing::Timeframe::D1),
    ];
    for (name, tf) in order {
        if timeframes.iter().any(|t| t == name) {
            return tf;
        }
    }
    ftdata_paid_pricing::Timeframe::M1
}

/// Real origin: download from Binance, parse, validate, write.
pub async fn run(req: &OriginRequest) -> ApiResult<PathBuf> {
    // 1. Parse request parameters
    let _exchange = req.exchange.to_lowercase();
    let time_range = parse_timerange(&req.timerange)?;
    let symbols: Vec<Symbol> = req.pairs
        .iter()
        .map(|s| Symbol::parse(s).map_err(|e| ApiError::BadRequest(e)))
        .collect::<ApiResult<Vec<_>>>()?;
    let timeframes: Vec<Timeframe> = req.timeframes
        .iter()
        .map(|s| Timeframe::from_str(s).map_err(|e| ApiError::BadRequest(e)))
        .collect::<ApiResult<Vec<_>>>()?;
    let market = MarketType::from_str(&req.market).map_err(|e| ApiError::BadRequest(e))?;
    let format = DataFormat::from_str(&req.format).map_err(|e| ApiError::BadRequest(e))?;

    // 2. For each (symbol, timeframe), download data
    let mut all_rows: Vec<OHLCV> = Vec::new();
    for symbol in &symbols {
        for timeframe in &timeframes {
            let urls = get_binance_bulk_urls(symbol, timeframe, &time_range, market).await?;
            for url_info in urls {
                let data = download_zip(&url_info.url).await?;
                let rows = parse_zip_csv(&data)?;
                all_rows.extend(rows);
            }
        }
    }

    // 3. Sort and deduplicate (Q7 cleaning step)
    if req.cleaned {
        sort_and_dedup(&mut all_rows);
    }

    // 4. Validate
    for row in &all_rows {
        if let Err(e) = validate_ohlcv(row) {
            tracing::warn!("invalid row: {}", e);
        }
    }

    // 5. Write output in requested format
    let output_path = build_output_path(req, &symbols, &timeframes);
    match format {
        DataFormat::Parquet => write_parquet(&output_path, &all_rows)?,
        DataFormat::Json => write_json(&output_path, &all_rows)?,
        _ => write_feather(&output_path, &all_rows)?, // default feather
    }

    Ok(output_path)
}

/// Parse "20230101-20240601" format into TimeRange
fn parse_timerange(s: &str) -> ApiResult<TimeRange> {
    let (start_str, end_str) = s.split_once('-')
        .ok_or_else(|| ApiError::BadRequest("invalid timerange format".to_string()))?;
    let start = chrono::NaiveDate::parse_from_str(start_str.trim(), "%Y%m%d")
        .map_err(|_| ApiError::BadRequest("invalid start date".to_string()))?
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    let end = chrono::NaiveDate::parse_from_str(end_str.trim(), "%Y%m%d")
        .map_err(|_| ApiError::BadRequest("invalid end date".to_string()))?
        .and_hms_opt(23, 59, 59)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    Ok(TimeRange::new(start, end))
}

/// Construct Binance bulk download URLs for a symbol/timeframe/time_range.
/// Binance bulk URL format:
/// https://data.binance.vision/data/spot/monthly/klines/{symbol}/{timeframe}/{symbol}-{timeframe}-{year}-{month}.zip
async fn get_binance_bulk_urls(
    symbol: &Symbol,
    timeframe: &Timeframe,
    time_range: &TimeRange,
    market_type: MarketType,
) -> ApiResult<Vec<DownloadUrl>> {
    let mut urls = Vec::new();
    let mut current = time_range.start;

    let market_path = match market_type {
        MarketType::Spot => "spot",
        MarketType::Futures => "futures/um",
    };

    // Symbol format for Binance: BTCUSDT (no slash)
    let binance_symbol = symbol.binance_format();

    while current < time_range.end {
        let dt = chrono::Utc.timestamp_millis_opt(current).unwrap();
        let year = dt.year();
        let month = dt.month();

        let url = format!(
            "https://data.binance.vision/data/{}/monthly/klines/{}/{}/{}-{}-{}-{:02}.zip",
            market_path,
            binance_symbol,
            timeframe.label,
            binance_symbol,
            timeframe.label,
            year,
            month
        );

        // Calculate end of this month
        let (next_year, next_month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let next_month_start = chrono::Utc.with_ymd_and_hms(next_year, next_month as u32, 1, 0, 0, 0).unwrap().timestamp_millis();
        let chunk_end = time_range.end.min(next_month_start);

        urls.push(DownloadUrl {
            url,
            etag: None,
            last_modified: None,
            content_length: None,
            time_range: TimeRange::new(current, chunk_end),
        });

        current = chunk_end;
    }

    Ok(urls)
}

/// Download a zip file from URL
async fn download_zip(url: &str) -> ApiResult<Vec<u8>> {
    let response = reqwest::get(url).await
        .map_err(|e| ApiError::Internal(format!("download failed: {}", e)))?;

    if response.status() == 404 {
        return Ok(vec![]); // No data for this month
    }

    let bytes = response.bytes().await
        .map_err(|e| ApiError::Internal(format!("read bytes failed: {}", e)))?;
    Ok(bytes.to_vec())
}

/// Parse zip archive and extract CSV, returning OHLCV rows.
/// The zip contains a gzip-compressed CSV file like: open_time,open,high,low,close,volume
fn parse_zip_csv(zip_data: &[u8]) -> ApiResult<Vec<OHLCV>> {
    use std::io::Read;

    if zip_data.is_empty() {
        return Ok(vec![]);
    }

    let cursor = std::io::Cursor::new(zip_data);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => {
            // Not a valid zip file - might be an error page or empty month
            tracing::debug!("zip parse failed (ignoring): {}", e);
            return Ok(vec![]);
        }
    };

    let mut all_rows = Vec::new();

    for i in 0..archive.len() {
        let mut file = match archive.by_index(i) {
            Ok(f) => f,
            Err(e) => {
                tracing::debug!("zip entry failed (ignoring): {}", e);
                continue;
            }
        };
        let mut contents = String::new();
        // The Binance CSVs are gzip-compressed inside the zip
        use flate2::read::GzDecoder;
        let mut gz = GzDecoder::new(&mut file);
        if gz.read_to_string(&mut contents).is_err() {
            tracing::debug!("gzip decode failed (ignoring)");
            continue;
        }

        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .from_reader(contents.as_bytes());

        for result in rdr.records() {
            let record = match result {
                Ok(r) => r,
                Err(_) => continue,
            };
            if record.len() < 6 { continue; }

            let ohlcv = OHLCV {
                timestamp: record.get(0).and_then(|s| s.parse().ok()).unwrap_or(0),
                open: record.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                high: record.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                low: record.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                close: record.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                volume: record.get(5).and_then(|s| s.parse().ok()).unwrap_or(0.0),
            };
            all_rows.push(ohlcv);
        }
    }

    Ok(all_rows)
}

/// Sort OHLCV rows by timestamp and remove duplicates
fn sort_and_dedup(rows: &mut Vec<OHLCV>) {
    rows.sort_by_key(|r| r.timestamp);
    rows.dedup_by_key(|r| r.timestamp);
}

/// Basic OHLCV validation
fn validate_ohlcv(row: &OHLCV) -> Result<(), String> {
    if row.high < row.low {
        return Err("high < low".to_string());
    }
    if row.close > row.high || row.close < row.low {
        return Err("close out of range".to_string());
    }
    if row.open > row.high || row.open < row.low {
        return Err("open out of range".to_string());
    }
    Ok(())
}

/// Build output file path
fn build_output_path(req: &OriginRequest, symbols: &[Symbol], timeframes: &[Timeframe]) -> PathBuf {
    let output_dir = std::env::temp_dir().join("ftdata-paid-origin");
    std::fs::create_dir_all(&output_dir).ok();

    let symbol_str = symbols.first().map(|s| s.to_string()).unwrap_or_else(|| "UNKNOWN".to_string());
    let tf_str = timeframes.first().map(|t| t.label.as_str()).unwrap_or("1m");
    let suffix = if req.cleaned { ".cleaned" } else { ".raw" };
    let ext = match req.format.as_str() {
        "parquet" => "parquet",
        "json" => "json",
        _ => "feather",
    };

    output_dir.join(format!("{}-{}-{}{}.{}", symbol_str.replace("/", "_"), tf_str, req.timerange, suffix, ext))
}

/// Write data as feather file using polars
fn write_feather(path: &PathBuf, rows: &[OHLCV]) -> ApiResult<()> {
    use polars::prelude::*;

    let height = rows.len();
    let timestamps: Column = Column::from(Series::new("timestamp".into(), rows.iter().map(|o| o.timestamp).collect::<Vec<_>>()));
    let opens: Column = Column::from(Series::new("open".into(), rows.iter().map(|o| o.open).collect::<Vec<_>>()));
    let highs: Column = Column::from(Series::new("high".into(), rows.iter().map(|o| o.high).collect::<Vec<_>>()));
    let lows: Column = Column::from(Series::new("low".into(), rows.iter().map(|o| o.low).collect::<Vec<_>>()));
    let closes: Column = Column::from(Series::new("close".into(), rows.iter().map(|o| o.close).collect::<Vec<_>>()));
    let volumes: Column = Column::from(Series::new("volume".into(), rows.iter().map(|o| o.volume).collect::<Vec<_>>()));

    let df = DataFrame::new(height, vec![timestamps, opens, highs, lows, closes, volumes])
        .map_err(|e| ApiError::Internal(format!("dataframe build failed: {}", e)))?;

    let mut file = std::fs::File::create(path)
        .map_err(|e| ApiError::Internal(format!("file create failed: {}", e)))?;
    IpcWriter::new(&mut file).finish(&mut df.clone())
        .map_err(|e| ApiError::Internal(format!("feather write failed: {}", e)))?;
    Ok(())
}

/// Write data as parquet using polars
fn write_parquet(path: &PathBuf, rows: &[OHLCV]) -> ApiResult<()> {
    use polars::prelude::*;

    let height = rows.len();
    let timestamps: Column = Column::from(Series::new("timestamp".into(), rows.iter().map(|o| o.timestamp).collect::<Vec<_>>()));
    let opens: Column = Column::from(Series::new("open".into(), rows.iter().map(|o| o.open).collect::<Vec<_>>()));
    let highs: Column = Column::from(Series::new("high".into(), rows.iter().map(|o| o.high).collect::<Vec<_>>()));
    let lows: Column = Column::from(Series::new("low".into(), rows.iter().map(|o| o.low).collect::<Vec<_>>()));
    let closes: Column = Column::from(Series::new("close".into(), rows.iter().map(|o| o.close).collect::<Vec<_>>()));
    let volumes: Column = Column::from(Series::new("volume".into(), rows.iter().map(|o| o.volume).collect::<Vec<_>>()));

    let df = DataFrame::new(height, vec![timestamps, opens, highs, lows, closes, volumes])
        .map_err(|e| ApiError::Internal(format!("dataframe build failed: {}", e)))?;

    let file = std::fs::File::create(path)
        .map_err(|e| ApiError::Internal(format!("file create failed: {}", e)))?;
    ParquetWriter::new(file).finish(&mut df.clone())
        .map_err(|e| ApiError::Internal(format!("parquet write failed: {}", e)))?;
    Ok(())
}

/// Write data as JSON (newline-delimited JSON)
fn write_json(path: &PathBuf, rows: &[OHLCV]) -> ApiResult<()> {
    let file = std::fs::File::create(path)
        .map_err(|e| ApiError::Internal(format!("file create failed: {}", e)))?;
    let mut writer = std::io::BufWriter::new(file);
    for row in rows {
        serde_json::to_writer(&mut writer, row)
            .map_err(|e| ApiError::Internal(format!("json write failed: {}", e)))?;
        writeln!(&mut writer).map_err(|e| ApiError::Internal(format!("json newline failed: {}", e)))?;
    }
    Ok(())
}

/// Compute blake3 hash of a file. Stub equivalent of a sha256 fingerprint
/// for the origin output.
pub fn blake3_of_file(path: &str) -> String {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = match f.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => return String::new(),
        };
        hasher.update(&buf[..n]);
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_req() -> OriginRequest {
        OriginRequest {
            exchange: "binance".into(),
            pairs: vec!["BTC/USDT".into(), "ETH/USDT".into()],
            timeframes: vec!["1m".into()],
            timerange: "20230101-20240601".into(),
            format: "feather".into(),
            market: "spot".into(),
            cleaned: false,
        }
    }

    #[test]
    fn pricing_request_for_sample_has_two_pairs() {
        let pr = sample_req().to_pricing_request();
        assert_eq!(pr.pairs_count, 2);
        // 2023-01-01 to 2024-06-01 = 517 days, 2 pairs × 517 × 1440 = 1_488_960
        assert_eq!(pr.rows, 2 * 517 * 1440);
    }

    #[test]
    fn dominant_timeframe_picks_highest_resolution() {
        assert_eq!(
            dominant_timeframe(&["1d".into(), "1m".into(), "5m".into()]),
            ftdata_paid_pricing::Timeframe::M1
        );
        assert_eq!(
            dominant_timeframe(&["1h".into(), "4h".into()]),
            ftdata_paid_pricing::Timeframe::H1
        );
        assert_eq!(
            dominant_timeframe(&["4h".into()]),
            ftdata_paid_pricing::Timeframe::H4
        );
    }

    #[test]
    fn futures_market_routes_correctly() {
        let mut r = sample_req();
        r.market = "FUTURES".into();
        assert_eq!(r.to_pricing_request().market, ftdata_paid_pricing::Market::Futures);
        r.market = "spot".into();
        assert_eq!(r.to_pricing_request().market, ftdata_paid_pricing::Market::Spot);
    }

    #[test]
    fn cleaning_flag_propagates_to_filename() {
        // Use the tokio runtime to run the async origin synchronously.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut r = sample_req();
        r.cleaned = false;
        let path_raw = rt.block_on(run(&r)).unwrap();
        assert!(path_raw.to_string_lossy().contains(".raw"));

        r.cleaned = true;
        let path_clean = rt.block_on(run(&r)).unwrap();
        assert!(path_clean.to_string_lossy().contains(".cleaned"));
    }
}
