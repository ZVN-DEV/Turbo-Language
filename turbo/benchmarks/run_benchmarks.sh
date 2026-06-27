#!/usr/bin/env bash
# Turbo Language Benchmark Suite
# Runs all bench_*.tb programs and reports timing for JIT and AOT modes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TURBO="$PROJECT_DIR/target/release/turbolang"

# Colors
BOLD="\033[1m"
CYAN="\033[1;36m"
GREEN="\033[32m"
YELLOW="\033[33m"
DIM="\033[90m"
RESET="\033[0m"

if [ "${TURBO_BENCH_SKIP_BUILD:-0}" != "1" ]; then
    echo "Building compiler (release)..."
    (cd "$PROJECT_DIR" && cargo build --release -p turbo-cli >/dev/null)
elif [ ! -f "$TURBO" ]; then
    echo "error: $TURBO does not exist and TURBO_BENCH_SKIP_BUILD=1 was set" >&2
    exit 1
fi

echo ""
printf "${BOLD}Turbo Language Benchmark Suite${RESET}\n"
printf "${DIM}==============================${RESET}\n"
printf "${DIM}Compiler: %s${RESET}\n" "$TURBO"
printf "${DIM}Date:     %s${RESET}\n" "$(date '+%Y-%m-%d %H:%M:%S')"
echo ""

TOTAL_BENCHMARKS=0
TOTAL_PASS=0
TOTAL_FAIL=0
FLOAT_TOLERANCE="${TURBO_BENCH_FLOAT_TOLERANCE:-0}"
BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/turbo-bench.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT

outputs_match() {
    local expected="$1"
    local actual="$2"
    if [ "$expected" = "$actual" ]; then
        return 0
    fi

    python3 - "$expected" "$actual" "$FLOAT_TOLERANCE" <<'PY'
import math
import sys

expected, actual, tolerance_text = sys.argv[1:]
try:
    tolerance = float(tolerance_text)
    expected_value = float(expected.strip())
    actual_value = float(actual.strip())
except ValueError:
    sys.exit(1)

if math.isfinite(expected_value) and math.isfinite(actual_value) and abs(expected_value - actual_value) <= tolerance:
    sys.exit(0)
sys.exit(1)
PY
}

for bench in "$SCRIPT_DIR"/bench_*.tb; do
    [ -f "$bench" ] || continue
    name="$(basename "$bench" .tb)"
    TOTAL_BENCHMARKS=$((TOTAL_BENCHMARKS + 1))

    printf "${CYAN}--- %s ---${RESET}\n" "$name"

    # Show expected output from comment
    expected=$(grep "^// Expected output:" "$bench" 2>/dev/null | sed 's/^\/\/ Expected output: //' || true)
    if [ -n "$expected" ]; then
        printf "${DIM}  expected: %s${RESET}\n" "$expected"
    fi

    # JIT mode (turbolang run)
    printf "  ${YELLOW}Cranelift JIT:${RESET} "
    jit_start=$(python3 -c "import time; print(time.time())")
    jit_err="$BUILD_DIR/${name}.jit.err"
    set +e
    jit_output=$("$TURBO" run "$bench" 2>"$jit_err")
    jit_status=$?
    set -e
    jit_end=$(python3 -c "import time; print(time.time())")
    jit_time=$(python3 -c "print(f'{${jit_end} - ${jit_start}:.3f}s')")
    printf "%s  ${DIM}(%s)${RESET}\n" "$jit_output" "$jit_time"
    if [ -s "$jit_err" ]; then
        sed 's/^/    stderr: /' "$jit_err"
    fi
    if [ "$jit_status" -ne 0 ]; then
        printf "  \033[31mJIT exited %d\033[0m\n" "$jit_status"
    fi

    # AOT mode (turbolang build + run native binary)
    printf "  ${YELLOW}Cranelift AOT:${RESET} "
    tmp_bin="$BUILD_DIR/$name"
    build_err="$BUILD_DIR/${name}.build.err"
    if "$TURBO" build "$bench" -o "$tmp_bin" >/dev/null 2>"$build_err"; then
        aot_start=$(python3 -c "import time; print(time.time())")
        aot_err="$BUILD_DIR/${name}.aot.err"
        set +e
        aot_output=$("$tmp_bin" 2>"$aot_err")
        aot_status=$?
        set -e
        aot_end=$(python3 -c "import time; print(time.time())")
        aot_time=$(python3 -c "print(f'{${aot_end} - ${aot_start}:.3f}s')")
        printf "%s  ${DIM}(%s)${RESET}\n" "$aot_output" "$aot_time"
        if [ -s "$aot_err" ]; then
            sed 's/^/    stderr: /' "$aot_err"
        fi
        if [ "$aot_status" -ne 0 ]; then
            printf "  \033[31mAOT exited %d\033[0m\n" "$aot_status"
        fi

        if [ "$jit_status" -eq 0 ] \
            && [ "$aot_status" -eq 0 ] \
            && outputs_match "$jit_output" "$aot_output" \
            && cmp -s "$jit_err" "$aot_err"; then
            printf "  ${GREEN}outputs match${RESET}\n"
            TOTAL_PASS=$((TOTAL_PASS + 1))
        else
            printf "  \033[31moutputs differ!\033[0m\n"
            TOTAL_FAIL=$((TOTAL_FAIL + 1))
        fi
    else
        printf "${DIM}(build failed, skipping AOT)${RESET}\n"
        if [ -s "$build_err" ]; then
            sed 's/^/    build stderr: /' "$build_err"
        fi
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi

    echo ""
done

printf "${BOLD}Results: %d/%d benchmarks passed (JIT/AOT output match)${RESET}\n" "$TOTAL_PASS" "$TOTAL_BENCHMARKS"
echo ""

if [ "$TOTAL_FAIL" -ne 0 ]; then
    exit 1
fi
