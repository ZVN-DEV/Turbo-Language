#!/usr/bin/env bash
# Real-world benchmark: word-frequency count over a generated multi-MB text file.
#
# This mirrors run_comparison.sh's conventions (best-of-N wall-clock, a warmup
# run, source-driven native baselines compiled into a temp dir) but for an
# end-to-end workload: read a file, tokenize on whitespace, count word
# frequencies in a hashmap, and print the top-20 words plus a TOTAL summary.
#
# Turbo (AOT and JIT), C (-O2), Rust (-O), and Go (build) all implement the
# identical algorithm over the identical, deterministically generated input.
# The runner ENFORCES byte-for-byte output equality across every language that
# is available; any mismatch fails the run. Missing toolchains are skipped
# gracefully (reported as N/A) rather than faked.
#
# Tunables (env):
#   TURBO_BENCH_ITERS        best-of-N iterations            (default 5)
#   WORDCOUNT_MB             input size in megabytes          (default 5)
#   TURBO_BENCH_BUILD_DIR    where to compile/generate        (default mktemp)
#   TURBO_BENCH_KEEP_BUILD_DIR  keep the build dir (1/0)      (default 0)
#   TURBO_BENCH_RUN_JIT      also time `turbolang run` (1/0)  (default 1)
set -euo pipefail

TURBO="$(cd "$(dirname "$0")/.." && pwd)/target/release/turbolang"
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BENCH_DIR="$(cd "$(dirname "$0")" && pwd)"
C_SRC="$BENCH_DIR/c/wordcount.c"
RUST_SRC="$BENCH_DIR/rust/wordcount.rs"
GO_SRC="$BENCH_DIR/go/wordcount.go"
TB_SRC="$BENCH_DIR/wordcount.tb"
GEN="$BENCH_DIR/gen_wordcount_input.py"

ITERATIONS="${TURBO_BENCH_ITERS:-5}"
WORDCOUNT_MB="${WORDCOUNT_MB:-5}"
RUN_JIT="${TURBO_BENCH_RUN_JIT:-1}"
BUILD_DIR="${TURBO_BENCH_BUILD_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/turbo-wordcount.XXXXXX")}"
KEEP_BUILD_DIR="${TURBO_BENCH_KEEP_BUILD_DIR:-0}"

if [ "$KEEP_BUILD_DIR" != "1" ]; then
    trap 'rm -rf "$BUILD_DIR"' EXIT
fi

if [ ! -x "$TURBO" ]; then
    echo "Building compiler (release)..." >&2
    cargo build --release --manifest-path "$PROJECT_DIR/Cargo.toml" -p turbo-cli >/dev/null
fi

mkdir -p "$BUILD_DIR/out"

INPUT="$BUILD_DIR/wordcount_input.txt"
echo "Generating ${WORDCOUNT_MB} MB deterministic input -> $INPUT" >&2
python3 "$GEN" "$INPUT" "$WORDCOUNT_MB"
INPUT_BYTES=$(wc -c < "$INPUT" | tr -d ' ')
export WORDCOUNT_INPUT="$INPUT"

time_ms() {
    local out_file="$1"
    shift
    local start end
    start=$(python3 -c "import time; print(int(time.time()*1000))")
    "$@" > "$out_file" 2>"$out_file.err"
    end=$(python3 -c "import time; print(int(time.time()*1000))")
    echo $(( end - start ))
}

best_of_n() {
    # Warmup run (not timed), then best-of-N timed runs.
    local out_file="$1"
    shift
    "$@" > "$out_file" 2>"$out_file.err" || true
    local best=999999999
    local best_output="$BUILD_DIR/out/best.$$"
    local run
    for run in $(seq 1 "$ITERATIONS"); do
        local candidate="$BUILD_DIR/out/run.$$.$run"
        local ms
        ms=$(time_ms "$candidate" "$@")
        if [ "$ms" -lt "$best" ]; then
            best="$ms"
            cp "$candidate" "$best_output"
        fi
    done
    cp "$best_output" "$out_file"
    echo "$best"
}

format_ms() {
    local value="$1"
    if [ -z "$value" ]; then
        printf "%10s" "N/A"
    else
        printf "%8s ms" "$value"
    fi
}

ratio() {
    local turbo_ms="$1"
    local baseline_ms="$2"
    if [ -z "$turbo_ms" ] || [ -z "$baseline_ms" ] || [ "$baseline_ms" -le 0 ] 2>/dev/null; then
        printf "%10s" "N/A"
    else
        python3 -c "import sys; sys.stdout.write(f'{$turbo_ms/$baseline_ms:>9.2f}x')"
    fi
}

FAILURES=0
REFERENCE=""        # path to the agreed reference output
REFERENCE_LABEL=""

check_output() {
    # Compare $1 (label) output file $2 against the reference; record mismatch.
    local label="$1"
    local out_file="$2"
    if [ -z "$REFERENCE" ]; then
        REFERENCE="$out_file"
        REFERENCE_LABEL="$label"
        return
    fi
    if ! cmp -s "$REFERENCE" "$out_file"; then
        echo "OUTPUT MISMATCH: $label disagrees with $REFERENCE_LABEL" >&2
        echo "  --- $REFERENCE_LABEL (first 5 lines) ---" >&2
        head -5 "$REFERENCE" | sed 's/^/    /' >&2
        echo "  --- $label (first 5 lines) ---" >&2
        head -5 "$out_file" | sed 's/^/    /' >&2
        FAILURES=$((FAILURES + 1))
    fi
}

