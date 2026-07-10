#!/usr/bin/env bash
# Measure N forced cold starts per runtime. Forces a cold start each round by
# flipping a BENCH_EPOCH env var (Lambda provisions a fresh sandbox on config
# change), invokes with --log-type Tail, and parses the platform-reported
# REPORT line. Results: results.csv
# Usage: ./bench.sh [iterations] [region]
set -euo pipefail
cd "$(dirname "$0")"

N="${1:-10}"
REGION="${2:-us-east-1}"
OUT=results.csv
echo "runtime,iteration,init_ms,duration_ms,billed_ms,memory_max_mb" > "$OUT"

bench_one() { # fn-name label iteration
    local fn="$1" label="$2" i="$3"
    aws lambda update-function-configuration --region "$REGION" \
        --function-name "$fn" \
        --environment "Variables={BENCH_EPOCH=$i,TURBO_ALLOW_PRIVATE_HOSTS=1}" >/dev/null
    aws lambda wait function-updated-v2 --region "$REGION" --function-name "$fn"

    local log report init dur billed mem
    log=$(aws lambda invoke --region "$REGION" --function-name "$fn" \
        --payload '{"name":"bench"}' --cli-binary-format raw-in-base64-out \
        --log-type Tail --query LogResult --output text /dev/null | base64 -d)
    report=$(grep "REPORT" <<<"$log")
    # REPORT field order: Duration, Billed Duration, Memory Size,
    # Max Memory Used, Init Duration — so the first "Duration:" match is the
    # handler duration.
    init=$(sed -n 's/.*Init Duration: \([0-9.]*\) ms.*/\1/p' <<<"$report")
    dur=$(grep -o 'Duration: [0-9.]* ms' <<<"$report" | head -1 | grep -o '[0-9.]*')
    billed=$(sed -n 's/.*Billed Duration: \([0-9.]*\) ms.*/\1/p' <<<"$report")
    mem=$(sed -n 's/.*Max Memory Used: \([0-9]*\) MB.*/\1/p' <<<"$report")
    if [ -z "$init" ]; then
        echo "WARN: $label iteration $i was NOT a cold start (no Init Duration) — discarding" >&2
        return 0
    fi
    echo "$label,$i,$init,$dur,$billed,$mem" >> "$OUT"
    echo "$label #$i: init ${init}ms, duration ${dur}ms, max mem ${mem}MB"
}

for i in $(seq 1 "$N"); do
    bench_one turbo-bench  turbo  "$i"
    bench_one node-bench   node   "$i"
    bench_one python-bench python "$i"
done

echo
echo "== median init duration (ms) per runtime =="
for label in turbo node python; do
    med=$(awk -F, -v l="$label" '$1==l {print $3}' "$OUT" | sort -n | awk '
        {a[NR]=$1}
        END {
            if (NR == 0) print "n/a"
            else if (NR % 2) print a[(NR+1)/2]
            else print (a[NR/2] + a[NR/2+1]) / 2
        }')
    printf "%-7s %s\n" "$label" "$med"
done
echo "full data: $OUT"
