#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== C++ Benchmark Suite ==="
echo ""

# --------------------------------------------------------------------------
# Build
# --------------------------------------------------------------------------
echo "[build] Compiling with g++ -O2 -std=c++17 -pthread ..."
g++ -O2 -std=c++17 -pthread main.cpp -o benchmark
echo "[build] Done."
echo ""

# --------------------------------------------------------------------------
# Run
# --------------------------------------------------------------------------
BENCH_ARG="${1:-all}"
echo "[run]   ./benchmark $BENCH_ARG"
echo ""
./benchmark "$BENCH_ARG"
