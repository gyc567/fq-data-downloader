//! CLI command implementations

use crate::Cli;
use ftdata_core::domain::*;
use ftdata_core::planner::{ChunkDecomposer, DownloadPlan, OverallPlan, SourceResolver, ChunkPlan, ChunkStatus};
use ftdata_analysis::gaps::{self, GapReport};
use ftdata_http::client::HttpClient;
use ftdata_http::rate_limit::TokenBucketLimiter;
use ftdata_storage::checkpoint::{CheckpointManager, CheckpointStatus};
use std::io::Read;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use zip::ZipArchive;

/// Download historical data
pub async fn download(cli: Cli) -> anyhow::Result<()> {
    let download_cmd = match &cli.command {
        crate::Commands::Download {
            exchange,
            pairs,
            timeframes,
            timerange,
            format,
        } => (exchange.clone(), pairs.clone(), timeframes.clone(), timerange.clone(), format.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    let (exchange_str, pairs, timeframes, timerange_str, format_str) = download_cmd;

    println!("ftdata download");
    println!("Exchange: {}", exchange_str);
    println!("Pairs: {:?}", pairs);
    println!("Timeframes: {:?}", timeframes);
    println!("Time range: {}", timerange_str);
    println!("Format: {}", format_str);
    println!("Output: {:?}", cli.output);

    let exchange = Exchange::from_str(&exchange_str)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Parse time range
    let time_range = TimeRange::parse(&timerange_str)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    // Parse timeframes
    let timeframes: Vec<Timeframe> = timeframes
        .iter()
        .filter_map(|t| Timeframe::from_str(t).ok())
        .collect();

    // Parse pairs
    let symbols: Vec<Symbol> = pairs
        .iter()
        .filter_map(|p| Symbol::parse(p).ok())
        .collect();

    // Create HTTP client with rate limiting
    let rate_limiter = TokenBucketLimiter::new(10.0, 4); // 10 req/s, 4 concurrent
    let http_client = HttpClient::new(rate_limiter);
    let http_client = Arc::new(http_client); // Wrap in Arc for parallel downloads

    // Create output directory
    let output_dir = cli.output.join(exchange.to_string());
    std::fs::create_dir_all(&output_dir)?;

    let format = DataFormat::from_str(&format_str).unwrap_or(DataFormat::Feather);

    // Download for each symbol and timeframe
    for symbol in &symbols {
        for timeframe in &timeframes {
            println!("\n--- Downloading {} {} ---", symbol, timeframe);

            // Get bulk URLs from exchange adapter
            let source = ftdata_sources::ExchangeAdapterFactory::create(exchange);
            let urls = source.get_bulk_urls(symbol, timeframe, &time_range).await?;

            if urls.is_empty() {
                // Fall back to REST API if no bulk URLs available
                println!("No bulk URLs available, falling back to REST API...");
                download_via_api(&http_client, exchange, symbol, timeframe, &time_range, &output_dir, format).await?;
                continue;
            }

            let total_chunks = urls.len();

            // Initialize checkpoint manager for resume support
            let db_path = cli.output.join("_checkpoints").join("checkpoints.db");
            let checkpoint_mgr = match CheckpointManager::new(&db_path) {
                Ok(mgr) => Arc::new(mgr),
                Err(e) => {
                    println!("  Warning: Could not initialize checkpoint database: {}", e);
                    println!("  Continuing without checkpoint support...");
                    continue;
                }
            };

            // Load existing checkpoints to skip completed chunks
            let existing_checkpoints: std::collections::HashSet<String> = {
                if let Ok(cps) = checkpoint_mgr.get_checkpoints(&exchange_str, &symbol.to_string(), &timeframe.label) {
                    cps.iter()
                        .filter(|cp| cp.status == CheckpointStatus::Completed)
                        .map(|cp| cp.chunk_url.clone())
                        .collect()
                } else {
                    std::collections::HashSet::new()
                }
            };

            // Filter URLs to only include non-completed chunks
            let pending_urls: Vec<_> = urls.iter()
                .filter(|u| !existing_checkpoints.contains(&u.url))
                .cloned()
                .collect();

            let pending_count = pending_urls.len();
            let skipped = total_chunks - pending_count;

            if skipped > 0 {
                println!("Skipping {} already completed chunks", skipped);
            }
            if pending_count > 0 {
                println!("Downloading {} chunks in parallel (max 4 concurrent)...", pending_count);
            }

            // Create progress bar for overall download progress
            use indicatif::{ProgressBar, ProgressStyle};
            let progress_bar = if pending_count > 0 {
                let pb = ProgressBar::new(pending_count as u64);
                pb.set_style(ProgressStyle::default_bar()
                    .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({percent}%) {msg}")
                    .unwrap()
                    .progress_chars("=>-"));
                pb.set_message("Downloading...");
                Some(pb)
            } else {
                None
            };

            // Download chunks in parallel with semaphore-controlled concurrency
            use tokio::sync::Semaphore;
            let semaphore = Arc::new(Semaphore::new(4));
            let symbol_clone = symbol.clone();
            let timeframe_clone = timeframe.clone();
            let output_dir_clone = output_dir.clone();
            let format_clone = format;
            let checkpoint_mgr_clone = checkpoint_mgr.clone();
            let exchange_str_clone = exchange_str.clone();
            let symbol_str_clone = symbol.to_string();
            let timeframe_label_clone = timeframe.label.clone();

            let handles: Vec<_> = pending_urls.iter().enumerate().map(|(idx, download_url)| {
                let http_client = http_client.clone();
                let semaphore = semaphore.clone();
                let symbol = symbol_clone.clone();
                let timeframe = timeframe_clone.clone();
                let output_dir = output_dir_clone.clone();
                let format = format_clone;
                let checkpoint_mgr = checkpoint_mgr_clone.clone();
                let exchange_str = exchange_str_clone.clone();
                let symbol_str = symbol_str_clone.clone();
                let timeframe_label = timeframe_label_clone.clone();
                let total_chunks = pending_count;
                let url_info = download_url.clone();

                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await;

                    let filename = url_info.url.split('/').last().unwrap_or("file");
                    println!("\n[{}/{}] Downloading {}...", idx + 1, total_chunks, filename);

                    // Mark as downloading in checkpoint
                    let cp = ftdata_storage::checkpoint::Checkpoint::new(
                        &exchange_str, &symbol_str, &timeframe_label, &url_info.url
                    );
                    let _ = checkpoint_mgr.save_checkpoint(&cp);
                    let _ = checkpoint_mgr.mark_downloading(&url_info.url);

                    // Download zip file
                    let zip_data = match http_client.download_file(&url_info.url).await {
                        Ok(data) => data,
                        Err(e) => {
                            println!("  Failed to download: {:?}", e);
                            let _ = checkpoint_mgr.mark_failed(&url_info.url);
                            return (idx, 0, "download_failed".to_string());
                        }
                    };

                    // Compute BLAKE3 checksum of the raw zip data
                    let checksum = {
                        let hash = blake3::hash(&zip_data);
                        hash.to_hex().to_string()
                    };

                    // Extract and parse CSV
                    match extract_and_parse_csv(&zip_data, &url_info.time_range).await {
                        Ok(ohlcv_data) => {
                            let count = ohlcv_data.len();
                            println!("  Downloaded {} candles", count);

                            // Write to file
                            let filepath = output_dir.join(format!("{}-{}.{}",
                                symbol.freqtrade_format(),
                                timeframe.label,
                                format.extension()
                            ));

                            match format {
                                DataFormat::Feather => {
                                    if let Err(e) = ftdata_storage::feather::write_feather(&filepath, &ohlcv_data) {
                                        println!("  Failed to write feather: {}", e);
                                        let _ = checkpoint_mgr.mark_failed(&url_info.url);
                                        return (idx, count, "write_failed".to_string());
                                    }
                                }
                                DataFormat::Parquet => {
                                    if let Err(e) = ftdata_storage::parquet::write_parquet(&filepath, &ohlcv_data) {
                                        println!("  Failed to write parquet: {}", e);
                                        let _ = checkpoint_mgr.mark_failed(&url_info.url);
                                        return (idx, count, "write_failed".to_string());
                                    }
                                }
                                _ => {
                                    println!("  Format not supported");
                                    let _ = checkpoint_mgr.mark_failed(&url_info.url);
                                    return (idx, count, "format_unsupported".to_string());
                                }
                            }

                            // Mark as completed
                            let _ = checkpoint_mgr.mark_completed(&url_info.url, url_info.etag.as_deref(), Some(&checksum));
                            println!("  Saved to {} (checksum: {})", filepath.display(), &checksum[..8]);
                            (idx, count, "success".to_string())
                        }
                        Err(e) => {
                            println!("  Failed to parse: {}", e);
                            let _ = checkpoint_mgr.mark_failed(&url_info.url);
                            (idx, 0, "parse_failed".to_string())
                        }
                    }
                })
            }).collect();

            // Wait for all downloads to complete
            let mut results: Vec<(usize, usize, String)> = Vec::new();
            for handle in handles {
                if let Ok(result) = handle.await {
                    results.push(result);
                    // Update progress bar
                    if let Some(ref pb) = progress_bar {
                        pb.inc(1);
                    }
                }
            }

            // Finalize progress bar
            if let Some(ref pb) = progress_bar {
                pb.finish_with_message("Done!");
            }

            // Sort by index and report
            results.sort_by_key(|r| r.0);
            let total_candles: usize = results.iter().map(|r| r.1).sum();
            let failed = results.iter().filter(|r| r.2 != "success").count();
            println!("\n=== Download Complete: {} candles ({} failed, {} skipped) ===", total_candles, failed, skipped);
        }
    }

    Ok(())
}

