# Turbo Roadmap -- Progressive Disclosure by Design

Turbo grows from a focused core into a universal language through **progressive disclosure**: every new capability is opt-in and does not complicate the base language. A developer who never needs GPU compute will never see GPU syntax. A developer who never targets mobile will never encounter mobile tooling. The language you learn on day one remains clean and simple no matter how many capabilities are added later.

---

## v1.0 -- Core (Current Design)

Everything in the current design documents. What ships on day one.

### Language
- Complete syntax: `let`, `fn`, `type`, `struct`, `match`, `if let`, `guard`, `for`, `while`
- Type system: generics, `T?` optionals, `T ! E` errors, algebraic data types, traits, structural interfaces
- Memory: CTRC ownership + auto-clone (Level 0), explicit refs (Level 1), regions/arenas (Level 2), `@manual` escape hatch (Level 3)
- Concurrency: `async`/`await`, `spawn`, `all()`, actors, channels, structured concurrency, supervision trees
- Agentic: `tool fn`, `agent` keyword, streaming, memory abstractions, multi-agent orchestration
- Effects: `async`, `throws`, `io`, `unsafe`, `diverges` tracked in function signatures

### Compilation
- LLVM backend for optimized release builds (x86-64, AArch64, RISC-V)
- Cranelift backend for sub-second incremental dev builds
- WebAssembly target (`wasm32-wasi`) with dedicated optimization pipeline
- Debug, release, and size-optimized build profiles

### Toolchain
- `turbolang build` / `turbolang run` / `turbolang test` / `turbolang bench`
- `turbolang fmt` -- single canonical style, zero config
- `turbolang lint` -- built-in linter with extensible rules
- `turbolang doc` -- documentation generator with tested examples
- `turbolang repl` -- interactive exploration environment
- LSP server for editor integration
- Package manager with lockfiles, workspaces, semantic versioning

### Standard Library
- `turbo/io` -- File I/O, buffered readers/writers, paths
- `turbo/http` -- HTTP client and server, `Router`, `Middleware`
- `turbo/json` -- JSON parsing and serialization
- `turbo/log` -- Structured logging
- `turbo/test` -- Test framework with `@test`, `@perf`, `@mock`, property-based testing
- `turbo/time` -- Timestamps, durations, timers
- `turbo/collections` -- `Map`, `Set`, `Deque`, `PriorityQueue`, `BTreeMap`
- `turbo/metrics` -- Counters, histograms, gauges, OpenTelemetry export
- `turbo/crypto` -- Hashing, JWT, HMAC, bcrypt

---

## v1.1 -- Script Mode

**Progressive disclosure principle:** Level 0 developers get scripting for free. The compilation step becomes invisible for simple programs.

### What Ships
- **`turbolang run script.tb`** executes single files directly. No `turbo.toml` required. No explicit types required. The compiler infers everything for local scripts.
- **Shebang support:** `#!/usr/bin/env turbolang` at the top of any `.tb` file makes it executable from the shell.
- **`turbolang repl`** enhanced with contextual hints, tab completion, and inline documentation. Ideal for learning and prototyping.
- **Full type inference for scripts:** Write `let x = 5` and `fn add(a, b) => a + b`. No annotations needed. When you want types, add them -- the compiler helps you migrate with `turbolang annotate`.
- **Implicit main:** A script file without an explicit `fn main()` wraps top-level statements in an async main automatically.
- **Script dependencies:** `// @dep turbo/http ^1.0` comment syntax for single-file dependency declarations.

### What Does NOT Change
The base language is identical. Script mode is a compiler front-end convenience, not a language fork. Any script can be promoted to a full project by adding `turbo.toml` and type annotations. The compiler provides `turbolang init` to scaffold a project from an existing script.

### Why v1.1
Scripting is the lowest-friction entry point. It lets developers start with Turbo the way they start with Python -- a single file, zero ceremony -- and grow into typed, structured projects as complexity warrants. This captures the "quick scripts" use case that v1.0 explicitly defers to other languages.

