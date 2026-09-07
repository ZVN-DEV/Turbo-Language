# Turbo master plan — acceptance and evaluator specification

Status: revision 3, independently reviewed with [TURBO-MASTER-PLAN.md](TURBO-MASTER-PLAN.md); final Architect → Critic approval on 2026-09-06. All numbers below are targets unless labeled baseline observation. No implementation or certification occurs in this planning turn.

## A. Evidence model

An execution record identifies goal/slice, source revision, changed files, fixture/data checksum, host CPU/OS/architecture, compiler/runtime versions, command/configuration, exit status, raw outputs and verdict. Store committed reproducible measurement artifacts under a dedicated future `benchmarks/results/<run-id>/` path; this is a proposed path, not a delivered runner. Reports must include failures and exclusions. Never substitute an LLM verdict for compiler correctness, runtime safety or numeric acceptance.

No new task should fork an already-open BL-27B/BL-28 fix. Reconcile those items before execution. Planning review approves plan quality only; implementation goals remain unstarted.

## B. Baseline commands and layers

From repository root, existing commands:

```sh
cargo test --workspace --manifest-path turbo/Cargo.toml
cargo build --release --manifest-path turbo/Cargo.toml
(cd turbo && ./tests/run_tests.sh)
turbo/tests/parity/run_parity.sh
cargo clippy --workspace --all-targets --manifest-path turbo/Cargo.toml -- -D warnings
cargo fmt --manifest-path turbo/Cargo.toml --all -- --check
turbo/tests/wasm/run_wasm_tests.sh
bash examples/libturbo-c-host/smoke.sh
bash turbo/crates/turbo-codegen-cranelift/runtime/tests.sh
```

WASM requires its documented toolchain; an unavailable toolchain is an unvalidated target, not a pass. The existing C-runtime entrypoint runs ASan with leak detection disabled; it is not leak evidence. Add supported UBSan configurations and TSan where the runtime/toolchain supports it. Separate allocation accounting/soaks prove the declared leak gates. Sanitizer absence cannot be used as evidence of safety.

Layers:

1. Unit/property tests: parser/sema, layouts, borrow/effect rules, drop paths, protocol parsers.
2. Integration/differential tests: JIT/AOT/WASM, optimization on/off, old/new migration, ABI hosts.
3. Real application E2E: fresh install/indexer/service/worker/GUI/game, with OS-native execution.
4. Fault/soak tests: abrupt termination, cancellation, malformed input, resource limits, contention.
5. Observability: compare cost reports to allocator/runtime counters and verify usable source attribution.
6. Human usability: consented participants and accessibility/manual flows; do not replace with generated personas.

## C. Performance evaluator (G2)

### Fixed suite

Define exact dataset generators/seeds and expected output before optimizing. Retain existing recursion and word-count workloads; add JSON parse/transform, string/token processing, hashmap update/churn, buffer scan, packed-particle update, tree traversal. Separately maintain SQLite, HTTP and worker E2E suites; do not blend I/O and CPU benchmarks into one headline average. Freeze a small held-out corpus owned by the verifier. Different algorithms belong in a separately labeled algorithmic comparison, not language/compiler parity.

Two code profiles per applicable workload:

- **Idiomatic:** straightforward managed Turbo vs idiomatic Rust, same algorithm/data structures and observable behavior; no intentional poor baseline.
- **Controlled:** borrowed/owned/region/noalloc Turbo vs equivalently explicit Rust. External libraries and safety restrictions match; Turbo's algorithm is not secretly implemented in Rust/C.

At E2, run currently expressible baseline forms and validate the evaluator. Freeze datasets/oracles/metric rules for future controlled forms, recording **pending capability** until the relevant G3 feature is implemented. E2 completion does not require those forms to compile and does not certify their speed. Final G2/G3 qualification requires every declared applicable form; pending forms are not silently omitted from aggregates.

### Measurement method

- Native AOT execution timing excludes compilation; development timing and clean/incremental compilation are separate metrics. JIT time is never presented as pure execution time.
- Use a monotonic high-resolution clock inside a single runner process, not two Python launches surrounding a short command.
- Warmup3 iterations;≥20 paired measured samples per workload/profile/host, randomized language order. Repeat in3 independent batches. Batch microkernels to≥200 ms per sample while reporting iteration count; do not pad application cold starts.
- Record median/p95 and bootstrapped95% confidence intervals for ratios. For a performance gate, the upper confidence bound must meet its limit. If noise makes the verdict inconclusive, repeat after diagnosing noise; never cherry-pick the best run.
- Idle controlled hardware, fixed power configuration and toolchain flags; identical CPU feature scope. Record background load and throttling. Validate outputs on every sample, not only the fastest one.
- Geometric mean is exp(mean(log(Turbo/Rust))) with equal predeclared workload weights. Individual caps still apply. Show every workload and absolute time.

