#!/bin/bash
# Comprehensive benchmark comparison: Turbo JIT vs C (-O2)
# Best of 3 runs, millisecond precision

TURBO="$(cd "$(dirname "$0")/.." && pwd)/target/release/turbolang"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
C_DIR="$BENCH_DIR/c"

BENCHMARKS="fib ack primes collatz sort string matrix matrix_unsafe float hashmap string_heavy"

time_ms() {
    local start=$(python3 -c "import time; print(int(time.time()*1000))")
    "$@" > /dev/null 2>&1
    local end=$(python3 -c "import time; print(int(time.time()*1000))")
    echo $(( end - start ))
}

best_of_3() {
    local best=999999
    for run in 1 2 3; do
        local ms=$(time_ms "$@")
        if [ "$ms" -lt "$best" ]; then best=$ms; fi
    done
    echo "$best"
}

printf "%-16s %10s %10s %10s\n" "Benchmark" "Turbo JIT" "C (-O2)" "Ratio"
printf "%-16s %10s %10s %10s\n" "--------" "---------" "-------" "-----"

for bench in $BENCHMARKS; do
    tb_file="$BENCH_DIR/bench_$bench.tb"
    c_bin="$C_DIR/$bench"

    if [ ! -f "$tb_file" ]; then echo "SKIP $bench (no .tb)"; continue; fi
    if [ ! -x "$c_bin" ]; then
        cc -O2 -o "$c_bin" "$C_DIR/$bench.c" 2>/dev/null
    fi

    turbo_ms=$(best_of_3 "$TURBO" run "$tb_file")
    c_ms=$(best_of_3 "$c_bin")

    if [ "$c_ms" -gt 0 ] 2>/dev/null; then
        ratio=$(python3 -c "print(f'{$turbo_ms/$c_ms:.2f}')")
    else
        ratio="N/A"
    fi

    printf "%-16s %8s ms %8s ms %9sx\n" "$bench" "$turbo_ms" "$c_ms" "$ratio"
done
