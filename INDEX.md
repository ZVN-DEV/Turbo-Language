# Turbo -- Project Index

Turbo is a compiled, type-safe, systems-capable programming language with JavaScript's developer experience, Rust's performance characteristics, and the first native language-level primitives for building AI agents. It compiles to native code via LLVM and to WebAssembly, ships a complete toolchain on day one, and manages memory through a simplified ownership model (CTRC + auto-clone) that eliminates garbage collection without demanding Rust-level annotation burden.

**North star:** JavaScript's soul. Rust's speed. Built for the AI age.

**Current status:** Design phase complete. Five review rounds (6.5 to 9.0 score progression). Final verdict: "Ship It."

---

## Design Documents

The complete language specification lives in `design/`. Each document covers one major subsystem.

| Document | Description |
|----------|-------------|
| [design/VISION.md](design/VISION.md) | Language identity, core pillars, target audiences, success metrics |
| [design/SYNTAX.md](design/SYNTAX.md) | Complete syntax reference with JavaScript cheat sheet for every construct |
| [design/TYPE-SYSTEM.md](design/TYPE-SYSTEM.md) | Types, generics, error handling (`T ! E`), string model, algebraic data types |
| [design/MEMORY-MODEL.md](design/MEMORY-MODEL.md) | CTRC ownership, auto-clone semantics, regions, arenas, escape hatches |
| [design/CONCURRENCY.md](design/CONCURRENCY.md) | Async/await, actors, channels, structured concurrency, supervision trees |
| [design/AGENTIC.md](design/AGENTIC.md) | AI agent primitives: `tool fn`, `agent` keyword, streaming, memory, supervision |
| [design/COMPILATION.md](design/COMPILATION.md) | LLVM + Cranelift backends, WASM pipeline, sanitizers, compilation modes |
| [design/TOOLCHAIN.md](design/TOOLCHAIN.md) | CLI (`turbolang build/run/test/fmt/bench`), testing framework, stdlib, profiler |
| [design/POLYGLOT.md](design/POLYGLOT.md) | FFI (C/C++), WASM interop, JavaScript bridge, TypeScript type generation |
| [design/ROADMAP.md](design/ROADMAP.md) | Progressive disclosure roadmap: v1.0 Core through v1.4 Distributed |
| [design/VARIANTS.md](design/VARIANTS.md) | Historical: memory model alternatives evaluated during design phase |

---

## Research Documents

The `research/` directory contains the competitive landscape analysis and language survey that informed every design decision. Nine documents covering 25 languages.

| Document | Description |
|----------|-------------|
| [research/00-landscape.md](research/00-landscape.md) | Programming language landscape overview and methodology |
| [research/01-design-innovations.md](research/01-design-innovations.md) | Key design innovations across modern languages |
| [research/02-polyglot-transpilation.md](research/02-polyglot-transpilation.md) | Polyglot and transpilation strategies |
| [research/03-languages-js-ts-python-ruby-elixir.md](research/03-languages-js-ts-python-ruby-elixir.md) | JavaScript, TypeScript, Python, Ruby, Elixir |
| [research/04-languages-rust-go-swift-zig-carbon.md](research/04-languages-rust-go-swift-zig-carbon.md) | Rust, Go, Swift, Zig, Carbon |
| [research/05-languages-cpp-csharp-kotlin-java-dart.md](research/05-languages-cpp-csharp-kotlin-java-dart.md) | C++, C#, Kotlin, Java, Dart |
| [research/06-languages-scala-haskell-lua-php-v.md](research/06-languages-scala-haskell-lua-php-v.md) | Scala, Haskell, Lua, PHP, V |
| [research/07-languages-fsharp-clojure-julia-r-perl.md](research/07-languages-fsharp-clojure-julia-r-perl.md) | F#, Clojure, Julia, R, Perl |
| [research/08-synthesis.md](research/08-synthesis.md) | Cross-language synthesis and final design recommendations |

---

