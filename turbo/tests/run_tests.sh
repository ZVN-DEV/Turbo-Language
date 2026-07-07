#!/usr/bin/env bash
# Integration test runner for Turbo phase1 / regression / adversarial tests.
#
# For each `foo.tb`, a matching `foo.expected` file declares the expected
# behaviour:
#
#   * If `foo.expected` starts with `ERROR:<pattern>`, the test is an
#     expected-error test. The compiler is expected to exit non-zero and
#     print a line matching `<pattern>` to stderr.
#   * Otherwise the test is a success test. The compiler is expected to exit
#     zero and print stdout that exactly matches the content of
#     `foo.expected`.
#
# Optional `foo.expected_stderr` files are diffed against captured stderr
# when present; if absent, stderr content is not asserted on (except for
# ERROR tests where the pattern match applies).
#
# Flags:
#   --verbose    Print expected/actual diffs on failure (default: brief)
set -euo pipefail

TURBO="./target/release/turbolang"
PASS=0
FAIL=0
SKIP=0
ERRORS=""
VERBOSE=0

for arg in "$@"; do
    case "$arg" in
        --verbose|-v) VERBOSE=1 ;;
        *) ;;
    esac
done

# mktemp wrapper that works on macOS and Linux.
make_tmp() {
    mktemp "${TMPDIR:-/tmp}/turbo-test.XXXXXX"
}

loopback_bind_denied() {
    command -v python3 >/dev/null 2>&1 || return 1
    python3 - <<'PY'
import errno
import socket
import sys

s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
try:
    s.bind(("127.0.0.1", 0))
except PermissionError:
    sys.exit(0)
except OSError as exc:
    sys.exit(0 if exc.errno == errno.EPERM else 1)
else:
    sys.exit(1)
finally:
    s.close()
PY
}

print_diff() {
    local label="$1"
    local expected="$2"
    local actual="$3"
    if [ "$VERBOSE" = "1" ]; then
        printf "        === %s expected ===\n" "$label"
        printf "%s\n" "$expected" | sed 's/^/          /'
        printf "        === %s actual   ===\n" "$label"
        printf "%s\n" "$actual" | sed 's/^/          /'
    fi
}

