# Turbo Vision

**Name**: Turbo

**One-liner**: Turbo is a systems-capable, type-safe, developer-loved language with JavaScript's spirit and Rust's performance — a small, honest core that ships as a compiled general-purpose language.

---

## Why Turbo Exists

The languages that offer real performance and safety demand a brutal learning curve. The languages that developers love lack the power to build serious systems. Turbo exists to close that gap.

Turbo is the language we wish we had: one that compiles to native code and WebAssembly, enforces safety without a garbage collector, ships world-class tooling on day one, and keeps its core surface small enough for a working engineer to hold in their head.

This is not incremental improvement. This is a new foundation.

> **A note on agentic features.** Earlier drafts of this document promised `agent` / `tool fn` keywords and first-class AI primitives baked into the language. That direction has been retired: agentic workflows will ship as a separate `turbo-agent` library built on top of the stable 1.0 core (async, HTTP, typed serialization), not as compiler-level keywords. The rationale is simple — language-level AI primitives would couple Turbo's core evolution to a rapidly-moving ecosystem (model providers, tool-calling schemas, memory strategies) that belongs in library space. See **COMPATIBILITY.md** for the stability contract.

---

## Core Pillars

### 1. Performance -- No Compromises

**Target**: Within 5% of C/Rust on compute-bound benchmarks. Within 10% on allocation-heavy workloads.

Performance is not a feature you add later. It is an architectural decision that permeates every layer of Turbo. We commit to:

- **No garbage collector by default.** Memory is managed through a simplified ownership model that learns from Rust's borrow checker but dramatically reduces the annotation burden. Regions, arenas, and deterministic destruction give developers precise control without the cognitive overhead.
- **Compiles to native code and WebAssembly.** LLVM backend for optimized native builds targeting x86-64, AArch64, and RISC-V. A dedicated WASM compilation pipeline for web and edge deployment. The same source code, multiple high-performance targets.
- **Zero-cost abstractions.** Generics are monomorphized for zero-cost performance, with optional runtime type metadata available via `@derive(TypeInfo)` for scenarios requiring reflection. Traits are statically dispatched by default (with opt-in dynamic dispatch via `dyn`). Iterators fuse and inline. The abstractions you use to write clear code should generate the same machine code you would write by hand.
- **Predictable performance.** No hidden allocations. No surprise GC pauses. No implicit copies of large data. Turbo makes the cost of operations visible and controllable. When you need deterministic latency -- for games, trading systems, audio, or real-time AI inference -- Turbo delivers.
- **Compile-time computation.** A powerful `const fn` system (inspired by Zig, adapted with familiar naming) lets you move work from runtime to compile time. Constant evaluation, compile-time code generation, and static reflection reduce runtime overhead to the absolute minimum.

We benchmark obsessively. Every feature is evaluated for its performance impact. If an abstraction cannot be zero-cost, it must justify its overhead with extraordinary utility.

### 2. Type Safety -- Sound, Expressive, Helpful

**Target**: A type system that catches real bugs at compile time while feeling like a productivity multiplier, not a tax.

Type safety is the foundation of reliable software. But too many type systems feel like they are fighting the developer instead of helping them. We commit to a type system that is both rigorous and humane:

- **Sound by default.** The type system does not lie. If it compiles, the types are correct. The `any` type exists for FFI boundaries and dynamic scenarios but is discouraged in application code. The type system guides developers toward `T?` optionals and generics instead.
- **No null. Ever.** The billion-dollar mistake ends here. Optional values are expressed as `T?`, forcing explicit handling. The `?` operator, `if let`, `guard let`, and pattern matching make working with optionals and results ergonomic, not tedious.
- **Algebraic data types with exhaustive matching.** Enums carry data. Pattern matching is exhaustive -- the compiler ensures you handle every case. When you add a variant, every match expression that needs updating is flagged at compile time.
- **Monomorphized generics with opt-in runtime type metadata.** Generics are monomorphized at compile time for zero-cost performance (like Rust and C++). Unlike Java's type erasure, Turbo can retain type information at runtime when you need it -- opt in with `@derive(TypeInfo)` on types that require reflection, runtime type checks, or serialization. This gives you zero-cost generics by default with the ability to access runtime type metadata when your use case demands it.
- **Result-based error handling with `?` propagation.** Errors are values, expressed as `T ! E` (read: "T or error E"). The `?` operator propagates errors concisely. No exceptions, no hidden control flow. Error types compose, and the compiler ensures you handle every failure path.
- **Structural typing for interfaces, nominal for data types.** Interfaces (traits) use structural subtyping -- if your type has the right shape, it satisfies the interface, no explicit declaration required. Data types (structs, enums) use nominal typing -- two types with the same fields are still distinct types. This gives you the flexibility of duck typing where it helps and the safety of nominal typing where it matters.
- **Effect tracking in signatures.** Functions declare their effects: `async`, `throws`, `io`, `unsafe`, `diverges`. The compiler tracks effect propagation. You can see at a glance what a function might do, and the type system ensures effects are handled appropriately.

