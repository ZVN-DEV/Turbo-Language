#!/usr/bin/env bash
# CLI argument passing tests for the `args()` builtin.
#
# The phase1 harness (run_tests.sh) always invokes `turbolang run <file>` with
# NO extra arguments, so it cannot exercise real arg passing. This dedicated
# runner does, and it asserts JIT ≡ AOT: a program produces the SAME args()
# whether run via `turbolang run f.tb -- a b c` or built and invoked as
# `./bin a b c`.
#
# argv convention under test: args()[0] is the FIRST user argument — the
# binary path (AOT) / the .tb source path (JIT) is excluded.
#
# Usage:
#   tests/args/run_args_tests.sh            # run
#   tests/args/run_args_tests.sh --verbose  # print expected/actual on failure
set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TURBO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

if [[ -x "$TURBO_ROOT/target/release/turbolang" ]]; then
    TURBOLANG="$TURBO_ROOT/target/release/turbolang"
elif [[ -x "$TURBO_ROOT/target/debug/turbolang" ]]; then
    TURBOLANG="$TURBO_ROOT/target/debug/turbolang"
else
    echo "error: no turbolang binary found in target/release or target/debug" >&2
    echo "Build first: cargo build --release --manifest-path $TURBO_ROOT/Cargo.toml" >&2
    exit 2
fi

VERBOSE=0
for arg in "$@"; do
    case "$arg" in
        -v|--verbose) VERBOSE=1 ;;
    esac
done

WORK_DIR=$(mktemp -d)
trap 'rm -rf "$WORK_DIR"' EXIT

PROG="$WORK_DIR/argprint.tb"
cat > "$PROG" <<'EOF'
fn main() {
    let a = args()
    print(len(a))
    let mut i = 0
    while i < len(a) {
        print(a[i])
        i = i + 1
    }
}
EOF

BIN="$WORK_DIR/argprint.bin"
if ! "$TURBOLANG" build "$PROG" -o "$BIN" > /dev/null 2>&1; then
    echo "FAIL build (could not compile argprint.tb to a native binary)" >&2
    exit 1
fi

PASS=0
FAIL=0

# check <name> <expected> <actual>
check() {
    local name="$1" expected="$2" actual="$3"
    if [[ "$actual" == "$expected" ]]; then
        printf "  PASS  %s\n" "$name"
        PASS=$((PASS + 1))
    else
        printf "  FAIL  %s\n" "$name"
        FAIL=$((FAIL + 1))
        if [[ $VERBOSE -eq 1 ]]; then
            printf "        --- expected ---\n%s\n        --- actual ---\n%s\n" \
                "$expected" "$actual"
        fi
    fi
}

# ── Case 1: three plain args, all three surfaces must agree ──────────
EXPECT_THREE=$'3\nalpha\nbeta\ngamma'
check "jit_separator"  "$EXPECT_THREE" "$("$TURBOLANG" run "$PROG" -- alpha beta gamma 2>/dev/null)"
check "jit_trailing"   "$EXPECT_THREE" "$("$TURBOLANG" run "$PROG" alpha beta gamma 2>/dev/null)"
check "aot_binary"     "$EXPECT_THREE" "$("$BIN" alpha beta gamma 2>/dev/null)"

# JIT ≡ AOT: the two backends must be byte-identical.
JIT_THREE="$("$TURBOLANG" run "$PROG" -- alpha beta gamma 2>/dev/null)"
AOT_THREE="$("$BIN" alpha beta gamma 2>/dev/null)"
check "jit_eq_aot_three" "$JIT_THREE" "$AOT_THREE"

# ── Case 2: empty args (the no-arg case must not crash, returns []) ──
EXPECT_EMPTY="0"
check "jit_empty" "$EXPECT_EMPTY" "$("$TURBOLANG" run "$PROG" 2>/dev/null)"
check "aot_empty" "$EXPECT_EMPTY" "$("$BIN" 2>/dev/null)"

# ── Case 3: hyphen-leading args after `--` are passed through ────────
EXPECT_HYPHEN=$'2\n--name\nalice'
check "jit_hyphen" "$EXPECT_HYPHEN" "$("$TURBOLANG" run "$PROG" -- --name alice 2>/dev/null)"
check "aot_hyphen" "$EXPECT_HYPHEN" "$("$BIN" --name alice 2>/dev/null)"

echo
printf "args tests: %d passed, %d failed\n" "$PASS" "$FAIL"
if [[ $FAIL -gt 0 ]]; then
    exit 1
fi
exit 0
