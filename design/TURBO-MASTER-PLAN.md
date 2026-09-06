# Turbo: approachable native programming, without a performance ceiling

Status: master plan revision 3, independently reviewed and APPROVED by Architect → Critic on 2026-09-06. This approves planning quality, not implementation, dependencies, incompatible language changes or deployment. None of the future capabilities or target numbers below are claimed as delivered.

Baseline: `11e9aaa`, reviewed 2026-09-06 UTC. Acceptance specification: [TURBO-ACCEPTANCE-SPEC.md](TURBO-ACCEPTANCE-SPEC.md). Existing execution history: [BACKLOG.md](BACKLOG.md). Consensus records live under `.omx/plans/`.

## 1. The destination

Turbo should be the language a TypeScript-fluent developer chooses when an application needs native execution, dependable distribution, or deeper control—and stays with as requirements become more demanding.

**Product promise:** write straightforward typed application code; inspect its costs; selectively introduce native layouts, borrowed access, ownership and allocation control; keep the rest of the application simple.

The destination includes excellent tools, services and job executors; supported native macOS/Linux/Windows applications; practical 2D games and simulation; safe embedding and WASM; and a bounded freestanding computational profile. These are distinct support contracts, not consequences of producing an executable.

### What this plan does not promise

- Universal Rust parity for every program, zero overhead from unrestricted automatic memory management, or static knowledge of every dynamic lifetime.
- Compatibility with arbitrary JavaScript/TypeScript/npm or Python packages. Call into existing ecosystems through explicit interfaces instead.
- A new AAA engine, a universal widget framework invented from scratch, an OS kernel, arbitrary device drivers, hard-real-time certification, mobile or GPU-language features as prerequisites for Turbo 1.0.
- Production safety from keyword restrictions alone, or from running untrusted native code in the compiler process.
- Dates, staffing capacity or cloud expenditure that the user has not supplied. Order is committed in this proposal; estimates are relative effort and gates, not calendar promises.

## 2. Ground truth and finding disposition

These are current facts or prior-turn observations. Every other acceptance number in this document is a **proposed target**.

| Evidence | Source / observation | Plan disposition |
|---|---|---|
| The language builds and runs; native tests passed | Prior turn: release/workspace tests; integration349 pass/10 skips; native parity37 pass; Clippy/fmt clean | Preserve these gates; expand application and cross-target evidence under G1/G4 |
| Native speed is useful but not Rust parity | Existing scripts: Fibonacci329 ms vs Rust261 ms; 5 MB word count195 ms vs124 ms on M5 Max; best-of3/5 | G2 establishes statistically stronger baselines and closes measured gaps |
| ARC, allocations, fixed-width slots and COW have costs | `turbo/crates/turbo-codegen-cranelift/runtime/turbo_rt.c:535`, `:618`, `:731`, `:4334`; `src/turbo_types.rs:43` | G2 representation/optimization; G3 opt-in control |
| Discarded COW calls change the receiver | `turbo/crates/turbo-parser/src/cow_rewrite.rs:1`; executed map example changed [1,2,3] to [2,4,6] | G1 semantics migration; preserve old programs explicitly |
| Advanced control ladder is not built | `design/MEMORY-MODEL.md:146` | Replace unbounded CTRC promise with progressively proven G3 capabilities |
| Outbound HTTP executes curl | `turbo/crates/turbo-codegen-cranelift/src/runtime.rs:1880`; `docs/serverless.md:65` | G4 native reusable HTTP/TLS client; dependency decision required |
| OS-thread concurrency; no static Send/Sync checking | `design/CONCURRENCY.md:3`; `docs/SAFETY.md:250` | G4 bounded scheduling, structured lifetimes and safe transfer contracts |
| Windows AOT core-only for several builtins | `docs/COMPATIBILITY.md:7`; `src/aot.rs:65` | G4 native Windows parity/release gate |
| WASM has semantic gaps | `design/BACKLOG.md:331` BL-27B and `:429` BL-28 | G6 owns these existing items; do not create duplicate fixes |
| Embedding is trusted-only and host-fatal | `turbo/crates/turbo-codegen-cranelift/include/libturbo.h:10`; `docs/libturbo.md:45` | G6 recoverable ABI and isolated untrusted execution |
| Example JSON breaks on control characters | `examples/http-sqlite-api/main.tb:19`; reproduced POST and subsequent GET failed JSON parsing | First G1 regression/fix slice; serializer, not ad-hoc escaping |
| Native GUI/game ecosystems are not supplied | `design/REACH-ROADMAP.md:199`; `examples/game-of-life/README.md:51` | G7 native SDK integrations and actual shipped reference apps |
| Roadmaps contain superseded or contradictory promises | `design/ROADMAP.md:3`; `design/MEMORY-MODEL.md:5`; current runtime string ARC/tests | G1 capability contract; documentation updated alongside behavior, not hygiene-only work |
| C interop is currently restricted | `turbo/crates/turbo-sema/src/lib.rs:415`; `src/ffi.rs:135` | G6 typed boundary ownership, aggregates/buffers/callbacks and header-driven tooling |

Source paths abbreviated with `src/` above are under `turbo/crates/turbo-codegen-cranelift/`. Line anchors describe this baseline; re-resolve at execution start.

## 3. Principles, decision drivers and alternatives (RALPLAN-DR)

### Principles

