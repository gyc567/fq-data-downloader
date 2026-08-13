# ftdata — High-Performance Historical Market Data Downloader

A Rust workspace for downloading and managing historical OHLCV market data from major cryptocurrency exchanges (Binance, Bybit, OKX), with optional x402-paid HTTP API access. Designed for algorithmic trading with Freqtrade compatibility.

[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Features

- **Bulk Download First** — Prefers bulk archive downloads over REST API for maximum speed
- **Resumable Downloads** — Dual checkpoint system (HTTP Range + logical timestamp)
- **Multi-Exchange Support** — Binance, Bybit, OKX with exchange-specific optimizations
- **Freqtrade Compatible** — Native Feather/Parquet output in Freqtrade's directory structure
- **MCP Native** — Structured JSON output for AI agent integration
- **Rate Limiting** — Token bucket per exchange to respect API limits
- **Data Validation** — Multi-layer validation (schema, timestamp, OHLC, gap, duplicate)
- **Gap Detection & Repair** — Automatic detection and repair of missing data
- **x402 Paid API** (new) — Optional HTTP layer with pay-per-request monetization

## Workspace Layout

```
ftdata/
├── ftdata-cli/             # CLI: download / update / plan / verify / gaps / repair
├── ftdata-core/            # domain types, checkpoint, planner, validator
├── ftdata-http/            # HTTP client, rate limiting, retry, range requests
├── ftdata-sources/         # exchange adapters (Binance, Bybit, OKX)
├── ftdata-storage/         # Feather/Parquet I/O via Polars
├── ftdata-analysis/        # gap detection, statistics, duplicate removal
├── ftdata-paid-pricing/    # x402 dynamic pricing formula (pure Rust)
├── ftdata-paid-facilitator/ # x402 PaymentVerifier trait + MockFacilitator
├── ftdata-paid-api/        # Axum HTTP API + ftdata-paid-server binary
│   ├── docker/Dockerfile
│   ├── examples/agent-client.ts
│   └── README.md
└── docs/
    ├── DESIGN.md
    ├── PAID_API_DESIGN.md
    ├── TUTORIAL.md
    └── AGENT_TUTORIAL.md
```

The first six crates (`ftdata-cli/core/http/sources/storage/analysis`) form the **open-source CLI** — MIT-licensed, no payment logic.

The last three (`ftdata-paid-*`) form the **paid HTTP API layer** — commercial crate family that wraps the CLI capabilities behind an x402 pay-per-request endpoint.

## Installation

### Prerequisites

- Rust 1.75+
- Linux / macOS / Windows

### Build from source

```bash
git clone https://github.com/gyc567/fq-data-downloader.git
cd fq-data-downloader
cargo build --release
# CLI binary: ./target/release/ftdata
# Paid server: ./target/release/ftdata-paid-server
```

## Quick start — CLI (free, MIT)

```bash
# Download BTC/USDT 1m data from Binance
ftdata download \
  --exchange binance --pairs BTC/USDT --timeframes 1m \
  --timerange 20230101-20230601

# Plan a download (dry-run, JSON output)
ftdata plan \
  --exchange binance --pairs BTC/USDT ETH/USDT --timeframes 1m 5m \
  --timerange 20230101-20231231 -j

# Verify downloaded data
ftdata verify --path user_data/data/binance/BTC_USDT-1m.feather

# Detect and repair gaps
ftdata gaps --exchange binance --pair BTC/USDT --timeframe 1m \
  --path user_data/data/binance/BTC_USDT-1m.feather
ftdata repair --exchange binance --pair BTC/USDT --timeframe 1m \
  --timerange 20230101-20230601
```

See `docs/TUTORIAL.md` for the full CLI reference and `docs/AGENT_TUTORIAL.md` for AI Agent integration.

## Quick start — Paid API server

```bash
cargo run --bin ftdata-paid-server
# server listens on 0.0.0.0:8080 by default
```

Then in another terminal:
```bash
# 1. Price preview (no payment needed)
curl -X POST http://localhost:8080/v1/quote \
  -H 'content-type: application/json' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'
# → {"quote_id":"...","price_usdc":"0.010446","payment_required":{...}}

# 2. Download (will 402 without payment)
curl -i -X POST http://localhost:8080/v1/download \
  -H 'content-type: application/json' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'

# 3. Submit with X-PAYMENT (mock facilitator accepts anything)
curl -i -X POST http://localhost:8080/v1/download \
  -H 'content-type: application/json' \
  -H 'x-payment: {"scheme":"exact","network":"base","asset":"usdc","payer":"0xA","amount":"0.010446","quote_id":"<qt>","signature":"0xMOCK","nonce":"n1","valid_until":99999999999}' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'
```

See [`ftdata-paid-api/README.md`](ftdata-paid-api/README.md) for the full route table and Phase 1 status.

## Docker

```bash
docker build -t ftdata-paid-server -f ftdata-paid-api/docker/Dockerfile .
docker run --rm -p 8080:8080 ftdata-paid-server
```

## TS Agent client

```bash
# Requires a running ftdata-paid-server and node + tsx:
npx tsx ftdata-paid-api/examples/agent-client.ts
```

See [`ftdata-paid-api/examples/agent-client.ts`](ftdata-paid-api/examples/agent-client.ts) for the reference implementation. Replace the mock `signPaymentProof` with viem/ethers for production.

## Tests

```bash
cargo test --workspace
# 86 tests across all crates
```

## Design docs

- [`docs/DESIGN.md`](docs/DESIGN.md) — CLI design (modules, data flow, schema)
- [`docs/PAID_API_DESIGN.md`](docs/PAID_API_DESIGN.md) — x402 paid API design (pricing, routes, settlement, ops)
- [`docs/TUTORIAL.md`](docs/TUTORIAL.md) — CLI usage tutorial
- [`docs/AGENT_TUTORIAL.md`](docs/AGENT_TUTORIAL.md) — AI Agent integration tutorial

## License

The CLI crates (`ftdata-cli`, `ftdata-core`, `ftdata-http`, `ftdata-sources`, `ftdata-storage`, `ftdata-analysis`) are MIT-licensed.

The `ftdata-paid-*` crates are part of a planned commercial offering; see `docs/PAID_API_DESIGN.md` §1 for the licensing model (CLI stays MIT, paid layer is commercial).
