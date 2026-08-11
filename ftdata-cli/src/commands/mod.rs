//! CLI command implementations

use crate::Cli;
use ftdata_core::domain::*;
use ftdata_core::planner::{ChunkDecomposer, DownloadPlan, OverallPlan, SourceResolver, ChunkPlan, ChunkStatus};
use ftdata_analysis::gaps::{self, GapReport};
use std::path::PathBuf;
use std::str::FromStr;

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

    // Create overall plan
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

            // Get bulk URLs if available
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

    println!("\n--- Download Plan ---");
    println!("Total chunks: {}", overall_plan.total_chunks);
    println!("Pending chunks: {}", overall_plan.total_pending_chunks);
    println!(
        "Estimated size: {:.2} GB",
        overall_plan.estimated_total_bytes as f64 / 1_073_741_824.0
    );

    println!("\n[MCP Output]");
    println!("{}", serde_json::to_string_pretty(&overall_plan.to_json())?);

    println!("\nNote: Download implementation is scaffolded.");
    println!("Run with --verbose for more details.");

    Ok(())
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

/// Clean partial/broken files
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

    if files_to_clean.is_empty() {
        println!("No partial files found.");
        return Ok(());
    }

    println!("\nFound {} file(s) to clean:", files_to_clean.len());
    for f in &files_to_clean {
        println!("  - {:?}", f);
    }

    if !clean_cmd.1 {
        println!("\nCleaning...");
        for f in &files_to_clean {
            std::fs::remove_file(f)?;
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