1. One language meaning across targets and execution modes; unsupported capabilities diagnose, never silently reinterpret.
2. Familiar common code; explicit mutation and observable costs. Complexity is opt-in, not surprising.
3. Performance improvements preserve specified safety and semantics; benchmark the complete useful work.
4. Applications validate the platform; every substantial foundation slice must unlock a concrete acceptance scenario.
5. Native platform breadth grows through narrow runtime/ABI contracts and libraries, not domain-specific compiler keywords.

### Three decision drivers

- Deliver practical adoption opportunities before a multi-year foundation rewrite completes.
- Remove architectural barriers to Rust-class representation, speed and memory control.
- Prevent runtime/platform divergence and unsafe boundaries from multiplying as reach expands.

### Viable options

| Option | Strongest case | Cost / reason not selected alone |
|---|---|---|
| A. Services-first on current ARC/Cranelift | Fastest route to useful CLI/API products; leverages shipped code | Current layout/control limitations become expensive to retrofit; does not satisfy user's control and wider-app goals |
| B. Foundation-first ownership/MIR/optimizing-backend rewrite | Clean opportunity to solve representation and analysis coherently | Delays useful applications; creates large parity/debugging risk; prior LLVM branch already illustrates duplication risk (`design/ROADMAP.md:24`) |
| C. Two-track, shared-contract development | Ships useful products while incrementally creating typed lowering, representation and control | Requires explicit interface ownership, migrations and work-in-progress limits |

**Selected: C.** Track P proves useful products; Track F removes performance/control ceilings. A supplies early delivery discipline; B supplies long-term architectural requirements without an all-at-once rewrite.

## 4. Support contract and release waves

Support labels apply to a **capability × target × execution mode**, not to an entire language indiscriminately:

- **Experimental:** known restrictions recorded, failures explicit; not recommended for production.
- **Supported:** named contract suite passes on native target runners and a reference application ships.
- **Production-qualified:** supported plus soak, fault/recovery, upgrade and operational evidence for the named workload.
- **Certified real-time / safety-critical:** not claimed by this plan. Passing latency tests is not certification.

| Wave | Exit outcome | Main goals | Approximate effort shape |
|---|---|---|---|
| W0 Trustworthy baseline | Stable semantic/migration proposal; reproduced defects fixed; benchmark and cost ledger validated | G1; G2.1 | M |
| W1 Useful native products | Directory indexer CLI/local API; native client; supported OS core; first representation gains | G1, G2.2, G4.1–4.2, G5.1 | L |
| W2 Native core maturity | Structured bounded runtime; SQLite services/worker recovery; typed lowering and memory controls; speed gates | G2.3–2.5, G3, G4.3–4.4, G5.2–5.4 (external SQL promotion follows W3 ABI) | XL |
| W3 Reach | Recoverable embedding, Node/Python boundaries, supported WASM and isolated execution | G6 | L–XL |
| W4 Applications beyond servers | Real cross-platform native desktop reference app and 2D game/tooling SDK | G7 | XL |
| W5 Constrained systems | Proven no-runtime/allocator-free computational profile on one freestanding target | G8 | L–XL, evidence-dependent |

Wave labels are sequencing, not versions. Scope a 1.0 core freeze after W2 only if its native stability gates pass; experimental SDKs may version independently. Failure to hit performance targets keeps the target open; it must not be hidden by a new slogan or averaging away regressions.

## 5. Eight outcome goals and executable slices

### G1 — A dependable, genuinely approachable language and toolchain

**Outcome:** a TypeScript-fluent developer can implement and distribute a useful native tool without encountering undocumented semantic changes or needing compiler knowledge.

- **G1.1 Correct the observed application defect.** Add newline/tab/control-character and Unicode round-trip tests, replace hand-built escaping in the SQLite example with the language's serializer, verify POST/list/restart behavior. Preserve SQL parameter binding. Touchpoints: example, JSON builtins/runtime, phase1 and application harness.
- **G1.2 Specify values, identity and mutation.** Propose context-independent pure transforms (`map`, `filter`, string transforms) and explicit assignment/mutation. Decide each builtin, including push/pop/sort, rather than blindly deleting the rewrite pass. Introduce a versioned language-edition contract in `turbo.toml` plus an explicit CLI selector for single files; unversioned existing programs stay legacy until a documented migration boundary. Build an AST-aware migration command with dry-run diffs, ambiguous cases requiring manual resolution, and golden before/after behavior. The new edition must not silently change existing packages. Touchpoints: AST/parser/COW rewrite/sema/formatter/CLI/project/dependency loader/LSP; all COW regression fixtures.
- **G1.3 Make the development loop complete.** Inferred-type hover, cross-module navigation, source locations through native lowering, stack traces/debugger stepping for supported constructs, stable diagnostics, package lock/install integrity and clear platform errors. Prefer existing tooling/formatting paths; no new homegrown debugger. Measure an edit→diagnostic→run cycle on a fixed representative 100-module/10k-LOC fixture: proposed warm p95≤1s on reference hardware, with clean-build time reported separately. A watch-process restart is not called state-preserving hot reload (`turbo/crates/turbo-cli/src/watch.rs:7`).
- **G1.4 Qualify the first-user path.** Fresh-machine install→new project→test→native binary→install a pinned package→debug a deliberate error→upgrade. At least five TS-fluent external evaluators, four completing a prewritten CLI task in≤60 min without author assistance; retain consented anonymous task timings/blockers. If participants are unavailable, product usability remains unvalidated; automated agents are not counted as humans.
- **G1.5 Close day-one ergonomics gaps.** Under the G1.2 edition contract and compiler-owner lane, implement a bounded application-authoring subset: contextual arrow-parameter inference beyond builtin-only special cases, record/struct construction shorthand, field destructuring, default arguments and explicit record-copy/update syntax. Define evaluation order, nominal-versus-structural type behavior and copy costs before lowering; anonymous dynamic objects and arbitrary TS type-system compatibility are not implied. Translate ten frozen small TS-style examples (configuration, filtering, parsing, error handling and records) into equivalent Turbo without requiring traits/impl blocks or memory annotations for those simple tasks. Each construct needs type-error, side-effect-order, format and native parity fixtures. Sources: `design/SYNTAX.md:3` and `docs/GETTING-STARTED.md:109`. This follows E3 and is scheduled serially with other compiler changes, not as a competing frontend rewrite.

