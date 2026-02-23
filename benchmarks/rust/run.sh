#!/usr/bin/env bash
# Benchmark runner for Rust suite.
# Make executable with: chmod +x run.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building Rust benchmarks (release mode) ===" >&2
cargo build --release 2>&1 >&2

echo "=== Running all benchmarks ===" >&2
./target/release/benchmark all
