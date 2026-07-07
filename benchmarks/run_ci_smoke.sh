#!/usr/bin/env bash
# CI perf-regression smoke test.
#
# Purpose: TurboLang's published performance claims (README, benchmarks/) have
# no CI coverage — a codegen regression could silently invalidate them. This
# script is a *fast* subset of the full comparison suite, sized to run in a
# GitHub Actions job in well under ~5 minutes, that:
#
#   1. builds turbolang (release),
#   2. runs a couple of representative benchmarks (fib, primes) as native
#      Turbo AOT binaries and compares them against C (cc -O2) baselines,
#   3. enforces byte-identical program output (correctness gate), and
#   4. gates on the Turbo/C wall-clock *ratio* vs a stored baseline
#      (benchmarks/ci_baseline.json), failing only on a large regression.
#
# Why ratios, not absolute times: shared CI runners are noisy and vary
# machine-to-machine, but the Turbo-vs-C ratio is far more stable because both
# programs run back-to-back on the same box. We still gate generously (default
# +30%) so normal runner jitter never trips a false alarm.
#
# Self-contained: the .tb sources and matching C baselines live in
# benchmarks/ci_smoke/ so this script never depends on turbo/benchmarks/ and
# can size the workloads (e.g. fib 35, not fib 40) for CI latency.
#
# Usage:
#   bash benchmarks/run_ci_smoke.sh                # gate against ci_baseline.json
#   TURBO_SMOKE_UPDATE_BASELINE=1 bash benchmarks/run_ci_smoke.sh   # rewrite baseline
#
# Env knobs:
#   TURBO_SMOKE_ITERS       best-of-N wall-clock samples per program (default 5)
#   TURBO_SMOKE_TOLERANCE   allowed ratio regression fraction (default 0.30)
#   TURBO_SMOKE_BENCHMARKS  space-separated benchmark names (default "fib primes")
#   TURBO_SMOKE_RESULTS_DIR where results.json / results.csv are written
#   TURBO_SMOKE_UPDATE_BASELINE=1  measure and overwrite ci_baseline.json, no gate

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
SMOKE_DIR="$SCRIPT_DIR/ci_smoke"
TURBO="$PROJECT_DIR/turbo/target/release/turbolang"
BASELINE_FILE="$SCRIPT_DIR/ci_baseline.json"

ITERATIONS="${TURBO_SMOKE_ITERS:-5}"
TOLERANCE="${TURBO_SMOKE_TOLERANCE:-0.30}"
BENCHMARKS="${TURBO_SMOKE_BENCHMARKS:-fib primes}"
RESULTS_DIR="${TURBO_SMOKE_RESULTS_DIR:-$SCRIPT_DIR/ci_smoke/results}"
UPDATE_BASELINE="${TURBO_SMOKE_UPDATE_BASELINE:-0}"

BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/turbo-ci-smoke.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT
mkdir -p "$BUILD_DIR/c" "$BUILD_DIR/turbo-aot" "$BUILD_DIR/out" "$RESULTS_DIR"
ROWS_TSV="$BUILD_DIR/rows.tsv"   # bench \t turbo_ms \t c_ms \t ratio \t baseline \t correct \t status
: > "$ROWS_TSV"

# --------------------------------------------------------------------------
# Build compiler (release) if not already present.
# --------------------------------------------------------------------------
if [ ! -x "$TURBO" ]; then
    echo "[build] turbolang release binary not found — building..." >&2
    cargo build --release --manifest-path "$PROJECT_DIR/turbo/Cargo.toml" -p turbo-cli >&2
fi
echo "[info]  turbolang: $("$TURBO" --version)" >&2

if ! command -v cc >/dev/null 2>&1; then
    echo "error: 'cc' not found — the C baseline is required for the smoke test" >&2
    exit 1
fi

# --------------------------------------------------------------------------
# Timing helpers (best-of-N wall clock, milliseconds).
# --------------------------------------------------------------------------
now_ms() { python3 -c "import time; print(int(time.time()*1000))"; }

time_ms() {
    local out_file="$1"; shift
    local start end
    start=$(now_ms)
    # Name a crashing/non-zero binary explicitly. Without this the failure trips
    # `set -e` inside the `$(...)` capture and aborts mid-table with only bash's
    # terse message, never reaching the OUTPUT-MISMATCH reporting path.
    if ! "$@" > "$out_file" 2>"$out_file.err"; then
        echo "error: benchmark command '$*' exited non-zero (see $out_file.err)" >&2
        exit 1
    fi
    end=$(now_ms)
    echo $(( end - start ))
}

