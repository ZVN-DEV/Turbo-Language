#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
javac Benchmark.java && java Benchmark all