**Exit:** new and legacy semantic corpora both pass; migration is explicit; reference CLI builds from clean supported hosts; P0/P1 known core correctness issues are closed; test counts cannot substitute for missing scenarios. G1.1 can ship before G1.2 finishes.

### G2 — Rust-class performance on representative native workloads

**Outcome:** ordinary Turbo is competitive, and controlled hot paths approach equivalent Rust without moving application logic into C/Rust.

- **G2.1 Establish the evaluator.** Extend existing fixtures rather than invent favorable replacements: recursion, parsing/word count, JSON transform, typed hashmap churn, byte-buffer scanning, packed-particle update, allocation-heavy tree walk, SQLite batch/query, HTTP compute endpoint and worker execution. Publish raw samples, correctness oracles, allocation counts/bytes, peak and live memory, compile time, artifact size and environment. Existing best-of-N scripts remain exploratory only; certification uses the companion protocol.

  E2 qualifies the runner, counters and currently expressible managed-code baselines. It freezes the input/oracle/metric specifications for future controlled fixtures, but labels those **pending capability**, not pass or a baseline failure. Borrowed/region/noalloc implementations and their G2 performance qualification run only after the corresponding G3 feature exists. New or changed inputs require explicit evaluator revision with historical results retained; absent capabilities cannot be averaged out of final goal qualification.
- **G2.2 Fix representation before blaming the backend.** Split into **G2.2a**, the layout/ABI/drop descriptor specification and assertions preserving current representation, and **G2.2b**, new compact representation after the first G2.3 lowering slice passes. Descriptors carry size/alignment/stride/drop/copy policy; implement compact primitive arrays, explicit contiguous buffers, aggregate inline storage where lifetime/ABI permits, allocation-free scalar Option/Result representations and geometric growth/reserve APIs. Keep old code valid under its edition. Test nested managed fields, recursive types, generic substitution, overflow, casts and FFI alignment. Touchpoints: `turbo_types.rs`, codegen expressions/statements/calls, both runtimes, ABI marshalling, parity tests. Never change eight-byte slots piecemeal without updating all readers/writers/drop paths.
- **G2.3 Introduce a small typed lowering boundary incrementally.** Carry resolved types, layouts, source spans, ownership/drop operations and control flow through a shared representation before backend lowering. First migrate one full feature slice and compare to old lowering; expand only with parity. This is not a prerequisite for G1 fixes or native networking. No second parser/sema or separate backend-specific language semantics.
- **G2.4 Optimize verified costs.** Scalar replacement/escape analysis, redundant retain/release elimination, move of uniquely-owned temporaries, bounds-check elimination with proofs, concrete generic specialization, iterator/transform fusion when observable behavior is unchanged. Every optimization has an on/off differential test and an allocation/IR assertion where applicable. Preserve checked/wrapping integer and floating-point rules; fast-math is not enabled silently.
- **G2.5 Decide release backend from evidence.** Keep Cranelift for development and the default until a bounded optimizing-backend experiment through shared lowering demonstrates a meaningful win. Compare current Cranelift, middle-end-only improvements and one candidate (e.g. LLVM); no dependency adopted in this planning turn. Candidate promotion requires complete supported-core parity,≤2× current clean release compile time on the fixed fixture, and≥10% geometric-mean speed improvement over improved Cranelift on the frozen CPU suite without >5% individual regressions. If no candidate passes, retain Cranelift, record the measured shortfall and continue targeted work. Do not revive the parked branch wholesale.

**Proposed final gates:** idiomatic native CPU-suite geometric-mean elapsed ratio≤1.15× Rust, no workload>1.35×; controlled hot-path suite≤1.05× geometric mean, no workload>1.15×. These are same-algorithm, equivalent-safety comparisons on both macOS ARM64 and Linux x86-64; report Windows/Linux ARM64 separately until qualified. Controlled code stays Turbo except identical external libraries used by both baselines. End-to-end service/worker performance is certified separately under G5. Performance qualification depends on G3 for controlled-memory cases; it is not a dependency preventing G3 work.

### G3 — Progressive memory control that actually removes costs

**Outcome:** four coherent levels with machine-checkable boundaries, not four incompatible language modes.

