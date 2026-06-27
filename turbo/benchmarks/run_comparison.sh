#!/usr/bin/env bash
# Comprehensive benchmark comparison.
#
# The runner is intentionally source-driven: native baselines are compiled into
# a temporary build directory instead of checked into turbo/benchmarks.

set -euo pipefail

TURBO="$(cd "$(dirname "$0")/.." && pwd)/target/release/turbolang"
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
C_DIR="$BENCH_DIR/c"
RUST_DIR="$BENCH_DIR/rust"
ITERATIONS="${TURBO_BENCH_ITERS:-3}"
BENCHMARKS="${TURBO_BENCHMARKS:-fib ack primes collatz sort string matrix matrix_unsafe float hashmap string_heavy}"
BUILD_DIR="${TURBO_BENCH_BUILD_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/turbo-bench.XXXXXX")}"
KEEP_BUILD_DIR="${TURBO_BENCH_KEEP_BUILD_DIR:-0}"
FLOAT_TOLERANCE="${TURBO_BENCH_FLOAT_TOLERANCE:-0}"
FAILURES=0

if [ "$KEEP_BUILD_DIR" != "1" ]; then
    trap 'rm -rf "$BUILD_DIR"' EXIT
fi

if [ ! -x "$TURBO" ]; then
    echo "Building compiler (release)..." >&2
    cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" -p turbo-cli >/dev/null
fi

mkdir -p "$BUILD_DIR/c" "$BUILD_DIR/rust" "$BUILD_DIR/turbo-aot" "$BUILD_DIR/out"

time_ms() {
    local out_file="$1"
    shift
    local start
    local end
    start=$(python3 -c "import time; print(int(time.time()*1000))")
    "$@" > "$out_file" 2>"$out_file.err"
    end=$(python3 -c "import time; print(int(time.time()*1000))")
    echo $(( end - start ))
}

best_of_n() {
    local out_file="$1"
    shift
    local best=999999999
    local best_output="$BUILD_DIR/out/best.$$"
    local run
    for run in $(seq 1 "$ITERATIONS"); do
        local candidate="$BUILD_DIR/out/run.$$.$run"
        local ms
        ms=$(time_ms "$candidate" "$@")
        if [ "$ms" -lt "$best" ]; then
            best="$ms"
            cp "$candidate" "$best_output"
        fi
    done
    cp "$best_output" "$out_file"
    echo "$best"
}

compile_c() {
    local bench="$1"
    local src="$C_DIR/$bench.c"
    if [ "$bench" = "matrix_unsafe" ] && [ ! -f "$src" ]; then
        src="$C_DIR/matrix.c"
    fi
    [ -f "$src" ] || return 1
    command -v cc >/dev/null 2>&1 || return 1
    cc -O2 -o "$BUILD_DIR/c/$bench" "$src"
}

compile_rust() {
    local bench="$1"
    local src="$RUST_DIR/$bench.rs"
    [ -f "$src" ] || return 1
    command -v rustc >/dev/null 2>&1 || return 1
    rustc -O "$src" -o "$BUILD_DIR/rust/$bench"
}

format_ms() {
    local value="$1"
    if [ -z "$value" ]; then
        printf "%10s" "N/A"
    else
        printf "%8s ms" "$value"
    fi
}

ratio_to_turbo() {
    local turbo_ms="$1"
    local baseline_ms="$2"
    if [ -z "$baseline_ms" ] || [ "$baseline_ms" -le 0 ] 2>/dev/null; then
        printf "%9s" "N/A"
    else
        python3 -c "import sys; sys.stdout.write(f'{$turbo_ms/$baseline_ms:>8.2f}x')"
    fi
}

outputs_match() {
    local expected="$1"
    local actual="$2"
    if cmp -s "$expected" "$actual"; then
        return 0
    fi

    python3 - "$expected" "$actual" "$FLOAT_TOLERANCE" <<'PY'
import math
import sys

expected_path, actual_path, tolerance_text = sys.argv[1:]
expected = open(expected_path, encoding="utf-8").read().strip()
actual = open(actual_path, encoding="utf-8").read().strip()

try:
    tolerance = float(tolerance_text)
    expected_value = float(expected)
    actual_value = float(actual)
except ValueError:
    sys.exit(1)

if math.isfinite(expected_value) and math.isfinite(actual_value) and abs(expected_value - actual_value) <= tolerance:
    sys.exit(0)
sys.exit(1)
PY
}

append_mismatch() {
    local status="$1"
    local label="$2"
    if [ -z "$status" ]; then
        printf "output-mismatch:%s" "$label"
    else
        printf "%s,output-mismatch:%s" "$status" "$label"
    fi
}

printf "%-16s %10s %10s %10s %10s %10s %10s\n" \
    "Benchmark" "Turbo JIT" "Turbo AOT" "C (-O2)" "Rust (-O)" "JIT/C" "JIT/Rust"
printf "%-16s %10s %10s %10s %10s %10s %10s\n" \
    "--------" "---------" "---------" "-------" "--------" "-----" "--------"

for bench in $BENCHMARKS; do
    tb_file="$BENCH_DIR/bench_$bench.tb"
    if [ ! -f "$tb_file" ]; then
        printf "%-16s %s\n" "$bench" "SKIP (no .tb file)"
        continue
    fi

    jit_out="$BUILD_DIR/out/$bench.turbo-jit"
    aot_out="$BUILD_DIR/out/$bench.turbo-aot"
    c_out="$BUILD_DIR/out/$bench.c"
    rust_out="$BUILD_DIR/out/$bench.rust"
    aot_bin="$BUILD_DIR/turbo-aot/$bench"

    turbo_jit_ms=$(best_of_n "$jit_out" "$TURBO" run "$tb_file")
    "$TURBO" build "$tb_file" -o "$aot_bin" >/dev/null 2>&1
    turbo_aot_ms=$(best_of_n "$aot_out" "$aot_bin")

    c_ms=""
    if compile_c "$bench"; then
        c_ms=$(best_of_n "$c_out" "$BUILD_DIR/c/$bench")
    fi

    rust_ms=""
    if compile_rust "$bench"; then
        rust_ms=$(best_of_n "$rust_out" "$BUILD_DIR/rust/$bench")
    fi

    status=""
    if ! outputs_match "$jit_out" "$aot_out"; then
        status="$(append_mismatch "$status" "turbo-aot")"
        FAILURES=$((FAILURES + 1))
    fi
    if [ -n "$c_ms" ] && ! outputs_match "$jit_out" "$c_out"; then
        status="$(append_mismatch "$status" "c")"
        FAILURES=$((FAILURES + 1))
    fi
    if [ -n "$rust_ms" ] && ! outputs_match "$jit_out" "$rust_out"; then
        status="$(append_mismatch "$status" "rust")"
        FAILURES=$((FAILURES + 1))
    fi

    printf "%-16s " "$bench"
    format_ms "$turbo_jit_ms"
    printf " "
    format_ms "$turbo_aot_ms"
    printf " "
    format_ms "$c_ms"
    printf " "
    format_ms "$rust_ms"
    printf " "
    ratio_to_turbo "$turbo_jit_ms" "$c_ms"
    printf " "
    ratio_to_turbo "$turbo_jit_ms" "$rust_ms"
    if [ -n "$status" ]; then
        printf "  %s" "$status"
    fi
    printf "\n"
done

if [ "$KEEP_BUILD_DIR" = "1" ]; then
    echo "Kept build directory: $BUILD_DIR"
fi

if [ "$FAILURES" -ne 0 ]; then
    echo "benchmark comparison failed: $FAILURES output mismatch(es)" >&2
    exit 1
fi
