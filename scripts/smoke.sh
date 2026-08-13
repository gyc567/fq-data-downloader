#!/usr/bin/env bash
# Human-runnable smoke test for ftdata-paid-server.
#
# Boots the server on an ephemeral port, hits all 5 endpoints, asserts
# the expected status codes + key fields, then cleans up. Exits non-zero
# on any failure so it can be wired into CI.
#
# Usage:
#   bash scripts/smoke.sh                       # builds release first
#   FTDATA_BIN=path/to/ftdata-paid-server bash scripts/smoke.sh
#
# Requires: bash, curl, jq (for response parsing).

set -euo pipefail

PORT="${FTDATA_PORT:-19999}"
BASE="http://127.0.0.1:${PORT}"
BIN="${FTDATA_BIN:-./target/release/ftdata-paid-server}"

# Pick the binary: prefer release, fall back to debug.
if [[ ! -x "$BIN" ]]; then
    BIN="./target/debug/ftdata-paid-server"
fi
if [[ ! -x "$BIN" ]]; then
    echo "[build] no ftdata-paid-server binary found; building release..."
    cargo build --release --bin ftdata-paid-server
    BIN="./target/release/ftdata-paid-server"
fi

# Helpers
red()   { printf "\033[31m%s\033[0m\n" "$*"; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
blue()  { printf "\033[34m%s\033[0m\n" "$*"; }

assert_status() {
    local got="$1" want="$2" label="$3"
    if [[ "$got" == "$want" ]]; then
        green "  [ok] $label: HTTP $got"
    else
        red   "  [FAIL] $label: expected HTTP $want, got $got"
        cleanup
        exit 1
    fi
}

assert_jq() {
    local expr="$1" want="$2" label="$3" file="$4"
    local got
    got="$(jq -r "$expr" "$file")"
    if [[ "$got" == "$want" ]]; then
        green "  [ok] $label: $expr = $got"
    else
        red   "  [FAIL] $label: $expr expected $want, got $got"
        cleanup
        exit 1
    fi
}

# Start server in background
blue "[start] launching $BIN on port $PORT"
FTDATA_BIND="127.0.0.1:${PORT}" "$BIN" > /tmp/ftdata-paid-smoke.log 2>&1 &
SERVER_PID=$!

cleanup() {
    if kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

# Wait for server to be reachable
for i in {1..30}; do
    if curl -sf "$BASE/v1/info" > /dev/null 2>&1; then
        green "[start] server is up after ${i} attempts"
        break
    fi
    sleep 0.1
done
if ! curl -sf "$BASE/v1/info" > /dev/null 2>&1; then
    red "[start] server did not become reachable in 3s"
    cat /tmp/ftdata-paid-smoke.log
    exit 1
fi

# 1. /v1/info
blue "[test] /v1/info"
status="$(curl -s -o /tmp/info.json -w '%{http_code}' "$BASE/v1/info")"
assert_status "$status" "200" "/v1/info"
assert_jq '.service' "ftdata-paid" "/v1/info service" /tmp/info.json
assert_jq '.endpoints | length >= 5' "true" "/v1/info has >=5 endpoints" /tmp/info.json

# 2. /v1/quote — base case (BTC/USDT 1 month 1m spot)
blue "[test] /v1/quote (BTC/USDT 1m 1mo spot)"
status="$(curl -s -o /tmp/quote.json -w '%{http_code}' -X POST "$BASE/v1/quote" \
    -H 'content-type: application/json' \
    -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201","market":"spot"}')"
assert_status "$status" "200" "/v1/quote"
assert_jq '.price_usdc | startswith("0.01")' "true" "/v1/quote price starts with 0.01" /tmp/quote.json
QUOTE_ID="$(jq -r '.quote_id' /tmp/quote.json)"
MAX_AMOUNT="$(jq -r '.payment_required.max_amount' /tmp/quote.json)"
blue "  quote_id=$QUOTE_ID max_amount=$MAX_AMOUNT"

# 3. /v1/download without payment → 402
blue "[test] /v1/download without payment (expect 402)"
status="$(curl -s -o /tmp/dl_402.json -w '%{http_code}' -X POST "$BASE/v1/download" \
    -H 'content-type: application/json' \
    -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}')"
assert_status "$status" "402" "/v1/download no-payment"
assert_jq '.error' "payment_required" "/v1/download 402 error code" /tmp/dl_402.json
CHALLENGE_QUOTE_ID="$(jq -r '.payment_required.quote_id' /tmp/dl_402.json)"
CHALLENGE_AMOUNT="$(jq -r '.payment_required.max_amount' /tmp/dl_402.json)"

# 4. /v1/download with valid payment → 202
blue "[test] /v1/download with X-PAYMENT (expect 202)"
PROOF=$(cat <<JSON
{"scheme":"exact","network":"base","asset":"usdc","payer":"0xSMOKE","amount":"$CHALLENGE_AMOUNT","quote_id":"$CHALLENGE_QUOTE_ID","signature":"0xSMOKE_SIG","nonce":"n1","valid_until":99999999999}
JSON
)
status="$(curl -s -o /tmp/dl_202.json -w '%{http_code}' -X POST "$BASE/v1/download" \
    -H 'content-type: application/json' \
    -H "x-payment: $PROOF" \
    -d '{"exchange":"binance","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-20230201"}')"
assert_status "$status" "202" "/v1/download with-payment"
assert_jq '.payment_settled' "true" "/v1/download payment_settled" /tmp/dl_202.json
JOB_ID="$(jq -r '.job_id' /tmp/dl_202.json)"
blue "  job_id=$JOB_ID"

# 5. /v1/jobs/{id} → 200 (initially may be queued/running)
blue "[test] /v1/jobs/$JOB_ID (poll until completed)"
for i in {1..30}; do
    status="$(curl -s -o /tmp/job.json -w '%{http_code}' "$BASE/v1/jobs/$JOB_ID")"
    if [[ "$(jq -r '.status' /tmp/job.json)" == "completed" ]]; then
        green "  [ok] job completed after ${i} polls"
        break
    fi
    sleep 0.1
done
assert_jq '.status' "completed" "/v1/jobs final status" /tmp/job.json

# 6. /v1/reconcile → 200 with at least 1 completed job
blue "[test] /v1/reconcile"
status="$(curl -s -o /tmp/recon.json -w '%{http_code}' "$BASE/v1/reconcile?since=0&until=9999999999")"
assert_status "$status" "200" "/v1/reconcile"
assert_jq '.jobs_completed >= 1' "true" "/v1/reconcile jobs_completed >= 1" /tmp/recon.json

# 7. Bad request → 400
blue "[test] /v1/quote bad exchange (expect 400)"
status="$(curl -s -o /tmp/bad.json -w '%{http_code}' -X POST "$BASE/v1/quote" \
    -H 'content-type: application/json' \
    -d '{"exchange":"kraken","pairs":["BTC/USDT"],"timeframes":["1m"],"timerange":"20230101-"}')"
assert_status "$status" "400" "/v1/quote bad-exchange"

green ""
green "==================================="
green "  All smoke tests passed."
green "==================================="
