"""
Python benchmark suite.

5 benchmarks printing JSON results.
Usage: python3 benchmark.py <benchmark_name|all>
"""

import sys
import json
import time
import hashlib
import multiprocessing


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def emit(name, elapsed_ms, result):
    """Print a single benchmark result as JSON."""
    print(json.dumps({
        "language": "python",
        "benchmark": name,
        "time_ms": round(elapsed_ms, 4),
        "result": str(result),
    }))


# ---------------------------------------------------------------------------
# 1. fib  --  naive recursive fibonacci(40)
# ---------------------------------------------------------------------------

def _fib(n):
    if n < 2:
        return n
    return _fib(n - 1) + _fib(n - 2)


def bench_fib():
    start = time.perf_counter()
    result = _fib(40)
    elapsed = (time.perf_counter() - start) * 1000.0
    emit("fib", elapsed, result)


# ---------------------------------------------------------------------------
# 2. trees  --  binary tree depth 21, allocate as tuples, compute checksum
# ---------------------------------------------------------------------------

def _make_tree(depth):
    if depth == 0:
        return (None, None)
    depth -= 1
    return (_make_tree(depth), _make_tree(depth))


def _check_tree(node):
    left, right = node
    if left is None:
        return 1
    return 1 + _check_tree(left) + _check_tree(right)


def bench_trees():
    depth = 21
    start = time.perf_counter()
    tree = _make_tree(depth)
    checksum = _check_tree(tree)
    elapsed = (time.perf_counter() - start) * 1000.0
    emit("trees", elapsed, checksum)


# ---------------------------------------------------------------------------
# 3. matrix  --  1000x1000 float matrix multiply (pure Python, no numpy)
# ---------------------------------------------------------------------------

def bench_matrix():
    n = 1000
    # Build two deterministic matrices using simple formulas
    a = [[(i * n + j) * 1.0e-6 for j in range(n)] for i in range(n)]
    b = [[(j * n + i) * 1.0e-6 for j in range(n)] for i in range(n)]

    start = time.perf_counter()

    # Transpose b for cache-friendly access
    bt = list(map(list, zip(*b)))

    c = [[0.0] * n for _ in range(n)]
    for i in range(n):
        ai = a[i]
        for j in range(n):
            btj = bt[j]
            s = 0.0
            for k in range(n):
                s += ai[k] * btj[k]
            c[i][j] = s

    elapsed = (time.perf_counter() - start) * 1000.0
    emit("matrix", elapsed, c[0][0])


# ---------------------------------------------------------------------------
# 4. strings  --  1 MB deterministic ASCII: word count, count "the", reverse,
#                 SHA-256 hash.  Result = word count.
# ---------------------------------------------------------------------------

def bench_strings():
    # Build a deterministic 1 MB ASCII string using an LCG
    target_len = 1_000_000
    words = [
        "the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog",
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta",
        "one", "two", "three", "four", "five", "six", "seven", "eight",
        "nine", "ten", "eleven", "twelve", "hello", "world", "benchmark",
        "test",
    ]
    seed = 42
    a_lcg = 1103515245
    c_lcg = 12345
    m_lcg = 2 ** 31
    parts = []
    length = 0
    while length < target_len:
        seed = (a_lcg * seed + c_lcg) % m_lcg
        word = words[seed % len(words)]
        parts.append(word)
        length += len(word) + 1  # +1 for the space
    text = " ".join(parts)
    # Trim to exactly target_len
    text = text[:target_len]

    start = time.perf_counter()

    word_count = len(text.split())
    the_count = text.split().count("the")
    reversed_text = text[::-1]
    h = hashlib.sha256(reversed_text.encode("ascii")).hexdigest()
    # Force use of computed values so nothing is optimised away
    _ = (the_count, h)

    elapsed = (time.perf_counter() - start) * 1000.0
    emit("strings", elapsed, word_count)


# ---------------------------------------------------------------------------
# 5. concurrent  --  multiprocessing Pool, 1000 tasks each computing fib(30)
#                     Result = sum of all results.
# ---------------------------------------------------------------------------

def _fib_worker(n):
    """Standalone top-level function so it is picklable."""
    if n < 2:
        return n
    a, b = 0, 1
    for _ in range(n - 1):
        a, b = b, a + b
    return b


def bench_concurrent():
    num_tasks = 1000
    fib_n = 30

    start = time.perf_counter()
    with multiprocessing.Pool() as pool:
        results = pool.map(_fib_worker, [fib_n] * num_tasks)
    total = sum(results)
    elapsed = (time.perf_counter() - start) * 1000.0
    emit("concurrent", elapsed, total)


# ---------------------------------------------------------------------------
# CLI dispatcher
# ---------------------------------------------------------------------------

BENCHMARKS = {
    "fib": bench_fib,
    "trees": bench_trees,
    "matrix": bench_matrix,
    "strings": bench_strings,
    "concurrent": bench_concurrent,
}


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <{'|'.join(BENCHMARKS)}|all>", file=sys.stderr)
        sys.exit(1)

    target = sys.argv[1].lower()

    if target == "all":
        for fn in BENCHMARKS.values():
            fn()
    elif target in BENCHMARKS:
        BENCHMARKS[target]()
    else:
        print(f"Unknown benchmark: {target}", file=sys.stderr)
        print(f"Available: {', '.join(BENCHMARKS)} or 'all'", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