### Target gates

| Measure | Idiomatic gate | Controlled gate |
|---|---:|---:|
| CPU suite geometric mean elapsed ratio | ≤1.15× Rust | ≤1.05× Rust |
| Any individual CPU workload | ≤1.35× Rust | ≤1.15× Rust |
| Live payload allocation, fixed≥32 MiB workloads | ≤1.25× Rust | ≤1.10× Rust |
| Noalloc kernel allocation and RC count after warmup | Not claimed | Exactly0 |

Measure live payload, total allocated bytes, allocation count, peak heap and peak RSS separately. Include allocator metadata/startup overhead as separate columns; subtract only measured fixed startup for the clearly labeled payload metric. Never subtract variable workload overhead. Return logical live bytes to baseline after each fixed-work batch; during a 1M-iteration churn test post-warmup RSS final-quartile median must be≤max(1.05× first-quartile median, first-quartile median+4 MiB), with fixed working set and evidence that allocator retention is not an unbounded trend.

Both macOS ARM64 and Linux x86-64 must pass claimed speed gates; Windows/Linux ARM64 have separate results until equivalent runner evidence exists. An optional new backend also must pass the master plan's parity/compile-time/10% improvement gate; benchmark aspiration is not justification to skip backend correctness.

## D. Semantic and memory adversarial matrix (G1/G2/G3)

| Area | Required positive cases | Required rejection/failure cases |
|---|---|---|
| Mutation edition | Pure transforms in statement/value/tail contexts; explicit mutation; legacy outputs preserved after AST migration | Mixed-edition import mismatch; ambiguous migration must not auto-rewrite; return-type changes cannot silently change new-edition mutation |
| Day-one ergonomics | Ten frozen application examples using contextual arrows, construction shorthand, destructuring, defaults and explicit record updates | Wrong contextual type, duplicate/missing fields, effectful default/update order, implicit dynamic-object or JS coercion assumptions; format and native parity required |
| Layout | u8/i16/f32 arrays, struct arrays, nested managed fields, enums, Option/Result, generics, alignment | Size/offset overflow, wrong-stride ABI, recursive unboxed layout, invalid casts |
| Views | Read-only subviews, unique mutable view, explicit owner transfer, zero-copy loop | Use after owner drop/resize, conflicting mutation, escaping through closure/map/result/return, spawn of short-lived borrow |
| Regions | Nested regions, early return/error, destructor/resource cleanup, explicit outward promotion | Implicit region escape in nested containers/callbacks/FFI; double-drop; missing nested promotion |
| Noalloc | Direct/generic/transitive pure kernels, checked primitive operations | Hidden string formatting, closure allocation, dynamic unknown allocator effects, RC path, unsafe FFI effects treated as safe |
| Optimizations | On/off equal output/error behavior; remove only proven redundant retains/checks | Aliasing, reentrancy, destructor side effects, failure paths, overflow/NaN/signed-zero differences |

Changing layouts must include both runtimes and every applicable JIT/AOT calling path (generic, closure, spawn, method, FFI). Add allocation accounting tests and sanitizer-backed nested release tests. Safe subset failures must diagnose rather than segfault. Do not assert an arbitrary general-purpose memory-safety theorem from a finite test suite; support tests with written ownership invariants and independent design review.

## E. Networking, platform and service matrix (G4/G5)