/// Extract ZIP or gzip and parse Binance CSV format
async fn extract_and_parse_csv(data: &[u8], time_range: &TimeRange) -> anyhow::Result<Vec<OHLCV>> {
    use std::io::Cursor;

    // Try ZIP first, then fall back to gzip
    let contents = if data.len() > 2 && data[0] == 0x50 && data[1] == 0x4B {
        // ZIP signature (PK)
        let cursor = Cursor::new(data);
        let mut archive = match ZipArchive::new(cursor) {
            Ok(a) => a,
            Err(_) => return Ok(vec![]),
        };

        if archive.len() == 0 {
            return Ok(vec![]);
        }

        let mut contents = String::new();
        let mut file = archive.by_index(0)?;
        file.read_to_string(&mut contents)?;
        contents
    } else {
        // Try gzip
        let cursor = Cursor::new(data);
        use flate2::read::GzDecoder;
        let mut decoder = GzDecoder::new(cursor);
        let mut contents = String::new();
        use std::io::Read;
        let _ = decoder.read_to_string(&mut contents)?;
        contents
    };

    parse_csv_contents(&contents, time_range)
}

/// Parse CSV contents into OHLCV data
fn parse_csv_contents(contents: &str, time_range: &TimeRange) -> anyhow::Result<Vec<OHLCV>> {
    let mut ohlcv_data = vec![];

    // Parse CSV lines (skip header)
    for line in contents.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 6 {
            let open_time: i64 = fields[0].parse().unwrap_or(0);
            let open: f64 = fields[1].parse().unwrap_or(0.0);
            let high: f64 = fields[2].parse().unwrap_or(0.0);
            let low: f64 = fields[3].parse().unwrap_or(0.0);
            let close: f64 = fields[4].parse().unwrap_or(0.0);
            let volume: f64 = fields[5].parse().unwrap_or(0.0);

            // Binance CSV timestamp formats:
            // - Nanoseconds (18+ digits): 1735689600000000 -> /1000 = 1735689600000 ms
            // - Milliseconds (13 digits): 1672531200000 -> already ms
            // - Seconds (10 digits): 1735689600 -> *1000 = 1735689600000 ms
            let open_time_ms = if open_time > 9999999999999 {
                // 14+ digits - nanoseconds -> divide by 1000
                open_time / 1000
            } else if open_time >= 946684800000 && open_time <= 1767225600000 {
                // 13 digits in valid millisecond range (2000-2026) -> already milliseconds
                open_time
            } else if open_time < 10000000000 {
                // 10 digits - seconds -> multiply by 1000
                open_time * 1000
            } else {
                open_time
            };

            // Filter by time range
            if open_time_ms >= time_range.start && open_time_ms < time_range.end {
                ohlcv_data.push(OHLCV {
                    timestamp: open_time_ms,
                    open,
                    high,
                    low,
                    close,
                    volume,
                });
            }
        }
    }

    // Sort by timestamp
    ohlcv_data.sort_by_key(|k| k.timestamp);

    Ok(ohlcv_data)
}

