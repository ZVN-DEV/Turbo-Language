# Turbo Performance Status

Turbo's original concept was "Rust speeds while looking and feeling like
TypeScript/JavaScript, with progressive disclosure for deeper control." That is
still the direction, but it is not yet a blanket current-state claim.

The honest current position is:

- Turbo is a native compiled language with no VM and no garbage collector.
- Turbo has credible early evidence on recursive compute and managed allocation
  correctness.
- Turbo still trails Rust on the committed diagnostic suite.
- All current qualification is **incomplete**: some samples are too short, host
  conditions and compiler-source provenance are not independently attested,
  held-out fixtures are missing, remaining workload categories are incomplete,
  and the allocation observer does not cover the whole process heap.
- The deeper Rust-class story depends on layout work, borrowed views, owned
  buffers, regions/noalloc profiles, better optimizer decisions, and stricter
  benchmark coverage.

## Current committed evidence

Only committed reproducible artifacts count here. Temporary `/tmp` diagnostics
and one-off local runs are useful for engineering, but not for public claims.

| Artifact | Host / method | Result | Public meaning |
|---|---|---|---|
| [`g2-initial-20260906`](../benchmarks/results/g2-initial-20260906/README.md) | Apple M5 Max, macOS 26.5.1; 3 batches × 20 measured pairs, output equality, bootstrap intervals; raw data in [`report.json`](../benchmarks/results/g2-initial-20260906/report.json) | `fib(40)` median paired elapsed ratio **1.444× Rust** with 95% interval **1.4325–1.4618** | Turbo is native and within striking distance on a narrow recursive CPU microbenchmark, but misses the target gate. |
| [`g2-initial-20260906`](../benchmarks/results/g2-initial-20260906/README.md) | Same run | existing 5 MiB word count **3.871× Rust** with 95% interval **3.7976–3.9549** | Output equality is proven; implementation shape is not equivalent enough for a CPU headline. It exposes strings/hashmaps/top-k work still needed. |
| [`g2-tree-diagnostic-20260906`](../benchmarks/results/g2-tree-diagnostic-20260906/README.md) | One-pair tree smoke plus allocation-profile runs; raw data in [`report.json`](../benchmarks/results/g2-tree-diagnostic-20260906/report.json) | recursive tree one-pair timing **1.317× Rust**; tracked ARC allocations/frees balanced; zero tracked live allocations at return | Promising diagnostic for recursive managed structures, not statistical performance qualification. |
| [`g2-particle-allocation-20260906`](../benchmarks/results/g2-particle-allocation-20260906/README.md) | 10,000 particles × 512 steps, allocation-profile runs; raw data in [`profile.json`](../benchmarks/results/g2-particle-allocation-20260906/profile.json) | **5,130,018 allocations and frees**, **10,002 peak tracked live allocations**, zero tracked live allocations at return | Confirms tracked ARC balance for that fixture and shows why layout/noalloc work matters. It is not a frame-budget or Rust-speed claim. |

These results are useful precisely because they show both promise and gaps. They
support a roadmap, not a victory lap.

## Performance goals

Turbo will not use "Rust speed" as an unqualified claim until the evaluator and
targets below pass on the required platforms.

| Measure | Managed / idiomatic Turbo target | Controlled Turbo target |
|---|---:|---:|
| CPU suite geometric mean elapsed ratio | ≤1.15× Rust | ≤1.05× Rust |
| Any individual CPU workload | ≤1.35× Rust | ≤1.15× Rust |
| Live payload allocation on fixed ≥32 MiB workloads | ≤1.25× Rust | ≤1.10× Rust |
| No-allocation kernel allocation and RC count after warmup | Not claimed | Exactly 0 |

"Managed" means straightforward Turbo using the default runtime ARC/COW model.
"Controlled" means future opt-in features such as borrowed views, owned buffers,
regions, tighter layouts, and noalloc kernels compared with equivalently explicit
Rust.

The full acceptance contract is
[design/TURBO-ACCEPTANCE-SPEC.md](../design/TURBO-ACCEPTANCE-SPEC.md).

## Measurement rules

Performance claims must follow the evaluator contract:

- AOT execution timing excludes compilation. JIT is reported separately and not
  presented as pure execution time.
- Each workload has exact dataset generators, seeds, expected output, and a
  matched Rust implementation before optimization.
- Samples use warmups, randomized Turbo/Rust order, at least 20 measured pairs
  per batch, 3 independent batches, and microkernels batched to at least 200 ms
  per measured sample.
- Reports include medians, p95, bootstrapped 95% confidence intervals, absolute
  times, raw commands, toolchain versions, host CPU/OS/architecture, source
  revisions, binary hashes, stdout/stderr hashes, failures, and exclusions.
- A performance gate passes only when the confidence interval upper bound meets
  the limit.
- macOS ARM64 and Linux x86-64 are required for promoted speed gates. Windows
  and Linux ARM64 remain separate until equivalent native runner evidence exists.
- Peak RSS, live payload, total allocated bytes, allocation count, peak heap, and
  allocator/startup overhead are separate metrics. Do not subtract variable
  workload overhead.

## Workload catalog that must exist

The current evidence is not enough. The fixed evaluator catalog needs these
categories, with each one explicitly labeled as CPU, application, I/O/service, or
diagnostic rather than blended into one headline:

- recursive compute;
- word count / text processing;
- JSON parse and transform;
- string/token processing;
- hashmap update and churn;
- buffer scan;
- packed particle update;
- tree traversal.

SQLite, HTTP, serverless, durable workers, desktop, games, and freestanding
targets require separate application or soak suites. Their numbers must not be
folded into a single CPU headline average.

## What needs to improve

The biggest known performance/memory work items are:

- compact scalar and aggregate layouts so Turbo stops paying pointer/ARC cost
  where Rust stores data inline;
- typed string, JSON, and hashmap paths that avoid repeated re-stringifying,
  parsing, copying, and allocation;
- borrowed read-only views and owned buffers for zero-copy loops;
- region allocation and request/frame scopes for batch lifetimes;
- noalloc kernel checking that rejects hidden formatting, closure allocation,
  dynamic allocation, RC traffic, and unsafe FFI effects;
- optimizer rules that remove only proven redundant retains, releases, checks,
  and temporaries without changing failure behavior;
- whole-workload memory attribution, not just the current shared-header ARC
  observer.

## Claiming policy

Allowed today:

- "Turbo compiles to native machine code through Cranelift."
- "Turbo runs without a VM or garbage collector."
- "Turbo's current benchmark evidence is promising but incomplete."
- "Rust-class speed and memory control are roadmap goals with concrete gates."

Not allowed today:

- "Rust's speed" without qualification.
- "No overhead."
- "Memory-bounded" as a universal property of every Turbo program.
- "Game-engine ready," "production desktop GUI ready," "kernel/no_std ready,"
  or "durable-worker production ready."
- Any benchmark improvement/regression claim derived from temporary `/tmp`
  diagnostics or a changed benchmark method.