## Example Applications

Five progressively complex applications demonstrating Turbo across different domains. Each is a complete project with `turbo.toml`, source code, and tests.

| Example | Description | Key Features Demonstrated |
|---------|-------------|--------------------------|
| [examples/task-agent/](examples/task-agent/) | Task management REST API with AI agent | Routes, agents, `tool fn`, testing, async, `Shared<T>` |
| [examples/web-api/](examples/web-api/) | Production bookmarking API (BookmarkAPI) | JWT auth, WebSocket, search, middleware, rate limiting, metrics |
| [examples/desktop-app/](examples/desktop-app/) | Native desktop markdown editor (TurboNotes) | Event-driven architecture, file I/O, AI writing assistant, keyboard shortcuts |
| [examples/realtime-system/](examples/realtime-system/) | Trading order matching engine (TurboExchange) | Zero-alloc hot paths, actors, regions, sub-microsecond latency |
| [examples/edge-wasm/](examples/edge-wasm/) | Edge image processing service (TurboEdge) | WASM compilation, streaming pipelines, `const fn`, CDN deployment |

---

## Showcase

The `showcase/` directory contains the public-facing website for Turbo.

| File | Description |
|------|-------------|
| [showcase/index.html](showcase/index.html) | Landing page with benchmarks, code samples, and feature highlights |
| [showcase/getting-started.html](showcase/getting-started.html) | Installation guide and quick tour of the language |
| [showcase/docs.html](showcase/docs.html) | Full reference documentation (syntax, types, stdlib, toolchain) |

---

## Design Reviews

The design was refined through five review rounds with progressively higher standards.

| Document | Description |
|----------|-------------|
| [design/REVIEW-ROUND-2.md](design/REVIEW-ROUND-2.md) | Second review: foundational feedback on type system and memory model |
| [design/REVIEW-ROUND-3.md](design/REVIEW-ROUND-3.md) | Third review: concurrency model and agentic primitives refinement |
| [design/REVIEW-ROUND-4.md](design/REVIEW-ROUND-4.md) | Fourth review: toolchain completeness and developer experience |
| [design/REVIEW-ROUND-5-FINAL.md](design/REVIEW-ROUND-5-FINAL.md) | Final review: 9.0 score, "Ship It" verdict |
| [design/DEVX-IMPROVEMENTS.md](design/DEVX-IMPROVEMENTS.md) | Developer experience improvements applied across all design docs |

---

## Benchmarks

The `benchmarks/` directory contains comparative performance benchmarks across 15+ languages, each implementing the same suite of algorithms. Results are collected in `results.csv` and `results.json`.

**Languages benchmarked:** C++, C#, Dart, Elixir, Go, Java, Julia, Kotlin, Python, Ruby, Rust, Scala, Swift, TypeScript, Zig

| File | Description |
|------|-------------|
| [benchmarks/run_all.sh](benchmarks/run_all.sh) | Script to execute all benchmark suites |
| [benchmarks/results.csv](benchmarks/results.csv) | Raw benchmark results (CSV) |
| [benchmarks/results.json](benchmarks/results.json) | Raw benchmark results (JSON) |

---

## Quick Start

```bash
# Read the vision
cat design/VISION.md

# Understand the syntax (with JS comparison)
cat design/SYNTAX.md

# See Turbo in action
cat examples/task-agent/src/main.tb

# See where we're headed
cat design/ROADMAP.md
```

---

## Document Reading Order

For someone new to the project, this reading order provides the best understanding:

1. **design/VISION.md** -- What Turbo is and why it exists
2. **design/SYNTAX.md** -- How code looks (especially the JS cheat sheet)
3. **design/TYPE-SYSTEM.md** -- How types work
4. **design/MEMORY-MODEL.md** -- How memory is managed (the key innovation)
5. **examples/task-agent/** -- See it all come together in real code
6. **design/ROADMAP.md** -- Where Turbo is headed
7. Everything else as needed
