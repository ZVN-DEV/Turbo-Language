#!/usr/bin/env bash
# Benchmark runner for Zig suite.
# Make executable with: chmod +x run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Zig benchmarks (ReleaseFast) ===" >&2
zig build -Doptimize=ReleaseFast 2>&1 >&2

echo "=== Running all benchmarks ===" >&2
./zig-out/bin/benchmark all
