#!/bin/bash
# Turbo Language Demo Runner
# Shows the compiler in action across different program types

set -e

TURBO="cargo run --quiet --"
PASS=0
FAIL=0
TESTS=()

divider() {
    printf "\n\033[90m%.0s─\033[0m" {1..60}
    echo
}

run_demo() {
    local file="$1"
    local desc="$2"
    local name=$(basename "$file" .tb)

    divider
    printf "\033[1;36m▶ %s\033[0m  \033[90m(%s)\033[0m\n" "$desc" "$file"
    echo

    # Show the source code
    printf "\033[33m"
    cat "$file"
    printf "\033[0m\n"

    # Run it
    printf "\033[90m→ Output:\033[0m\n"
    if output=$($TURBO run "$file" 2>&1); then
        printf "\033[32m%s\033[0m\n" "$output"
        PASS=$((PASS + 1))
    else
        printf "\033[31m%s\033[0m\n" "$output"
        FAIL=$((FAIL + 1))
    fi
}

run_error_demo() {
    local file="$1"
    local desc="$2"

    divider
    printf "\033[1;36m▶ %s\033[0m  \033[90m(should error)\033[0m\n" "$desc"
    echo

    printf "\033[33m"
    cat "$file"
    printf "\033[0m\n"

    printf "\033[90m→ Compiler output:\033[0m\n"
    if output=$($TURBO run "$file" 2>&1); then
        printf "\033[31mERROR: Should have failed but didn't!\033[0m\n"
        FAIL=$((FAIL + 1))
    else
        printf "\033[32m%s\033[0m\n" "$output"
        PASS=$((PASS + 1))
    fi
}

# ── Header ──────────────────────────────────────────────

echo
printf "\033[1;35m  ╔══════════════════════════════════════╗\033[0m\n"
printf "\033[1;35m  ║     TURBO LANGUAGE — DEMO RUNNER     ║\033[0m\n"
printf "\033[1;35m  ╚══════════════════════════════════════╝\033[0m\n"

# ── Build first ─────────────────────────────────────────

printf "\n\033[90mBuilding compiler...\033[0m "
cargo build --quiet 2>&1
printf "\033[32m✓\033[0m\n"

# ── Working Programs ────────────────────────────────────

run_demo tests/phase1/hello.tb "Hello World"
run_demo tests/phase1/fizzbuzz.tb "FizzBuzz (loops, conditionals, modulo)"
run_demo tests/phase1/fibonacci.tb "Fibonacci Sequence (recursion)"
run_demo tests/phase1/primes.tb "Prime Sieve (while loops, early return)"
run_demo tests/phase1/collatz.tb "Collatz Conjecture (mutation, while)"
run_demo tests/phase1/gcd.tb "GCD & LCM (Euclidean algorithm)"
run_demo tests/phase1/math_utils.tb "Math Utilities (abs, max, min, clamp, pow)"
run_demo tests/phase1/recursion.tb "Factorial (10! = 3628800)"
run_demo tests/phase1/short_circuit.tb "Short-Circuit Evaluation (&&, ||)"

# ── Error Detection Demos ──────────────────────────────

# Create temp error test files
TMP=$(mktemp -d)

cat > "$TMP/type_error.tb" << 'EOF'
fn main() {
    let x = "hello"
    let y = x + 1
}
EOF

cat > "$TMP/immutable.tb" << 'EOF'
fn main() {
    let x = 42
    x = 99
}
EOF

cat > "$TMP/undefined.tb" << 'EOF'
fn main() {
    print(oops)
}
EOF

cat > "$TMP/wrong_args.tb" << 'EOF'
fn add(a: i64, b: i64) -> i64 { a + b }
fn main() {
    add(1, 2, 3)
}
EOF

cat > "$TMP/wrong_type.tb" << 'EOF'
fn double(x: i64) -> i64 { x * 2 }
fn main() {
    double("hello")
}
EOF

cat > "$TMP/return_mismatch.tb" << 'EOF'
fn get_name() -> i64 {
    "alice"
}
fn main() { get_name() }
EOF

run_error_demo "$TMP/type_error.tb" "Type Error: arithmetic on strings"
run_error_demo "$TMP/immutable.tb" "Immutability: can't assign to let binding"
run_error_demo "$TMP/undefined.tb" "Undefined Variable"
run_error_demo "$TMP/wrong_args.tb" "Wrong Argument Count"
run_error_demo "$TMP/wrong_type.tb" "Wrong Argument Type"
run_error_demo "$TMP/return_mismatch.tb" "Return Type Mismatch"

rm -rf "$TMP"

# ── Summary ─────────────────────────────────────────────

divider
echo
printf "\033[1m  Results: \033[32m%d passed\033[0m" "$PASS"
if [ "$FAIL" -gt 0 ]; then
    printf ", \033[31m%d failed\033[0m" "$FAIL"
fi
echo
echo
