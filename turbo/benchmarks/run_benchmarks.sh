#!/usr/bin/env bash
# Turbo Language Benchmark Suite
# Runs all bench_*.tb programs and reports timing for JIT and AOT modes.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TURBO="$PROJECT_DIR/target/release/turbo"

# Colors
BOLD="\033[1m"
CYAN="\033[1;36m"
GREEN="\033[32m"
YELLOW="\033[33m"
DIM="\033[90m"
RESET="\033[0m"

if [ ! -f "$TURBO" ]; then
    echo "Building compiler (release)..."
    (cd "$PROJECT_DIR" && cargo build --release 2>/dev/null)
fi

echo ""
printf "${BOLD}Turbo Language Benchmark Suite${RESET}\n"
printf "${DIM}==============================${RESET}\n"
printf "${DIM}Compiler: %s${RESET}\n" "$TURBO"
printf "${DIM}Date:     %s${RESET}\n" "$(date '+%Y-%m-%d %H:%M:%S')"
echo ""

TOTAL_BENCHMARKS=0
TOTAL_PASS=0

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

    # JIT mode (turbo run)
    printf "  ${YELLOW}Cranelift JIT:${RESET} "
    jit_start=$(python3 -c "import time; print(time.time())")
    jit_output=$("$TURBO" run "$bench" 2>&1) || true
    jit_end=$(python3 -c "import time; print(time.time())")
    jit_time=$(python3 -c "print(f'{${jit_end} - ${jit_start}:.3f}s')")
    printf "%s  ${DIM}(%s)${RESET}\n" "$jit_output" "$jit_time"

    # AOT mode (turbo build + run native binary)
    printf "  ${YELLOW}Cranelift AOT:${RESET} "
    tmp_bin="/tmp/turbo_bench_${name}"
    if "$TURBO" build "$bench" -o "$tmp_bin" >/dev/null 2>&1; then
        aot_start=$(python3 -c "import time; print(time.time())")
        aot_output=$("$tmp_bin" 2>&1) || true
        aot_end=$(python3 -c "import time; print(time.time())")
        aot_time=$(python3 -c "print(f'{${aot_end} - ${aot_start}:.3f}s')")
        printf "%s  ${DIM}(%s)${RESET}\n" "$aot_output" "$aot_time"
        rm -f "$tmp_bin"

        if [ "$jit_output" = "$aot_output" ]; then
            printf "  ${GREEN}outputs match${RESET}\n"
            TOTAL_PASS=$((TOTAL_PASS + 1))
        else
            printf "  \033[31moutputs differ!\033[0m\n"
        fi
    else
        printf "${DIM}(build failed, skipping AOT)${RESET}\n"
    fi

    echo ""
done

printf "${BOLD}Results: %d/%d benchmarks passed (JIT/AOT output match)${RESET}\n" "$TOTAL_PASS" "$TOTAL_BENCHMARKS"
echo ""
