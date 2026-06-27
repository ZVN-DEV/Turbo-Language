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

## Artifact Policy

Do not commit generated benchmark executables. The release consistency check
flags tracked binary artifacts under language baseline directories, and
`turbo/.gitignore` ignores local benchmark binaries produced during manual
experiments.

Tracked files should be source, scripts, or documentation only.
