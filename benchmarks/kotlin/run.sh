#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
kotlinc Benchmark.kt -include-runtime -d benchmark.jar && java -jar benchmark.jar all