| Level | Developer experience | Required guarantee |
|---|---|---|
| 0 Managed values | Ordinary application code; ARC/COW fallback | Defined value/identity rules, safe cleanup, no required lifetime annotations |
| 1 Observe and borrow | Cost report, borrowed read-only views, unique mutable views, owned buffers | No hidden full-buffer copy for a view; no escaping invalid borrow; mutation exclusivity |
| 2 Regions and explicit ownership | Region-scoped scratch allocation, explicit move/clone and allocator-backed buffers | Escaping references rejected or explicitly promoted; deterministic cleanup; provenance tracked |
| 3 Allocation-constrained kernels | Transitive no-allocation function contract and narrow unsafe/native boundary | No implicit allocation/RC activity in accepted kernels; forbidden operations fail at compile time |

- **G3.1 Specify and implement views/owned buffers.** Start with byte/numeric slices and lexical borrows; define aliasing, invalidation on resize, lifetimes across return/closure/async/FFI and mutable exclusivity. Cross-boundary annotations may be required for expert APIs; do not claim inference eliminates all complexity. Borrow-checking restrictions apply only where needed, while Level 0 retains its managed semantics.
- **G3.2 Introduce regions with escape safety.** Region values cannot escape in arrays/maps/closures/results or spawned work. Permit explicit copy/promotion into an outer allocator, with ownership of nested fields defined. Destructor/resource effects are executed exactly once, including early return/error paths; resetting memory is not a substitute for closing files or sockets. Start single-threaded; add cross-thread region use only after a separately proven contract.
- **G3.3 Enforce allocation-constrained functions.** Proposed attribute/API spelling remains an RFC decision. Track allocation effects transitively across generic calls, dynamic dispatch, callbacks and FFI; unknown effects are rejected in the constrained subset, not assumed safe. `noalloc` does not by itself mean no-blocking/no-locking or hard-real-time; introduce independent checked restrictions only where G7/G8 require them. Unsafe escape routes are labeled and excluded from safe guarantees.
- **G3.4 Make costs visible.** Source-attributed counters/reports for allocations, live bytes, copies, RC operations, region promotions and high-water marks. Cross-check runtime instrumentation with allocator counters; profiling overhead is measured and excluded from release speed runs. Supply the same CLI/indexer/particle example in managed→borrowed→region→constrained forms with equal results.

**Exit:** positive/negative ownership suites pass on native JIT/AOT and supported WASM subset; no heap allocation or RC operation after warmup in certified constrained kernels; `[u8]`/buffer storage is byte-dense under its declared layout; controlled live payload allocation≤1.10× equivalent Rust, managed≤1.25× on the fixed≥32 MiB workloads (RSS and fixed runtime overhead reported separately). Fixed-workload churn returns logical live allocations to baseline; RSS does not show unbounded growth. Cycles require a documented policy before new shared-reference types ship; do not quietly introduce a GC to rescue unspecified cycles.

### G4 — A portable, safe runtime for real operating systems and concurrency

**Outcome:** macOS ARM64/x86-64, Linux ARM64/x86-64 and Windows x86-64 have named, tested native contracts—not binaries with runtime-error stubs for advertised APIs.

- **G4.1 Native networking.** Introduce a portable transport interface and bounded native HTTP/TLS client with certificate/hostname verification, cancellation, deadlines, streaming and reusable connections. Evaluate maintained libraries; never hand-roll TLS. Eliminate curl/process dependence from the client and Lambda loop. Test redirect/SSRF policy including redirects and resolved addresses, proxies, malformed responses, certificates and resource cleanup. Keep server-side TLS termination behind proxy initially; optional native server TLS reuses the verified transport later.
- **G4.2 OS parity.** Replace Windows concurrency/network/process stubs, specify UTF-8↔OS path conversions and non-Unicode path handling, process arguments/environment/exit status, monotonic clocks, signals/cancellation equivalents, filesystem watching and service lifecycle. Use an OS abstraction with shared conformance tests; consolidate duplicated runtime behavior selectively, not a full runtime rewrite. Promote each target only after native-runner tests and clean-install artifacts pass.
- **G4.3 Bounded concurrency and safe transfer.** Separate blocking/CPU task pool from nonblocking network I/O. Bounded queues, backpressure, typed channels, cancellation, structured parent/child completion, explicit shutdown and thread-affine resources. Specify which types may cross threads; reject unsafe sharing/borrow escape before replacing the scheduler. Before G3.1, the safe new transfer surface accepts only scalars, explicitly copied messages and documented synchronized handles; borrowed/nontransferable managed values are rejected. Expanding transfer to views/owned buffers requires G3.1's accepted lifetime/alias contract. Existing spawn behavior remains compatible within a documented edition/runtime contract. Prefer internal service reactor first; expose general async APIs only when suspension, cancellation and resource lifetimes have a complete contract.
- **G4.4 Prove the operational model.** Portable HTTP framing and connection conformance, 10k idle connections without 10k threads, bounded overload, graceful and forced shutdown, malformed client rejection, stable FD/thread counts and a 24 h soak. Thread/queue bounds are configurable and measured. HTTP/2/WebSocket support is library/protocol work added only against named tests; include both in the supported-service roadmap, not new language syntax.

**Exit:** no advertised Windows AOT stub remains; first-class target core/parity suites are required CI; documented macOS/Linux/Windows behavior differences are tested. Runtime transfer rules are checked; race tooling where available and stress tests elsewhere support—not replace—the ownership argument. Native client image operates with no curl executable installed.

### G5 — Production-qualified tools, services, task workers and serverless

**Outcome:** three maintained reference products prove adoption value.

