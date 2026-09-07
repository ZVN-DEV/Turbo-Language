# Turbo Roadmap

Turbo's roadmap is about one promise: familiar code first, native execution
today, and deeper control only when the program needs it. There are no calendar
promises here. Each section moves forward when the dependency and acceptance
criteria are met.

## Market focus

Turbo is strongest where TypeScript/JavaScript-shaped teams want native
artifacts without moving all the way to Rust's day-one complexity.

| Market | Fit now | What must improve |
|---|---|---|
| CLI tools and developer utilities | Strong | package polish, installer smoke tests, more real-user flows |
| Local/system-adjacent apps | Strong for backends and tools | OS conformance on macOS, Linux, and Windows; packaging; filesystem/process edge cases |
| Small HTTP + SQLite services | Good behind a reverse proxy | native TLS/client stack, bounded async runtime, soak/load evidence |
| Task servers and workers | Promising | durable queue contract, cancellation, recovery, backpressure, 24-hour fault/soak tests |
| Native desktop apps | Promising backend/tooling language | GUI bindings, accessibility inspection, signing/notarization/package tests |
| Game development | Good first for tools, asset pipelines, simulation kernels | no-allocation hot paths, graphics/input/audio bindings, frame-budget evidence |
| Systems/freestanding | Future planned profile | no-heap/no-OS profile, explicit panic model, emulator/hardware proof |

Turbo should not pretend to be a mature game engine language, a kernel language,
a browser app framework, or a distributed-systems platform yet. Those can become
credible only after the lower-level runtime, memory, OS, and benchmark work lands.

## Public release lines

