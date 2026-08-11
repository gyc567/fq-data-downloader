# ftdata Agent Tutorial

A comprehensive guide for AI agents to use ftdata for cryptocurrency market data downloading and management.

## Overview

ftdata provides an MCP (Model Context Protocol) interface that allows AI agents to:
- Download historical OHLCV data from Binance, Bybit, and OKX
- Plan and estimate download requirements
- Verify data integrity
- Detect and repair data gaps
- Convert between data formats

## MCP Server Setup

### For Claude Code

```bash
# Install ftdata MCP server (when published)
npx @ftdata/mcp-server install

# Or configure manually in ~/.claude/settings.json
```

### For Other Agents (Cursor, Windsurf, etc.)

Add to your MCP settings JSON:

```json
{
  "mcpServers": {
    "ftdata": {
      "command": "node",
      "args": ["/path/to/ftdata-mcp-server"],
      "env": {
        "FTDATA_OUTPUT_DIR": "./data"
      }
    }
  }
}
```

## Tool Definitions

When connected via MCP, ftdata provides the following tools:

### 1. `ftdata_plan`

Plan a data download without executing it.

**Parameters:**
```json
{
  "exchange": "binance|bybit|okx",
  "pairs": ["BTC/USDT", "ETH/USDT"],
  "timeframes": ["1m", "4h", "1d"],
  "timerange": "20230101-20231231",
  "output_dir": "./data"
}
```

**Response:**
```json
{
  "estimated_total_bytes": 1073741824,
  "total_chunks": 12,
  "plans": [
    {
      "exchange": "binance",
      "symbol": "BTC/USDT",
      "timeframe": "4h",
      "time_range": {
        "from": 1672531200000,
        "to": 1704067200000
      },
      "chunks": 12,
      "source": "bulk"
    }
  ]
}
```

---

### 2. `ftdata_download`

Download historical market data.

**Parameters:**
```json
{
  "exchange": "binance",
  "pairs": ["BTC/USDT"],
  "timeframes": ["4h"],
  "timerange": "20230101-20251231",
  "format": "feather",
  "output_dir": "./data"
}
```

**Response:**
```json
{
  "status": "success",
  "downloaded": [
    {
      "symbol": "BTC/USDT",
      "timeframe": "4h",
      "candles": 6562,
      "from": "2023-01-01T00:00:00Z",
      "to": "2025-12-31T23:59:59Z"
    }
  ],
  "files_written": ["./data/binance/BTC_USDT-4h.feather"]
}
```

---

### 3. `ftdata_inspect`

Inspect downloaded dataset metadata.

**Parameters:**
```json
{
  "path": "./data/binance/BTC_USDT-4h.feather"
}
```

**Response:**
```json
{
  "symbol": "BTC/USDT",
  "timeframe": "4h",
  "exchange": "binance",
  "rows": 6562,
  "size_bytes": 1048576,
  "date_range": {
    "from": "2023-01-01T00:00:00Z",
    "to": "2025-12-31T23:59:59Z"
  },
  "format": "feather",
  "verified": true
}
```

---

### 4. `ftdata_gaps`

Detect gaps in OHLCV data.

**Parameters:**
```json
{
  "exchange": "binance",
  "symbol": "BTC/USDT",
  "timeframe": "4h",
  "path": "./data/binance/BTC_USDT-4h.feather",
  "output_format": "json"
}
```

**Response:**
```json
{
  "exchange": "binance",
  "symbol": "BTC/USDT",
  "timeframe": "4h",
  "total_gaps": 2,
  "gaps": [
    {
      "from_ts": 1672531200000,
      "to_ts": 1672617600000,
      "duration": "8h",
      "missing_candles": 2
    }
  ]
}
```

---

### 5. `ftdata_repair`

Repair gaps by re-downloading missing data.

**Parameters:**
```json
{
  "exchange": "binance",
  "symbol": "BTC/USDT",
  "timeframe": "4h",
  "timerange": "20230101-20231231",
  "output_dir": "./data"
}
```

**Response:**
```json
{
  "status": "repaired",
  "gaps_filled": 2,
  "candles_added": 186
}
```

---

### 6. `ftdata_convert`

Convert between data formats.

**Parameters:**
```json
{
  "input": "./data/binance/BTC_USDT-4h.feather",
  "output": "./data/binance/BTC_USDT-4h.parquet",
  "format": "parquet"
}
```

**Response:**
```json
{
  "status": "converted",
  "input_rows": 6562,
  "output_file": "./data/binance/BTC_USDT-4h.parquet",
  "size_reduction_percent": 35
}
```