- **G5.1 Indexer.** One codebase provides directory scan→incremental index→SQLite→CLI query and optional local HTTP query. Exercise Unicode paths, cancellation, large files, bounded memory, restart/recovery and pinned packaging. Include buffered/chunked file I/O over the stable buffer interface; qualify a corpus containing at least one 512 MiB file so the memory gate cannot be passed merely by choosing tiny files. An initial feature demo works on shipped platforms; bounded-memory qualification follows G2.2b and Windows parity follows G4.2. Publish a TS/Go/Rust comparison where functionality is truly equivalent, including cold startup and memory—not just hot loops.
- **G5.2 Service kit.** Library-level router/middleware, structured JSON, configuration, request IDs, structured logs, metrics/traces, health/readiness, TLS-aware deployment guidance, SQLite transactions/migrations and one production SQL integration through G6 ABI or a separately reviewed native interface. Authentication/crypto uses maintained implementations, with explicit authorization examples and negative tests. Keep database handles/resources typed at the language boundary; no unchecked forged integer handles.
- **G5.3 Durable worker kit.** Single-node SQLite-backed queue first: transactional claim, leases/expiry, retries/backoff, idempotency keys, cancellation, crash-safe state, bounded CPU/I/O execution, metrics and dead-letter inspection. Specify at-least-once delivery; externally visible effects require idempotent handlers or an outbox, not an exactly-once marketing claim. Kill/restart at every state transition; no acknowledged durable job disappears. Distributed/multi-host scheduling is a later extension triggered by this local contract and a concrete multi-host reference scenario.
- **G5.4 Serverless qualification.** Update existing Lambda/Cloud Run/Fly examples to native networking, tested packaging, actual CPU architecture support, cloud-secret injection, invocation errors/timeouts and graceful shutdown. Test Lambda with the local mock first; real cloud runs require authorized accounts/budget and are separate evidence. Do not claim universal cold-start wins from process-start proxies. Qualify 20 cold starts/provider/runtime/configuration, warm invocation distributions and total billed duration against matched baselines.

**Exit:** indexer processes a fixed1 GiB corpus with≤256 MiB peak RSS on reference hosts; cancellation completes≤2s. Service and worker24 h fault/soak gates pass; acknowledged jobs survive forced restarts; bounded worker concurrency holds. At equal semantics/concurrency, proposed service CPU-endpoint throughput≥0.90× matched Rust and p99≤1.20× at 70% of the slower implementation's measured sustainable capacity. I/O-limited cases are reported separately. Release includes deploy/runbooks, upgrades and reproducible failure recovery, not only a happy-path demo.

### G6 — An ecosystem bridge, recoverable embedding and trustworthy WASM

**Outcome:** adopt Turbo one component at a time; use isolated execution for untrusted workloads.

- **G6.1 Stable C-facing ABI.** Versioned handles, explicit buffer/string ownership and lengths, primitive/aggregate layout rules, callbacks with lifetime/thread-affinity contracts, error/trap return channel and ABI conformance. Host→Turbo calls accept arguments and typed buffers/results, not only zero-argument scalars. Recoverable VM execution must not route traps through process::exit; native standalone applications retain their documented process behavior. One VM's cleanup must not invalidate another VM's state. Fault injection and ABI fuzzing are mandatory.
- **G6.2 Ecosystem bindings.** Header-driven binding generator for a documented subset; reject unsupported declarations. Prove it with three useful libraries covering database, compression and graphics/window/input needs; library selection, license and packaging are explicit decisions. Add Node/Python wrappers only after buffer ABI/recovery are sound; benchmark boundary overhead and batch calls, avoid claiming speedup from tiny FFI calls. Keep npm/PyPI execution compatibility out of scope.
- **G6.3 WASM semantic completion.** First execute isolated remediation slices for existing BL-27B/BL-28 with reproducing native/WASM tests. Their fixes must pass before new WASM features, shared-lowering migration or supported-target promotion. These slices occupy the compiler-owner lane; no independent WASM rewrite may race layout/lowering work. Then align typed lowering/layout/drop semantics, closures/generics and containers with native. Enumerate supported pure-core features and WASI APIs; three-way native JIT/AOT/WASM tests cannot xfail a feature labeled supported. Provide generated host bindings/TypeScript declarations and one deployed host example; no assumption that all WASI hosts expose identical APIs.
- **G6.4 Isolation profiles.** Native libturbo remains trusted code even after trap recovery. For untrusted scripts, use a WASM engine or separately sandboxed OS process with explicit import capabilities, filesystem/network denial by default, time/fuel/memory/output limits and termination tests. Engine/library selected under dependency review. Expose trusted embedding and isolated workers as different APIs. Add a client-side playground only when its browser host contract is proven.

**Exit:** two independent VM instances survive alternating loads/errors/unloads in stress tests; deliberate language traps do not terminate the native host; ABI argument/buffer round trips pass; unknown imports denied; untrusted infinite-loop/allocation/I/O escape tests are contained. WASM supported-core conformance is100% with explicit platform exclusions, and Node/Python samples install from clean hosts without toolchain surprises.

### G7 — Real native desktop applications and practical game development

**Outcome:** two usable SDK surfaces with independent release lifecycles, sharing G2/G3/G4/G6 foundations.

