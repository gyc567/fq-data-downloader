//! ftdata CLI - High-Performance Historical Market Data Downloader

mod commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name = "ftdata")]
#[command(version = "0.1.0")]
#[command(author = "ftdata contributors")]
#[command(about = "High-Performance Historical Market Data Downloader", long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Log format
    #[arg(long, global = true, default_value = "text")]
    log_format: String,

    /// Output directory
    #[arg(short, long, global = true, default_value = "user_data/data")]
    output: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Download historical data
    Download {
        /// Exchange to download from
        #[arg(long)]
        exchange: String,

        /// Trading pairs (e.g., BTC/USDT ETH/USDT)
        #[arg(long, num_args = 1..)]
        pairs: Vec<String>,

        /// Timeframes (e.g., 1m 5m 1h)
        #[arg(long, default_value = "1m")]
        timeframes: Vec<String>,

        /// Time range (e.g., 20200101-20250101)
        #[arg(long, default_value = "20200101-")]
        timerange: String,

        /// Output format (feather, parquet, json)
        #[arg(long, default_value = "feather")]
        format: String,

        /// Market type (spot, futures)
        #[arg(long, default_value = "spot")]
        market: String,
    },

    /// Update to latest (incremental)
    Update {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pairs
        #[arg(long, num_args = 1..)]
        pairs: Vec<String>,

        /// Timeframes
        #[arg(long, default_value = "1m")]
        timeframes: Vec<String>,
    },

    /// Download data before existing range (prepend)
    Prepend {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pair
        #[arg(long)]
        pair: String,

        /// Timeframe
        #[arg(long, default_value = "1m")]
        timeframe: String,

        /// Start date
        #[arg(long)]
        from: String,
    },

    /// Resume interrupted downloads
    Resume,

    /// Verify data integrity
    Verify {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pair
        #[arg(long)]
        pair: String,

        /// Timeframe
        #[arg(long, default_value = "1m")]
        timeframe: String,
    },

    /// List detected gaps
    Gaps {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pair
        #[arg(long)]
        pair: String,

        /// Timeframe
        #[arg(long, default_value = "1m")]
        timeframe: String,

        /// Output as JSON
        #[arg(long, short)]
        json: bool,
    },

    /// Repair gaps by re-downloading
    Repair {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pair
        #[arg(long)]
        pair: String,

        /// Timeframe
        #[arg(long, default_value = "1m")]
        timeframe: String,
    },

    /// Inspect dataset metadata
    Inspect {
        /// Path to file
        #[arg(long)]
        path: PathBuf,
    },

    /// List available datasets
    List {
        /// Exchange filter
        #[arg(long)]
        exchange: Option<String>,
    },

    /// Convert between formats
    Convert {
        /// Input file
        #[arg(long)]
        input: PathBuf,

        /// Output file
        #[arg(long)]
        output: PathBuf,
    },

    /// Clean partial/broken files
    Clean {
        /// Exchange
        #[arg(long)]
        exchange: Option<String>,

        /// Dry run
        #[arg(long, short)]
        dry_run: bool,
    },

    /// Show download plan (dry-run)
    Plan {
        /// Exchange
        #[arg(long)]
        exchange: String,

        /// Trading pairs
        #[arg(long, num_args = 1..)]
        pairs: Vec<String>,

        /// Timeframes
        #[arg(long, default_value = "1m")]
        timeframes: Vec<String>,

        /// Time range
        #[arg(long, default_value = "20200101-")]
        timerange: String,

        /// Output as JSON
        #[arg(long, short)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let filter = if cli.verbose {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    match cli.log_format.as_str() {
        "json" => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .json()
                .init();
        }
        _ => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .init();
        }
    }

    // Execute command
    match cli.command {
        Commands::Download { .. } => commands::download(cli).await?,
        Commands::Update { .. } => commands::update(cli).await?,
        Commands::Prepend { .. } => commands::prepend(cli).await?,
        Commands::Resume => commands::resume(cli).await?,
        Commands::Verify { .. } => commands::verify(cli).await?,
        Commands::Gaps { .. } => commands::gaps(cli).await?,
        Commands::Repair { .. } => commands::repair(cli).await?,
        Commands::Inspect { .. } => commands::inspect(cli).await?,
        Commands::List { .. } => commands::list(cli).await?,
        Commands::Convert { .. } => commands::convert(cli).await?,
        Commands::Clean { .. } => commands::clean(cli).await?,
        Commands::Plan { .. } => commands::plan(cli).await?,
    }

    Ok(())
}
