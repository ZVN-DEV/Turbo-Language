# Turbo Benchmark Suite

This directory contains source-only benchmark fixtures for comparing Turbo JIT
and AOT execution against native baselines.

## Commands

```bash
# Build the compiler if needed, then compare JIT and AOT output for every .tb benchmark.
./turbo/benchmarks/run_benchmarks.sh

# Compare Turbo JIT, Turbo AOT, C, and Rust baselines.
./turbo/benchmarks/run_comparison.sh

# Run a fast subset while iterating on benchmark tooling.
TURBO_BENCHMARKS=fib TURBO_BENCH_ITERS=1 ./turbo/benchmarks/run_comparison.sh
```

`run_comparison.sh` compiles C and Rust baselines into a temporary directory and
removes that directory on exit. Set `TURBO_BENCH_KEEP_BUILD_DIR=1` to inspect the
generated binaries after a run.

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
