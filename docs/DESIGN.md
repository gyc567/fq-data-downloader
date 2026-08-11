# ftdata — High-Performance Historical Market Data Downloader

> A Rust-native, Freqtrade-compatible historical OHLCV data downloader with bulk download support, resumable transfers, and multi-layer validation.

---

## 1. Project Overview

**Project Name**: `ftdata`
**Goal**: Replace slow, unreliable script-based data downloading with a production-grade Rust tool that outputs Freqtrade-native formats.

**Core Problem Solved**:
Downloading years of 1m OHLCV data for multiple pairs and timeframes via REST API is extremely slow (tens of thousands of requests). This tool prioritizes bulk archive downloads over REST API calls, supports true resume across crashes, and validates data integrity before committing.

**Output Target**:
```
user_data/data/binance/
├── BTC_USDT-1m.feather
├── BTC_USDT-5m.feather
├── BTC_USDT-1h.feather
├── ETH_USDT-1m.feather
└── ...
```

Freqtrade reads these files directly — no code changes, no plugin installation.

---

## 2. Design Principles

1. **Bulk First** — Historical archives (zip files from Binance/Bybit/OKX) download 10-100x faster than REST API. Use bulk whenever available; REST only for incremental updates or unsupported ranges.

2. **Dual Checkpoint Resume** — HTTP `Range` resume for bulk files; logical timestamp checkpoint for REST API. These are architecturally distinct and must not be conflated.

3. **SQLite Owns State, Files Don't** — All download state lives in SQLite. Files on disk are authoritative only after SQLite commits. No JSON checkpoint files that corrupt under concurrent access.

4. **Data Correctness > Download Speed** — Validation is multi-layer: checksum (transport) + schema + timestamp alignment + OHLC constraints + gap detection + duplicate detection.

5. **Freqtrade-Compatible Output** — Default Feather, optional Parquet/JSON. Tool is standalone and has zero runtime dependency on Freqtrade.

6. **MCP-Native Data Engine** — Structured JSON output for all commands, designed to be called programmatically by `freqtrade_dev_mcp`.

---

## 3. Architecture

```
                         ┌─────────────────────┐
                         │    ftdata CLI        │
                         │  (clap + tracing)   │
                         └──────────┬──────────┘
                                    │
                         ┌──────────▼──────────┐
                         │   Download Planner   │
                         │  (timerange, pairs)  │
                         └──────────┬──────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    ↓                               ↓
             Bulk Archive                     REST API
             (HTTP + Range)              (Timestamp-based)
                    │                               │
             ┌──────┴──────┐                 ┌──────┴──────┐
             │ HTTP Range  │                 │ Logical     │
             │   Resume    │                 │ Checkpoint   │
             └─────────────┘                 └─────────────┘
                    │                               │
                    └───────────────┬───────────────┘
                                    ↓
                         ┌─────────────────────┐
                         │  Async Scheduler     │
                         │  Rate Limiter        │
                         │  Retry + Backoff     │
                         │  (tokio + reqwest)   │
                         └──────────┬──────────┘
                                    │
                         ┌──────────▼──────────┐
                         │  SQLite State DB    │
                         │  (rusqlite)         │
                         └──────────┬──────────┘
                                    ↓
                         ┌─────────────────────┐
                         │  Raw Chunk Storage  │
                         │  (.part files)      │
                         └──────────┬──────────┘
                                    ↓
                         ┌─────────────────────┐
                         │     Validator        │
                         │  Schema · OHLC      │
                         │  Timestamp · Gap    │
                         │  Duplicate          │
                         └──────────┬──────────┘
                                    ↓
                         ┌─────────────────────┐
                         │    Normalizer        │
                         │  Sort · Dedup        │
                         └──────────┬──────────┘
                                    ↓
                    ┌───────────────┴───────────────┐
                    ↓                               ↓
               Feather                         Parquet
                    │                               │
                    └───────────────┬───────────────┘
                                    ↓
                         ┌─────────────────────┐
                         │   Manifest + Index  │
                         │  (per-file metadata) │
                         └──────────┬──────────┘
                                    ↓
                         user_data/data/{exchange}
                                    ↓
                              Freqtrade
                         backtesting / hyperopt
```

---

## 4. Technical Stack

