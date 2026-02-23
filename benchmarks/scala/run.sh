#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

# Try scala-cli first (modern, no separate compile step needed).
# Fall back to classic scalac + scala if scala-cli is not available.
if command -v scala-cli &>/dev/null; then
    scala-cli run Benchmark.scala -- all
elif command -v scalac &>/dev/null; then
    scalac Benchmark.scala
    scala Benchmark all
else
    echo "ERROR: Neither scala-cli nor scalac found on PATH." >&2
    exit 1
fi