---

### 7. `ftdata_list`

List available downloaded datasets.

**Parameters:**
```json
{
  "exchange": "binance",
  "output_dir": "./data"
}
```

**Response:**
```json
{
  "exchange": "binance",
  "datasets": [
    {
      "symbol": "BTC/USDT",
      "timeframe": "4h",
      "path": "./data/binance/BTC_USDT-4h.feather",
      "rows": 6562
    },
    {
      "symbol": "ETH/USDT",
      "timeframe": "4h",
      "path": "./data/binance/ETH_USDT-4h.feather",
      "rows": 6562
    }
  ]
}
```

---

## Agent Workflows

### Workflow 1: Prepare Backtesting Data

```
User: "I need BTC and ETH 4h data for 2023-2025 for backtesting"

Agent:
1. Call ftdata_plan to estimate requirements
2. Call ftdata_download for each pair/timeframe
3. Call ftdata_gaps to verify data quality
4. If gaps found, call ftdata_repair
5. Call ftdata_inspect to confirm final dataset
```

**Example Agent Code:**
```python
# Step 1: Plan the download
plan_result = await mcp.call_tool("ftdata_plan", {
    "exchange": "binance",
    "pairs": ["BTC/USDT", "ETH/USDT"],
    "timeframes": ["4h"],
    "timerange": "20230101-20260101"
})

# Step 2: Download data
for pair in ["BTC/USDT", "ETH/USDT"]:
    await mcp.call_tool("ftdata_download", {
        "exchange": "binance",
        "pairs": [pair],
        "timeframes": ["4h"],
        "timerange": "20230101-20260101",
        "output_dir": "./backtest_data"
    })

# Step 3: Verify data quality
gaps = await mcp.call_tool("ftdata_gaps", {
    "exchange": "binance",
    "symbol": "BTC/USDT",
    "timeframe": "4h",
    "path": "./backtest_data/binance/BTC_USDT-4h.feather"
})

if gaps.total_gaps > 0:
    await mcp.call_tool("ftdata_repair", {
        "exchange": "binance",
        "symbol": "BTC/USDT",
        "timeframe": "4h",
        "output_dir": "./backtest_data"
    })
```

---

### Workflow 2: Daily Data Update

```
User: "Update my BTC data to the latest"

Agent:
1. Call ftdata_inspect to see current data range
2. Call ftdata_download with timerange from last date to now
3. Call ftdata_verify to confirm integrity
```

**Example Agent Code:**
```python
# Get current data status
inspect = await mcp.call_tool("ftdata_inspect", {
    "path": "./data/binance/BTC_USDT-4h.feather"
})

last_date = inspect.date_range.to  # e.g., "2025-08-01"
today = datetime.now().strftime("%Y%m%d")

# Update from last date to today
await mcp.call_tool("ftdata_download", {
    "exchange": "binance",
    "pairs": ["BTC/USDT"],
    "timeframes": ["4h"],
    "timerange": f"{last_date[:8]}-{today}",
    "output_dir": "./data"
})
```

---

### Workflow 3: Format Conversion for Trading Bot

```
User: "Convert my data to Parquet format for my trading bot"

Agent:
1. Call ftdata_list to find all datasets
2. Call ftdata_convert for each file
```

**Example Agent Code:**
```python
# List all datasets
datasets = await mcp.call_tool("ftdata_list", {
    "exchange": "binance",
    "output_dir": "./data"
})

# Convert each to Parquet
for dataset in datasets.datasets:
    await mcp.call_tool("ftdata_convert", {
        "input": dataset.path,
        "output": dataset.path.replace(".feather", ".parquet"),
        "format": "parquet"
    })
```

---

### Workflow 4: Gap Detection Before Backtest

```
User: "Check if my data has gaps before I run a backtest"

Agent:
1. Call ftdata_gaps on all datasets
2. Report any gaps found
3. Offer to repair if needed
```

**Example Agent Code:**
```python
datasets = await mcp.call_tool("ftdata_list", {
    "exchange": "binance",
    "output_dir": "./data"
})

gap_report = []
for dataset in datasets.datasets:
    gaps = await mcp.call_tool("ftdata_gaps", {
        "exchange": "binance",
        "symbol": dataset.symbol,
        "timeframe": dataset.timeframe,
        "path": dataset.path
    })
    if gaps.total_gaps > 0:
        gap_report.append({
            "dataset": f"{dataset.symbol} {dataset.timeframe}",
            "gaps": gaps.gaps
        })

if gap_report:
    print("⚠️ Gaps detected:")
    for item in gap_report:
        print(f"  {item['dataset']}: {len(item['gaps'])} gaps")
    print("\nRecommended: Run ftdata_repair to fill gaps")
else:
    print("✅ No gaps detected - data is ready for backtesting")
```

