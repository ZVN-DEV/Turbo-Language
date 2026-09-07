#!/usr/bin/env bash
#
# Compile and run the turbo_rt C runtime tests.
#
# Usage:
#     bash turbo/crates/turbo-codegen-cranelift/runtime/tests.sh
#
# Exits non-zero on any compile error or test failure.
#
# The runtime is built with -DTURBO_WITH_SQLITE so turbo_rt.c pulls in the
# SQLite shim (turbo_rt_sqlite.c), giving the C twins of the sqlite_* builtins
# real coverage. The vendored amalgamation is compiled once (with relaxed
# warnings — it is upstream code, not ours) and linked into the AddressSanitizer
# build so the shim's copy semantics are checked for heap-overflow / UAF.

set -euo pipefail

# Resolve the directory this script lives in so it works no matter
# where the user invokes it from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

CC="${CC:-cc}"
test_dir="$(mktemp -d)"
trap 'rm -rf "$test_dir"' EXIT

OUT="$test_dir/test_rt"
SANITIZE_FLAGS=(-fsanitize=address -fno-omit-frame-pointer)

# SQLite compile flags — keep in sync with build.rs and src/lib.rs SQLITE_CFLAGS.
SQLITE_FLAGS=(
    -DSQLITE_THREADSAFE=1
    -DSQLITE_OMIT_LOAD_EXTENSION
    -DSQLITE_OMIT_DEPRECATED
    -DSQLITE_DQS=0
    -DSQLITE_DEFAULT_MEMSTATUS=0
    -DSQLITE_OMIT_SHARED_CACHE
)

# 1) Compile the vendored SQLite amalgamation once (relaxed warnings — it is
#    generated upstream code, not warning-clean under -Werror).
SQLITE_O="$test_dir/sqlite3.o"
echo "== compiling vendored sqlite3.c =="
"$CC" -O2 -fPIC "${SQLITE_FLAGS[@]}" -c vendor/sqlite3.c -o "$SQLITE_O"

# 2) Build the test harness + runtime (with the SQLite shim pulled in) under
#    AddressSanitizer and run it.
# -std=c11    : portable C11
# -Wall -Wextra -Werror : warnings are errors so we catch new sloppy code
# -Wno-unused-function / -Wno-unused-parameter : turbo_rt has cold helpers
# -DRT_TEST_BUILD : forward-compat hook to opt out of things during testing
# -DTURBO_WITH_SQLITE / -Ivendor : pull in + resolve the SQLite shim + header
echo "== compiling turbo_rt + tests with $CC + ASan =="
"$CC" \
    -std=c11 \
    -Wall -Wextra -Werror \
    -Wno-unused-function -Wno-unused-parameter \
    "${SANITIZE_FLAGS[@]}" \
    -DRT_TEST_BUILD \
    -DTURBO_WITH_SQLITE \
    -Ivendor \
    -o "$OUT" \
    tests/test_rt.c \
    turbo_rt.c \
    "$SQLITE_O" \
    -lpthread -lm

echo "== running $OUT =="
# detect_leaks=0: the suite intentionally leaves arena strings / results
# allocated (we test memory *safety* — use-after-free, overflow — not leaks;
# and macOS LSan is unsupported anyway). halt_on_error=1 fails fast.
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:halt_on_error=1}" "$OUT"
echo "== compiling instrumented runtime calibration with ASan =="
"$CC" -std=c11 -Wall -Wextra -Werror -Wno-unused-function -Wno-unused-parameter \
    "${SANITIZE_FLAGS[@]}" -DRT_TEST_BUILD -DTURBO_WITH_SQLITE -DTURBO_ALLOCATION_PROFILE \
    -Ivendor -o "$test_dir/test_rt_profile" tests/test_rt.c turbo_rt.c \
    turbo_alloc_profile.c "$SQLITE_O" -lpthread -lm
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:halt_on_error=1}" "$test_dir/test_rt_profile"

echo "== allocation observer invariants + concurrency (ASan/UBSan) =="
"$CC" -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -fno-omit-frame-pointer \
    tests/test_alloc_profile.c turbo_alloc_profile.c -lpthread -o "$test_dir/test_alloc_profile"
ASAN_OPTIONS="${ASAN_OPTIONS:-detect_leaks=0:halt_on_error=1}" "$test_dir/test_alloc_profile"
echo "== c-runtime-tests OK =="