| Component | Library | Rationale |
|-----------|---------|-----------|
| Async Runtime | `tokio` | Network-bound workload; async file I/O for seek-based resume |
| HTTP Client | `reqwest` | Connection pool, HTTP/2, streaming, proxy, TLS — all needed features built-in |
| CLI | `clap` | Typed argument parsing, subcommands, help generation |
| Serialization | `serde` | JSON/TOML/YAML for config and output |
| Database | `rusqlite` (bundled) | Lightweight, embedded, no server process; bundled SQLite avoids version mismatch |
| Hashing | `blake3` | Fast, incremental, parallelizable hash for integrity verification |
| Time | `chrono` | Time range parsing (`20200101-`), timezone handling |
| Progress | `indicatif` | Multi-progress bars, shared state across tasks |
| DataFrame | `polars` | Arrow-native, streaming, multi-threaded, SIMD; handles >RAM datasets |
| Columnar | `apache-arrow` | Feather read/write via Polars |
| Error Handling | `thiserror` + `anyhow` | Domain errors + anyhow for propagate |
| Logging | `tracing` | Structured logging, JSON output for CI/MCP |

---

## 5. Data Model

### 5.1 Download State Schema (SQLite)

```sql
CREATE TABLE downloads (
    id              INTEGER PRIMARY KEY,
    exchange        TEXT NOT NULL,
    market_type     TEXT NOT NULL,  -- spot, futures
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    candle_type     TEXT NOT NULL,  -- ohlcv, mark, index, premium, funding
    start_ts        INTEGER,        -- millis
    end_ts          INTEGER,
    status          TEXT NOT NULL,  -- pending, downloading, verified, failed
    source          TEXT NOT NULL,   -- bulk, api
    bytes_total     INTEGER,
    bytes_downloaded INTEGER,
    checksum        TEXT,
    retry_count     INTEGER DEFAULT 0,
    error           TEXT,
    created_at      INTEGER,
    updated_at      INTEGER
);

CREATE TABLE chunks (
    id              INTEGER PRIMARY KEY,
    download_id     INTEGER REFERENCES downloads(id),
    chunk_start     INTEGER NOT NULL,  -- millis
    chunk_end       INTEGER NOT NULL,
    status          TEXT NOT NULL,    -- pending, downloading, verified, failed
    size            INTEGER,
    checksum        TEXT,
    etag            TEXT,
    last_modified   TEXT,
    retry_count     INTEGER DEFAULT 0,
    error           TEXT,
    UNIQUE(download_id, chunk_start)
);

CREATE TABLE files (
    path            TEXT PRIMARY KEY,
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    market_type     TEXT NOT NULL,
    candle_type     TEXT NOT NULL,
    from_ts         INTEGER,
    to_ts           INTEGER,
    rows            INTEGER,
    size            INTEGER,
    checksum        TEXT,
    format          TEXT NOT NULL,   -- feather, parquet, json, jsongz
    verified        INTEGER DEFAULT 0,
    created_at      INTEGER,
    updated_at      INTEGER
);

CREATE TABLE gaps (
    id              INTEGER PRIMARY KEY,
    exchange        TEXT NOT NULL,
    symbol          TEXT NOT NULL,
    timeframe       TEXT NOT NULL,
    market_type     TEXT NOT NULL,
    candle_type     TEXT NOT NULL,
    from_ts         INTEGER NOT NULL,
    to_ts           INTEGER NOT NULL,
    reason          TEXT,
    status          TEXT DEFAULT 'open',  -- open, repaired, ignored
    created_at      INTEGER
);
```

### 5.2 Chunk Granularity

| Source | Chunk Unit | Rationale |
|--------|------------|-----------|
| Bulk archive | Per archive file | Archives are already natural chunks; no splitting needed |
| REST API (Binance) | 1000 candles | API limit per request; time-based splitting also acceptable |
| REST API (Bybit) | 200 candles | Bybit's limit |
| Local processing | Fixed row count (e.g. 100k rows) | For memory-efficient streaming |

### 5.3 Candle Types (V1.5)

```
MarketData
├── SpotOHLCV       (V1)
├── FuturesOHLCV    (V1.5)
├── MarkPrice       (V1.5)
├── IndexPrice      (V1.5)
├── PremiumIndex    (V1.5)
└── FundingRate     (V1.5)
```

