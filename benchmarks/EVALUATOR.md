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

# Three batches of20 measured pairs across the four implemented cases:
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
- `buffer_scan`:32MiB logical `[u8]` data, four dependent checksum/mutation passes.
  Current Turbo storage is not assumed packed. The oracle uses affine composition
  of the256-byte repeating pattern, cross-checked against a literal bytearray
  simulation at boundary sizes. Same safe algorithm and output in Turbo/Rust.
- `hashmap_churn`:2,097,152 seeded operations on4096 integer and precomputed string
  keys, including reads/updates/removals. Independent final weighted digests and
  lengths validate both key domains. Hashing/storage and key ownership differ
  between runtimes; this measures language+runtime, not backend code alone.
- `particle_update`:10,000 seeded six-field `f64` particles,32,768 fixed steps of
  symplectic Euler with dt=1/64. A closed-form integer oracle checks the digest of
  **all six fields**, independently tested against a literal step simulation.
  Seeds and updates stay on an exact dyadic lattice (1/65536) through the permitted
  65,536-step maximum: no floating-point tolerance can conceal drift. Turbo uses
  managed struct values; Rust uses inline `Vec<Particle>` storage. This deliberately
  exposes current ownership/layout costs; it does not claim packed/noalloc Turbo,
  rendering performance or frame-latency qualification. Step count was sized with
  a preliminary512-step smoke before freezing v3; outputs are unchanged per step,
  not padded with sleeps. Any samples under200ms still block qualification.
- `string_tokens`: frozen eight-record UTF-8 log corpus,1,048,576 cyclic visits,
  literal pipe splitting, ASCII-margin trim, literal `INFO:`/`WARN:` label and
  em-dash replacement, empty-field removal and token counts in lexical order.
  Unicode is otherwise preserved byte-for-byte, including combining marks and
  mixed scripts. The independent oracle weights each record by its visit count
  instead of replaying the native loop. Exact output includes every token/count
  and processed input bytes (record separators excluded), with a SHA256 check.
  Both implementations use the same safe token/count algorithm; allocation,
  hashing and string representations remain language/runtime costs. This CPU
  workload includes one small corpus read and final ordering, not general I/O
  throughput. Native tests also run a separate corpus with a ZWJ emoji.
- The fixed v4 manifest pins `TURBO_BENCH_SIZE` / `TURBO_BENCH_STEPS` for evaluation.
  Small positive values can be used when invoking fixtures directly for tests.
  Unknown, non-string, non-ASCII or non-positive overrides are rejected by the
  evaluator. File-input bytes and logical in-memory input bytes are distinguished.
- JSON transform and tree walk still need
  fixtures/oracles and workload sizing. SQLite, HTTP and
  worker suites remain separate application/service qualification work.
- Controlled profiles remain pending G3 capabilities. Their presence in the
  manifest does not mean borrowed/region/noalloc code has compiled or passed.
- Whole-runtime allocation/live-byte/RC coverage, held-out corpus, full CPU suite,
  clean host/provenance attestation and cross-host qualification are outstanding.

Future slices must close these gaps rather than deleting them from the report.
The purpose of this slice is reliable evidence on existing code—not an easier
definition of the master plan's success.

### Known Unicode parity work (not covered by the token fixture)

Current JIT `upper`/`lower`/`trim` use Rust Unicode operations; the AOT C
counterparts only change ASCII case and trim space/tab/CR/LF. Empty-separator
`split` also differs (Rust scalar boundaries with empty ends versus C byte
elements). These are unresolved runtime behavior gaps, not a license to claim
general Unicode parity from this benchmark. The fixed corpus and oracle reject
NUL, non-UTF-8 and whitespace outside the shared ASCII-margin subset. Literal
label replacement is the declared log-processing task, **not** a substitute
implementation of Unicode case folding or normalization. G1 native semantic
parity must settle and test these operations before broader qualification.

Native probe on the unchanged compiler at `5e1c1ba` confirmed these differences:

| Expression | JIT output | AOT output |
| --- | --- | --- |
| `upper("Straße")` | `STRASSE` | `STRAßE` |
| `lower("É")` | `é` | `É` |
| `len(trim(" x "))` (U+00A0 margins) | `1` | `5` |
| `len(split("é", ""))` | `3` | `2` |

To reproduce, put these four expressions in `print(...)` statements inside
`fn main()`, then compare `turbolang run` against `turbolang build` and the native
binary. The token benchmark does not exercise these unsupported parity cases;
its success must not close this defect list. The corpus's trailing spaces and
tabs are intentional data for trimming tests, not formatting whitespace.

## Shared-header allocation profiling (G2.1 next slice)

### Recursive-data prerequisites

The tree fixture exposed compiler and ownership defects before timing could be
meaningful. Nominal names are now registered before payload/layout resolution;
unknown enum payload types produce errors without deleting payload slots. Native
drop lowering predeclares cleanup helpers for recursive type graphs, avoiding
unbounded compiler recursion without changing the16-byte header or field slots.

