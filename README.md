# ftdata — High-Performance Historical Market Data Downloader

A Rust workspace for downloading and managing historical OHLCV market data from major cryptocurrency exchanges (Binance, Bybit, OKX), with optional x402-paid HTTP API access.

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
└── docs/
    └── PAID_API_DESIGN.md  # x402 monetization design (L1)
```

The first six crates (`ftdata-cli/core/http/sources/storage/analysis`) form the **open-source CLI** — MIT-licensed, no payment logic.

The last three (`ftdata-paid-*`) form the **paid HTTP API layer** — separate commercial crate family that wraps the CLI capabilities behind an x402 pay-per-request endpoint.

## Quick start

### CLI (free, MIT)

```bash
cargo build --release
./target/release/ftdata download \
  --exchange binance --pairs BTC/USDT --timeframes 1m \
  --timerange 20230101-20230601
```

See `docs/TUTORIAL.md` and `docs/AGENT_TUTORIAL.md` for more.

### Paid API server

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

### Docker

```bash
docker build -t ftdata-paid-server -f ftdata-paid-api/docker/Dockerfile .
docker run --rm -p 8080:8080 ftdata-paid-server
```

### TS Agent client

```bash
# Requires a running ftdata-paid-server and node + tsx:
npx tsx ftdata-paid-api/examples/agent-client.ts
```

See [`ftdata-paid-api/examples/agent-client.ts`](ftdata-paid-api/examples/agent-client.ts) for the reference implementation.

## Tests

```bash
cargo test --workspace
# 86 tests across all crates
```

## Design docs

- [`docs/DESIGN.md`](docs/DESIGN.md) — CLI design
- [`docs/PAID_API_DESIGN.md`](docs/PAID_API_DESIGN.md) — x402 paid API design
- [`docs/TUTORIAL.md`](docs/TUTORIAL.md) — CLI usage tutorial
- [`docs/AGENT_TUTORIAL.md`](docs/AGENT_TUTORIAL.md) — AI Agent integration tutorial

## License

The CLI crates (`ftdata-cli`, `ftdata-core`, `ftdata-http`, `ftdata-sources`, `ftdata-storage`, `ftdata-analysis`) are MIT-licensed. The `ftdata-paid-*` crates are part of a planned commercial offering; see `docs/PAID_API_DESIGN.md` for the licensing model.