### 3. Developer Love -- Because Life Is Too Short for Bad Tooling

**Target**: Sub-second incremental compilation in dev mode. Error messages that teach. A complete toolchain from day one.

A language is only as good as the experience of using it. We refuse to ship Turbo and tell developers to wait for the ecosystem to catch up. From the first public release, developers will have:

- **Sub-second incremental compilation.** In dev mode, the compiler uses a cranelift-style fast backend, incremental compilation, and fine-grained dependency tracking to rebuild only what changed. Edit, save, see results -- in under a second for most projects. Full LLVM optimization is reserved for release builds.
- **Error messages that teach.** Every error message includes: what went wrong, where it went wrong, why it went wrong, and how to fix it. Inspired by Elm and Rust, Turbo's error messages are mini-tutorials. They show the relevant code with color-coded annotations, suggest fixes (with `--fix` to auto-apply), and link to detailed explanations. A new developer should be able to learn Turbo primarily through compiler feedback.
- **A complete toolchain, shipped together:**
  - **Package manager** -- Dependency resolution, lockfiles, workspaces, publishing, semantic versioning enforcement. Think Cargo, but with lessons learned.
  - **Formatter** -- One canonical style, zero configuration, instant execution. Code formatting debates are over.
  - **Linter** -- Catches not just errors but anti-patterns, performance pitfalls, and style issues. Extensible with custom rules.
  - **Test runner** -- Built into Turbo. Tests live next to code. Property-based testing, snapshot testing, and benchmark testing are first-class.
  - **LSP server** -- Full Language Server Protocol support from day one. Autocomplete, go-to-definition, find-references, rename, inline type hints, and real-time error checking in every major editor.
  - **REPL** -- An interactive environment for exploration, prototyping, and learning. Supports full language features including type inference and autocomplete.
  - **Documentation generator** -- Generates beautiful, searchable documentation from doc comments. Examples in doc comments are compiled and tested. The standard library documentation is the gold standard.
- **Clean, expressive syntax.** We have studied 25 languages to find the most readable, writable, and learnable syntax for each construct. The goal is code that reads like well-written prose: clear intent, minimal ceremony, no line noise. Semicolons are optional (newline-terminated statements). Braces for blocks. Type annotations where they help, inference where they do not.
- **Amazing onboarding.** A new developer should go from zero to a running program in under five minutes. The installation is a single command. The `new` command scaffolds a project. The tutorial is interactive and runs in the REPL. The standard library is comprehensive and well-documented.

### 4. A Small, Honest Core

**Target**: A language whose full feature surface can be held in one engineer's head, and whose stable parts stay stable.

Turbo deliberately does not ship agentic keywords, GPU kernels, a distributed actor system, or other framework-shaped features as compiler built-ins. Those belong in libraries that can iterate on their own cadence without dragging the whole language along for the ride.

What stays in the core:

- The type system (generics, traits, ADTs, pattern matching, Result/Optional).
- Memory model (CoW values, deterministic destruction, arenas where they help).
- Async/await, spawn, channels, mutex.
- C FFI.
- AOT + JIT compilation and a WASM target.
- The built-in toolchain (formatter, LSP, tests, REPL, playground).

What lives outside the core, as separate libraries with their own SemVer stories:

- **`turbo-agent`** — the agent/tool/provider surface that earlier drafts of Turbo tried to bake in. Ships after the core hits 1.0, builds on async + HTTP + typed serialization, and can evolve with the LLM ecosystem without re-cutting the compiler. Out of scope for the core language contract.
- **`turbo-tensor` / GPU compute, mobile UI, distributed actors** — explored in the roadmap as potential sidecar libraries, not as language keywords.

This is a trust move. A small core that actually ships and stays stable is worth more to real users than a giant feature list with shifting ground underneath it.

### 5. Broad Reach -- One Language, Every Platform

**Target**: Write once, deploy to native, web, server, embedded, and edge -- without compromise.