V1 ships with Spot OHLCV only. Futures types are architected but gated.

---

## 6. Download Strategies

### 6.1 Bulk Download (Priority 1)

```
SourceResolver
    │
    ├── Bulk archive available? ──YES──> Bulk HTTP Downloader
    │                                        │
    └── NO ──────────────────────────────────> REST API Downloader
                                                   │
                                              Incremental only
```

**Supported Bulk Sources**:

| Exchange | Archive Type | URL Pattern |
|----------|-------------|-------------|
| Binance | monthly zip | `https://data.binance.vision/data/spot/monthly/klines/{symbol}/1m/{symbol}-1m-{YYYY}-{MM}.zip` |
| Bybit | daily zip | `https://raw.githubusercontent.com/bybit-exchange/bybit-archive/main/spot/1m/{symbol}/{symbol}-1m-{YYYY}-{MM}-{DD}.zip` |
| OKX | daily csv | `https://www.okx.com/priapi/v5/market/history-candle?bar=1m&instId={symbol}&after={ts}&before={ts}` |

**HTTP Range Resume**:
```
1. Initial download: GET with no Range header
   Response: 200 OK, Content-Length: 5.1 GB

2. Crash at 2.3 GB
3. Resume: GET with Range: bytes=2415919104-
   Response: 206 Partial Content

4. Server doesn't support Range? → delete .part, re-download from scratch
```

**ETag / Last-Modified for Update Detection**:
```
1. Download: save ETag, Last-Modified, Content-Length to SQLite
2. Next update: HEAD + If-None-Match: "etag"
3. 304 → skip entirely
4. 200 → re-download (archive may have been corrected)
```

### 6.2 REST API Fallback (Priority 2)

Used when:
- Bulk archive doesn't exist for the time range
- Only the most recent data is needed (bulk only covers historical, not live)
- Exchange doesn't have bulk archives

**Logical Timestamp Checkpoint**:
```
SQLite: last_completed_timestamp = 2024-06-01 00:00:00

Crash recovery:
  startTime = 2024-06-01 00:00:00
  limit = 1000
  Continue from where we left off.
```

### 6.3 Concurrency Model

```
Global Scheduler
├── BTC/USDT 1m  (Chunk #001-080)
├── BTC/USDT 5m  (Chunk #001-016)
├── ETH/USDT 1m  (Chunk #001-080)
├── SOL/USDT 1m  (Chunk #001-080)
└── ...

Concurrency Limits:
  global: 32 workers total
  per-exchange: 8 workers
  per-host: 4 workers
  rate-limit: adaptive per-exchange
```

---

## 7. Rate Limiting

### 7.1 Two-Tier Limiter

```rust
trait RateLimiter: Send + Sync {
    async fn acquire(&self, endpoint: &str) -> Result<(), RateLimitError>;
}

// Per-exchange limiter instances
struct BinanceLimiter { /* 1200 requests/min */ }
struct BybitLimiter   { /* 10 requests/sec */ }
struct OkxLimiter     { /* 20 requests/2sec */ }
```

### 7.2 Retry Strategy

**Retry On**:
- `timeout`
- `connection reset`
- `502`, `503`, `504`
- `429` (with Retry-After)

**Do Not Retry**:
- `400` — bad request parameters
- `401` / `403` — auth issues (fail immediately)
- `404` — resource not found
- `418` — exchange ban (enter cooldown)

**Backoff**:
```
1s → 2s → 4s → 8s → 16s → 32s (max)
     + random jitter (±10%)
     + Retry-After header if present
Max retries: 5
Max backoff: 5 minutes
```

---

## 8. Validation Pipeline

Every downloaded chunk passes through:

```
Download
    │
    ▼
Checksum (BLAKE3)          ← File transport integrity
    │
    ▼
Schema Validation           ← All required fields present, correct types
    │
    ▼
Timestamp Validation         ← Ascending, no duplicates, aligned to timeframe
    │
    ▼
OHLC Constraints            ← high >= max(open,close), low <= min(open,close), volume >= 0
    │
    ▼
Gap Detection               ← Missing candles identified and recorded
    │
    ▼
Duplicate Detection          ← Same timestamp → deduplicate
    │
    ▼
Sort + Commit
```

