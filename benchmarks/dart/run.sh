#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
dart compile exe benchmark.dart -o benchmark && ./benchmark all
