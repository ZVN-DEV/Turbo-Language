# Managed-particle allocation diagnostic

The10,000-particle fixture at **512 steps** gives byte-identical output and
identical shared-header counters in JIT/AOT. The timing-suite v3 default is
32,768 steps; this smaller diagnostic is explicitly not that timing profile.

The retained observer records in [profile.json](profile.json) count5,130,018
allocations and frees, with10,002 peak live allocations and zero tracked live
allocations at entry return. This is consistent with allocation per managed
particle update and motivates the planned ownership/layout work. It is not a
whole-heap no-leak proof, Rust comparison, frame budget or latency measurement.
The shared-header exclusions in [EVALUATOR.md](../../EVALUATOR.md) still apply.

Collected on macOS/ARM64 using a supplied local debug `+allocation-profile`
compiler. Source and compiler binary fingerprints are retained; the compiler's
source provenance is not independently attested. The source matches the v3
fixture hash. Collection occurred during local regression verification; no
durations, RSS or performance ratios are promoted from these diagnostic runs.
Both records passed `parse_allocation_profile`, including stdout validation and
allocation-balance checks. Only the observer's structured records are retained.

Reproduce from the repository root with a separate instrumented build:

```sh
cargo build -p turbo-cli --features allocation-profile \
  --manifest-path turbo/Cargo.toml --target-dir /tmp/turbo-particle-profile-build

TURBO_ALLOC_PROFILE=1 TURBO_BENCH_SIZE=10000 TURBO_BENCH_STEPS=512 \
  /tmp/turbo-particle-profile-build/debug/turbolang run \
  turbo/benchmarks/bench_particle_update.tb

/tmp/turbo-particle-profile-build/debug/turbolang build \
  turbo/benchmarks/bench_particle_update.tb -o /tmp/turbo-particle-profile-aot
TURBO_ALLOC_PROFILE=1 TURBO_BENCH_SIZE=10000 TURBO_BENCH_STEPS=512 \
  /tmp/turbo-particle-profile-aot
```

Expected stdout is `21010065`, `10000`, `512`, each on its own line. The sole
stderr line is the `TURBO_ALLOC_PROFILE` record. Future compiler revisions may
legitimately change these counts; retain this baseline, do not update it in place.