**OHLCV Validation Rule**:
```rust
fn validate_ohlcv(row: &OHLCV) -> bool {
    row.high >= row.open.max(row.close) &&
    row.high >= row.close.max(row.open) &&
    row.low  <= row.open.min(row.close) &&
    row.low  <= row.close.min(row.open) &&
    row.volume >= 0 &&
    row.timestamp % timeframe_ms == 0
}
```

**Gap Recording**:
```rust
struct Gap {
    exchange: String,
    symbol: String,
    timeframe: String,
    from_ts: i64,
    to_ts: i64,
    reason: String,
    status: GapStatus,
}
```

---

## 9. Storage and Commit Protocol

### 9.1 Directory Layout

```
data/
├── raw/
│   ├── binance/
│   ├── bybit/
│   └── okx/
│
├── normalized/
│   ├── binance/
│   └── ...
│
└── freqtrade/           ← Final output
    ├── binance/
    ├── bybit/
    └── okx/
        BTC_USDT-1m.feather
        ETH_USDT-1m.feather
        ...
```

### 9.2 Commit Protocol (Critical Order)

```
1. Download raw chunk
2. Calculate BLAKE3 checksum
3. Parse and validate
4. Append to timeframe file (Feather append mode)
5. fsync() the file
6. Atomic rename .part → final
7. Update SQLite: status = 'verified', rows = N
```

**Never update SQLite before file is fsynced and renamed.**

### 9.3 Temp File Naming

```
BTC_USDT-1m.feather              ← Final (readable by Freqtrade)
BTC_USDT-1m.feather.downloading  ← Lock file (prevents concurrent write)
BTC_USDT-1m.feather.part         ← In-progress chunk download
```

Recovery logic:
- `.lock` present → abnormal exit → inspect state
- `.part` present without `.lock` → interrupted download → resume
- Only final file present → normal completion

---

## 10. CLI Interface

### 10.1 Commands

| Command | Description |
|---------|-------------|
| `ftdata download` | Download historical data |
| `ftdata update` | Update to latest (incremental) |
| `ftdata prepend` | Download data before existing range |
| `ftdata resume` | Resume interrupted downloads |
| `ftdata verify` | Verify data integrity |
| `ftdata gaps` | List detected gaps |
| `ftdata repair` | Re-download missing ranges |
| `ftdata inspect` | Show file metadata and stats |
| `ftdata list` | List available datasets |
| `ftdata convert` | Convert between formats |
| `ftdata clean` | Remove partial/broken files |
| `ftdata plan` | Dry-run: show download plan |

### 10.2 Key Examples

```bash
# Download BTC, ETH, SOL from 2020 to now
ftdata download \
  --exchange binance \
  --pairs BTC/USDT ETH/USDT SOL/USDT \
  --timeframes 1m 5m 1h \
  --timerange 20200101- \
  --output user_data/data

# Update to latest (fast, only missing data)
ftdata update \
  --exchange binance \
  --pairs BTC/USDT \
  --timeframes 1m 5m

# Check what would be downloaded (no network calls)
ftdata plan \
  --exchange binance \
  --pairs BTC/USDT ETH/USDT \
  --timeframes 1m 5m \
  --timerange 20200101- \
  --output user_data/data

# Resume after crash
ftdata resume

# Find gaps in existing data
ftdata gaps \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m

# Repair gaps (re-download missing chunks)
ftdata repair \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m

# Verify downloaded data
ftdata verify \
  --exchange binance \
  --pair BTC/USDT \
  --timeframe 1m
```

### 10.3 Plan Output (MCP Integration)

```bash
$ ftdata plan --exchange binance --pairs BTC/USDT --timeframes 1m 5m --timerange 20200101-
Download Plan
──────────────────────────────────────────────────────

BTC/USDT 1m
  local:   2023-01-01 → 2026-08-10
  missing: 2020-01-01 → 2023-01-01
  source:  bulk
  chunks:  37
  size:    18.2 GB

BTC/USDT 5m
  local:   none
  missing: 2020-01-01 → 2026-08-10
  source:  bulk
  chunks:  9
  size:    4.2 GB

Total:
  files:   46
  download: 103 GB
```

Machine-readable JSON:
```bash
ftdata plan --exchange binance --pairs BTC/USDT --format json
```

---

## 11. Module Structure

