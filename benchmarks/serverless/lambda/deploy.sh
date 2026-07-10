#!/usr/bin/env bash
# Deploy the three benchmark functions (turbo-bench, node-bench, python-bench),
# configured identically: 128MB, x86_64, trivial JSON-echo handlers.
# Usage: ./deploy.sh <lambda-execution-role-arn> [region]
set -euo pipefail
cd "$(dirname "$0")"

ROLE_ARN="${1:?usage: deploy.sh <lambda-execution-role-arn> [region]}"
REGION="${2:-us-east-1}"
MEMORY=128

echo "== turbo: cross-compile + zip"
(cd turbo && turbolang build main.tb --target linux-x86 -o handler \
    && chmod +x bootstrap && zip -q -j function.zip bootstrap handler)

echo "== node: zip"
(cd node && zip -q -j function.zip index.mjs)

echo "== python: zip"
(cd python && zip -q -j function.zip handler.py)

create() { # name runtime handler zip
    aws lambda create-function --region "$REGION" \
        --function-name "$1" --runtime "$2" --handler "$3" \
        --architectures x86_64 --memory-size "$MEMORY" \
        --zip-file "fileb://$4" --role "$ROLE_ARN" >/dev/null
    echo "created $1"
}

create turbo-bench  provided.al2023 unused        turbo/function.zip
create node-bench   nodejs22.x      index.handler node/function.zip
create python-bench python3.13      handler.handler python/function.zip

echo "waiting for functions to become Active..."
for fn in turbo-bench node-bench python-bench; do
    aws lambda wait function-active-v2 --region "$REGION" --function-name "$fn"
done
echo "done — run ./bench.sh"
