#!/usr/bin/env bash
# JIT <-> AOT parity test runner.
#
# For each .tb file in tests/parity/programs, this script:
#   1. Runs it through the JIT (`turbolang run`)
#   2. Compiles it to a native binary (`turbolang build`) and runs that
#   3. Diffs the two stdouts (and stderrs, if a failure-path test)
#
# Default contract (success path): both must exit with status 0 and
# produce byte-identical stdout. Stderr is ignored.
#
# Failure-path contract (opt-in via sibling files): if a sibling file
# `<program>.expected_exit` exists next to `<program>.tb`, the harness
# switches into failure-path mode for that program:
#   - `<program>.expected_exit` (required): single integer, the exit
#      code BOTH the JIT and AOT runs must produce.
#   - `<program>.expected_stderr` (optional): a substring that must
#      appear in BOTH the JIT and AOT stderrs (substring match, not
#      regex). Use this to pin error messages so JIT and AOT stay in
#      lockstep on failure diagnostics. If absent, only the exit code
#      is checked. Stderr is NOT byte-compared between JIT and AOT —
#      the two backends legitimately differ on color codes, paths,
#      line numbers, etc. The substring pin is the source of truth.
#
# Expected-divergence (xfail) contract: if a sibling file
# `<program>.expect_divergence` exists next to `<program>.tb`, the
# harness flips the stdout comparison — JIT and AOT stdout MUST differ.
# If they happen to agree, the test fails with "no longer diverges",
# signaling that whatever runtime bug/gap the xfail was pinning has
# been fixed and the xfail should be removed. The `.expect_divergence`
# file's contents (if any) are a human-readable reason string printed
# when the test runs. xfail tests are counted in a separate bucket
# (XFAIL) and do NOT contribute to FAILED. xfail is mutually exclusive
# with failure-path mode.
#
# Usage:
#   tests/parity/run_parity.sh              # run all programs
#   tests/parity/run_parity.sh --verbose    # print both outputs on failure

set -u

# Resolve script dir and turbo root so we can run from anywhere.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TURBO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PROGRAMS_DIR="$SCRIPT_DIR/programs"

# Prefer a release binary if it exists (for speed); fall back to debug.
if [[ -x "$TURBO_ROOT/target/release/turbolang" ]]; then
    TURBOLANG="$TURBO_ROOT/target/release/turbolang"
elif [[ -x "$TURBO_ROOT/target/debug/turbolang" ]]; then
    TURBOLANG="$TURBO_ROOT/target/debug/turbolang"
else
    echo "error: no turbolang binary found in target/release or target/debug" >&2
    echo "Build first: cargo build --manifest-path $TURBO_ROOT/Cargo.toml" >&2
    exit 2
fi

VERBOSE=0
for arg in "$@"; do
    case "$arg" in
        -v|--verbose) VERBOSE=1 ;;
        -h|--help)
            sed -n '2,17p' "$0"
            exit 0 ;;
        *)
            echo "error: unknown argument: $arg" >&2
            exit 2 ;;
    esac
done

# Strip ANSI escape codes so we compare just the semantic output.
strip_ansi() {
    local esc
    esc=$(printf '\033')
    sed -E "s/${esc}\[[0-9;]*m//g"
}

print_diff() {
    local a="$1"
    local b="$2"
    echo "--- jit -------"
    cat "$a"
    echo "--- aot -------"
    cat "$b"
    echo "--- diff ------"
    diff -u "$a" "$b" || true
    echo "---------------"
}

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

PASSED=0
FAILED=0
XFAIL=0
FAILED_NAMES=()
XFAIL_NAMES=()