---

## v1.2 -- GPU & Compute

**Progressive disclosure principle:** Level 2-3 developers get GPU when they need it. The base language does not change. GPU is an opt-in import with a new annotation.

### What Ships
- **`@gpu` kernel blocks** that compile to CUDA (NVIDIA), Metal (Apple), and Vulkan (cross-platform) compute shaders via LLVM's GPU backends.
  ```
  @gpu fn matrix_multiply(a: Tensor<f32>, b: Tensor<f32>) -> Tensor<f32> {
      let row = gpu.thread_id.y
      let col = gpu.thread_id.x
      // Kernel body -- runs on GPU
  }
  ```
- **`turbo/tensor` stdlib:** Multi-dimensional arrays with compile-time shape checking. Shapes are part of the type: `Tensor<f32, [128, 64]>` is a 128x64 matrix. Shape mismatches are compile-time errors.
- **SIMD intrinsics:** `@simd fn dot_product(a: [f32; N], b: [f32; N]) -> f32` for CPU-level vectorization. The compiler auto-vectorizes where possible and provides explicit control where needed.
- **`turbo/ml` for inference pipelines:** Load ONNX models, run inference with typed inputs/outputs. This is for deploying trained models, not training them -- training stays in Python where the ecosystem lives.
- **Python interop:** Call Python libraries from Turbo (similar to PyO3 for Rust). Bridge to NumPy, pandas, and the ML ecosystem without leaving Turbo for the surrounding application code.

### What Does NOT Change
A developer who never imports `turbo/tensor` or writes `@gpu` sees zero new syntax. The base language, type system, memory model, and toolchain are untouched. GPU support is a compilation target and a standard library addition, not a language change.

### Why v1.2
GPU compute is the next frontier after scripting. With the AI boom driving demand for inference at the edge and in applications, Turbo developers need a path to GPU without switching languages. By providing inference pipelines (not training frameworks), we complement Python rather than competing with it.

---

## v1.3 -- Mobile

**Progressive disclosure principle:** Application developers get mobile when targets are ready. Mobile is a compilation target and a UI library, not a language modification.

### What Ships
- **iOS target:** ARM64 via LLVM. Swift/ObjC bridge for calling platform APIs (UIKit, SwiftUI interop, system services).
  ```bash
  turbolang build --target ios-arm64
  turbolang build --target ios-arm64-simulator
  ```
- **Android target:** ARM64 via LLVM. Kotlin/JNI bridge for calling Android SDK APIs.
  ```bash
  turbolang build --target android-arm64
  ```
- **`turbo/ui` declarative UI framework:** Cross-platform UI components that compile to native platform widgets. Inspired by SwiftUI and Jetpack Compose, expressed in Turbo syntax.
  ```
  fn app_view(state: AppState) -> View {
      Column {
          Text("Hello, {state.name}")
          Button("Tap me", on_click: state.increment)
          if state.count > 0 {
              Text("Count: {state.count}")
          }
      }
  }
  ```
- **Platform SDK bindings:** Camera, GPS, notifications, biometrics, haptics, app lifecycle, deep links.
- **Hybrid option:** Compile UI to WASM and run in a native WebView for faster iteration during development, with the option to compile to fully native widgets for release builds.

### What Does NOT Change
A developer targeting server, CLI, or WASM never encounters `turbo/ui` or mobile tooling. The `--target` flag is the only entry point. The base language, memory model, and type system are unchanged.

### Why v1.3
Mobile follows GPU because the compilation infrastructure (LLVM ARM64 backends) is shared. By v1.3, Turbo's LLVM pipeline is battle-tested on native and WASM targets, making mobile a natural extension. The `turbo/ui` framework leverages the existing reactive patterns from Turbo's async and actor systems.

---

## v1.4 -- Distributed

**Progressive disclosure principle:** Advanced users get distribution primitives. This is a standard library and runtime extension, not a language change.

