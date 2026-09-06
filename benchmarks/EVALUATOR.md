# Native performance evaluator (G2.1)

This is the first implemented measurement slice of G2.1, not completion of G2.1
or proof of Rust parity. It replaces the old best-of-N/shell timestamp method for
new evidence while retaining the existing fixtures and older exploratory scripts.
No compiler optimization is part of this change. Python3.10+ standard library only.

## Run and test

```sh
cargo build --release --manifest-path turbo/Cargo.toml
python3 -m unittest discover -s benchmarks -p test_evaluator.py -v

# Quick execution/format/oracle check, NOT a performance qualification:
python3 benchmarks/evaluator.py --samples 2 --batches 1 --warmups 1 --bootstrap 100 \
  --output /tmp/turbo-evaluator-smoke

# Three batches of20 measured pairs, with three warmup pairs per case/batch:
python3 benchmarks/evaluator.py --output benchmarks/results/my-new-run

# Explicitly require qualification; currently exits3 (incomplete):
python3 benchmarks/evaluator.py --cases fib --check --output /tmp/turbo-evaluator-check
```

Output directory must not exist. Existing evidence is never overwritten. Builds
and generated input live in a temporary directory; executables are not committed.
An OS-held lock prevents two evaluator invocations in the same checkout from
overlapping. A competing invocation fails visibly; process exit releases the
lock automatically. The ignored lock file is intentionally not unlinked. This
does not prove that unrelated applications or other worktrees are idle.
The report records their commands, hashes, sizes and compile time separately from
execution. `samples.jsonl` is flushed after each result **before** validation, so
failed output, exits and timeouts survive in the evidence. `report.json` summarizes
the run, qualification blockers and case pairs. A top-level scope field identifies
the initial subset, and `working_tree_status` preserves Git porcelain records for
staged, unstaged and untracked paths (including Git's unusual-filename quoting).
Never publish a result without
its source revision, input/fixture hashes, tool versions and protocol.

Exit codes:0 = measurements/oracles completed (not qualification);1 = a complete
checked suite failed its gates;2 = invalid invocation/build/output/runtime error;
3 = `--check` incomplete;4 = `--check` inconclusive;130 = interrupted. Consult the
report, not merely exit0. Presently the full checked suite cannot pass because the
missing work below is real, not because the numeric limits were relaxed.

## What is measured

- One runner's monotonic `perf_counter_ns` spans process creation through exit.
  It includes exec/startup and up to1ms polling delay; it is **not** in-process
  kernel timing. Runs below200ms are flagged; do not combine many process launches
  and pretend that this is in-program kernel batching.
- Per-child `wait4` resource accounting supplies peak RSS: bytes on macOS and
  KiB converted to bytes on Linux. This is not cumulative `RUSAGE_CHILDREN`, not
  live heap, and not the sum of a process tree. It includes executable/runtime/
  allocator costs as the OS reports them. Compiler RSS and workload RSS are not
  interchangeable. Windows collection is not yet implemented.
- Warmups and measured pairs randomize Turbo/Rust order using a recorded seed.
  Every output is checked, not only the fastest sample. All raw times and RSS
  values are retained. Runtime stderr must be empty; compiler build diagnostics
  are recorded separately. stdout/stderr are separately bounded; failed, noisy and
  timed-out processes never contribute passing samples.
- Ratio estimator: median of within-pair Turbo/Rust elapsed ratios. Per-language
  median/p95 are also shown. Equal-weight workload geometric means are computed
  **within categories only**. A hierarchical bootstrap resamples common batch IDs,
  then paired observations within each selected batch. Percentile95% intervals
  use a recorded seed/draw count. Statistical intervals do not certify an idle
  machine, independence from external load, or a sufficiently representative suite.
- Gate PASS requires upper CI bounds within both geometric-mean and per-case caps.
  Lower bounds beyond a cap produce FAIL; intervals crossing it are INCONCLUSIVE.
  Missing scope produces INCOMPLETE while retaining observed subset failures.

The approved targets remain in [the acceptance specification](../design/TURBO-ACCEPTANCE-SPEC.md).
No allocation count or live-byte value is inferred from RSS: those fields are
explicitly null / `not_instrumented`. Power/load configuration and compiler source
provenance are not automatically attested; a supplied compiler binary is hashed,
but its version string does not prove it was built from the reported checkout.
Only safe benchmark-specific environment overrides are introduced; inherited
environment values/secrets are never dumped into reports.

## Frozen scope, actual readiness

[`evaluator-cases.json`](evaluator-cases.json) retains all eleven declared cases.
Runnable source fixtures and the word-count generator are checksum-pinned; an
intentional fixture revision must update the manifest and retain old results.

- `fib`: existing native recursive fib(40), independent fixed expected value.
- `wordcount`: existing5MiB seeded ASCII workload, independently checked through
  Python Counter + sorted top20 oracle. **Application comparison only:** Turbo
  repeatedly scans to select top20; Rust sorts all entries and uses different
  token/storage operations. Matching output does not make these identical
  implementations. Word count is not included in the pure CPU geometric mean.
- JSON transform, string tokens, hashmap churn, buffer scan, particle update and
  tree walk still need fixtures/oracles and workload sizing. SQLite, HTTP and
  worker suites remain separate application/service qualification work.
- Controlled profiles remain pending G3 capabilities. Their presence in the
  manifest does not mean borrowed/region/noalloc code has compiled or passed.
- Runtime allocation/live-byte/RC counters, held-out corpus, full CPU suite,
  clean host/provenance attestation and cross-host qualification are outstanding.

Future slices must close these gaps rather than deleting them from the report.
The purpose of this slice is reliable evidence on existing code—not an easier
definition of the master plan's success.

## API/reference basis

Uses Python's documented [wait4](https://docs.python.org/3/library/os.html#os.wait4)
and [resource usage fields](https://docs.python.org/3/library/resource.html), with
Linux's [getrusage](https://man7.org/linux/man-pages/man2/getrusage.2.html) RSS units.
The collector tests execute real child processes, including timeout/reaping and
output overflow, in addition to deterministic statistical and evidence tests.