Allocation-gated regressions also cover call-scoped argument references, `??`
temporary cleanup, and payload-free data-enum constructors. Borrowed arguments
stay alive while later arguments are evaluated: readonly holds are released by
the caller after the call, mutable references by the callee. The same argument
hold rule covers named calls, methods, UFCS and function-value calls.

`turbo-cli/tests/recursive_types.rs` executes the actual regression fixtures in
JIT/AOT. Under `allocation-profile`, six fixtures require matching valid counters,
zero live shared-header allocations/bytes and allocations equal to frees after
200 rounds of construction/traversal/aliasing/replacement. This is not a new
cycle collector, arbitrary-depth stack guarantee, WASM qualification or whole-heap
proof. The separate indirect-call test proves argument safety only; reclamation
of function/closure environment allocations remains outstanding and is not
silently included in the balanced-allocation claim. The timed tree workload
itself remains pending until its dataset, Rust counterpart and oracle are added.

### Building and collecting profiles

Build a separate diagnostic compiler. Keep it separate from the ordinary release
compiler used for timing; the feature changes `--version` to include
`+allocation-profile`, and the evaluator rejects that flavor as `--compiler`.
Normal builds compile out all observer hooks and do not link observer code.
Reports also carry structured `tools.timing_build` and `allocation_metrics.build`
fields identifying standard versus instrumented flavors. These identify the
verified flavor handshake, not an independent attestation of source provenance.

```sh
# A separate target directory keeps instrumented binaries out of the normal path.
cargo build --release -p turbo-cli --features allocation-profile \
  --manifest-path turbo/Cargo.toml --target-dir /tmp/turbo-allocation-build

python3 benchmarks/evaluator.py \
  --profile-compiler /tmp/turbo-allocation-build/release/turbolang \
  --output benchmarks/results/a-new-profile-run
```

The evaluator first finishes ordinary timing samples. It then builds an
instrumented AOT executable and collects separate AOT/JIT profiles (three each
by default, configurable with `--profile-samples`). These diagnostic durations
and RSS values are not mixed into timing summaries. JIT diagnostic duration also
includes compilation. Exact overhead is workload/build-dependent; retain these
times rather than presenting instrumented timings as production performance.

Profiling requires both the Cargo feature and `TURBO_ALLOC_PROFILE=1` at program
execution. The evaluator supplies the environment switch only in its profile
phase. A returning entry point emits one `TURBO_ALLOC_PROFILE` JSON line on
stderr. Missing/duplicate records, other stderr, invalid observer state, negative
or missing counters, and unbalanced accounting fail validation. A trap or early
process exit without a returned entry does not become a zero-allocation report.

Coverage is deliberately **shared-header ARC**, not the whole process heap:

- Allocations, actual heap frees and arena reclamations; live/peak tracked object
  counts and data-region bytes; cumulative data and header bytes separately.
- Data-region bytes include allocated capacity, terminators and container
  metadata. They are not merely logical user payload. The existing16-byte ARC
  header and allocation layouts are unchanged.
- Retain/release call counts include no-op calls; operation counts cover actual
  shared-header counter mutations, including direct COW decrements.
- JIT hooks use the existing allocation registry and actual deallocation path;
  AOT hooks use shared-header allocation/free and arena reset. The same C observer
  implements accounting for both. Unknown frees, duplicate registrations,
  overlapping profile scopes and overflow invalidate evidence, not program state.
- Requested registry storage (bucket table plus entries) is tracked separately as
  `peak_observer_bytes`; this is not total observer RSS or allocator metadata. Observer
  bookkeeping is never counted as program allocation or used to free user data.

Excluded: compiler memory, non-header hashmap/thread/synchronization backing
storage, Rust-library temporaries, I/O buffers and foreign libraries such as
SQLite. JIT and AOT may legitimately allocate differently; no general count parity
is assumed. A scope ends at entry return, not proof that detached tasks completed.
The CLI/native entry calibration is exact in both modes: one array,32 data bytes,16 header
bytes, one retain, two releases and no live allocation. C tests also cover real
arena reclamation, overflow, unknown frees and concurrent observer updates.

Reports use `allocation_metrics.status="measured_partial"` after successful
collection, while whole-heap fields remain null and qualification stays
incomplete. Full allocation coverage, reference-language instrumentation and the
remaining fixture/host/provenance work are still required. This is not a memory
safety proof or a claim that all runtime allocations are tracked.
Direct `libturbo` C-API entry calls and fork-without-exec profiling are not supported
measurement entry points in this slice.

## API/reference basis

Uses Python's documented [wait4](https://docs.python.org/3/library/os.html#os.wait4)
and [resource usage fields](https://docs.python.org/3/library/resource.html), with
Linux's [getrusage](https://man7.org/linux/man-pages/man2/getrusage.2.html) RSS units.
The collector tests execute real child processes, including timeout/reaping and
output overflow, in addition to deterministic statistical and evidence tests.