Use [GitHub Releases](https://github.com/ZVN-DEV/Turbo-Language/releases) as
the authoritative source for which versioned artifacts are actually available.
The capability scope below describes the current v0.16 release line; if a tag or
package has not been published yet, treat the same scope as release-candidate
documentation rather than a published availability claim. Platform support still
follows [COMPATIBILITY.md](COMPATIBILITY.md).

- Native JIT and AOT compilation through Cranelift.
- Static type checking with structs, enums, pattern matching, generics, traits,
  optionals, results, closures, and first-class function values.
- Built-in HashMap, JSON, SQLite, HTTP server, file I/O, process/env access,
  formatter, tests, REPL/playground, package install/search, and LSP.
- Runtime ARC and copy-on-write release coverage has expanded across strings,
  arrays, structs, enums, results, optionals, recursive values, and typed
  containers.
- JSON extraction keeps source text meaning across selected values and validates
  bounded malformed cases against serde behavior.
- The paired evaluator now records raw commands, output hashes, confidence
  intervals, allocation-profile records, and explicit incomplete qualification.
- New diagnostic fixtures cover recursive tree allocation balance and managed
  particle allocation balance.
- macOS/Linux native path as the primary support target; Windows and WASM remain
  narrower and explicitly qualified by compatibility status.

This is enough for useful real programs, especially tools, local services,
single-binary demos, and bounded compute experiments. It is not enough for a
blanket "Rust speed" claim.

## In development

These are the near-term release-closeout and correctness lanes already supported
by committed evidence or active work:

1. **Truthful release posture.** Public claims must match committed evidence.
   Success means the README, roadmap, performance page, safety docs, changelog,
   package copy, and website say the same thing.
2. **Performance evaluator hardening.** The evaluator must keep raw commands,
   hardware/toolchain/source provenance, randomized paired samples, output
   equality, confidence intervals, and qualification status. Success means a
   benchmark can fail without being hidden.
3. **JSON workload parity API closure.** The current balanced JSON fixture is
   useful diagnostic evidence, but the Rust comparison is explicitly not yet
   algorithmically equivalent because Turbo lacks the same parse-once and
   counter-entry path. Success means the comparison remains labeled diagnostic
   until equivalent APIs exist, then graduates only through the normal evaluator
   qualification rules.
4. **Managed memory correctness.** ARC/COW release paths must continue expanding
   across structs, enums, results, optionals, recursive values, closures, and
   containers. Success means sanitizer runs, profile counters, and native parity
   all agree.
5. **Benchmarked workload coverage.** Current evidence now reaches beyond fib
   and word count, but the full catalog is not qualified. JSON parse/transform,
   string/token processing, hashmap churn, buffer scan, packed particles, and
   tree traversal all need reproducible fixtures, fair comparators, enough sample
   duration, host/provenance evidence, and explicit pass/fail status.

## Planned

These are the product and language layers that turn Turbo from promising into
credible for larger markets.

### Predictable JavaScript-familiar semantics

Turbo should keep the JS/TS on-ramp while removing semantic surprises. Planned
work includes editions/migrations, record updates, contextual arrows, clearer
mutation rules, and compatibility tests for old examples.

Acceptance:

- Existing examples keep their behavior or fail with migration diagnostics.
- New syntax is introduced through an edition or explicit compatibility boundary.
- Formatter, parser, semantic analysis, JIT, AOT, and docs move together.

### Rust-class performance and memory control

The current runtime ARC/COW model is the starting point. The deeper goal is a
progressive control ladder: better layouts, borrowed views, owned buffers,
regions, no-allocation kernels, and eventually manual/unsafe escape hatches for
code that genuinely needs them.

Acceptance:

- Managed CPU geometric mean is ≤1.15× Rust and each individual workload is
  ≤1.35× Rust under the published evaluator.
- Controlled Turbo is ≤1.05× Rust geometric mean and ≤1.15× per workload once
  the controlled language features exist.
- Fixed ≥32 MiB live-payload workloads are ≤1.25× Rust managed and ≤1.10× Rust
  controlled.
- No-allocation kernels report exactly zero allocations and zero RC operations
  after warmup.

### Service and worker runtime

Turbo's service story should become more than "thread per connection behind a
proxy." Planned work includes native client TLS, bounded async scheduling,
cancellation, backpressure, durable worker recovery, load/soak tests, and clear
deployment guidance.

Acceptance:

- HTTP and worker claims pass protocol, cancellation, overload, fault-injection,
  and 24-hour soak suites.
- Throughput and p99 latency are reported against matched Rust implementations
  at sustainable capacity, not as universal RPS numbers.
- Provider/serverless claims include real provider credentials, forced cold
  starts, warm invocations, raw metrics, and equal architecture/memory/work.

### Platform support

Turbo should earn each platform label by running on that platform, not by
cross-compiling an object file.

Acceptance:

- macOS ARM64 and Linux x86-64 are required for promoted performance claims.
- Windows x86-64, macOS x86-64, and Linux ARM64 have explicit separate results
  until their native runners pass the same support matrix.
- Path encodings, long paths, spaces, process quoting, environment behavior,
  filesystem watching, cleanup, service lifecycle, and SQLite are covered.

### Native desktop and game development

The first credible layer is tools and backends. Native UI and game runtime work
comes after platform, memory, and scheduler work are measurable.

Acceptance:

- Desktop: real native widget/accessibility inspection, keyboard navigation,
  screen-reader labels, IME/composition, dialogs/menus, clipboard/drag/drop,
  DPI/multiwindow, graceful quit, and package install/uninstall/update smoke.
- Games: deterministic particle fixtures, 30-minute 60 Hz sessions, CPU update
  p99 ≤4 ms and frame CPU p99 ≤8 ms on a reference host, zero update-loop
  allocations after warmup, save/reload checksums, resize/input/device-loss
  handling, and resource lifetime assertions.

### Freestanding / systems profile

Turbo's native compiler makes this an eventual planned direction, but it needs a
separate profile rather than ordinary runtime ARC binaries.

Acceptance:

- Link-map and symbol inspection prove no heap, RC, or OS dependencies.
- C host tests run before emulator or hardware oracle tests.
- Panic behavior, stack/resource limits, and forbidden-operation diagnostics are
  explicit.
- The comparator is Rust `no_std` with matched algorithm and safety boundaries.

## Exploratory

These areas are intentionally not committed product promises yet:

- GPU kernels, tensor/ML runtime, and mobile targets.
- Distributed actors, service mesh primitives, and consensus libraries.
- Untrusted sandbox execution inside the same process.
- Core-language agent/tool keywords. Agent/tool work belongs in sidecar
  libraries once the service, async, and typed-serialization layers are ready.

Exploratory does not mean "never." It means Turbo needs evidence and lower-level
foundations before these should be marketed as product commitments.

## How roadmap items graduate

A feature moves from planned to released only when it has:

- implementation in both relevant runtime paths;
- behavior tests that would fail without it;
- native parity where applicable;
- compatibility/migration notes when syntax or semantics change;
- reproducible benchmark or soak evidence for performance/resource claims;
- honest documentation of limitations that remain.

If any of those are missing, the feature can still be discussed as planned or
experimental, but not as a shipped guarantee.