- **Native compilation via LLVM.** Target x86-64, AArch64, and RISC-V with full optimization. Produce standalone binaries with no runtime dependencies. Cross-compilation is a first-class workflow.
- **WebAssembly as a first-class target.** WASM is not an afterthought -- it is a primary compilation target with its own optimization pipeline. Build web frontends, edge functions, and browser-based tools in the same language as your backend.
- **JavaScript interop bridge.** For WASM targets, a seamless interop layer lets you call JavaScript APIs, manipulate the DOM, and integrate with existing JS ecosystems. TypeScript type definitions are generated automatically.
- **C FFI for ecosystem access.** Call into C libraries with zero overhead. Bind to system APIs, graphics libraries, database drivers, and the vast ecosystem of C code. FFI declarations are type-checked and memory-safe at the boundary.
- **Embedded and IoT support.** A `no_std` mode strips the standard library to a minimal core suitable for microcontrollers and constrained environments. Deterministic memory management and no GC make Turbo viable where Rust goes today.
- **Server-native.** Async runtime built in. HTTP, gRPC, and WebSocket support in the standard library. Connection pooling, graceful shutdown, and structured concurrency are language-level patterns.

---

## Why Now?

The conditions for Turbo have never been better:

**Rust proved you do not need a garbage collector for memory safety -- but the learning curve is too steep for most developers.** Rust's insight was revolutionary: ownership and borrowing can replace garbage collection. But Rust's implementation demands expertise that most developers do not have and do not need. We can simplify the model dramatically for 90% of use cases while providing escape hatches for the other 10%.

**TypeScript showed that developers want types -- but they want sound types with great developer experience.** TypeScript's adoption proved that developers will embrace type systems when they help rather than hinder. But TypeScript's types are unsound by design, and its performance ceiling is JavaScript. We can do better on both fronts.

**The WebAssembly ecosystem is mature enough for a "compile everywhere" strategy.** WASI, component model, and browser WASM support have reached the tipping point. A language designed today can target WASM as a first-class platform without the compromises earlier languages had to make.

**Developer tooling expectations are at an all-time high.** Cargo, Go's built-in tools, Deno, and Bun have shown developers what great tooling looks like. A new language that ships without a package manager, formatter, and LSP is dead on arrival. We ship the complete experience from day one.

---

## What Turbo Is NOT

Clarity about what Turbo is not is as important as clarity about what it is:

- **Not a "better JavaScript."** We do not compile to JavaScript. We do not target the JavaScript runtime. We compile to native machine code and WebAssembly. JavaScript interop exists for the WASM-in-browser use case, but Turbo is not part of the JS ecosystem.

- **Not a Rust clone.** We learn from Rust enormously -- ownership, algebraic types, pattern matching, traits, zero-cost abstractions. But Turbo is not Rust. We simplify the memory model (fewer lifetime annotations, more inference, arena-based patterns as defaults). We prioritize developer experience over maximum control. If Rust is a formula one car, Turbo is a high-performance sports car: almost as fast, dramatically easier to drive.

- **Not an AI-native language.** Earlier drafts positioned Turbo as "agentic-first" with language-level `agent` / `tool fn` keywords. That positioning has been retired. Agent and tool workflows will ship as `turbo-agent`, a library on top of the stable core — not as compiler features.

- **Not an academic language.** Every feature in Turbo must earn its place through practical utility. We do not add features because they are theoretically elegant. We add them because real developers building real software need them. Features that do not pull their weight get cut.

- **Not a niche language.** We do not target a single domain. Turbo is a general-purpose language that happens to be exceptional for AI agents, systems programming, web development, and embedded systems. A developer who learns Turbo can use it for everything from a microcontroller to a distributed AI system.

---

## Design Philosophy

### Steal from the Best

We have studied 25 languages in depth -- their syntax, semantics, type systems, memory models, tooling, and communities. For every language construct, we ask: who did this best? Then we take that idea, adapt it to our context, and make it ours.

This is not about novelty. This is about synthesis. The best programming language of the 2020s should not ignore the lessons of the languages that came before it.

### Data-Driven Decisions

When we face a design decision -- which memory model? which syntax for generics? which error handling strategy? -- we do not argue from taste. We prototype multiple approaches, benchmark them, user-test them, and let the data decide.

We are building multiple memory model variants and running them through real-world workloads. The variant that offers the best balance of performance, safety, and ergonomics wins. Not the one that is most elegant. Not the one that is most familiar. The one that works best.

### Iterative, Not Waterfall

We do not design the entire language in isolation and then ship it. We build working subsets, put them in developers' hands, measure what works and what does not, and iterate.

The first milestone is a minimal viable language that can compile and run basic programs. From there, we add features in order of impact, testing each one against real use cases before committing to it.

### Community-First

Turbo is open source from day one. Not "open source after we've made all the decisions" -- open source during the design process. An RFC process governs Turbo's evolution. Major decisions are discussed publicly. The community has real input, not performative input.

