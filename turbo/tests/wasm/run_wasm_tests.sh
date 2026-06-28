#!/usr/bin/env bash
#
# WASM execution test runner for TurboLang.
#
# For each `foo.tb` in this directory with a matching `foo.expected`, this
# script:
#   1. Builds it to a wasm32-wasi module via `turbolang build --target wasm`.
#   2. Runs the module under `wasmtime`.
#   3. Diffs captured stdout against `foo.expected`.
#
# This complements the native integration tests in ../run_tests.sh (which it
# deliberately does NOT touch) by exercising the separate WASM C-transpiler
# backend + turbo_rt_wasm.c runtime end to end.
#
# Graceful skipping:
#   * If `wasmtime` is not on PATH, every test is SKIPPED and the script
#     exits 0 — the WASM runtime is optional tooling.
#   * If the WASM build toolchain (wasm-ld / WASM-capable clang / WASI
#     sysroot) is not installed, tests are SKIPPED rather than failed.
#
# Usage:
#   bash turbo/tests/wasm/run_wasm_tests.sh [--verbose]
#
# Exits non-zero only on a real failure (build error that is not a missing
# toolchain, a wasmtime crash, or an stdout mismatch).

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# turbo/ project dir is two levels up (turbo/tests/wasm -> turbo).
TURBO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Allow overriding the compiler binary; default to the release build.
TURBO="${TURBO:-$TURBO_DIR/target/release/turbolang}"

VERBOSE=0
for arg in "$@"; do
    case "$arg" in
        --verbose|-v) VERBOSE=1 ;;
        *) ;;
    esac
done

PASS=0
FAIL=0
SKIP=0
FAILED_TESTS=""

echo "== TurboLang WASM execution tests =="

# Gate 1: wasmtime must be available to run the modules.
if ! command -v wasmtime >/dev/null 2>&1; then
    echo "SKIP: wasmtime not found on PATH — skipping all WASM execution tests."
    echo "      (install with e.g. \`brew install wasmtime\`)"
    exit 0
fi

# The compiler binary must exist (build it first with:
#   cargo build --release --manifest-path turbo/Cargo.toml ).
if [ ! -x "$TURBO" ]; then
    echo "ERROR: turbolang release binary not found at $TURBO"
    echo "       Build it: cargo build --release --manifest-path turbo/Cargo.toml"
    exit 1
fi

TMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/turbo-wasm-tests.XXXXXX")"
trap 'rm -rf "$TMP_ROOT"' EXIT

# True if the build error text indicates the WASM toolchain is simply not
# installed (as opposed to a real compile failure we should surface).
toolchain_missing() {
    printf '%s' "$1" | grep -qiE \
        "wasm-ld not found|WASM-capable clang not found|WASI sysroot not found"
}

shopt -s nullglob
tb_files=("$SCRIPT_DIR"/*.tb)
if [ ${#tb_files[@]} -eq 0 ]; then
    echo "No .tb files found in $SCRIPT_DIR"
    exit 0
fi

for tb in "${tb_files[@]}"; do
    name="$(basename "$tb" .tb)"
    expected="${tb%.tb}.expected"

    if [ ! -f "$expected" ]; then
        echo "  [SKIP] $name (no .expected file)"
        SKIP=$((SKIP + 1))
        continue
    fi

    wasm_out="$TMP_ROOT/$name.wasm"
    build_log="$("$TURBO" build "$tb" --target wasm --output "$wasm_out" 2>&1)"
    build_status=$?

    if [ $build_status -ne 0 ]; then
        if toolchain_missing "$build_log"; then
            echo "  [SKIP] $name (WASM toolchain not installed)"
            SKIP=$((SKIP + 1))
            continue
        fi
        echo "  [FAIL] $name (build error)"
        [ "$VERBOSE" = "1" ] && printf '%s\n' "$build_log" | sed 's/^/          /'
        FAIL=$((FAIL + 1))
        FAILED_TESTS="$FAILED_TESTS $name"
        continue
    fi

    if [ ! -f "$wasm_out" ]; then
        echo "  [FAIL] $name (no wasm output produced)"
        FAIL=$((FAIL + 1))
        FAILED_TESTS="$FAILED_TESTS $name"
        continue
    fi

    actual="$(wasmtime "$wasm_out" 2>/dev/null)"
    run_status=$?
    expected_content="$(cat "$expected")"

    if [ $run_status -ne 0 ]; then
        echo "  [FAIL] $name (wasmtime exited $run_status)"
        FAIL=$((FAIL + 1))
        FAILED_TESTS="$FAILED_TESTS $name"
        continue
    fi

    if [ "$actual" = "$expected_content" ]; then
        echo "  [PASS] $name"
        PASS=$((PASS + 1))
    else
        echo "  [FAIL] $name (stdout mismatch)"
        if [ "$VERBOSE" = "1" ]; then
            printf "        === expected ===\n%s\n" "$expected_content" | sed 's/^/          /'
            printf "        === actual ===\n%s\n" "$actual" | sed 's/^/          /'
        fi
        FAIL=$((FAIL + 1))
        FAILED_TESTS="$FAILED_TESTS $name"
    fi
done

echo ""
echo "== WASM tests: $PASS passed, $FAIL failed, $SKIP skipped =="
if [ $FAIL -ne 0 ]; then
    echo "Failed:$FAILED_TESTS"
    exit 1
fi
exit 0