run_test() {
    local tb_file="$1"
    local expected_file="${tb_file%.tb}.expected"
    local expected_stderr_file="${tb_file%.tb}.expected_stderr"
    local test_name
    test_name=$(basename "$tb_file" .tb)

    if [ ! -f "$expected_file" ]; then
        printf "  SKIP  %s (no .expected file)\n" "$test_name"
        SKIP=$((SKIP + 1))
        return
    fi

    if [ "$test_name" = "http_server" ] && loopback_bind_denied; then
        printf "  SKIP  %s (loopback bind denied by sandbox)\n" "$test_name"
        SKIP=$((SKIP + 1))
        return
    fi

    local expected
    expected=$(cat "$expected_file")

    # Capture stdout and stderr separately, plus the exit code, without
    # letting `set -e` terminate the runner.
    local tmp_stderr
    tmp_stderr=$(make_tmp)
    local actual_stdout
    local exit_code=0
    actual_stdout=$("$TURBO" run "$tb_file" 2>"$tmp_stderr") || exit_code=$?
    local actual_stderr
    actual_stderr=$(cat "$tmp_stderr")
    rm -f "$tmp_stderr"

    # ERROR test path: first line of .expected starts with ERROR:
    if echo "$expected" | head -1 | grep -q "^ERROR:"; then
        local error_pattern
        error_pattern=$(echo "$expected" | head -1 | sed 's/^ERROR://')
        if [ "$exit_code" -eq 0 ]; then
            printf "  FAIL  %s (expected non-zero exit, got 0)\n" "$test_name"
            printf "        expected error: %s\n" "$error_pattern"
            print_diff stdout "$expected" "$actual_stdout"
            FAIL=$((FAIL + 1))
            ERRORS="$ERRORS\n  - $test_name"
            return
        fi
        # Strip ANSI escape sequences from diagnostic output so the grep
        # pattern can match across styled spans.
        local esc
        esc=$(printf '\033')
        local stripped_stderr
        local stripped_stdout
        stripped_stderr=$(printf "%s" "$actual_stderr" | sed -E "s/${esc}\[[0-9;]*m//g")
        stripped_stdout=$(printf "%s" "$actual_stdout" | sed -E "s/${esc}\[[0-9;]*m//g")
        # Diagnostics should appear on stderr, but fall back to stdout for
        # back-compat with older tests that printed errors to stdout.
        if echo "$stripped_stderr" | grep -qi "$error_pattern" \
            || echo "$stripped_stdout" | grep -qi "$error_pattern"; then
            printf "  PASS  %s\n" "$test_name"
            PASS=$((PASS + 1))
        else
            printf "  FAIL  %s\n" "$test_name"
            printf "        expected error: %s\n" "$error_pattern"
            print_diff stderr "$error_pattern" "$stripped_stderr"
            print_diff stdout "" "$stripped_stdout"
            FAIL=$((FAIL + 1))
            ERRORS="$ERRORS\n  - $test_name"
        fi
        return
    fi

    # Non-ERROR test: expect success exit code.
    if [ "$exit_code" -ne 0 ]; then
        printf "  FAIL  %s (exited with %d)\n" "$test_name" "$exit_code"
        print_diff stdout "$expected" "$actual_stdout"
        if [ -n "$actual_stderr" ] && [ "$VERBOSE" = "1" ]; then
            printf "        === stderr ===\n"
            printf "%s\n" "$actual_stderr" | sed 's/^/          /'
        fi
        FAIL=$((FAIL + 1))
        ERRORS="$ERRORS\n  - $test_name"
        return
    fi

    if [ "$actual_stdout" != "$expected" ]; then
        printf "  FAIL  %s\n" "$test_name"
        if [ "$VERBOSE" = "1" ]; then
            print_diff stdout "$expected" "$actual_stdout"
        else
            printf "        expected: %s\n" "$(echo "$expected" | head -3)"
            printf "        actual:   %s\n" "$(echo "$actual_stdout" | head -3)"
        fi
        FAIL=$((FAIL + 1))
        ERRORS="$ERRORS\n  - $test_name"
        return
    fi

    # Optional stderr assertion (opt-in, backward-compatible).
    if [ -f "$expected_stderr_file" ]; then
        local expected_stderr
        expected_stderr=$(cat "$expected_stderr_file")
        if [ "$actual_stderr" != "$expected_stderr" ]; then
            printf "  FAIL  %s (stderr mismatch)\n" "$test_name"
            print_diff stderr "$expected_stderr" "$actual_stderr"
            FAIL=$((FAIL + 1))
            ERRORS="$ERRORS\n  - $test_name"
            return
        fi
    fi

    printf "  PASS  %s\n" "$test_name"
    PASS=$((PASS + 1))
}

echo "Building compiler..."
cargo build --release

echo ""
echo "=== Phase 1 Tests ==="
for f in tests/phase1/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

echo ""
echo "=== Regression Tests ==="
for f in tests/regression/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

echo ""
echo "=== Import Tests ==="
for f in tests/phase1/imports/*.tb; do
    [ -f "$f" ] || continue
    run_test "$f"
done

echo ""
echo "=== Adversarial Tests ==="
for f in tests/adversarial/*.tb; do
    [ -f "$f" ] && run_test "$f"
done

# CLI argument passing can't go through run_test() because that harness always
# invokes `turbolang run <file>` with no extra args. Delegate to the dedicated
# args runner, which exercises `run -- a b c`, trailing args, and a built
# binary, asserting JIT ≡ AOT.
echo ""
echo "=== CLI Args Tests ==="
if tests/args/run_args_tests.sh; then
    PASS=$((PASS + 1))
else
    printf "  FAIL  cli_args\n"
    FAIL=$((FAIL + 1))
    ERRORS="$ERRORS\n  - cli_args (tests/args/run_args_tests.sh)"
fi

echo ""
echo "================================"
printf "Results: %d passed, %d failed, %d skipped\n" "$PASS" "$FAIL" "$SKIP"
if [ "$FAIL" -gt 0 ]; then
    printf "Failed tests:%b\n" "$ERRORS"
    exit 1
else
    echo "All tests passed!"
    exit 0
fi
