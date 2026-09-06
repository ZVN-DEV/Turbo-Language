# Recursive tree diagnostic — not performance qualification

This is a **one-pair smoke** of the v5 tree workload, followed by separate
instrumented JIT/AOT runs. The numeric timing ratios/intervals in the raw report
are not statistically meaningful with one pair and are not speed-parity claims.
The evaluator correctly returns `qualification: incomplete`.

Sixteen depth19 trees are built, walked and dropped:16,777,200 logical nodes in
total,1,048,575 nodes per round. The independent flat-array oracle matches all
native outputs. Each instrumented mode reports16,777,204 shared-header allocations
and frees (node storage plus parameter parsing), zero live allocations at entry
return, and peak1,048,575 live allocations. The full JIT/AOT counter records match.
Small native tests separately compare one round against four rounds at the same
depth to catch accumulation between rounds. These facts concern tracked ARC
storage, not the whole heap, Rust allocation counts or cycle collection.

The normal compiler was supplied from the local release build; the instrumented
compiler was a separate debug `+allocation-profile` build. Compiler binaries,
source files and evaluator are fingerprinted in [report.json](report.json), but
independent compiler-source provenance and controlled host conditions are not
attested. The report records the actual dirty worktree based on `fb118a0` while
the v5 fixture/evaluator changes were being implemented; do not relabel it as a
clean-commit qualification run. [samples.jsonl](samples.jsonl) retains raw events,
including ordinary builds/timings and separately labeled diagnostic profiles.

Reproduce from the repository root (use a fresh output directory):

```sh
cargo build --release --manifest-path turbo/Cargo.toml
cargo build -p turbo-cli --features allocation-profile \
  --manifest-path turbo/Cargo.toml --target-dir /tmp/turbo-tree-profile-build
python3 benchmarks/evaluator.py --cases tree_walk \
  --samples 1 --batches 1 --warmups 1 --bootstrap 100 --timeout 180 \
  --profile-compiler /tmp/turbo-tree-profile-build/debug/turbolang \
  --profile-samples 1 --output /tmp/turbo-tree-diagnostic-new
```

For a full sampling protocol, omit the sampling overrides. Full G2 qualification
still requires the remaining workloads, controlled forms, memory coverage and
host/provenance gates in [the evaluator contract](../../EVALUATOR.md).