- **G7.1 Choose integrations through executable spikes.** Compare narrow native-platform adapters with a mature cross-platform toolkit for desktop; compare graphics/window/input/audio libraries for games. Evaluate native widgets/accessibility, license, maintenance, OS support, binary/dependency cost, callbacks, packaging and FFI fit. Select from working examples, not screenshots. No toolkit is installed or predetermined by this plan.
- **G7.2 Native desktop SDK and reference app.** Build a graphical indexer front end with native menus/dialogs, keyboard navigation, text input/IME, accessibility tree, clipboard/drag-drop, notifications, settings, multiple-window lifecycle and background indexing/cancellation. Verify on macOS/Linux/Windows. A WebView implementation can be an optional application mode, but does not satisfy the native-widget acceptance gate. Supply installer/update/signing/notarization recipes and credential-independent local packaging tests; credentialed distribution remains a separate release gate.
- **G7.3 Game/tool SDK and reference game.** Window/input/audio/rendering/math/asset-loader bindings, resource lifetime rules and an allocation-conscious update loop. Ship a small playable 2D game plus a procedural asset tool; pause/resume, save/load, input rebinding, audio-device loss and resize/DPI behavior tested. A library binding alone is not a supported game-development product.
- **G7.4 Performance and extensibility proof.** Reference 10k-particle CPU update kernel at 60 Hz: proposed p99≤4 ms on named reference hardware; CPU frame work p99≤8 ms, zero hot-update allocations after warmup, no heap-growth trend during 30 min sessions. GPU time measured separately. Gameplay reload is staged module replacement with explicit state migration/rejection; compiler watch restart is not hot reload. Trusted gameplay embedding may use G6; untrusted mods must use isolated profiles. Add one 3D renderer integration only after 2D acceptance, without claiming a general3D engine.

**Exit:** fresh install produces runnable GUI and game artifacts on all three OS families; native accessibility/manual usability and automated interaction tests pass; profiling locates Turbo source functions; broken assets/invalid saves produce errors rather than host termination. External2D-game and desktop evaluators complete one extension task each; usability evidence remains separate from compiler correctness.

### G8 — A bounded route into constrained systems programming

**Outcome:** use Turbo for tightly controlled computational components without requiring its application runtime.

- **G8.1 Specify freestanding profile.** Build on G3.3: no implicit heap, no ARC, no OS/thread/network/database dependencies; explicit allocator interfaces only where the profile allows them; integer/layout/endianness/volatile/atomic/unsafe rules documented. The supported subset is named and checked. This is not a claim that all managed Turbo programs run without a runtime.
- **G8.2 Prove static-library integration first.** Compile a parser/control/signal-processing kernel into a C-callable object, integrate with a native host, and verify symbols contain no forbidden runtime dependencies. Cross-check output and resource bounds with an equivalent Rust no_std implementation.
- **G8.3 One freestanding target.** Choose a target/emulator only after backend/toolchain feasibility checks; run a fixed noalloc kernel without an OS, validate stack bounds experimentally/static analysis where available and define panic behavior. If the chosen backend cannot target it, retain the static-library deliverable and record the outstanding target work; do not mark G8 complete.

**Exit:** reproducible toolchain, link-map proof, emulator/hardware execution, no hidden heap/RC/OS calls, boundary safety tests. Hard-real-time, kernel/driver ecosystems, GPU kernels and mobile are follow-on proposals with explicit prerequisites—not implied supported markets.

## 6. Dependency graph, ownership and critical path

```text
Early value:  G1.1 fixes -> G2.1 evaluator -> useful native product slices
Core spine:   G1.2 -> G2.2a contracts -> G2.3 lowering -> G2.2b storage -> G3
Optimization: shared lowering/layout contracts -> G2.4/G2.5
Services:     G1 contracts -> G4 native client/OS/concurrency -> G5
Reach:        G2 layouts + G3 ownership -> G6.1 ABI -> G6.2 bindings -> G7
WASM:         BL-27B/BL-28 fixes + shared contracts -> G6.3 supported WASM
Isolation:    G6.3 + capability host -> G6.4 -> untrusted jobs/mods
Systems:      G3 constrained profile + ABI/backend feasibility -> G8
```

Scheduling dependencies are **slice-level**: G5.1's feature demo and G1.1 do not wait for entire compiler foundations; G7 discovery can precede its implementation gates. G5 external SQL bindings depend on G6.1/G6.2 and close in W3, but local SQLite service/worker qualification closes independently in W2. W2 does not mean all of G5 is complete. G6.3 must fix the present WASM bugs in place before migrating to shared lowering; preserve tests through both. Aggregate goals close only after all their exit gates pass. A compact-layout feature outside the WASM supported subset must hard-error on WASM until its layout contract is ported; target labels cannot silently authorize divergent semantics.

At most two active implementation lanes plus one independent verification lane initially. One owner controls layout/ownership/lowering interfaces. Parallel lanes may change disjoint OS adapters, application fixtures or packages only after interface contracts are merged. No simultaneous independent rewrites of parser/sema/codegen ownership rules.

## 7. Acceptance and release discipline

The companion acceptance spec defines fixtures, formulas, test layers and evidence retention. No goal closes from prose assertions or agent consensus alone. Tests must fail against the pre-change implementation for newly fixed behavior. Changes to semantics, ABI, layout or memory always receive independent review and adversarial cases.

Baseline commands remain required: workspace tests; release build; `tests/run_tests.sh`; native parity; Clippy all-targets with warnings denied; fmt. Add runtime C tests/sanitizers, WASM conformance and relevant application/platform gates. `cargo build --release` optimizes the Rust compiler executable; it is not evidence of new optimization of emitted Turbo code.

