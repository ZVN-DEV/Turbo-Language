#!/usr/bin/env bash
#
# Compile and run the turbo_rt C runtime tests.
#
# Usage:
#     bash turbo/crates/turbo-codegen-cranelift/runtime/tests.sh
#
# Exits non-zero on any compile error or test failure.

set -euo pipefail

# Resolve the directory this script lives in so it works no matter
# where the user invokes it from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

CC="${CC:-cc}"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

OUT="$TMPDIR/test_rt"

echo "== compiling turbo_rt + tests with $CC =="

# -std=c11    : portable C11
# -Wall -Wextra : warnings on
# -Werror     : warnings are errors so we catch new sloppy code
# -Wno-unused-function / -Wno-unused-parameter : turbo_rt has a few
#               cold helpers that aren't reached by every test build
# -DRT_TEST_BUILD : forward-compat hook in case the runtime ever wants
#                   to opt out of something during testing
# -lpthread / -lm : turbo_rt uses pthreads and math
"$CC" \
    -std=c11 \
    -Wall -Wextra -Werror \
    -Wno-unused-function -Wno-unused-parameter \
    -DRT_TEST_BUILD \
    -o "$OUT" \
    tests/test_rt.c \
    turbo_rt.c \
    -lpthread -lm

echo "== running $OUT =="
"$OUT"
echo "== c-runtime-tests OK =="