```
ftdata/
├── crates/
│   ├── ftdata-cli/          # Main binary, clap commands
│   │
│   ├── ftdata-core/         # Shared domain logic
│   │   ├── domain/          # OHLCV, CandleType, TimeRange, etc.
│   │   ├── planner/         # Download planning, chunk decomposition
│   │   ├── scheduler/       # Task scheduling, concurrency
│   │   ├── checkpoint/      # SQLite state management
│   │   ├── validator/       # Multi-layer validation
│   │   └── error/           # Domain errors (DownloadError enum)
│   │
│   ├── ftdata-http/         # HTTP client layer
│   │   ├── client/          # reqwest wrapper
│   │   ├── range/           # HTTP Range handling
│   │   ├── retry/           # Exponential backoff + jitter
│   │   └── rate_limit/      # Token bucket / adaptive limiter
│   │
│   ├── ftdata-sources/      # Exchange adapters
│   │   ├── binance/
│   │   ├── bybit/
│   │   ├── okx/
│   │   └── generic/         # Fallback for unknown exchanges
│   │
│   ├── ftdata-storage/      # I/O layer
│   │   ├── raw/             # Temp .part files
│   │   ├── feather/         # Feather read/write via Polars
│   │   ├── parquet/         # Parquet output
│   │   └── sqlite/          # State database
│   │
│   └── ftdata-analysis/     # Post-download analysis
│       ├── gaps/            # Gap detection + reporting
│       ├── duplicates/      # Deduplication
│       └── statistics/      # Dataset stats
│
├── tests/
├── benchmarks/
└── docs/
```

### 11.1 Adapter Trait

```rust
#[async_trait]
pub trait MarketDataSource: Send + Sync {
    fn exchange(&self) -> &str;
    fn supported_timeframes(&self) -> Vec<Timeframe>;
    fn supported_market_types(&self) -> Vec<MarketType>;

    async fn get_available_range(
        &self,
        symbol: &str,
        timeframe: &Timeframe,
        market_type: MarketType,
    ) -> Result<Option<TimeRange>, DownloadError>;

    async fn get_bulk_urls(
        &self,
        symbol: &str,
        timeframe: &Timeframe,
        time_range: &TimeRange,
    ) -> Result<Vec<DownloadUrl>, DownloadError>;

    async fn fetch_ohlcv(
        &self,
        symbol: &str,
        timeframe: &Timeframe,
        start_time: i64,
        end_time: i64,
        limit: u32,
    ) -> Result<Vec<OHLCV>, DownloadError>;

    fn rate_limiter(&self) -> Arc<dyn RateLimiter>;
}
```

---

## 12. Error Model

```rust
#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("rate limited, retry after {0}s")]
    RateLimited(u64),

    #[error("exchange ban (418), cooling down")]
    ExchangeBan,

    #[error("invalid response from exchange: {0}")]
    InvalidResponse(String),

    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("data gap detected: {from} → {to}")]
    DataGap { from: i64, to: i64 },

    #[error("invalid OHLCV at row {row}: {reason}")]
    InvalidOHLCV { row: u64, reason: String },

    #[error("unsupported exchange: {0}")]
    UnsupportedExchange(String),

    #[error("unsupported timeframe: {0}")]
    UnsupportedTimeframe(String),

    #[error("storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("resume state corrupted: {0}")]
    ResumeStateCorrupted(String),
}
```

All errors are structured and can be returned to MCP callers as typed JSON.

---

## 13. Crash Recovery

On any crash, `ftdata resume` determines action per file:

| File State | Action |
|------------|--------|
| Final file exists + SQLite `verified` | Skip |
| `.part` exists + SQLite `downloading` | Resume HTTP Range at byte offset |
| `.part` exists + no SQLite record | Delete `.part`, re-download |
| Final file exists + SQLite `pending` | Inspect: validate or redownload |
| Final file missing + SQLite `verified` | Redownload (data loss detected) |

---

## 14. Versioning and Manifests

### 14.1 Per-File Manifest

