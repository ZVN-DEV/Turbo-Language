#!/usr/bin/env bash
set -euo pipefail

TURBO="./target/release/turbo"
PASS=0
FAIL=0
SKIP=0
ERRORS=""

run_test() {
    local tb_file="$1"
    local expected_file="${tb_file%.tb}.expected"
    local test_name=$(basename "$tb_file" .tb)

    if [ ! -f "$expected_file" ]; then
        printf "  SKIP  %s (no .expected file)\n" "$test_name"
        SKIP=$((SKIP + 1))
        return
    fi

    local expected
    expected=$(cat "$expected_file")
    local actual
    actual=$($TURBO run "$tb_file" 2>&1) || true

    # Check if this is an expected-error test
    if echo "$expected" | head -1 | grep -q "^ERROR:"; then
        local error_pattern
        error_pattern=$(echo "$expected" | head -1 | sed 's/^ERROR://')
        if echo "$actual" | grep -qi "$error_pattern"; then
            printf "  PASS  %s\n" "$test_name"
            PASS=$((PASS + 1))
        else
            printf "  FAIL  %s\n" "$test_name"
            printf "        expected error: %s\n" "$error_pattern"
            printf "        actual: %s\n" "$actual"
            FAIL=$((FAIL + 1))
            ERRORS="$ERRORS\n  - $test_name"
        fi
    else
        if [ "$actual" = "$expected" ]; then
            printf "  PASS  %s\n" "$test_name"
            PASS=$((PASS + 1))
        else
            printf "  FAIL  %s\n" "$test_name"
            printf "        expected: %s\n" "$(echo "$expected" | head -3)"
            printf "        actual:   %s\n" "$(echo "$actual" | head -3)"
            FAIL=$((FAIL + 1))
            ERRORS="$ERRORS\n  - $test_name"
        fi
    fi
}

echo "Building compiler..."
cargo build --release 2>/dev/null

echo ""
echo "=== Phase 1 Tests ==="
for f in tests/phase1/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

echo ""
echo "=== Regression Tests ==="
for f in tests/regression/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

echo ""
echo "=== Adversarial Tests ==="
for f in tests/adversarial/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

echo ""
echo "================================"
printf "Results: %d passed, %d failed, %d skipped\n" "$PASS" "$FAIL" "$SKIP"
if [ $FAIL -gt 0 ]; then
    printf "Failed tests:%b\n" "$ERRORS"
    exit 1
else
    echo "All tests passed!"
    exit 0
fi
