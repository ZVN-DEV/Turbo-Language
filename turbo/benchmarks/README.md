# Turbo Benchmark Suite

This directory contains source-only benchmark fixtures for comparing Turbo JIT
and AOT execution against native baselines.

## Commands

```bash
# Build the compiler if needed, then compare JIT and AOT output for every .tb benchmark.
./turbo/benchmarks/run_benchmarks.sh

# Compare Turbo JIT, Turbo AOT, C, and Rust baselines.
./turbo/benchmarks/run_comparison.sh

# Compare Turbo fib output with available Go, Node.js, Python, and Ruby baselines.
./turbo/benchmarks/run_external_baselines.sh

# Run a fast subset while iterating on benchmark tooling.
TURBO_BENCHMARKS=fib TURBO_BENCH_ITERS=1 ./turbo/benchmarks/run_comparison.sh
```

`run_comparison.sh` compiles C and Rust baselines into a temporary directory and
removes that directory on exit. Set `TURBO_BENCH_KEEP_BUILD_DIR=1` to inspect the
generated binaries after a run.

`run_external_baselines.sh` runs source-only external fixtures under
`go/`, `js/`, `python/`, and `ruby/` when the corresponding runtime is available.
It fails on Turbo/runtime exits and output mismatches. Missing runtimes are
reported as skips by default; set `TURBO_BENCH_EXTERNAL_REQUIRE_ALL=1` when a
machine is expected to have every runtime installed. The command fails if every
selected runtime is skipped; set `TURBO_BENCH_EXTERNAL_ALLOW_EMPTY=1` only for
environment-probe jobs where an all-skipped result is acceptable.

Benchmark output comparisons are exact by default, including float benchmarks.
Set `TURBO_BENCH_FLOAT_TOLERANCE=<decimal>` only when intentionally comparing an
approximate external baseline that cannot use Turbo's canonical float display
policy.

## Real-world benchmark: word-count

The benchmarks above are microbenchmarks. `run_wordcount.sh` is a single
end-to-end workload that exercises the language the way real programs do: read a
multi-MB text file, tokenize it on whitespace, count word frequencies in a
hashmap, and print the top-20 words plus a `TOTAL <words> <unique>` summary —
covering file I/O, strings, hashmaps, and sorting.

```bash
# Generate input, warm up, best-of-5, enforce output equality across languages.
./turbo/benchmarks/run_wordcount.sh

# Bigger input, more iterations, keep the build dir to inspect it.
WORDCOUNT_MB=20 TURBO_BENCH_ITERS=9 TURBO_BENCH_KEEP_BUILD_DIR=1 \
  ./turbo/benchmarks/run_wordcount.sh
```

- **Source of truth:** `wordcount.tb` (Turbo) plus identical-algorithm baselines
  in `c/wordcount.c`, `rust/wordcount.rs`, and `go/wordcount.go`. All read the
  same input and must print byte-for-byte identical output or the run fails.
- **Input:** `gen_wordcount_input.py` deterministically generates a Zipf-like
  text file (fixed seed + fixed vocabulary), so the top-N ranking and totals are
  stable and reproducible. The file is generated into a temp build dir, not
  committed.
- **Fair comparison:** Turbo runs via AOT (`turbolang build`) for the native
  comparison; the JIT (`turbolang run`) is also timed. Tunables:
  `WORDCOUNT_MB` (default 5), `TURBO_BENCH_ITERS` (default 5),
  `TURBO_BENCH_RUN_JIT` (default 1).
- Missing toolchains (`cc`, `rustc`, `go`) are skipped gracefully and reported
  as N/A rather than faked.

Measured numbers (best of 5, ~5 MB input, Apple M5 Max / macOS 26.5.1,
2026-06-27): C `~110 ms`, Rust `~125 ms`, Go `~130 ms`, Turbo AOT `~240 ms`
(~2.2x C), Turbo JIT `~220 ms`. On this string/hashmap-heavy workload Turbo
lands further behind C than on fib40; the gap is dominated by runtime
hashmap/string handling, not codegen.

## Artifact Policy

Do not commit generated benchmark executables. The release consistency check
flags tracked binary artifacts under language baseline directories, and
`turbo/.gitignore` ignores local benchmark binaries produced during manual
experiments.

Tracked files should be source, scripts, or documentation only.
