#!/usr/bin/env bash
# Local end-to-end test of the Lambda function against a mock runtime API —
# no AWS account needed. Requires: turbolang, python3, curl.
set -euo pipefail
cd "$(dirname "$0")"

PORT=19099
OUT=$(mktemp -t turbo-lambda-received)

python3 mock_runtime.py "$PORT" "$OUT" 2 &
MOCK=$!
trap 'kill $MOCK 2>/dev/null || true' EXIT
sleep 0.3

AWS_LAMBDA_RUNTIME_API="127.0.0.1:$PORT" \
TURBO_ALLOW_PRIVATE_HOSTS=1 \
TURBO_LAMBDA_MAX=2 \
    turbolang run main.tb

wait "$MOCK" 2>/dev/null || true

echo "--- responses the mock received:"
cat "$OUT"
echo

grep -q 'Hello, invocation-1!' "$OUT" || { echo "FAIL: missing response 1"; exit 1; }
grep -q 'Hello, invocation-2!' "$OUT" || { echo "FAIL: missing response 2"; exit 1; }
grep -q '/runtime/invocation/req-1/response' "$OUT" || { echo "FAIL: wrong response path"; exit 1; }
echo "PASS: 2 invocations handled end-to-end"
rm -f "$OUT"