best_of_n() {
    local out_file="$1"; shift
    local best=999999999 run ms candidate
    local best_out="$BUILD_DIR/out/best.$$"
    for run in $(seq 1 "$ITERATIONS"); do
        candidate="$BUILD_DIR/out/run.$$.$run"
        ms=$(time_ms "$candidate" "$@")
        if [ "$ms" -lt "$best" ]; then
            best="$ms"
            cp "$candidate" "$best_out"
        fi
    done
    cp "$best_out" "$out_file"
    echo "$best"
}

# --------------------------------------------------------------------------
# Run the suite.
# --------------------------------------------------------------------------
printf "%-10s %12s %12s %10s %10s %16s\n" \
    "Benchmark" "Turbo AOT" "C (-O2)" "Turbo/C" "Baseline" "Status" >&2
printf "%-10s %12s %12s %10s %10s %16s\n" \
    "---------" "---------" "-------" "-------" "--------" "------" >&2

GATE_FAILURES=0
CORRECTNESS_FAILURES=0

for bench in $BENCHMARKS; do
    tb_file="$SMOKE_DIR/$bench.tb"
    c_file="$SMOKE_DIR/$bench.c"
    if [ ! -f "$tb_file" ] || [ ! -f "$c_file" ]; then
        echo "error: missing sources for '$bench' ($tb_file / $c_file)" >&2
        exit 1
    fi

    aot_bin="$BUILD_DIR/turbo-aot/$bench"
    c_bin="$BUILD_DIR/c/$bench"
    aot_out="$BUILD_DIR/out/$bench.turbo-aot"
    c_out="$BUILD_DIR/out/$bench.c"

    # Build both. Capture compiler output so a failure lands in the CI log with a
    # diagnostic instead of aborting silently under `set -e`.
    if ! "$TURBO" build "$tb_file" -o "$aot_bin" >"$BUILD_DIR/build-$bench.log" 2>&1; then
        echo "error: turbolang build failed for '$bench':" >&2
        cat "$BUILD_DIR/build-$bench.log" >&2
        exit 1
    fi
    cc -O2 -o "$c_bin" "$c_file"

    # Best-of-N timings.
    turbo_ms=$(best_of_n "$aot_out" "$aot_bin")
    c_ms=$(best_of_n "$c_out" "$c_bin")

    # (c) Correctness: byte-identical program output.
    correct=1
    if ! cmp -s "$aot_out" "$c_out"; then
        correct=0
        CORRECTNESS_FAILURES=$((CORRECTNESS_FAILURES + 1))
    fi

    # Ratio (guard divide-by-zero for sub-millisecond C runs).
    ratio=$(python3 -c "c=$c_ms; print(f'{$turbo_ms/c:.4f}' if c>0 else '999')")

    # Baseline lookup.
    baseline=""
    if [ -f "$BASELINE_FILE" ]; then
        baseline=$(python3 -c "import json; d=json.load(open('$BASELINE_FILE')); print(d.get('benchmarks',{}).get('$bench',{}).get('turbo_over_c',''))")
    fi

    # Gate / status.
    if [ "$correct" -eq 0 ]; then
        status="OUTPUT-MISMATCH"
    elif [ "$UPDATE_BASELINE" = "1" ]; then
        status="measured"
    elif [ -n "$baseline" ]; then
        # Fail only if ratio exceeds baseline * (1 + tolerance).
        status=$(python3 -c "print('FAIL' if $ratio > $baseline*(1+$TOLERANCE) else 'ok')")
        [ "$status" = "FAIL" ] && GATE_FAILURES=$((GATE_FAILURES + 1))
    else
        status="no-baseline"
    fi

    printf "%-10s %9s ms %9s ms %10s %10s %16s\n" \
        "$bench" "$turbo_ms" "$c_ms" "$ratio" "${baseline:-null}" "$status" >&2

    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
        "$bench" "$turbo_ms" "$c_ms" "$ratio" "${baseline:-}" "$correct" "$status" >> "$ROWS_TSV"
done

# --------------------------------------------------------------------------
# Emit artifacts (results.json + results.csv) from the accumulated rows.
# --------------------------------------------------------------------------
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")
PLATFORM="$(uname -s) $(uname -m)"