- Client: valid/expired/untrusted/wrong-host certificates; redirects to forbidden networks; DNS/address-policy consistency; chunked/length responses; slow peers; cancellation; connection reuse; UTF-8 and binary bodies. No curl binary present. Native library backend selection must explain CA roots, proxy support and supported TLS versions.
- Server: content-length/transfer-encoding conflicts, duplicate headers, slowloris, malformed requests, pipelining/keep-alive, disconnected clients, streaming backpressure and response injection. Exercise behind a real supported proxy and directly on loopback. Protocol conformance is not inferred from matching ordinary requests.
- Concurrency:≤configured worker threads and queue capacity under overload; no unbounded thread-per-idle-connection behavior;10k idle clients with bounded resources; cancellations return capacity; typed channel/borrow transfer rejections; clean shutdown and forced-shutdown limits.
- OS: native release builds and common conformance on macOS ARM64/x86-64, Linux ARM64/x86-64 and Windows x86-64. Path encodings, long paths, spaces, process quoting, environment, filesystem watching, console/service lifecycle, cleanup and SQLite are explicit cases. Cross-compilation without executing the result is not platform support.
- Indexer: seeded 1 GiB mixed-file corpus including at least one 512 MiB file, Unicode/deep paths, unreadable files, file mutation during indexing, restart and corruption recovery. Output matches oracle; peak RSS≤256 MiB and cancellation≤2s. Buffered/chunked file reads must pass independently before memory qualification; a happy-path feature demo is not this gate.
- Service soak:24 h mixed successful/error requests, bounded working set, CPU/I/O mixture, client churn and periodic cancellation. No positive FD/thread leak trend, live allocation accounting balanced. p99 and throughput measured at predeclared concurrency; latency runs use an open-loop load generator that does not hide queuing delays.
- Relative endpoint gate: establish sustainable capacities first; run at 70% of the smaller Rust/Turbo capacity. Require throughput≥0.90× Rust and p99≤1.20× with matched functionality; throughput qualification and latency qualification use separate appropriate runs. No universal RPS claim.
- Worker: deterministic kill points before/after enqueue commit, claim commit, external effect, ack commit and lease renewal; run≥1000 seeded fault sequences plus24 h soak. Recovered durable jobs reach completed/retry/dead-letter states; no acknowledged durable enqueue disappears. Duplicate deliveries allowed by at-least-once contract; idempotent test handlers prove effects are not duplicated. Clock-skew/lease expiry scenarios included before multi-host promotion.
- Cloud: local Lambda mock proves protocol/failure paths; deployment accounts required for provider claims. For each named provider/configuration,≥20 forced cold invocations and≥1000 warm invocations, equal architecture/memory/work, raw provider metrics and billed duration. If credentials absent, mark cloud qualification pending and keep local/runtime work moving.

## F. Interop, isolation, desktop, games and freestanding (G6/G7/G8)

- C ABI: scalar/aggregate/buffer ownership, embedded NUL and explicit lengths, callback reentrancy, wrong handle/generation, thread affinity, failed compilation, trap recovery and repeated load/unload. Two VMs alternate10k operations without cross-instance corruption; host survives deliberate bounds/division/assert traps. Raw unsafe memory faults need OS isolation and are not promised recoverable by a native language trap channel.
- Node/Python: clean install; arguments/results/batched buffers; release/unload/cancellation; benchmark small vs large boundary calls so the crossover cost is visible. No source claim of npm/PyPI compatibility.
- WASM: first reproduce and remediate BL-27B/BL-28 in isolated compiler-owned slices, before new lowering/features. Then 100% pass for named supported-core corpus, three-way output/error contract. Explicit unsupported capability exclusions cannot cover already-advertised core semantics. Host-specific WASI/browser imports are separately tested.
- Untrusted profile: denied filesystem/process/network by default; allowlist imports; fuel/time, memory, output and recursion/stack containment. Infinite loops, allocation bombs, malicious imports and inaccessible host handles cannot terminate/corrupt the parent or access denied data. Native trusted embedding is never rebranded as sandboxed simply because traps return errors.
- Desktop: real native widget/accessibility inspection; keyboard-only navigation, screen-reader labels, IME/composition, dialogs/menus, clipboard/drag/drop, DPI/multiwindow, cancellation and graceful quit on all 3 OS families. Human accessibility sign-off recorded. Package install/uninstall/update smoke; signing/notarization tested only with authorized credentials, not faked.
- Games: deterministic10k-particle kernel fixtures;30min60 Hz sessions; CPU update p99≤4 ms and frame CPU p99≤8 ms on fixed reference host, GPU timing separate. Zero update-loop allocations after warmup; save/reload checksum; device loss/input reconnect/resize/DPI; asset failure, state migration on trusted module reload; memory/resource lifetime assertions. This does not certify hard-real-time audio.
- Freestanding: link-map/symbol inspection proves no heap/RC/OS dependencies; C host then emulator/hardware oracle; explicit panic behavior and stack resource evidence; checked rejection of forbidden operations. Rust no_std comparator matches algorithm and safety. Failure to run on the target leaves G8 incomplete even if object generation succeeds.

## G. Release, migration and regression policy

Every semantic/API/layout change ships tests, migration/compatibility guidance and its own rollback strategy before promotion. Compiler internals can roll back; user data migrations require backups and reversible/forward-recovery tests. Default package edition is pinned; dependencies built under a different edition cross a specified interface or fail with a diagnostic, never reinterpret syntax silently.

P0/P1 correctness/safety failures block the affected release; keep unrelated validated features moving behind accurate capability labels. Default optimizer may not regress any frozen workload>5% without an explicit independently reviewed tradeoff record; performance target failure cannot be waived by silently removing fixtures. New benchmark versions retain old results and rationale. Two consecutive passing release candidates are required for supported→production-qualified promotion.

Verification output must distinguish pass, fail, blocked and not-run, record skipped helper fixtures separately, and include all raw commands. An independent verifier owns final implementation evidence. A planning Architect/Critic approval is not a test result or user authorization to deploy.