Promotion requires two consecutive release candidates passing the named target contract. A skipped target, missing cloud credential or unsigned artifact is recorded as unvalidated, not silently green. Normal local edit/test work continues when external promotion is blocked.

Every shipped slice follows the repo Lore commit protocol. Preserve unrelated work and existing BL history. No unsupported performance/marketing statement enters README/website; publish only committed reproducible runs with scope/hardware/date.

## 8. Pre-mortem and responses

1. **Foundation rewrite consumes the product.** Warning: two consecutive foundation slices land without passing an application-level benefit or new ownership/layout oracle. Response: halt new backend scope, deliver the smallest accepted vertical slice, retain legacy lowering until replacement passes. Owner: architect/lead.
2. **Easy syntax hides cost or unsafety.** Warning: implicit-copy/RC counts rise, a borrow escapes, or new managed code needs pervasive annotations. Response: reject unsafe optimization, tighten effect/ownership boundaries, retain ARC fallback for Level 0, require source-attributed cost proof and migration tests. Owner: runtime lead + independent verifier.
3. **Platform breadth creates several incompatible languages.** Warning: newly supported target uses xfails for common-core semantics or SDK uses undocumented raw handles. Response: block target promotion, fix shared contract first, narrow support labels without deleting the promised goal. Owner: platform lead + QA.

Additional risks: backend dependency size/build time (G2.5 gate); library licensing/maintenance (G6/G7 decision review); benchmark gaming (frozen corpus/safety equivalents/held-out tests); migration breaks (edition+AST migrator); cancellation leaks and data loss (G4/G5 fault injection); untrusted native execution (G6 separate isolation API); cloud/signing access (local tests plus explicitly blocked release evidence).

## 9. Architecture decision record

**Decision:** two-track incremental native-platform strategy; managed value semantics by default, explicit control for specialized code; shared typed/layout/ownership contracts; domain ecosystems in libraries.

**Drivers:** useful adoption now, removal of runtime performance ceilings, and consistent safe portability.

**Alternatives:** services-only; full foundation rewrite; immediate LLVM revival; unmanaged-by-default memory; embedding a JS VM for npm compatibility. The first two remain viable strategies but do not balance this user's goals; the latter three lose default simplicity, evidence discipline or native identity.

**Why chosen:** extends the implementation that already works while addressing the concrete barriers found in review. Does not require a speculative universal compile-time memory solution.

**Consequences:** two semantic editions during migration; richer compiler representations; explicit expert annotations at difficult boundaries; substantial independent SDK work; a benchmark suite can reject hoped-for claims. No promise that ARC matches ownership costs for arbitrary graphs.

**Follow-ups:** G1 semantic RFC; G2 layout/IR RFC and backend experiment; G3 ownership/effect/region RFC; G4 portable transport/scheduler decisions; G6 ABI/isolation decisions; G7 SDK selection. Detailed syntax, dependency selections and incompatible ABI changes require their own design review before implementation. This roadmap approves investigation/outcomes, not an invented frozen syntax.

## 10. Execution handoff and plan ownership

This is a planning deliverable. It does not start a Codex goal, automation, implementation branch, dependency install or deployment.

Available role roster relevant here: architect (high), executor (medium; high for memory/ABI work), test-engineer (medium), code-reviewer (high), verifier (high), dependency-expert (high), researcher (bounded official evidence), product-designer (high), docs-reviewer-writer (high), critic (high). Use current installed role model contracts, not stale hardcoded model names. No worker role outside an active OMX team.

Recommended follow-up: `$ultragoal` for durable goal execution; `$performance-goal` for the G2 measured optimization branch. Native App subagents can execute independent bounded slices. In an attached OMX CLI/tmux session, Team + Ultragoal can combine leader-owned goal checkpoints with parallel implementation lanes. Team is not directly available from this outside-tmux App surface.

Example launch hints, **not executed**:

```text
$ultragoal Execute the reviewed Turbo master plan, starting with G1.1 and G2.1; use its acceptance spec and slice dependencies.
$performance-goal Pursue G2 using the frozen evaluator; preserve semantics, safety and compile-time guardrails.
$team 2:executor Execute disjoint approved G4 OS-adapter and G5 application slices; one owner controls shared runtime interfaces.
omx team 2:executor "Execute approved Turbo G4/G5 slices from design/TURBO-MASTER-PLAN.md; verify design/TURBO-ACCEPTANCE-SPEC.md before handoff"
```

Confirm installed team CLI syntax when actually launching. Team verification: each lane supplies changed scope, regression proof, test output and interface compatibility; independent verifier runs combined parity/application gates; leader checkpoints only completed outcomes before shutdown. `$ralph` is an explicitly selected single-owner fallback, not the default. No research automation is needed for this product plan.

Requested documents are versionable under `design/`; `.omx/plans/` holds intake/review handoff records. Circular's current authorized project list lacks Turbo; no external tasks were created or misfiled. When a Turbo project becomes available, materialize these eight parent goals with their numbered slices once, reconcile BL-27/28 and existing issues, and track execution there. Do not treat a planning review as a human-approved Circular REVIEW step.

## 11. First execution package: strict entry order and owned lanes