python3 - "$ROWS_TSV" "$RESULTS_DIR" "$TIMESTAMP" "$PLATFORM" "$ITERATIONS" "$TOLERANCE" <<'PY'
import csv, json, sys
rows_tsv, results_dir, ts, platform, iters, tol = sys.argv[1:7]
rows = []
with open(rows_tsv) as f:
    for bench, turbo_ms, c_ms, ratio, baseline, correct, status in csv.reader(f, delimiter="\t"):
        rows.append({
            "benchmark": bench,
            "turbo_aot_ms": int(turbo_ms),
            "c_ms": int(c_ms),
            "turbo_over_c": float(ratio),
            "baseline_turbo_over_c": float(baseline) if baseline else None,
            "output_bytes_match": correct == "1",
            "status": status,
        })

with open(f"{results_dir}/results.json", "w") as f:
    json.dump({
        "timestamp": ts,
        "platform": platform,
        "methodology": (
            f"Best-of-{iters} wall-clock. Turbo AOT native binary vs C (cc -O2), "
            "back-to-back on the same runner. Gate: fail if turbo_over_c exceeds "
            f"baseline by more than {float(tol)*100:.0f}%. Output is byte-compared for correctness."
        ),
        "results": rows,
    }, f, indent=2)
    f.write("\n")

with open(f"{results_dir}/results.csv", "w", newline="") as f:
    w = csv.writer(f)
    w.writerow(["benchmark", "turbo_aot_ms", "c_ms", "turbo_over_c",
                "baseline_turbo_over_c", "output_match", "status"])
    for r in rows:
        w.writerow([r["benchmark"], r["turbo_aot_ms"], r["c_ms"], r["turbo_over_c"],
                    r["baseline_turbo_over_c"] if r["baseline_turbo_over_c"] is not None else "",
                    "1" if r["output_bytes_match"] else "0", r["status"]])
PY

echo "" >&2
echo "[info]  results: $RESULTS_DIR/results.json + results.csv" >&2

# --------------------------------------------------------------------------
# Baseline update mode: rewrite ci_baseline.json from this run and exit.
# --------------------------------------------------------------------------
if [ "$UPDATE_BASELINE" = "1" ]; then
    # Never seed a baseline from a miscompiling run: a wrong-output benchmark would
    # bake its (meaningless) ratio into the gate everyone else is measured against.
    if [ "$CORRECTNESS_FAILURES" -ne 0 ]; then
        echo "refusing to update baseline: $CORRECTNESS_FAILURES benchmark(s) produced output that differs from C." >&2
        exit 1
    fi
    python3 - "$ROWS_TSV" "$BASELINE_FILE" "$PLATFORM" "$TOLERANCE" <<'PY'
import csv, json, sys
from datetime import datetime, timezone
rows_tsv, out_path, platform, tol = sys.argv[1:5]
benches = {}
with open(rows_tsv) as f:
    for bench, turbo_ms, c_ms, ratio, *_ in csv.reader(f, delimiter="\t"):
        benches[bench] = {"turbo_over_c": float(ratio)}
doc = {
    "_comment": ("Baseline Turbo-AOT/C wall-clock ratios for the CI perf smoke "
                 "(benchmarks/run_ci_smoke.sh). Ratios, not absolute ms, because they "
                 "are far more stable across noisy shared runners. The gate fails only "
                 "when a measured ratio exceeds baseline * (1 + tolerance_fraction)."),
    "_update_procedure": ("Rebuild release, then run "
                 "`TURBO_SMOKE_UPDATE_BASELINE=1 bash benchmarks/run_ci_smoke.sh` on a "
                 "quiet machine (ideally a Linux GitHub runner, since that is what the "
                 "nightly gate uses). Review the diff — ratios should stay near ~1x C — "
                 "and commit. Bump only on an intentional, understood codegen change."),
    "tolerance_fraction": float(tol),
    "generated_on_platform": platform,
    "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "benchmarks": benches,
}
with open(out_path, "w") as f:
    json.dump(doc, f, indent=2)
    f.write("\n")
print(f"[baseline] wrote {out_path}", file=sys.stderr)
PY
    echo "[baseline] update complete — review and commit ci_baseline.json" >&2
    exit 0
fi

# --------------------------------------------------------------------------
# Final gate.
# --------------------------------------------------------------------------
if [ "$CORRECTNESS_FAILURES" -ne 0 ]; then
    echo "SMOKE FAILED: $CORRECTNESS_FAILURES benchmark(s) produced output that differs from the C baseline." >&2
    exit 1
fi
if [ "$GATE_FAILURES" -ne 0 ]; then
    echo "SMOKE FAILED: $GATE_FAILURES benchmark(s) regressed >${TOLERANCE} vs baseline Turbo/C ratio." >&2
    exit 1
fi
echo "SMOKE PASSED: all benchmarks within ${TOLERANCE} of baseline Turbo/C ratio, output byte-identical." >&2