# Shell glob is sorted by default; keep the order deterministic.
shopt -s nullglob
programs=("$PROGRAMS_DIR"/*.tb)
shopt -u nullglob

if [[ ${#programs[@]} -eq 0 ]]; then
    echo "error: no .tb programs found in $PROGRAMS_DIR" >&2
    exit 2
fi

for src in "${programs[@]}"; do
    name=$(basename "$src" .tb)
    jit_out="$WORK_DIR/$name.jit.out"
    jit_err="$WORK_DIR/$name.jit.err"
    aot_out="$WORK_DIR/$name.aot.out"
    aot_err="$WORK_DIR/$name.aot.err"
    aot_bin="$WORK_DIR/$name.bin"

    # Optional failure-path metadata (see header comment).
    expected_exit_file="${src%.tb}.expected_exit"
    expected_stderr_file="${src%.tb}.expected_stderr"
    expect_divergence_file="${src%.tb}.expect_divergence"
    failure_mode=0
    xfail_mode=0
    xfail_reason=""
    expected_exit=0
    expected_stderr_pattern=""
    if [[ -f "$expected_exit_file" ]]; then
        failure_mode=1
        # Trim whitespace/newlines from the integer.
        expected_exit=$(tr -d '[:space:]' < "$expected_exit_file")
        if [[ -f "$expected_stderr_file" ]]; then
            expected_stderr_pattern=$(cat "$expected_stderr_file")
        fi
    fi
    if [[ -f "$expect_divergence_file" ]]; then
        xfail_mode=1
        xfail_reason=$(cat "$expect_divergence_file")
    fi
    if [[ $failure_mode -eq 1 && $xfail_mode -eq 1 ]]; then
        echo "FAIL $name (cannot combine .expected_exit with .expect_divergence)"
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("$name")
        continue
    fi

    # 1. JIT run.
    jit_exit=0
    "$TURBOLANG" run "$src" > "$jit_out" 2> "$jit_err" || jit_exit=$?
    if [[ $failure_mode -eq 0 ]]; then
        if [[ $jit_exit -ne 0 ]]; then
            echo "FAIL $name (jit exit=$jit_exit)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                cat "$jit_out"
                cat "$jit_err" >&2
            fi
            continue
        fi
    else
        if [[ "$jit_exit" != "$expected_exit" ]]; then
            echo "FAIL $name (jit exit=$jit_exit, expected $expected_exit)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                cat "$jit_err" >&2
            fi
            continue
        fi
        if [[ -n "$expected_stderr_pattern" ]] && ! grep -qF -- "$expected_stderr_pattern" "$jit_err"; then
            echo "FAIL $name (jit stderr missing expected substring)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                echo "expected substring: $expected_stderr_pattern"
                echo "--- jit stderr ---"
                cat "$jit_err"
                echo "------------------"
            fi
            continue
        fi
    fi

    # 2. AOT build.
    build_exit=0
    "$TURBOLANG" build "$src" -o "$aot_bin" > /dev/null 2>&1 || build_exit=$?
    if [[ $build_exit -ne 0 ]]; then
        echo "FAIL $name (aot build exit=$build_exit)"
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("$name")
        continue
    fi

    # 3. AOT run.
    aot_exit=0
    "$aot_bin" > "$aot_out" 2> "$aot_err" || aot_exit=$?
    if [[ $failure_mode -eq 0 ]]; then
        if [[ $aot_exit -ne 0 ]]; then
            echo "FAIL $name (aot binary exit=$aot_exit)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                cat "$aot_out"
                cat "$aot_err" >&2
            fi
            continue
        fi
    else
        if [[ "$aot_exit" != "$expected_exit" ]]; then
            echo "FAIL $name (aot exit=$aot_exit, expected $expected_exit)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                cat "$aot_err" >&2
            fi
            continue
        fi
        if [[ -n "$expected_stderr_pattern" ]] && ! grep -qF -- "$expected_stderr_pattern" "$aot_err"; then
            echo "FAIL $name (aot stderr missing expected substring)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                echo "expected substring: $expected_stderr_pattern"
                echo "--- aot stderr ---"
                cat "$aot_err"
                echo "------------------"
            fi
            continue
        fi
    fi

    # 4. Normalize & diff stdout.
    #
    # Success and failure modes: JIT and AOT stdout must be byte-identical
    # after ANSI stripping. xfail mode: they MUST differ, otherwise the
    # test fails with "no longer diverges".
    #
    # Note: there is intentionally NO cmp -s on stderr here. The only
    # stderr contract is the substring match against .expected_stderr
    # above — JIT and AOT legitimately differ on color codes, file
    # paths, line numbers, etc., and locking them to byte-identity is
    # a maintenance trap.
    jit_norm="$WORK_DIR/$name.jit.norm"
    aot_norm="$WORK_DIR/$name.aot.norm"
    strip_ansi < "$jit_out" > "$jit_norm"
    strip_ansi < "$aot_out" > "$aot_norm"
    if [[ $xfail_mode -eq 1 ]]; then
        if cmp -s "$jit_norm" "$aot_norm"; then
            echo "FAIL $name (xfail: no longer diverges — remove .expect_divergence)"
            FAILED=$((FAILED + 1))
            FAILED_NAMES+=("$name")
            if [[ $VERBOSE -eq 1 ]]; then
                echo "xfail reason was: $xfail_reason"
                echo "--- jit+aot (identical) ---"
                cat "$jit_norm"
                echo "---------------------------"
            fi
            continue
        fi
        echo "XFAIL $name (${xfail_reason:-expected divergence})"
        XFAIL=$((XFAIL + 1))
        XFAIL_NAMES+=("$name")
        continue
    fi
    if ! cmp -s "$jit_norm" "$aot_norm"; then
        echo "FAIL $name (jit/aot stdout differ)"
        FAILED=$((FAILED + 1))
        FAILED_NAMES+=("$name")
        if [[ $VERBOSE -eq 1 ]]; then
            print_diff "$jit_norm" "$aot_norm"
        fi
        continue
    fi

    echo "PASS $name"
    PASSED=$((PASSED + 1))
done

echo
TOTAL=$((PASSED + FAILED + XFAIL))
if [[ $XFAIL -gt 0 ]]; then
    echo "parity: $PASSED passed, $XFAIL xfail, $FAILED failed (of $TOTAL)"
else
    echo "parity: $PASSED passed, $FAILED failed (of $TOTAL)"
fi

if [[ $XFAIL -gt 0 ]]; then
    echo "expected-divergence (xfail) programs:"
    for n in "${XFAIL_NAMES[@]}"; do
        echo "  - $n"
    done
fi

if [[ $FAILED -gt 0 ]]; then
    echo "failing programs:"
    for n in "${FAILED_NAMES[@]}"; do
        echo "  - $n"
    done
    exit 1
fi
exit 0
