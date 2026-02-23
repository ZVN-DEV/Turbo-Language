#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Install deps if needed
if [ ! -d "node_modules" ]; then
  echo "Installing dependencies..." >&2
  npm install --silent
fi

npx tsx benchmark.ts all
