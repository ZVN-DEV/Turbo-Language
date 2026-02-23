#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"
swiftc -O main.swift -o benchmark
./benchmark all
