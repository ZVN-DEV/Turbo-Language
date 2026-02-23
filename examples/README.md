# Turbo Examples

Five progressively complex example projects demonstrating what Turbo is built for and how it works in practice. Each is a complete project with `turbo.toml`, source code, and tests.

## Examples

| Example | Description | Key Features |
|---------|-------------|--------------|
| [task-agent](./task-agent/) | Task Management API with AI Agent | REST API, `tool fn`, `agent`, async, `Shared<T>`, testing |
| [web-api](./web-api/) | Production Bookmarking API (BookmarkAPI) | JWT auth, WebSocket, search, middleware, rate limiting, metrics |
| [desktop-app](./desktop-app/) | Native Desktop Markdown Editor (TurboNotes) | Event-driven architecture, file I/O, AI writing assistant, deterministic memory |
| [realtime-system](./realtime-system/) | Trading Order Matching Engine (TurboExchange) | Zero-alloc hot paths, actors, regions, sub-microsecond latency |
| [edge-wasm](./edge-wasm/) | Edge Image Processing (TurboEdge) | WASM compilation, streaming pipelines, `const fn`, CDN deployment |

### task-agent (Starter)

The recommended first example. A REST API for task management with an AI agent that can organize and discuss tasks. Demonstrates the core Turbo experience: routes, types, error handling, async, agents, and testing -- all in a single, readable project. If you learn one example, learn this one.

### web-api (Intermediate)

A production-quality social bookmarking API covering patterns real web services need: JWT authentication with token revocation, CORS middleware, rate limiting, pagination, full-text search, WebSocket broadcasting for real-time sync, structured logging, and OpenTelemetry metrics. Shows that Turbo replaces Node.js/Express with less code and more safety.

### desktop-app (Intermediate)

A native desktop markdown editor with AI-powered writing assistance. Demonstrates event-driven architecture using algebraic data types (every user interaction flows through a single `AppEvent` type with exhaustive pattern matching), file system watching, keyboard shortcuts, and AI summarization. The critical advantage: deterministic memory means zero GC pauses during typing.

### realtime-system (Advanced)

A financial exchange order matching engine -- the hardest domain in software. Demonstrates Turbo's systems programming depth: zero-allocation hot paths using `region {}` blocks, actor isolation for order books, pre-allocated ring buffers, lock-free data structures, and sub-microsecond matching latency. Uses all four memory levels (auto-clone for the API layer, regions for the matching engine, `@manual` for the ring buffer). If Turbo can build this, it can build anything.

### edge-wasm (Advanced)

An image processing service that compiles to WebAssembly and deploys to CDN edge nodes (Cloudflare Workers, Vercel Edge, Deno Deploy). Demonstrates WASM as a first-class compilation target: streaming image pipelines, `const fn` for compile-time lookup tables, edge caching strategies, and SIMD-accelerated convolutions. The same codebase runs natively for local development and as WASM in production.

---

## What Turbo Is Built For

- **Web APIs and microservices** -- Async HTTP server, typed routes, structured logging, metrics. Turbo's built-in `Server` and `Router` mean zero framework boilerplate. Response types are checked at compile time. Performance matches C/Go servers because there is no garbage collector.

- **AI agent systems** -- First-class `tool` and `agent` keywords, streaming, multi-agent orchestration. The compiler auto-generates JSON schemas from tool function signatures, validates agent configurations statically, and provides rich IDE support. Go from prototype to production without changing frameworks.

- **CLI tools** -- Fast native binaries, argument parsing, file I/O. Turbo compiles to small, self-contained executables with instant startup time. The standard library includes everything you need for CLI applications.

- **Real-time systems** -- Deterministic memory (no GC pauses), low-latency networking. Turbo's ownership model means no stop-the-world garbage collection. Tail latencies are predictable. Combined with lightweight tasks and M:N scheduling, Turbo handles millions of concurrent connections.

- **Data processing pipelines** -- Pipe operators, concurrent processing, streaming. The `|>` pipe operator makes data transformations read top-to-bottom. Combine with `async` generators and `for await` for streaming pipelines that process data as it arrives.

- **Desktop applications** -- Native performance, cross-compilation, WASM for web views. Turbo compiles to native code on macOS, Linux, and Windows. For hybrid desktop apps, compile UI components to WASM and run them in web views alongside native code.

- **Systems programming** -- When you need Level 2-3 memory control. Turbo's progressive memory model lets you start with automatic management and drop down to explicit allocators, arenas, and unsafe blocks when you need full control.

- **Edge computing** -- WASM compilation, small binary size. Turbo targets WebAssembly natively, producing compact binaries suitable for edge runtimes. Cold start times are measured in microseconds, not milliseconds.

---

## What Turbo Is Suited For (Roadmap)

These use cases are not supported in v1.0 but are on the progressive disclosure roadmap. Each ships as an opt-in capability that does not change the base language.

- **Quick scripts (v1.1)** -- Script mode adds `turbo run script.tb` with zero type annotations required, shebang support (`#!/usr/bin/env turbo`), and an enhanced REPL. No `turbo.toml` needed for single files. When your script grows into a real tool, add types and structure incrementally. See [design/ROADMAP.md](../design/ROADMAP.md).

- **GPU compute and ML inference (v1.2)** -- `@gpu` kernel blocks compile to CUDA/Metal/Vulkan. `turbo/tensor` provides multi-dimensional arrays with compile-time shape checking. `turbo/ml` runs ONNX model inference. Python interop bridges to the ML ecosystem. Training stays in Python -- Turbo handles the deployment and application layer. See [design/ROADMAP.md](../design/ROADMAP.md).

- **Mobile apps (v1.3)** -- iOS and Android targets via LLVM ARM64 backends. `turbo/ui` provides a cross-platform declarative UI framework. Swift/ObjC bridge (iOS) and Kotlin/JNI bridge (Android) for platform APIs. `turbo build --target ios-arm64` is all it takes. See [design/ROADMAP.md](../design/ROADMAP.md).

- **Distributed systems (v1.4)** -- `turbo/cluster` extends the existing actor model across machines with location-transparent messaging. Service mesh integration, distributed tracing, and consensus primitives. See [design/ROADMAP.md](../design/ROADMAP.md).

---

## What Turbo Is NOT For (Use Something Else)

- **Legacy system integration** -- Use whatever the legacy system uses. If you are maintaining a Java codebase, write Java. Turbo's FFI can call C libraries, but deep integration with managed runtimes (JVM, CLR, BEAM) is not a design goal.

---

## Running Examples

Each example directory contains a `turbo.toml` and can be run with:

```bash
cd examples/task-agent
turbo run          # Build and run
turbo test         # Run all tests
turbo build        # Build only
```

For more detail on each example, see the README.md or BRIEFING.md inside its directory.