```json
// BTC_USDT-1m.manifest.json
{
  "exchange": "binance",
  "symbol": "BTC/USDT",
  "timeframe": "1m",
  "market_type": "spot",
  "candle_type": "ohlcv",
  "format": "feather",
  "from": "2020-01-01T00:00:00Z",
  "to": "2026-08-11T00:00:00Z",
  "rows": 3478000,
  "duplicates_removed": 12,
  "gaps_detected": 3,
  "checksum": "bla3:abc123...",
  "source": "binance_bulk",
  "downloaded_at": "2026-08-11T12:00:00Z",
  "chunks": [
    {"range": "2020-01-2020-06", "status": "verified"},
    {"range": "2020-07-2020-12", "status": "verified"}
  ]
}
```

### 14.2 Dataset Versioning (V2)

```
binance-btc-usdt-1m/
├── v1/  (2020-01-01 → 2025-12-31)
├── v2/  (2020-01-01 → 2026-06-30)
└── v3/  (2020-01-01 → 2026-08-11)
```

---

## 15. Performance Targets

Measured on: Binance BTC/USDT 1m, 2020-01-01 → 2026-08-01 (~6.5 years)

| Metric | Freqtrade | ftdata (target) |
|--------|-----------|-----------------|
| Download time | ~8-24h (REST) | ~20-60min (bulk) |
| Network throughput | ~5-20 Mbps | ~500+ Mbps |
| Resume after crash | Poor (restarts) | Precise (per-chunk) |
| Data validation | Basic | Multi-layer |
| Format | JSON/JSON.gz | Feather (default) |

Exact numbers depend on exchange bulk availability and network conditions. Benchmarks will be measured and published with releases.

---

## 16. V1 Scope

### Included
- ✅ Binance, Bybit, OKX exchanges
- ✅ Spot OHLCV
- ✅ Bulk download + REST fallback
- ✅ HTTP Range resume
- ✅ Logical timestamp checkpoint
- ✅ SQLite state management
- ✅ Gap detection + duplicate detection
- ✅ OHLC + timestamp validation
- ✅ Feather output (default)
- ✅ Parquet output (optional)
- ✅ `update` command (incremental)
- ✅ `prepend` command (historical extension)
- ✅ `verify` command
- ✅ `gaps` / `repair` commands
- ✅ `plan` dry-run with machine-readable output
- ✅ Progress UI (indicatif)
- ✅ Structured logging (tracing)
- ✅ MCP-friendly JSON output

### Excluded from V1
- ❌ GUI / Web interface
- ❌ Futures mark/index/premium/funding candles
- ❌ Trade data aggregation
- ❌ Dataset versioning
- ❌ Object storage (S3/GCS)
- ❌ Distributed scheduler
- ❌ DuckDB integration
- ❌ ML features
- ❌ Automatic resampling

### V1.5 Candidates
- Futures OHLCV
- Mark / Index / Premium / Funding data
- Trade data → OHLCV aggregation

### V2 Candidates
- Dataset versioning
- Object storage
- Distributed download
- DuckDB integration
- MCP native server

---

## 17. Release Artifacts

Each GitHub release includes:

```
ftdata-vX.Y.Z/
├── ftdata-linux-amd64
├── ftdata-linux-arm64
├── ftdata-macos-arm64
├── ftdata-macos-x86_64
└── ftdata-windows-amd64.exe
```

Install:
```bash
brew install ftdata       # macOS
cargo install ftdata      # via crates.io
docker pull ghcr.io/gyc567/ftdata
```

---

## 18. Integration with freqtrade_dev_mcp

```
Agent (Claude Code / Codex)
        │
        ▼
freqtrade_dev_mcp
  │
  ├── ftdata plan      → returns structured JSON (datasets, chunks, size)
  ├── ftdata download  → returns completion JSON (rows, gaps, checksum)
  ├── ftdata verify    → returns validation result
  ├── ftdata gaps      → returns gap list
  └── ftdata inspect   → returns file metadata
        │
        ▼
  Freqtrade backtesting
        │
        ▼
  Results → Experiment Registry
```

All MCP interactions use structured JSON — no parsing stdout.

---

## 19. Design Principles Summary

| # | Principle |
|---|-----------|
| 1 | Bulk over REST — 10-100x faster |
| 2 | HTTP Range + Logical Timestamp dual resume |
| 3 | SQLite owns state, files don't |
| 4 | Data correctness > download speed |
| 5 | Freqtrade-compatible output, zero coupling |
| 6 | MCP-native with structured JSON output |
