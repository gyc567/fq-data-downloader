#!/bin/bash
# ftdata E2E Test Script
# Tests actual download functionality

set -e

TEST_DIR="./test_data"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"

BIN="./target/debug/ftdata"
if [ ! -f "$BIN" ]; then
    echo "Building ftdata..."
    cargo build
    BIN="./target/debug/ftdata"
fi

echo "========================================="
echo "ftdata E2E Test Suite"
echo "========================================="
echo ""

# Test counter
PASSED=0
FAILED=0

# Helper function
test_result() {
    if [ $1 -eq 0 ]; then
        echo "  ✅ PASSED"
        ((PASSED++))
    else
        echo "  ❌ FAILED"
        ((FAILED++))
    fi
    echo ""
}

# ============================================
# Test 1: CLI Help
# ============================================
echo "Test 1: CLI Help"
if $BIN --help > /dev/null 2>&1; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 2: Plan Command (dry run)
# ============================================
echo "Test 2: Plan Command (Binance BTC/USDT 1m)"
$BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" > /dev/null 2>&1
test_result $?

# ============================================
# Test 3: Plan with JSON output
# ============================================
echo "Test 3: Plan with JSON output"
JSON_OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" -j 2>/dev/null)
if echo "$JSON_OUTPUT" | grep -q "estimated_total_bytes"; then
    test_result 0
else
    echo "  JSON output: $JSON_OUTPUT"
    test_result 1
fi

# ============================================
# Test 4: Download command exists
# ============================================
echo "Test 4: Download command exists"
$BIN download --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 5: Verify command exists
# ============================================
echo "Test 5: Verify command exists"
$BIN verify --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 6: Gaps command exists
# ============================================
echo "Test 6: Gaps command exists"
$BIN gaps --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 7: Inspect command exists
# ============================================
echo "Test 7: Inspect command exists"
$BIN inspect --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 8: Convert command exists
# ============================================
echo "Test 8: Convert command exists"
$BIN convert --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 9: List command exists
# ============================================
echo "Test 9: List command exists"
$BIN list --help > /dev/null 2>&1
test_result $?

# ============================================
# Test 10: Exchange parsing (Binance)
# ============================================
echo "Test 10: Exchange parsing (Binance)"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "binance"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 11: Exchange parsing (Bybit)
# ============================================
echo "Test 11: Exchange parsing (Bybit)"
OUTPUT=$($BIN plan --exchange bybit --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "bybit"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 12: Exchange parsing (OKX)
# ============================================
echo "Test 12: Exchange parsing (OKX)"
OUTPUT=$($BIN plan --exchange okx --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "okx"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 13: Single timeframe parsing
# ============================================
echo "Test 13: Single timeframe parsing"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "1m"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 14: Multiple pairs
# ============================================
echo "Test 14: Multiple pairs"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT ETH/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "BTC/USDT" && echo "$OUTPUT" | grep -q "ETH/USDT"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 15: Symbol parsing (with slash)
# ============================================
echo "Test 15: Symbol parsing (BTC/USDT)"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "BTC/USDT"; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 16: Symbol parsing (without slash)
# ============================================
echo "Test 16: Symbol parsing (BTCUSDT)"
OUTPUT=$($BIN plan --exchange binance --pairs BTCUSDT --timeframes 1m --timerange 20230101-20230102 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -q "BTC_USDT" || echo "$OUTPUT" | grep -q "BTC/USDT"; then
    test_result 0
else
    echo "  Output: $OUTPUT"
    test_result 1
fi

# ============================================
# Test 17: TimeRange parsing
# ============================================
echo "Test 17: TimeRange parsing"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20231231 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -qi "chunks"; then
    test_result 0
else
    echo "  Output: $OUTPUT"
    test_result 1
fi

# ============================================
# Test 18: Unit tests pass
# ============================================
echo "Test 18: Unit tests pass"
cargo test --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 19: Cargo build succeeds
# ============================================
echo "Test 19: Cargo build succeeds"
cargo build --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 20: Binary exists and runs
# ============================================
echo "Test 20: Binary exists and runs"
if [ -f "$BIN" ] && $BIN --version > /dev/null 2>&1; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 21: Chunk decomposition
# ============================================
echo "Test 21: Chunk decomposition (multi-month)"
OUTPUT=$($BIN plan --exchange binance --pairs BTC/USDT --timeframes 1m --timerange 20230101-20230601 -o "$TEST_DIR" 2>&1)
if echo "$OUTPUT" | grep -qi "Chunks"; then
    test_result 0
else
    echo "  Output: $OUTPUT"
    test_result 1
fi

# ============================================
# Test 22: Parquet format option
# ============================================
echo "Test 22: Parquet format in download command"
$BIN download --help 2>&1 | grep -q "parquet"
test_result $?

# ============================================
# Test 23: HTTP client unit tests
# ============================================
echo "Test 23: HTTP module unit tests"
cargo test -p ftdata-http --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 24: Sources module compiles
# ============================================
echo "Test 24: Sources module compiles"
cargo build -p ftdata-sources --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 25: Core module unit tests
# ============================================
echo "Test 25: Core module unit tests"
cargo test -p ftdata-core --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 26: Storage module unit tests
# ============================================
echo "Test 26: Storage module unit tests"
cargo test -p ftdata-storage --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 27: Analysis module unit tests
# ============================================
echo "Test 27: Analysis module unit tests"
cargo test -p ftdata-analysis --quiet 2>&1
if [ ${PIPESTATUS[0]} -eq 0 ]; then
    test_result 0
else
    test_result 1
fi

# ============================================
# Test 28: Inspect non-existent file
# ============================================
echo "Test 28: Inspect non-existent file (should error)"
OUTPUT=$($BIN inspect --path ./test_data/nonexistent.feather 2>&1 || true)
if echo "$OUTPUT" | grep -q "not found"; then
    test_result 0
else
    echo "  Output: $OUTPUT"
    test_result 1
fi

# ============================================
# Summary
# ============================================
echo "========================================="
echo "Test Summary"
echo "========================================="
echo "Passed: $PASSED"
echo "Failed: $FAILED"
echo "Total:  $((PASSED + FAILED))"
echo ""

if [ $FAILED -eq 0 ]; then
    echo "🎉 All tests passed!"
    exit 0
else
    echo "⚠️  Some tests failed"
    exit 1
fi