### What Ships
- **`turbo/cluster` for distributed actors:** Actors that transparently span multiple machines. Location-transparent messaging with the same `actor` syntax from v1.0.
  ```
  let worker = spawn RemoteActor on cluster.node("worker-1")
  worker.send(ProcessData(payload))
  ```
- **Service mesh integration:** Built-in service discovery, health checks, load balancing. Turbo services register themselves and discover peers without external orchestration for simple deployments.
- **Distributed tracing:** Built on the existing `turbo/metrics` and OpenTelemetry foundation from v1.0. Traces span across machines and services automatically when using `turbo/cluster` actors.
- **Consensus primitives:** `turbo/cluster/raft` for leader election and replicated state machines. Opt-in, composable, and tested.

### What Does NOT Change
Single-machine Turbo code runs identically. The `actor` keyword and `spawn` syntax from v1.0 are unchanged -- distribution is an extension of the existing concurrency model, not a replacement. A developer who deploys to a single server never encounters `turbo/cluster`.

### Why v1.4
Distribution comes last because it depends on every prior layer: the actor model (v1.0), the mature compilation pipeline (v1.2-1.3), and real-world feedback from production deployments. Distributed systems are the hardest to get right, so they benefit from the most runtime maturity.

---

## Progressive Disclosure Table

Every feature maps to a disclosure level. Developers discover complexity only when they need it.

| Feature | Level 0 (JS dev) | Level 1 (Intermediate) | Level 2 (Advanced) | Level 3 (Expert) |
|---------|-----------------|----------------------|-------------------|-----------------|
| Variables | `let x = 5` | `let x: i32 = 5` | -- | -- |
| Functions | `fn f(x) => x + 1` | `fn f(x: i32) -> i32` | `@inline fn f(...)` | -- |
| Memory | Auto-clone (invisible) | `let ref x = ...` | `region { }` | `@manual` + alloc/free |
| Errors | `?` propagation | `T ! E` types | Custom error hierarchies | Effect system |
| Concurrency | `await` | `spawn`, `all()` | Actors, channels | Supervision trees |
| Agents | `Agent.quick(...)` | `tool fn` + `agent` | Multi-agent orchestration | Custom providers |
| Performance | Just works | `@perf` tests | `@inline`, `const fn` | `region {}`, SIMD |
| Targets | `turbolang run` (native) | `--target wasm32` | `--target ios-arm64` | `@gpu` kernels |
| Scripts | `turbolang run file.tb` | `turbo.toml` project | Workspaces | Custom build steps |

### How to Read This Table

- **Level 0:** A JavaScript or Python developer writes working Turbo code without learning any new concepts. Types are inferred. Memory is automatic. Errors propagate with `?`. This is the on-ramp.
- **Level 1:** The developer adds type annotations, structures code into modules, uses the package manager, and writes tests. This is where most application developers settle.
- **Level 2:** The developer uses advanced features for performance or capability: regions for zero-allocation hot paths, actors for isolation, WASM or mobile targets, GPU kernels for compute.
- **Level 3:** The developer controls everything: manual memory management, custom allocators, supervision hierarchies, distributed actors, SIMD intrinsics. This is systems programming territory.

The critical property: **moving up a level never breaks code written at a lower level.** Level 0 code is valid forever. Adding a type annotation does not change semantics. Opting into regions does not affect code outside the region. GPU kernels do not change how CPU code compiles. Each level is a strict superset.

---

## Release Philosophy

Each version follows the same pattern:

1. **New capabilities are additive.** No breaking changes to existing syntax, semantics, or APIs.
2. **New capabilities are opt-in.** No new imports, annotations, or concepts appear unless the developer explicitly requests them.
3. **The base language stays clean.** A `turbolang run hello.tb` that prints "Hello, world!" looks identical in v1.0 and v1.4.
4. **Migration is gradual.** Developers adopt new features at their own pace. There is no "v1.2 migration guide" because nothing breaks.

This is the core promise of progressive disclosure: **Turbo grows without burdening the developers who do not need the growth.**