Readiness is explicit: G1.1/G2.1 are execution-ready briefs after normal preflight; G1.2/G2.2a/G4.1 begin with reviewed contract/selection deliverables; later work is roadmap-ready, not permission to invent final syntax or skip a design gate.

| Entry order | Foundation owner: compiler executor | Product owner: runtime/application executor | Proof required before next entry |
|---|---|---|---|
| E0 | Resolve current checkout, relevant AGENTS and existing BL owners; no feature edits | No parallel implementation | Existing work preserved; map the user-directed plan to backlog once, without checking off future goals |
| E1 | G1.1: failing JSON regression, serializer fix, native proof | No concurrent compiler/runtime edits | Control-character/Unicode POST→GET→restart tests pass; regression fails before fix |
| E2 | G2.1: evaluator/counters and correctness oracles; freeze baseline after E1 | May review fixture scenarios read-only | Paired measurements reproducible; instrumentation validated; no source behavior changes disguised as measurement |
| E3 | G1.2 semantic contract, legacy/new-edition corpus, AST migration proof | G4.1 dependency/transport design and external protocol harness only; no shared runtime edits | Semantic migration approved; transport ABI and capability/error/lifetime contract accepted by compiler owner |
| E4 | G2.2a layout/drop/ABI descriptors preserving current behavior | G4.1 native-client adapter against accepted interface, outside compiler-owned files | Descriptor parity and no-curl happy/failure paths; compiler owner integrates any builtin/sema hook sequentially |
| E5 | G2.3 migrate one full typed-lowering feature slice | G5.1 feature demo and existing-API application tests; no new layout dependence | Old/new lowering differential pass, source/ownership/layout contract explicit |
| E6 | G2.2b compact byte/numeric buffers, then G3.1 views; never concurrent internal rewrites | Buffered I/O/indexer qualification only after buffer interface merged; otherwise adapter tests | All layout readers/writers/drop/call paths pass; source-visible cost benefit measured |
| E7 | Sequential G2 optimization/G3 regions/noalloc or isolated BL-27B/BL-28 remediation as selected by dependencies | G4 OS/concurrency and G5 SQLite worker slices against merged contracts | Independent combined qualification; no aggregate goal closed from a partial slice |

**Named responsibility map (roles, not assumed human staffing):** compiler executor exclusively owns AST/parser/sema/typed lowering/layout/codegen and shared drop/call ABI. Runtime/application executor owns isolated OS/transport modules, packages, example applications and application harnesses after interface acceptance. Existing shared `runtime.rs`, `turbo_rt.c` and builtin registries are edited by one explicitly assigned owner per slice; default compiler owner integrates adapter hooks. A separate verifier owns holdout cases, migration oracle review, combined tests and promotion verdicts; it does not silently fix the implementation under review. Lead resolves conflicts and gates continuation. Interface changes require consumer tests before the other lane resumes.

Only two implementation lanes and one verifier may be active; a role may wait or do read-only discovery when dependencies are unmet. Research, reviews and design proposals are not loopholes to start competing code rewrites. This keeps native-client/application work useful while foundation changes progress in one strict sequence.

## 12. External architectural evidence

Bounded official-source research, retrieved 2026-09-06, informs constraints rather than predetermining dependency choices:

- [Rust ownership](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html) and [borrowing](https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html): borrowing provides access without ownership transfer; use this as a model for explicit lifetime/alias contracts, not a claim that Turbo already has them.
- [Rust Embedded Book: no_std](https://doc.rust-lang.org/beta/embedded-book/intro/no-std.html): freestanding/core functionality and allocator-backed facilities are separate concerns. A noalloc kernel, a no_std library and an entire kernel OS are not interchangeable goals.
- [Cranelift](https://cranelift.dev/): the upstream project emphasizes fast compilation and comparatively simple optimization. This supports preserving the development backend and requiring measured evidence before adopting a heavier release backend; it does not establish a particular Turbo speed ceiling.
- [WASI](https://wasi.dev/) and [Wasmtime](https://docs.wasmtime.dev/): capabilities and host-granted resources define the guest boundary. Isolation still depends on engine configuration and correct host imports; neither source certifies Turbo's implementation. Wasmtime is an example to evaluate, not an adopted dependency.

Evidence wrapper: best-practice-research with a bounded researcher lane. GUI/library architecture is the plan's inference from host boundaries and product goals, not a prescription from WASI documentation. Pin exact dependency/API versions during implementation decisions.

## Review changelog

- Revision 1: initial source-grounded plan; accepts the broader user destination, makes memory/control and platform support explicit, and separates performance targets from observed results. Independent consensus pending.
- Revision 2: Architect feedback incorporated: strict E0–E7 rollout, named role/file ownership, G2.2 contract/storage split, WASM remediation prerequisites and no competing lowering rewrites. Also clarified safe pre-borrow thread transfer, external SQL's W3 qualification and the large-file streaming prerequisite. Awaiting re-review.
- Revision 3: Critic's sole blocking source reference corrected to `turbo/crates/turbo-cli/src/watch.rs:7`. Accepted optional improvements: bounded G1.5 ergonomics and explicit pending-capability treatment of future G2/G3 fixtures. Full Architect→Critic re-review required before final consensus.
- Final consensus: dedicated Architect re-review APPROVE, followed by dedicated Critic re-review APPROVE; no blocking findings remain. Review artifacts and terminal planning handoff are under `.omx/plans/turbo-native-platform-*`. Only planning documents were changed; all eight implementation outcomes remain unstarted.
