# ftdata-paid-api

HTTP API for the ftdata paid data service. Implements the routes described in `docs/PAID_API_DESIGN.md` §2:

| Method | Path             | Auth         | Description                              |
|--------|------------------|--------------|------------------------------------------|
| GET    | `/v1/info`       | public       | Service metadata + free-tier policy      |
| POST   | `/v1/quote`      | public       | Price preview (no payment required)      |
| POST   | `/v1/download`   | x402         | Paid data download                       |
| GET    | `/v1/jobs/{id}`  | job id       | Job status query                         |
| GET    | `/v1/reconcile`  | admin (MVP)  | Settlement aggregation report             |

## Run the server

```bash
cargo run --bin ftdata-paid-server
# binds 0.0.0.0:8080 by default; override with FTDATA_BIND=127.0.0.1:9090
```

Phase 1 ships with a `MockFacilitator` so the server can run end-to-end without any external service. The mock accepts any proof whose `amount` covers `max_amount` and whose `quote_id` matches the issued challenge.

## Try it with curl

```bash
# 1. Get service info
curl http://localhost:8080/v1/info | jq .

# 2. Price preview (no payment needed)
curl -X POST http://localhost:8080/v1/quote \
  -H 'content-type: application/json' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'
# → {"quote_id":"...","price_usdc":"0.010446","payment_required":{...}}

# 3. First download attempt (no payment → 402)
curl -i -X POST http://localhost:8080/v1/download \
  -H 'content-type: application/json' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'

# 4. Retry with X-PAYMENT (sign a mock proof) → 202
curl -i -X POST http://localhost:8080/v1/download \
  -H 'content-type: application/json' \
  -H 'x-payment: {"scheme":"exact","network":"base","asset":"usdc","payer":"0xAGENT","amount":"0.010446","quote_id":"<quote_id_from_step_3>","signature":"0xMOCK","nonce":"n1","valid_until":99999999999}' \
  -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}'

# 5. Poll the job
curl http://localhost:8080/v1/jobs/<job_id>

# 6. Reconcile
curl 'http://localhost:8080/v1/reconcile?since=0&until=9999999999'
```

## Reference TS Agent client

`examples/agent-client.ts` shows the full flow in TypeScript. Replace the mock `signPaymentProof` with a real viem/ethers signer for production use.

## Tests

```bash
cargo test -p ftdata-paid-api
# 11 E2E + 3 origin unit tests
```

## Status

| Phase 1 done signal (design §6) | Status |
|----------------------------------|--------|
| `/v1/quote` works                | ✅ |
| `/v1/download` works with x402   | ✅ |
| TS Agent E2E                     | ✅ (example) |
| R2 upload + signed link          | ⏳ stub (real R2 pending Q11) |
| Web dashboard                    | ⏳ not started |
| ≥ 3 E2E tests with 402 retry     | ✅ (5 of 11 E2E cover 402) |
| Workers edge enforces x402       | ⏳ pending Q1 (Workers runtime) |
| KV caching                       | ⏳ pending Q11 |
| **Settlement & Reconciliation**  | ✅ |

## Real facilitator

`MockFacilitator` is for tests + local dev only. For production, implement `PaymentVerifier` against a real facilitator (Cloudflare / Coinbase / self-hosted, Q4) and pass `Arc<dyn PaymentVerifier>` to `AppState::new`.