We believe the best languages are built by communities, not committees.

### Opinionated Defaults, Escape Hatches Available

There should be one obvious way to do common things. The formatter enforces one style. The package manager enforces one project structure. The error handling uses one pattern. The async model uses one runtime.

But power users can go deeper. Unsafe blocks for manual memory management. Custom allocators for performance-critical paths. Raw pointers when you need them. The defaults are safe and ergonomic; the escape hatches are explicit and auditable.

---

## Target Audiences (in Priority Order)

### 1. Backend/Systems Engineers
Building servers, CLI tools, infrastructure software, databases, message queues, and distributed systems. These developers need performance, reliability, and control. They currently choose between Rust (powerful but demanding), Go (simple but limited), and C++ (fast but dangerous). Turbo offers Rust-class performance with dramatically better ergonomics.

### 2. Full-Stack Developers
Building web applications with WASM frontends and native backends in the same language. These developers currently juggle TypeScript on the frontend and a different language on the backend. Turbo lets them write the entire stack in one language, sharing types, validation logic, and business rules across the boundary.

### 3. Game and Real-Time Developers
Building games, audio engines, trading systems, robotics, and anything that demands deterministic performance. These developers need precise memory control, predictable latency, and zero-overhead abstractions. Turbo's ownership model and arena-based allocation patterns give them the control they need without the complexity of raw C++ or full Rust lifetimes.

### 4. Embedded Developers
Building for IoT devices, microcontrollers, and constrained environments. These developers need small binaries, no runtime dependencies, and predictable memory usage. Turbo's `no_std` mode and compile-time computation give them a modern, type-safe language that fits in the smallest targets.

### 5. Future: `turbo-agent` Library Users
Developers who eventually want to build LLM-powered agents, tool-calling systems, or RAG pipelines on top of Turbo. This is not a core-language audience — it's a library audience served by the post-1.0 `turbo-agent` sidecar. Turbo's typed async/HTTP/serialization primitives make it a credible substrate for that library, but the language itself doesn't ship agentic keywords.

---

## Success Metrics

We hold ourselves accountable to concrete, measurable goals:

| Metric | Target | Timeframe |
|--------|--------|-----------|
| TIOBE Index ranking | Top 20 | Within 3 years of stable release |
| StackOverflow "loved" rating | >80% | First year of survey eligibility |
| Incremental compile time (dev mode) | <2 seconds | At stable release |
| Performance vs. Rust | Within 10% on standard benchmarks | At stable release |
| Performance vs. C (compute-bound) | Within 5% | At stable release |
| Package registry | 1,000+ packages | Within 1 year of stable release |
| Active developers | 10,000+ | Within 2 years of stable release |
| Production deployments | 100+ companies | Within 2 years of stable release |

---

## Roadmap

Turbo ships as a focused core and grows through progressive disclosure -- every new capability is opt-in and never complicates the base language. See [ROADMAP.md](ROADMAP.md) for the full plan.

| Version | Milestone | Key Addition |
|---------|-----------|-------------|
| **v1.0** | Core | Syntax, types, CTRC memory, async, full toolchain, LLVM + WASM |
| **v1.1** | Script Mode | `turbolang run file.tb` with zero config, shebang, REPL, full inference |
| **v1.2+** | GPU & Compute | SIMD intrinsics and ML/GPU kernels delivered as sidecar libraries on top of stable core primitives, not new compiler keywords |
| **v1.3+** | Mobile | iOS + Android targets via LLVM. UI frameworks ship as libraries, not language built-ins |
| **v1.4+** | Distributed | Distributed-actor and consensus crates built on v1.0's `actor` + `spawn`; no new syntax |

> **Sidecar, not syntax.** The 2026-04-09 retirement of `agent` / `tool fn` taught us that domain features tied to fast-moving ecosystems (AI frameworks, GPU APIs, UI toolkits, cluster runtimes) belong in companion libraries whose cadence can diverge from the compiler's stability contract. The v1.2+ rows above are framework-shaped, not keyword-shaped.

The principle: a "Hello, world!" in v1.0 looks identical in v1.4. Complexity is always opt-in.

---

## The Road Ahead

This document is a compass, not a map. The destination is clear: a compiled, type-safe, systems-capable language that makes building real software a joy rather than a struggle, with a core small enough to stay stable. The exact path will be shaped by data, community input, and the reality of implementation.

What we are building combines systems-level performance, a sound type system, and a world-class developer experience in a package that ships today and promises a well-defined stability contract at 1.0.

---

*This is a living document. It will evolve as we learn, build, and grow. Every word here is a commitment, not a wish. We will hold ourselves to this vision -- and we invite the community to hold us to it too.*