---

## Exchange Configuration

### Binance
- **Bulk Archives:** Available at `data.binance.vision`
- **Timeframes:** 1m, 5m, 15m, 30m, 1h, 4h, 1d
- **Data From:** July 2017
- **Rate Limit:** 20 requests/sec

### Bybit
- **Bulk Archives:** Available at `raw.githubusercontent.com/bybit-exchange`
- **Timeframes:** 1m, 5m, 15m, 30m, 1h, 4h, 1d
- **Data From:** October 2019
- **Rate Limit:** 10 requests/sec

### OKX
- **Bulk Archives:** Not available, uses REST API
- **Timeframes:** 1m, 5m, 15m, 30m, 1h, 4h, 1d
- **Data From:** January 2019
- **Rate Limit:** 10 requests/sec

---

## Data Formats

| Format | Extension | Use Case |
|--------|-----------|----------|
| Feather | `.feather` | Fastest read/write, Freqtrade default |
| Parquet | `.parquet` | Compressed, smaller files |
| JSON | `.json` | Human-readable, debugging |
| JSONGz | `.json.gz` | Compressed JSON |

---

## Directory Structure

ftdata creates a Freqtrade-compatible directory structure:

```
data/
├── binance/
│   ├── BTC_USDT-1m.feather
│   ├── BTC_USDT-4h.feather
│   └── ETH_USDT-4h.feather
├── bybit/
│   └── ...
└── okx/
    └── ...
```

---

## Error Handling

### Common Errors and Solutions

**404 Not Found (Data Not Available)**
```json
{"error": "No bulk URLs available for this timerange"}
```
*Solution:* Check if the data exists on the exchange. Future dates are not available.

**Rate Limited**
```json
{"error": "Rate limit exceeded"}
```
*Solution:* Wait 60 seconds and retry. ftdata implements automatic rate limiting.

**Checksum Mismatch**
```json
{"error": "Checksum verification failed"}
```
*Solution:* Run `ftdata_repair` to re-download corrupted chunks.

**Invalid Symbol**
```json
{"error": "Invalid symbol format"}
```
*Solution:* Use format `BTC/USDT` or `BTCUSDT` (automatic conversion).

---

## Best Practices for Agents

1. **Always Plan First**
   - Use `ftdata_plan` before downloading to estimate time and storage
   - Prevents running out of disk space mid-download

2. **Verify After Download**
   - Always call `ftdata_inspect` after download to confirm data
   - Check row count matches expected

3. **Check for Gaps**
   - Run `ftdata_gaps` before any backtest
   - Gaps can cause significant backtesting errors

4. **Use Feather for Speed**
   - Feather format is 10x faster than Parquet for reading
   - Use Parquet only when disk space is critical

5. **Download inChunks**
   - For large datasets, download month-by-month
   - Allows for easy resume if interrupted

6. **Store Checkpoints**
   - ftdata tracks download progress in SQLite
   - Enables resume after crashes

---

## Prompt Templates for Agents

### Template 1: Full Backtest Setup
```
I need historical market data for [STRATEGY_NAME] backtesting.

Requirements:
- Exchange: [EXCHANGE]
- Pairs: [PAIRS]
- Timeframes: [TIMEFRAMES]
- Date range: [START_DATE] to [END_DATE]
- Format: [FORMAT]

Please:
1. Plan the download and estimate size
2. Download all required data
3. Verify data integrity
4. Check for and repair any gaps
5. Confirm the final dataset is ready
```

### Template 2: Data Update
```
My existing dataset at [PATH] needs to be updated to the latest data.

Please:
1. Inspect current data range
2. Download only the new data since [LAST_DATE]
3. Verify the updated dataset
```

### Template 3: Format Conversion
```
I need to convert all [EXCHANGE] data from Feather to Parquet format
for use with [TRADING_BOT_NAME].

Please:
1. List all existing datasets
2. Convert each to Parquet
3. Report the total size reduction
```

---

## See Also

- [README.md](../README.md) - Project overview and installation
- [TUTORIAL.md](./TUTORIAL.md) - Detailed usage tutorial
- [TEST_REPORT.md](./TEST_REPORT.md) - Test results and coverage
- [DESIGN.md](./DESIGN.md) - Technical design document