/// Download data via REST API (for exchanges without bulk archives)
async fn download_via_api(
    http_client: &HttpClient,
    exchange: Exchange,
    symbol: &Symbol,
    timeframe: &Timeframe,
    time_range: &TimeRange,
    output_dir: &std::path::Path,
    format: DataFormat,
) -> anyhow::Result<()> {
    use ftdata_core::domain::Timeframe;

    // Calculate how many API calls needed based on timeframe
    let candles_per_request = 1000u32;
    let timeframe_ms = timeframe.millis;
    let total_ms = time_range.end - time_range.start;
    let total_candles = (total_ms / timeframe_ms) as u32;
    let api_calls_needed = (total_candles + candles_per_request - 1) / candles_per_request;

    println!("  Estimated {} candles, {} API calls needed", total_candles, api_calls_needed);

    let mut all_ohlcv = Vec::new();
    let mut current_start = time_range.start;

    // For OKX, use their history-candle API
    // For Bybit, use their kline API
    let bar = match exchange {
        Exchange::Bybit => {
            // Bybit uses numeric intervals: 1, 3, 5, 15, 30, 60, 120, 240, etc.
            match timeframe.label.as_str() {
                "1m" => "1",
                "3m" => "3",
                "5m" => "5",
                "15m" => "15",
                "30m" => "30",
                "1h" => "60",
                "2h" => "120",
                "4h" => "240",
                "6h" => "360",
                "12h" => "720",
                "1d" => "D",
                "1w" => "W",
                _ => "60",
            }
        }
        _ => timeframe.label.as_str(),
    };

    while current_start < time_range.end {
        let current_end = (current_start + timeframe_ms * candles_per_request as i64).min(time_range.end);

        let url = match exchange {
            Exchange::OKX => {
                format!(
                    "https://www.okx.com/api/v5/market/history-candle?instId={}&bar={}&after={}&before={}&limit={}",
                    symbol.freqtrade_format().replace("_", "-"),
                    bar,
                    current_end,
                    current_start,
                    candles_per_request
                )
            }
            Exchange::Bybit => {
                format!(
                    "https://api.bybit.com/v5/market/kline?category=spot&symbol={}&interval={}&start={}&end={}&limit={}",
                    symbol.freqtrade_format().replace("_", ""),
                    bar,
                    current_start,
                    current_end,
                    candles_per_request
                )
            }
            _ => {
                // Binance uses bulk archives, shouldn't reach here
                println!("  REST API not supported for this exchange");
                return Ok(());
            }
        };

        println!("  Fetching {}-{}...", current_start, current_end);

        match http_client.download_file(&url).await {
            Ok(data) => {
                let json_str = String::from_utf8_lossy(&data);
                let ohlcv_batch = parse_api_response(&json_str, exchange)?;

                if ohlcv_batch.is_empty() {
                    println!("  No data returned, stopping");
                    break;
                }

                println!("  Got {} candles", ohlcv_batch.len());
                all_ohlcv.extend(ohlcv_batch);
                current_start = current_end;
            }
            Err(e) => {
                println!("  API request failed: {:?}", e);
                break;
            }
        }

        // Rate limiting - be nice to the API
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    if !all_ohlcv.is_empty() {
        // Sort and dedup
        all_ohlcv.sort_by_key(|k| k.timestamp);
        all_ohlcv.dedup_by_key(|k| k.timestamp);

        let filename = format!("{}-{}.{}",
            symbol.freqtrade_format(),
            timeframe.label,
            format.extension()
        );
        let filepath = output_dir.join(&filename);

        match format {
            DataFormat::Feather => {
                if let Err(e) = ftdata_storage::feather::write_feather(&filepath, &all_ohlcv) {
                    println!("  Failed to write feather: {}", e);
                } else {
                    println!("  Saved {} candles to {}", all_ohlcv.len(), filepath.display());
                }
            }
            DataFormat::Parquet => {
                if let Err(e) = ftdata_storage::parquet::write_parquet(&filepath, &all_ohlcv) {
                    println!("  Failed to write parquet: {}", e);
                } else {
                    println!("  Saved {} candles to {}", all_ohlcv.len(), filepath.display());
                }
            }
            _ => {
                println!("  Format not yet supported");
            }
        }
    }

    Ok(())
}

/// Parse OHLCV data from API JSON response
fn parse_api_response(json_str: &str, exchange: Exchange) -> anyhow::Result<Vec<OHLCV>> {
    let mut ohlcv_data = Vec::new();

    match exchange {
        Exchange::OKX => {
            #[derive(serde::Deserialize)]
            struct OkxResponse {
                data: Vec<Vec<String>>,
            }

            if let Ok(response) = serde_json::from_str::<OkxResponse>(json_str) {
                for candle in response.data {
                    if candle.len() >= 6 {
                        let timestamp: i64 = candle[0].parse().unwrap_or(0);
                        let open: f64 = candle[1].parse().unwrap_or(0.0);
                        let high: f64 = candle[2].parse().unwrap_or(0.0);
                        let low: f64 = candle[3].parse().unwrap_or(0.0);
                        let close: f64 = candle[4].parse().unwrap_or(0.0);
                        let volume: f64 = candle[5].parse().unwrap_or(0.0);

                        if timestamp > 0 {
                            ohlcv_data.push(OHLCV {
                                timestamp,
                                open,
                                high,
                                low,
                                close,
                                volume,
                            });
                        }
                    }
                }
            }
        }
        Exchange::Bybit => {
            #[derive(serde::Deserialize)]
            struct BybitResponse {
                result: BybitResult,
            }
            #[derive(serde::Deserialize)]
            struct BybitResult {
                list: Vec<Vec<String>>,
            }

            if let Ok(response) = serde_json::from_str::<BybitResponse>(json_str) {
                for candle in response.result.list {
                    if candle.len() >= 6 {
                        let timestamp: i64 = candle[0].parse().unwrap_or(0);
                        let open: f64 = candle[1].parse().unwrap_or(0.0);
                        let high: f64 = candle[2].parse().unwrap_or(0.0);
                        let low: f64 = candle[3].parse().unwrap_or(0.0);
                        let close: f64 = candle[4].parse().unwrap_or(0.0);
                        let volume: f64 = candle[5].parse().unwrap_or(0.0);

                        if timestamp > 0 {
                            ohlcv_data.push(OHLCV {
                                timestamp,
                                open,
                                high,
                                low,
                                close,
                                volume,
                            });
                        }
                    }
                }
            }
        }
        _ => {}
    }

    Ok(ohlcv_data)
}

/// Update to latest (incremental)
pub async fn update(cli: Cli) -> anyhow::Result<()> {
    let update_cmd = match &cli.command {
        crate::Commands::Update {
            exchange,
            pairs,
            timeframes,
        } => (exchange.clone(), pairs.clone(), timeframes.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata update");
    println!("Exchange: {}", update_cmd.0);
    println!("Pairs: {:?}", update_cmd.1);
    println!("Timeframes: {:?}", update_cmd.2);
    println!("\nNote: Update implementation is scaffolded.");

    Ok(())
}

/// Prepend historical data before existing range
pub async fn prepend(cli: Cli) -> anyhow::Result<()> {
    let prepend_cmd = match &cli.command {
        crate::Commands::Prepend {
            exchange,
            pair,
            timeframe,
            from,
        } => (exchange.clone(), pair.clone(), timeframe.clone(), from.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata prepend");
    println!("Exchange: {}", prepend_cmd.0);
    println!("Pair: {}", prepend_cmd.1);
    println!("Timeframe: {}", prepend_cmd.2);
    println!("From: {}", prepend_cmd.3);
    println!("\nNote: Prepend implementation is scaffolded.");

    Ok(())
}

/// Resume interrupted downloads
pub async fn resume(cli: Cli) -> anyhow::Result<()> {
    println!("ftdata resume");
    println!("Checking for interrupted downloads...");

    // Check for part files
    let temp_dir = cli.output.join("_temp");
    if !temp_dir.exists() {
        println!("No interrupted downloads found.");
        return Ok(());
    }

    let part_files: Vec<_> = std::fs::read_dir(&temp_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "part")
                        .unwrap_or(false)
                })
                .collect()
        })
        .unwrap_or_default();

    if part_files.is_empty() {
        println!("No interrupted downloads found.");
        return Ok(());
    }

    println!("Found {} interrupted download(s):", part_files.len());
    for entry in &part_files {
        println!("  - {:?}", entry.path().file_name());
    }

    println!("\nNote: Resume implementation is scaffolded.");

    Ok(())
}

/// Verify data integrity
pub async fn verify(cli: Cli) -> anyhow::Result<()> {
    let verify_cmd = match &cli.command {
        crate::Commands::Verify {
            exchange,
            pair,
            timeframe,
        } => (exchange.clone(), pair.clone(), timeframe.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata verify");
    println!("Exchange: {}", verify_cmd.0);
    println!("Pair: {}", verify_cmd.1);
    println!("Timeframe: {}", verify_cmd.2);

    let symbol = Symbol::parse(&verify_cmd.1)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let timeframe = Timeframe::from_str(&verify_cmd.2)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let file_path = cli.output.join(&verify_cmd.0).join(format!(
        "{}-{}.feather",
        symbol.freqtrade_format(),
        timeframe.label
    ));

    if !file_path.exists() {
        return Err(anyhow::anyhow!("File not found: {:?}", file_path));
    }

    println!("\nVerifying: {:?}", file_path);

    // Use analysis module to inspect
    let stats = ftdata_analysis::inspect_file(&file_path)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("\n--- Verification Results ---");
    println!("Rows: {}", stats.rows);
    println!("Size: {} bytes", stats.size_bytes);
    let date_range = stats.date_range.clone();
    if let Some(range) = &date_range {
        println!("Date range: {}", range);
    }

    let result = serde_json::json!({
        "status": "verified",
        "file": file_path.to_string_lossy(),
        "rows": stats.rows,
        "size_bytes": stats.size_bytes,
        "date_range": date_range,
    });

    println!("\n[MCP Output]");
    println!("{}", serde_json::to_string_pretty(&result)?);

    Ok(())
}

/// List detected gaps
pub async fn gaps(cli: Cli) -> anyhow::Result<()> {
    let gaps_cmd = match &cli.command {
        crate::Commands::Gaps {
            exchange,
            pair,
            timeframe,
            json,
        } => (exchange.clone(), pair.clone(), timeframe.clone(), *json),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata gaps");
    println!("Exchange: {}", gaps_cmd.0);
    println!("Pair: {}", gaps_cmd.1);
    println!("Timeframe: {}", gaps_cmd.2);

    let symbol = Symbol::parse(&gaps_cmd.1)
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let timeframe = Timeframe::from_str(&gaps_cmd.2)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let file_path = cli.output.join(&gaps_cmd.0).join(format!(
        "{}-{}.feather",
        symbol.freqtrade_format(),
        timeframe.label
    ));

    if !file_path.exists() {
        println!("File not found: {:?}", file_path);
        return Ok(());
    }

    // Read and analyze for gaps
    let df = ftdata_storage::feather::read_feather(&file_path)
        .map_err(|e| anyhow::anyhow!("feather error: {}", e))?;

    let timestamps: Vec<i64> = df
        .column("timestamp")
        .ok()
        .and_then(|c| c.i64().ok())
        .map(|c| (0..c.len()).filter_map(|i| c.get(i)).collect())
        .unwrap_or_default();

    let exchange = Exchange::from_str(&gaps_cmd.0)
        .unwrap_or(Exchange::Binance);
    let detector = ftdata_analysis::gaps::GapDetector::new(exchange, symbol.clone(), timeframe.clone());
    let detected_gaps = detector.detect_gaps(&timestamps);

    let report = GapReport::from_gaps(&exchange, &symbol, &timeframe, &detected_gaps);

    if gaps_cmd.3 {
        // JSON output
        println!("{}", serde_json::to_string_pretty(&report.to_json())?);
    } else {
        println!("\n{}", gaps::format_gaps(&detected_gaps));
    }

    Ok(())
}

/// Repair gaps by re-downloading
pub async fn repair(cli: Cli) -> anyhow::Result<()> {
    let repair_cmd = match &cli.command {
        crate::Commands::Repair {
            exchange,
            pair,
            timeframe,
        } => (exchange.clone(), pair.clone(), timeframe.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata repair");
    println!("Exchange: {}", repair_cmd.0);
    println!("Pair: {}", repair_cmd.1);
    println!("Timeframe: {}", repair_cmd.2);
    println!("\nNote: Repair implementation is scaffolded.");

    Ok(())
}

/// Inspect dataset metadata
pub async fn inspect(cli: Cli) -> anyhow::Result<()> {
    let inspect_cmd = match &cli.command {
        crate::Commands::Inspect { path } => path.clone(),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata inspect");
    println!("Path: {:?}", inspect_cmd);

    if !inspect_cmd.exists() {
        return Err(anyhow::anyhow!("File not found: {:?}", inspect_cmd));
    }

    let stats = ftdata_analysis::inspect_file(&inspect_cmd)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    println!("\n--- Dataset Info ---");
    println!("Symbol: {} {}", stats.exchange, stats.symbol);
    println!("Timeframe: {}", stats.timeframe);
    println!("Format: {}", stats.format);
    println!("Rows: {}", stats.rows);
    println!("Size: {} bytes ({:.2} MB)", stats.size_bytes, stats.size_bytes as f64 / 1_048_576.0);
    let date_range = stats.date_range.clone();
    if let Some(range) = &date_range {
        println!("Date range: {}", range);
    }

    println!("\n[MCP Output]");
    println!("{}", serde_json::to_string_pretty(&stats.to_json())?);

    Ok(())
}

/// List available datasets
pub async fn list(cli: Cli) -> anyhow::Result<()> {
    let list_cmd = match &cli.command {
        crate::Commands::List { exchange } => exchange.clone(),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata list");

    let exchange_filter = list_cmd.as_ref().map(|s| s.as_str());

    // Scan output directory
    if !cli.output.exists() {
        println!("No datasets found. Output directory does not exist.");
        return Ok(());
    }

    let mut datasets = vec![];

    for entry in std::fs::read_dir(&cli.output)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let exchange_name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if let Some(filter) = exchange_filter {
            if filter != exchange_name {
                continue;
            }
        }

        // Scan for data files
        if let Ok(files) = std::fs::read_dir(&path) {
            for file in files {
                if let Ok(file) = file {
                    let filename = file.file_name().to_string_lossy().to_string();
                    if filename.ends_with(".feather") || filename.ends_with(".parquet") {
                        let full_path = path.join(&filename);
                        if let Ok(stats) = std::fs::metadata(&full_path) {
                            datasets.push(serde_json::json!({
                                "exchange": exchange_name,
                                "file": filename,
                                "size_bytes": stats.len(),
                            }));
                        }
                    }
                }
            }
        }
    }

    if datasets.is_empty() {
        println!("No datasets found.");
    } else {
        println!("\n--- Datasets ---");
        println!("{}", serde_json::to_string_pretty(&datasets)?);
    }

    Ok(())
}

/// Convert between formats
pub async fn convert(cli: Cli) -> anyhow::Result<()> {
    let convert_cmd = match &cli.command {
        crate::Commands::Convert { input, output } => (input.clone(), output.clone()),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata convert");
    println!("Input: {:?}", convert_cmd.0);
    println!("Output: {:?}", convert_cmd.1);

    if !convert_cmd.0.exists() {
        return Err(anyhow::anyhow!("Input file not found"));
    }

    let input_ext = convert_cmd.0.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let output_ext = convert_cmd.1.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    println!("Converting {} -> {}", input_ext, output_ext);

    // Read source
    let ohlcv_data: Vec<OHLCV> = if input_ext == "feather" {
        let df = ftdata_storage::feather::read_feather(&convert_cmd.0)?;
        ftdata_storage::feather::df_to_ohlcv(&df)
    } else if input_ext == "parquet" {
        let df = ftdata_storage::parquet::read_parquet(&convert_cmd.0)?;
        // Convert to OHLCV via DataFrame conversion
        let timestamps = df.column("timestamp").unwrap().i64().unwrap();
        let opens = df.column("open").unwrap().f64().unwrap();
        let highs = df.column("high").unwrap().f64().unwrap();
        let lows = df.column("low").unwrap().f64().unwrap();
        let closes = df.column("close").unwrap().f64().unwrap();
        let volumes = df.column("volume").unwrap().f64().unwrap();

        (0..df.height()).filter_map(|i| {
            Some(OHLCV {
                timestamp: timestamps.get(i)?,
                open: opens.get(i)?,
                high: highs.get(i)?,
                low: lows.get(i)?,
                close: closes.get(i)?,
                volume: volumes.get(i)?,
            })
        }).collect()
    } else {
        return Err(anyhow::anyhow!("Unsupported input format: {}", input_ext));
    };

    // Write output
    if output_ext == "feather" {
        ftdata_storage::feather::write_feather(&convert_cmd.1, &ohlcv_data)?;
    } else if output_ext == "parquet" {
        ftdata_storage::parquet::write_parquet(&convert_cmd.1, &ohlcv_data)?;
    } else {
        return Err(anyhow::anyhow!("Unsupported output format: {}", output_ext));
    }

    println!("Conversion complete: {:?}", convert_cmd.1);

    Ok(())
}

/// Clean partial/broken files and failed checkpoints
pub async fn clean(cli: Cli) -> anyhow::Result<()> {
    let clean_cmd = match &cli.command {
        crate::Commands::Clean { exchange, dry_run } => (exchange.clone(), *dry_run),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata clean");
    if let Some(exchange) = &clean_cmd.0 {
        println!("Exchange: {}", exchange);
    }
    println!("Dry run: {}", clean_cmd.1);

    let temp_dir = cli.output.join("_temp");
    let locks_dir = cli.output.join("_locks");
    let checkpoint_db = cli.output.join("_checkpoints").join("checkpoints.db");

    let mut files_to_clean = vec![];

    // Find part files
    if temp_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.path().extension().map(|e| e == "part").unwrap_or(false) {
                    files_to_clean.push(entry.path());
                }
            }
        }
    }

    // Find lock files
    if locks_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&locks_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                if entry.path().extension().map(|e| e == "lock").unwrap_or(false) {
                    files_to_clean.push(entry.path());
                }
            }
        }
    }

    // Clear failed checkpoints from database
    let mut checkpoints_cleared = 0u64;
    if checkpoint_db.exists() {
        if let Ok(mgr) = ftdata_storage::checkpoint::CheckpointManager::new(&checkpoint_db) {
            let exchange_filter = clean_cmd.0.as_deref();
            checkpoints_cleared = mgr.clear_failed(exchange_filter)?;
            if checkpoints_cleared > 0 {
                println!("Found {} failed checkpoint(s) to clear", checkpoints_cleared);
            }
        }
    }

    if files_to_clean.is_empty() && checkpoints_cleared == 0 {
        println!("No partial files or failed checkpoints found.");
        return Ok(());
    }

    if !files_to_clean.is_empty() {
        println!("\nFound {} file(s) to clean:", files_to_clean.len());
        for f in &files_to_clean {
            println!("  - {:?}", f);
        }
    }

    if !clean_cmd.1 {
        println!("\nCleaning...");
        for f in &files_to_clean {
            std::fs::remove_file(f)?;
        }
        if checkpoints_cleared > 0 {
            println!("Cleared {} failed checkpoint(s)", checkpoints_cleared);
        }
        println!("Clean complete.");
    } else {
        println!("\nDry run - no files were deleted.");
    }

    Ok(())
}

/// Show download plan (dry-run)
pub async fn plan(cli: Cli) -> anyhow::Result<()> {
    let plan_cmd = match &cli.command {
        crate::Commands::Plan {
            exchange,
            pairs,
            timeframes,
            timerange,
            json,
        } => (exchange.clone(), pairs.clone(), timeframes.clone(), timerange.clone(), *json),
        _ => return Err(anyhow::anyhow!("invalid command")),
    };

    println!("ftdata plan");
    println!("Exchange: {}", plan_cmd.0);
    println!("Pairs: {:?}", plan_cmd.1);
    println!("Timeframes: {:?}", plan_cmd.2);
    println!("Time range: {}", plan_cmd.3);

    let exchange = Exchange::from_str(&plan_cmd.0)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let time_range = TimeRange::parse(&plan_cmd.3)
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let timeframes: Vec<Timeframe> = plan_cmd.2
        .iter()
        .filter_map(|t| Timeframe::from_str(t).ok())
        .collect();

    let symbols: Vec<Symbol> = plan_cmd.1
        .iter()
        .filter_map(|p| Symbol::parse(p).ok())
        .collect();

    let mut overall_plan = OverallPlan::new();

    for symbol in &symbols {
        for timeframe in &timeframes {
            let source_resolver = SourceResolver::new(exchange);
            let source = source_resolver.resolve_source(true);
            let tf = timeframe.clone();

            let mut plan = DownloadPlan::new(
                exchange,
                symbol.clone(),
                tf.clone(),
                MarketType::Spot,
                CandleType::OHLCV,
                time_range,
                source,
            );

            if source == DownloadSource::Bulk {
                let decomposer = ChunkDecomposer::new(exchange, tf.clone());
                let chunks = decomposer.decompose_monthly(time_range);

                for chunk in chunks {
                    plan.chunks.push(ChunkPlan {
                        start: chunk.start,
                        end: chunk.end,
                        status: ChunkStatus::Pending,
                        estimated_size: decomposer.estimate_chunk_size(&chunk),
                        url: None,
                        etag: None,
                    });
                }
            }

            plan.estimated_size_bytes = plan
                .chunks
                .iter()
                .map(|c| c.estimated_size)
                .sum();

            overall_plan.add_plan(plan);
        }
    }

    if plan_cmd.4 {
        // JSON output
        println!("\n{}", serde_json::to_string_pretty(&overall_plan.to_json())?);
    } else {
        println!("\n--- Download Plan ---");
        for p in &overall_plan.plans {
            println!("\n{} {} {}", p.exchange, p.symbol, p.timeframe);
            println!("  Time range: {} - {}", p.time_range.start, p.time_range.end);
            println!("  Source: {}", p.source);
            println!("  Chunks: {}", p.chunks.len());
            println!("  Estimated size: {:.2} GB", p.estimated_size_bytes as f64 / 1_073_741_824.0);
        }

        println!("\n--- Summary ---");
        println!("Total files: {}", overall_plan.total_chunks);
        println!("Total estimated download: {:.2} GB", overall_plan.estimated_total_bytes as f64 / 1_073_741_824.0);
    }

    Ok(())
}
