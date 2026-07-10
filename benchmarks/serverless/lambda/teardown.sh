#!/usr/bin/env bash
# Delete the three benchmark functions and local build artifacts.
set -euo pipefail
cd "$(dirname "$0")"
REGION="${1:-us-east-1}"
for fn in turbo-bench node-bench python-bench; do
    aws lambda delete-function --region "$REGION" --function-name "$fn" 2>/dev/null \
        && echo "deleted $fn" || echo "$fn not found"
done
rm -f turbo/function.zip turbo/handler node/function.zip python/function.zip
