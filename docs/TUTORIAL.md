# ftdata Tutorial - Complete Guide

This tutorial covers everything you need to know to effectively use ftdata for downloading and managing cryptocurrency historical data.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Basic Download Operations](#basic-download-operations)
3. [Understanding the Plan Command](#understanding-the-plan-command)
4. [Working with Multiple Pairs and Timeframes](#working-with-multiple-pairs-and-timeframes)
5. [Data Verification and Quality Checks](#data-verification-and-quality-checks)
6. [Gap Detection and Repair](#gap-detection-and-repair)
7. [Format Conversion](#format-conversion)
8. [Incremental Updates](#incremental-updates)
9. [Resume Interrupted Downloads](#resume-interrupted-downloads)
10. [Advanced Usage](#advanced-usage)

---

## Getting Started

### Installation

```bash
# Clone and build
git clone https://github.com/gyc567/fq-data-downloader.git
cd fq-data-downloader
cargo build --release

# Add to PATH (optional)
export PATH="$PATH:$(pwd)/target/release"
```

### Verify Installation

```bash
ftdata --help
```

---

## Basic Download Operations

### Download Single Pair, Single Timeframe

The most basic operation - download BTC/USDT 1-minute data for January 2023:

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201
```

Output:
```
ftdata download
Exchange: binance
Pairs: ["BTC/USDT"]
Timeframes: ["1m"]
Time range: 20230101-20230201
Format: feather
Output: "user_data/data"

--- Download Plan ---
Total chunks: 1
Pending chunks: 1
Estimated size: 0.00 GB
```

### Download with Specific Output Directory

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201 \
  -o ./my_data
```

### Download as Parquet Format

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201 \
  --format parquet
```

---

## Understanding the Plan Command

The `plan` command shows what would be downloaded without actually downloading. This is useful for:

- Estimating download size
- Understanding chunk breakdown
- Verifying timerange parameters

### Dry Run with JSON Output

```bash
ftdata plan \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230601 \
  -j
```

JSON output:
```json
{
  "estimated_total_bytes": 360000,
  "total_chunks": 6,
  "total_pending_chunks": 6,
  "plans": [
    {
      "exchange": "binance",
      "symbol": "BTC/USDT",
      "timeframe": "1m",
      "time_range": {
        "from": 1672531200000,
        "to": 1685548800000
      },
      "chunks": 6,
      "pending_chunks": 6,
      "source": "bulk",
      "estimated_size_bytes": 360000
    }
  ]
}
```

### Plan Multiple Pairs

```bash
ftdata plan \
  --exchange binance \
  --pairs BTC/USDT ETH/USDT BNB/USDT \
  --timeframes 1m 5m \
  --timerange 20230101-20230401
```

---

## Working with Multiple Pairs and Timeframes

### Download Multiple Pairs

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT ETH/USDT BNB/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201
```

### Download Multiple Timeframes

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m 5m 15m 1h \
  --timerange 20230101-20230201
```

### Realistic Full Download Example

Download 2 years of data for backtesting:

```bash
ftdata download \
  --exchange binance \
  --pairs BTC/USDT ETH/USDT \
  --timeframes 1m 5m 15m 1h 4h 1d \
  --timerange 20220101-20240101 \
  --format feather
```

**Note:** This will take significant time and disk space. Use `plan` first to estimate.

---

## Data Verification and Quality Checks

### Inspect Downloaded Data

```bash
ftdata inspect --path user_data/data/binance/BTC_USDT-1m.feather
```

Output:
```
--- Dataset Info ---
Symbol: binance BTC/USDT
Timeframe: 1m
Format: feather
Rows: 44640
Size: 1342368 bytes (1.28 MB)
Date range: 2023-01-01 00:00:00 → 2023-01-31 23:59:00

[MCP Output]
{"rows": 44640, ...}
```

### Verify Data Integrity

```bash
ftdata verify --path user_data/data/binance/BTC_USDT-1m.feather
```

Output:
```
--- Verification Results ---
Rows: 44640
Size: 1342368 bytes

[MCP Output]
{"status": "verified", "file": "user_data/data/binance/BTC_USDT-1m.feather", ...}
```

### List All Downloaded Datasets

```bash
ftdata list --exchange binance
```

---

## Gap Detection and Repair

Gaps in OHLCV data can cause significant issues in backtesting. ftdata provides tools to detect and repair them.

### Detect Gaps

```bash
ftdata gaps \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m \
  --path user_data/data/binance/BTC_USDT-1m.feather
```

Output (if gaps found):
```
--- Gap Detection ---
BTC/USDT 1m: 3 gap(s) detected:

  2023-01-15 10:30:00 → 2023-01-15 12:45:00
    Duration: 2h 15m
    Reason: missing 135 candles (8100000ms gap)

  2023-01-20 03:00:00 → 2023-01-20 03:05:00
    Duration: 5m
    Reason: missing 4 candles (240000ms gap)
```

### JSON Output for Gap Detection

```bash
ftdata gaps \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m \
  --path user_data/data/binance/BTC_USDT-1m.feather \
  -j
```

### Repair Gaps

```bash
ftdata repair \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m \
  --timerange 20230101-20230201
```

The repair command will:
1. Identify gaps in the existing data
2. Re-download the missing portions
3. Merge the data
4. Verify integrity

---

## Format Conversion

ftdata can convert between different data formats.

### Feather to Parquet

```bash
ftdata convert \
  --input user_data/data/binance/BTC_USDT-1m.feather \
  --output user_data/data/binance/BTC_USDT-1m.parquet
```

### Parquet to Feather

```bash
ftdata convert \
  --input user_data/data/binance/BTC_USDT-1m.parquet \
  --output user_data/data/binance/BTC_USDT-1m.feather
```

### Batch Conversion

For batch conversion, use a shell loop:

```bash
for file in user_data/data/binance/*.feather; do
  ftdata convert --input "$file" --output "${file%.feather}.parquet"
done
```

---

## Incremental Updates

To get the latest data without re-downloading everything:

### Update to Latest

```bash
ftdata update \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m
```

This appends new candles since the last download.

### Prepend Data (Download Earlier Data)

```bash
ftdata prepend \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20190101-20200101
```

---

## Resume Interrupted Downloads

If a download was interrupted (network failure, etc.):

```bash
ftdata resume \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m
```

The resume command will:
1. Check SQLite checkpoint database
2. Identify incomplete chunks
3. Resume from where it left off

### Force Clean and Restart

If resume doesn't work, clean and restart:

```bash
ftdata clean \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m

ftdata download \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201
```

---

## Advanced Usage

### MCP/AI Integration

ftdata is designed to work with AI agents via structured JSON output:

```bash
# Get complete plan as JSON
ftdata plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -j

# Get inspection data as JSON
ftdata inspect --path user_data/data/binance/BTC_USDT-1m.feather -j

# Get gap report as JSON
ftdata gaps --exchange binance --pair BTC/USDT --timeframe 1m --path user_data/data/binance/BTC_USDT-1m.feather -j
```

### Programmatic Usage

You can call ftdata from other programs:

```python
import subprocess
import json

result = subprocess.run([
    'ftdata', 'plan',
    '--exchange', 'binance',
    '--pairs', 'BTC/USDT',
    '--timeframes', '1m',
    '--timerange', '20230101-20230102',
    '-j'
], capture_output=True, text=True)

plan = json.loads(result.stdout)
print(f"Estimated size: {plan['estimated_total_bytes']} bytes")
```

### Custom Time Ranges

```bash
# Last 30 days
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230601-

# Specific year
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20220101-20230101

# Last N days (calculate dates manually)
ftdata download --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230601-20230801
```

### Different Exchanges

**Bybit:**
```bash
ftdata download \
  --exchange bybit \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201
```

**OKX:**
```bash
ftdata download \
  --exchange okx \
  --pairs BTC/USDT \
  --timeframes 1m \
  --timerange 20230101-20230201
```

---

## Troubleshooting

### Slow Download Speed

- Check your internet connection
- Try during off-peak hours
- Use bulk download (default) instead of API

### Large Disk Space Usage

```bash
# Use parquet format (smaller)
ftdata download --format parquet ...

# Download only needed timeframes
ftdata download --timeframes 1m 5m 1h ...
```

### "No data available" Error

- Check if the exchange supports the requested timerange
- Binance has data from ~July 2017
- Bybit has data from ~October 2019
- OKX has data from ~January 2019

### Debug Issues

```bash
# Verbose logging
ftdata download -vv ...

# JSON log format
ftdata download --log-format json ...
```

---

## Best Practices

1. **Always use `plan` first** - Estimate size and time before downloading
2. **Use appropriate timeframes** - 1m for day trading, 1h/4h for swing trading
3. **Verify after download** - Always run `verify` on new data
4. **Check for gaps** - Run `gaps` before starting backtests
5. **Use consistent formats** - Stick to feather for speed, parquet for storage
6. **Backup checkpoints** - The SQLite database tracks all progress

---

## Next Steps

- Read the [ARCHITECTURE.md](ARCHITECTURE.md) for deep dive into internals
- Check [DESIGN.md](docs/DESIGN.md) for technical design details
- Join discussions for questions and feature requests
