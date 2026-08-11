# ftdata - High-Performance Historical Market Data Downloader

A Rust-based, production-grade tool for downloading and managing historical OHLCV market data from major cryptocurrency exchanges. Designed for algorithmic trading with Freqtrade compatibility.

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Features

- **Bulk Download First** - Prefers bulk archive downloads over API calls for maximum speed
- **Resumable Downloads** - Dual checkpoint system (HTTP Range + logical timestamp)
- **Multi-Exchange Support** - Binance, Bybit, OKX with exchange-specific optimizations
- **Freqtrade Compatible** - Native Feather/Parquet output in Freqtrade's directory structure
- **MCP Native** - Structured JSON output for AI agent integration
- **Rate Limiting** - Token bucket per exchange to respect API limits
- **Data Validation** - Multi-layer validation (schema, timestamp, OHLC, gap, duplicate)
- **Gap Detection & Repair** - Automatic detection and repair of missing data

## Architecture

```
ftdata/
├── ftdata-cli         # CLI interface (12 commands)
├── ftdata-core        # Domain types, checkpoint, planner, validator
├── ftdata-http        # HTTP client, rate limiting, retry, range requests
├── ftdata-sources     # Exchange adapters (Binance, Bybit, OKX)
├── ftdata-storage     # Feather/Parquet I/O via Polars
└── ftdata-analysis    # Gap detection, statistics, duplicate removal
```

## Installation

### Prerequisites