echo
echo "Word-count benchmark — input ${INPUT_BYTES} bytes (~${WORDCOUNT_MB} MB), best of ${ITERATIONS}"
echo

# ── Turbo AOT (the fair native comparison) ───────────────────────────────────
aot_bin="$BUILD_DIR/wordcount-aot"
"$TURBO" build "$TB_SRC" -o "$aot_bin" >/dev/null 2>&1
aot_out="$BUILD_DIR/out/turbo-aot.txt"
turbo_aot_ms=$(best_of_n "$aot_out" "$aot_bin")
check_output "turbo-aot" "$aot_out"

# ── Turbo JIT (optional) ─────────────────────────────────────────────────────
turbo_jit_ms=""
if [ "$RUN_JIT" = "1" ]; then
    jit_out="$BUILD_DIR/out/turbo-jit.txt"
    turbo_jit_ms=$(best_of_n "$jit_out" "$TURBO" run "$TB_SRC")
    check_output "turbo-jit" "$jit_out"
fi

# ── C (-O2) ──────────────────────────────────────────────────────────────────
c_ms=""
if command -v cc >/dev/null 2>&1 && [ -f "$C_SRC" ]; then
    if cc -O2 -o "$BUILD_DIR/wordcount-c" "$C_SRC" 2>"$BUILD_DIR/cc.err"; then
        c_out="$BUILD_DIR/out/c.txt"
        c_ms=$(best_of_n "$c_out" "$BUILD_DIR/wordcount-c" "$INPUT")
        check_output "c" "$c_out"
    else
        echo "C: compile failed (see $BUILD_DIR/cc.err)" >&2
    fi
else
    echo "C: skipped (no cc)" >&2
fi

# ── Rust (-O) ────────────────────────────────────────────────────────────────
rust_ms=""
if command -v rustc >/dev/null 2>&1 && [ -f "$RUST_SRC" ]; then
    if rustc -O "$RUST_SRC" -o "$BUILD_DIR/wordcount-rust" 2>"$BUILD_DIR/rustc.err"; then
        rust_out="$BUILD_DIR/out/rust.txt"
        rust_ms=$(best_of_n "$rust_out" "$BUILD_DIR/wordcount-rust" "$INPUT")
        check_output "rust" "$rust_out"
    else
        echo "Rust: compile failed (see $BUILD_DIR/rustc.err)" >&2
    fi
else
    echo "Rust: skipped (no rustc)" >&2
fi

# ── Go (build) ───────────────────────────────────────────────────────────────
go_ms=""
if command -v go >/dev/null 2>&1 && [ -f "$GO_SRC" ]; then
    # Build outside any module by giving go a throwaway GOPATH/GOCACHE under the build dir.
    go_build_dir="$BUILD_DIR/go-build"
    mkdir -p "$go_build_dir"
    cp "$GO_SRC" "$go_build_dir/main.go"
    if ( cd "$go_build_dir" && GO111MODULE=off GOFLAGS= go build -o "$BUILD_DIR/wordcount-go" main.go ) 2>"$BUILD_DIR/go.err"; then
        go_out="$BUILD_DIR/out/go.txt"
        go_ms=$(best_of_n "$go_out" "$BUILD_DIR/wordcount-go" "$INPUT")
        check_output "go" "$go_out"
    else
        echo "Go: compile failed (see $BUILD_DIR/go.err)" >&2
    fi
else
    echo "Go: skipped (no go)" >&2
fi

# ── Results table ────────────────────────────────────────────────────────────
echo
printf "%-22s %12s %12s\n" "Language" "Best (ms)" "vs C"
printf "%-22s %12s %12s\n" "----------------------" "------------" "------------"

print_row() {
    local label="$1" ms="$2"
    printf "%-22s " "$label"
    format_ms "$ms"
    printf " "
    ratio "$ms" "$c_ms"
    printf "\n"
}

[ -n "$c_ms" ]    && print_row "C (clang -O2)" "$c_ms"
[ -n "$rust_ms" ] && print_row "Rust (rustc -O)" "$rust_ms"
print_row "Turbo (AOT, Cranelift)" "$turbo_aot_ms"
[ -n "$go_ms" ]   && print_row "Go (go build)" "$go_ms"
[ -n "$turbo_jit_ms" ] && print_row "Turbo (JIT)" "$turbo_jit_ms"

echo
echo "Reference output ($REFERENCE_LABEL):"
sed 's/^/  /' "$REFERENCE"

if [ "$KEEP_BUILD_DIR" = "1" ]; then
    echo
    echo "Kept build directory: $BUILD_DIR"
fi

echo
if [ "$FAILURES" -ne 0 ]; then
    echo "FAIL: $FAILURES output mismatch(es) across languages" >&2
    exit 1
fi
echo "OK: all available languages produced identical output."
