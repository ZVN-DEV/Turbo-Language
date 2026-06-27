#!/usr/bin/env bash
# Optional external-language benchmark smoke.
#
# This validates that tracked source-only baselines in go/js/python/ruby still
# produce the same observable result as the Turbo benchmark fixture. Runtime
# tools are optional by default so local contributors do not need every language
# installed, but a run must execute at least one external baseline to pass.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TURBO="$PROJECT_DIR/target/release/turbolang"

BENCHMARKS="${TURBO_BENCH_EXTERNAL_BENCHMARKS:-fib}"
LANGS="${TURBO_BENCH_EXTERNAL_LANGS:-go js python ruby}"
REQUIRE_ALL="${TURBO_BENCH_EXTERNAL_REQUIRE_ALL:-0}"
ALLOW_EMPTY="${TURBO_BENCH_EXTERNAL_ALLOW_EMPTY:-0}"
BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/turbo-external-bench.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT

if [ "${TURBO_BENCH_SKIP_BUILD:-0}" != "1" ]; then
    echo "Building compiler (release)..." >&2
    (cd "$PROJECT_DIR" && cargo build --release -p turbo-cli >/dev/null)
elif [ ! -x "$TURBO" ]; then
    echo "error: $TURBO does not exist and TURBO_BENCH_SKIP_BUILD=1 was set" >&2
    exit 1
fi

failures=0
skipped=0
ran=0

runtime_for_lang() {
    case "$1" in
        go) printf "%s" "${GO_BIN:-go}" ;;
        js) printf "%s" "${NODE_BIN:-node}" ;;
        python) printf "%s" "${PYTHON_BIN:-python3}" ;;
        ruby) printf "%s" "${RUBY_BIN:-ruby}" ;;
        *)
            echo "error: unsupported external benchmark language '$1'" >&2
            return 2
            ;;
    esac
}

source_for_lang() {
    case "$1" in
        go) printf "%s/%s/%s.go" "$SCRIPT_DIR" "$1" "$2" ;;
        js) printf "%s/%s/%s.js" "$SCRIPT_DIR" "$1" "$2" ;;
        python) printf "%s/%s/%s.py" "$SCRIPT_DIR" "$1" "$2" ;;
        ruby) printf "%s/%s/%s.rb" "$SCRIPT_DIR" "$1" "$2" ;;
    esac
}

run_external() {
    local lang="$1"
    local bench="$2"
    local src="$3"
    local runtime="$4"
    local out="$5"
    local err="$6"

    case "$lang" in
        go) "$runtime" run "$src" >"$out" 2>"$err" ;;
        js | python | ruby) "$runtime" "$src" >"$out" 2>"$err" ;;
    esac
}

printf "%-10s %-10s %s\n" "Language" "Benchmark" "Status"
printf "%-10s %-10s %s\n" "--------" "---------" "------"

for bench in $BENCHMARKS; do
    tb_file="$SCRIPT_DIR/bench_$bench.tb"
    if [ ! -f "$tb_file" ]; then
        echo "error: missing Turbo benchmark fixture: $tb_file" >&2
        failures=$((failures + 1))
        continue
    fi

    turbo_out="$BUILD_DIR/$bench.turbo.out"
    turbo_err="$BUILD_DIR/$bench.turbo.err"
    if ! "$TURBO" run "$tb_file" >"$turbo_out" 2>"$turbo_err"; then
        printf "%-10s %-10s FAIL turbo-exit\n" "turbo" "$bench"
        sed 's/^/  stderr: /' "$turbo_err" >&2
        failures=$((failures + 1))
        continue
    fi

    for lang in $LANGS; do
        runtime="$(runtime_for_lang "$lang")" || {
            failures=$((failures + 1))
            continue
        }
        src="$(source_for_lang "$lang" "$bench")"
        if [ ! -f "$src" ]; then
            printf "%-10s %-10s SKIP missing-source\n" "$lang" "$bench"
            if [ "$REQUIRE_ALL" = "1" ]; then
                failures=$((failures + 1))
            else
                skipped=$((skipped + 1))
            fi
            continue
        fi

        if ! command -v "$runtime" >/dev/null 2>&1; then
            printf "%-10s %-10s SKIP missing-runtime:%s\n" "$lang" "$bench" "$runtime"
            if [ "$REQUIRE_ALL" = "1" ]; then
                failures=$((failures + 1))
            else
                skipped=$((skipped + 1))
            fi
            continue
        fi

        external_out="$BUILD_DIR/$bench.$lang.out"
        external_err="$BUILD_DIR/$bench.$lang.err"
        if ! run_external "$lang" "$bench" "$src" "$runtime" "$external_out" "$external_err"; then
            printf "%-10s %-10s FAIL runtime-exit\n" "$lang" "$bench"
            sed 's/^/  stderr: /' "$external_err" >&2
            failures=$((failures + 1))
            continue
        fi

        if cmp -s "$turbo_out" "$external_out"; then
            printf "%-10s %-10s PASS\n" "$lang" "$bench"
            ran=$((ran + 1))
        else
            printf "%-10s %-10s FAIL output-mismatch\n" "$lang" "$bench"
            echo "  expected Turbo output:" >&2
            sed 's/^/    /' "$turbo_out" >&2
            echo "  external output:" >&2
            sed 's/^/    /' "$external_out" >&2
            failures=$((failures + 1))
        fi
    done
done

echo ""
printf "External baselines: %d ran, %d skipped, %d failed\n" "$ran" "$skipped" "$failures"

if [ "$failures" -ne 0 ]; then
    exit 1
fi

if [ "$ran" -eq 0 ] && [ "$ALLOW_EMPTY" != "1" ]; then
    echo "external benchmark smoke failed: no external baselines ran" >&2
    exit 1
fi
