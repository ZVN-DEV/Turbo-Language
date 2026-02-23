#!/usr/bin/env bash
# Run all Julia benchmarks
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
julia "$SCRIPT_DIR/benchmark.jl" all