- Rust 1.75+ ([install rust](https://rustup.rs))
- Linux/macOS/Windows

### Build from Source

```bash
# Clone the repository
git clone https://github.com/gyc567/fq-data-downloader.git
cd fq-data-downloader

# Build release version
cargo build --release

# The binary will be at ./target/release/ftdata
./target/release/ftdata --help
```

### Pre-built Binaries

Download from the [Releases](https://github.com/gyc567/fq-data-downloader/releases) page.

## Quick Start

### Download BTC/USDT 1m data from Binance

```bash
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230601
```

### Plan a download (dry-run)

```bash
ftdata plan --exchange binance --pairs BTC/USDT ETH/USDT --timeframes 1m 5m --timerange 20230101-20231231 -j
```

### Verify downloaded data

```bash
ftdata verify --path user_data/data/binance/BTC_USDT-1m.feather
```

### Detect and repair gaps

```bash
# Detect gaps
ftdata gaps --exchange binance --pair BTC/USDT --timeframe 1m --path user_data/data/binance/BTC_USDT-1m.feather

# Repair gaps
ftdata repair --exchange binance --pair BTC/USDT --timeframe 1m --timerange 20230101-20230601
```

## CLI Commands

### Data Download

| Command | Description |
|---------|-------------|
| `download` | Download historical data with full options |
| `update` | Update to latest (incremental download) |
| `prepend` | Download data before existing range |
| `resume` | Resume interrupted downloads |
| `plan` | Show download plan (dry-run) |

### Data Quality

| Command | Description |
|---------|-------------|
| `verify` | Verify data integrity and checksum |
| `gaps` | List detected gaps in dataset |
| `repair` | Repair gaps by re-downloading |
| `inspect` | Inspect dataset metadata |

### Utilities

| Command | Description |
|---------|-------------|
| `list` | List available datasets |
| `convert` | Convert between formats (feather/parquet/json) |
| `clean` | Clean partial/broken files |
| `help` | Print help information |

## Usage Examples

### Complete Workflow

```bash
# 1. Plan your download
ftdata plan --exchange binance \
  --pairs BTC/USDT ETH/USDT BNB/USDT \
  --timeframes 1m 5m 15m \
  --timerange 20220101-20230101

# 2. Download data
ftdata download --exchange binance \
  --pairs BTC/USDT ETH/USDT \
  --timeframes 1m 5m \
  --timerange 20220101-20230101 \
  --format feather

# 3. Verify integrity
ftdata verify --path user_data/data/binance/BTC_USDT-1m.feather

# 4. Check for gaps
ftdata gaps --exchange binance --pair BTC/USDT --timeframe 1m \
  --path user_data/data/binance/BTC_USDT-1m.feather

# 5. Repair if needed
ftdata repair --exchange binance --pair BTC/USDT --timeframe 1m
```

### Exchange-Specific URLs

**Binance** (bulk archives available):
```bash
# Monthly klines as ZIP archives
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230201
```

**Bybit**:
```bash
# Daily archives
ftdata download --exchange bybit --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230110
```

**OKX** (no bulk archives, uses REST API):
```bash
# Falls back to REST API
ftdata download --exchange okx --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102
```

### Multi-Timeframe Download

```bash
# Download multiple timeframes at once
ftdata download --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m 5m 15m 1h 4h 1d \
  --timerange 20230101-20231231
```

### JSON Output for MCP/AI Integration

```bash
# Get structured JSON output
ftdata plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -j

# Inspect with JSON output
ftdata inspect --path user_data/data/binance/BTC_USDT-1m.feather -j
```

## Configuration

### Output Directory

```bash
# Default: user_data/data
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m -o ./data
```

### Data Formats

| Format | Extension | Description |
|--------|-----------|-------------|
| Feather | `.feather` | Apache Arrow feather (default, fastest) |
| Parquet | `.parquet` | Apache Parquet (compressed, smaller) |
| JSON | `.json` | Plain JSON |
| JSONGz | `.json.gz` | Gzipped JSON |

### Time Range Format

```
YYYYMMDD-YYYYMMDD
```

Examples:
- `20200101-20250101` - Full range from 2020 to 2025
- `20230101-` - From 2023 to now
- `-20230101` - Everything before 2023

## Data Directory Structure

ftdata creates Freqtrade-compatible directory structure:

```
user_data/data/
├── binance/
│   ├── BTC_USDT-1m.feather
│   ├── BTC_USDT-5m.feather
│   ├── ETH_USDT-1m.feather
│   └── ...
├── bybit/
│   └── ...
└── okx/
    └── ...
```

## Architecture Deep Dive

### Checkpoint System

ftdata uses SQLite for tracking download progress:

```bash
# Checkpoints stored at
~/.ftdata/checkpoints.db
```

The checkpoint system tracks:
- Download status (pending, downloading, verified, failed)
- Downloaded byte ranges
- Checksums for validation
- ETag/Last-Modified for cache validation

### Rate Limiting

Each exchange has its own rate limiter:

| Exchange | Rate Limit |
|----------|------------|
| Binance | 20 requests/sec |
| Bybit | 10 requests/sec |
| OKX | 10 requests/sec |

### Chunk Decomposition

Downloads are split into monthly chunks for:
- Parallel downloading
- Resume support
- Progress tracking
- Reduced memory usage

### Validation Layers

1. **Schema Validation** - Correct column types and structure
2. **Timestamp Validation** - Proper alignment to timeframe
3. **OHLC Validation** - High ≥ max(Open, Close), Low ≤ min(Open, Close)
4. **Gap Detection** - Missing candles identified
5. **Duplicate Detection** - Duplicate timestamps removed

## Troubleshooting

### Common Issues

**"No space left on device"**
```bash
# Use parquet format (smaller files)
ftdata download --exchange binance --pairs BTC/USDT --format parquet
```

**"Rate limit exceeded"**
```bash
# Wait and retry
sleep 60
ftdata resume --exchange binance --pairs BTC/USDT --timeframes 1m
```

**"Checksum mismatch"**
```bash
# Re-download the corrupted chunk
ftdata repair --exchange binance --pair BTC/USDT --timeframe 1m --timerange 20230101-20230201
```

**"Resume failed"**
```bash
# Clean and restart
ftdata clean --exchange binance --pairs BTC/USDT --timeframes 1m
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m
```

### Debug Mode

```bash
# Enable verbose logging
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m -vv --log-format json
```

### Check Download Status

```bash
# Inspect downloaded data
ftdata inspect --path user_data/data/binance/BTC_USDT-1m.feather

# List all datasets
ftdata list --exchange binance
```

## Contributing

Contributions are welcome! Please read these guidelines before submitting PRs.

### Development Setup

```bash
# Fork and clone
git clone https://github.com/gyc567/fq-data-downloader.git
cd fq-data-downloader

# Create a branch
git checkout -b feature/your-feature-name

# Run tests
cargo test

# Build
cargo build --release

# Format
cargo fmt --check
```

### Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` to catch issues
- Add tests for new features
- Update documentation

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

## License

MIT License - see [LICENSE](LICENSE) file.

## Related Projects

- [Freqtrade](https://www.freqtrade.io/) - Cryptocurrency trading bot
- [Binance public data](https://github.com/binance/binance-publicdata/)
- [Bybit data archive](https://github.com/bybit-exchange/bybit-archive)

## Support

- Open an [Issue](https://github.com/gyc567/fq-data-downloader/issues) for bugs
- Discussions for questions
- PRs welcome!

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for version history.
