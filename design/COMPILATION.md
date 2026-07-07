# Compilation Strategy

> **Status: Partially implemented — read backend claims with care.** The single source of truth for the real pipeline is [`CLAUDE.md`](../CLAUDE.md): `Lexer → Parser → AST → Sema → Cranelift (JIT or AOT)`. Key corrections to this document:
> - **Cranelift is the only code-generation backend.** The LLVM backend was **removed in 0.9.1**. There is no dual-backend "automatic Cranelift-for-dev / LLVM-for-release" switch: both `turbolang run` (JIT) and `turbolang build` (AOT) use Cranelift. LTO, PGO, and "full LLVM optimization" are **Planned**, not shipping.
> - **The WASM path is not LLVM→wasm32.** It compiles **AST → C → `clang --target=wasm32-wasi`**. The auto-generated JS bridge, `--npm` output, and TypeScript `.d.ts` generation are **Planned**.
> - **The HIR/MIR middle-end, borrow/region analysis, and monomorphization passes** described under "Compiler Architecture" are **Planned** — today Sema type-checks the AST and Cranelift lowers it directly.
> - **ASan** works today (used in the C-runtime test harness). **TSan/LSan**, `--coverage`, `--timings`, `--sanitize` flags, no-std/embedded targets, RISC-V, PGO, and incremental caching are **Planned** unless verified in the CLI.
> - **Cross-compilation to Linux x86_64 is real.** Other target triples listed below are aspirational.
>
> Unbuilt subsections are marked **Planned** inline.

## Core Decision: WASM-First for Web

No JS codegen target. WASM + robust JS interop bridge via WASM Component Model + JS bindings.

Rather than transpiling to JavaScript (as TypeScript, CoffeeScript, and others do), we compile directly to WebAssembly and provide a first-class JS interop bridge. This gives us predictable performance, a single compilation pipeline, and the ability to target both web and non-web environments from the same codebase.

---

## Compilation Targets

> **Backend column reality:** Cranelift is the only backend; the WASM path is AST → C → clang. The table below shows the *intended* target matrix; RISC-V and the JS interop bridge are Planned.

| Target | Backend (today) | Use Case |
|--------|---------|----------|
| Native (x86_64, ARM64) | Cranelift | CLI tools, servers, desktop apps |
| WebAssembly | AST → C → `clang wasm32-wasi` | Edge computing, WASI |
| Web interop | WASM + JS bridge layer *(Planned)* | DOM access, browser APIs, calling JS libs |

---

## Compilation Profiles

### Dev Mode -- Sub-Second Iteration (Cranelift Backend)

- Uses [Cranelift](https://cranelift.dev/) as the code generation backend for fast compilation
- Cranelift is a fast, lightweight code generator designed for JIT and ahead-of-time compilation — it trades peak optimization quality for dramatically faster compile times
- Target: <1 second for incremental builds (sub-200ms for small changes)
- Minimal optimizations (just enough to run correctly — no LTO, no vectorization, no PGO)
- Rich debug info, source maps, full DWARF symbols
- Hot reload support via file watching
- Inspired by Zig's fast compilation, Go's speed, Vite's dev server model, and Rust's ongoing cranelift integration

> **Planned — not yet implemented.** The "automatic Cranelift-for-dev / LLVM-for-release" switch below does not exist. The LLVM backend was removed in 0.9.1; both `run` and `build` use Cranelift today, and there is no separate optimizing release backend. The dev/release commands and flags in the next block describe the intended model.

```
# Typical dev workflow — all use Cranelift for sub-second builds
turbolang dev          # starts dev server with file watching (Cranelift)
turbolang run          # build and run (Cranelift)
turbolang build        # defaults to dev mode (Cranelift)
turbolang build --dev  # explicit dev mode (Cranelift)

# Release builds use LLVM — the switch is automatic
turbolang build --release   # full LLVM optimization pipeline
```

### Release Mode -- Maximum Performance

> **Planned — not yet implemented.** There is no LLVM release backend and no LTO/PGO pipeline today. `turbolang build --release` runs through Cranelift like every other build.

- Full LLVM backend with all optimizations
- LTO (Link-Time Optimization) -- whole program optimization
- PGO (Profile-Guided Optimization) support
- Dead code elimination, function inlining, vectorization
- Target: within 5% of hand-optimized C/Rust performance
- Build time: 10-60 seconds for medium projects

```
turbolang build --release                # full optimizations
turbolang build --release --lto          # with link-time optimization
turbolang build --release --pgo=train    # generate PGO profile data
turbolang build --release --pgo=use      # apply PGO profile data
```

### WASM Mode

> **Partially implemented.** The real WASM path is **AST → C → `clang --target=wasm32-wasi`**, not LLVM→wasm32. The auto-generated JS bridge, `--npm` packaging, and `.d.ts` generation are **Planned**.

- LLVM -> wasm32-wasi target
- WASM-specific optimizations (binaryen, wasm-opt)
- Size optimization available (`--opt-size`)
- JS bridge layer auto-generated (like wasm-bindgen)
- WASI support for server-side WASM

```
turbolang build --target wasm               # standard WASM output
turbolang build --target wasm --opt-size    # size-optimized WASM
turbolang build --target wasm --npm         # npm-compatible package output
```

### Embedded Mode

> **Planned — not yet implemented.** No-std, embedded target triples, and panic=abort mode do not exist today.

- No-std support (no heap allocation required)
- Size-optimized (`-Os`, `-Oz`)
- Panic = abort (no unwinding)
- Target-specific intrinsics
- Cross-compilation built in

```
turbolang build --target thumbv7em-none-eabihf       # ARM Cortex-M4
turbolang build --target riscv32imc-unknown-none-elf  # RISC-V embedded
turbolang build --embedded --opt-size                  # minimal binary
```

---

## Compiler Architecture

> **Planned — the real pipeline is simpler.** Per [`CLAUDE.md`](../CLAUDE.md), the shipping pipeline is `Lexer → Parser → AST → Sema → Cranelift`. There is no HIR or MIR, no borrow/region analysis, no separate monomorphization pass, and no LLVM-IR path. The HIR/MIR/LLVM architecture below is design intent.

### Frontend

- **Lexer** -> **Parser** -> **AST** -> **HIR** (High-level IR)
- Type checking and inference on HIR
- Borrow checking / memory model analysis on HIR
- Macro expansion at AST level

### Middle (MIR -- Mid-level IR)

- HIR -> MIR lowering
- Ownership/borrowing/region analysis
- Monomorphization of generics
- Optimization passes (constant folding, dead code, inlining)

### Backend Options

- MIR -> LLVM IR -> native code (release mode — full optimization)
- MIR -> Cranelift -> native (dev mode — fast compilation)
- MIR -> LLVM IR -> wasm32 (WASM mode)

### Compilation Pipeline Diagram

```
                        Source Files (.tb)
                              |
                              v
                    +-------------------+
                    |      Lexer        |  Tokenization
                    +-------------------+
                              |
                              v
                    +-------------------+
                    |      Parser       |  Syntax analysis
                    +-------------------+
                              |
                              v
                    +-------------------+
                    |       AST         |  Abstract Syntax Tree
                    |  (macro expand)   |
                    +-------------------+
                              |
                              v
                    +-------------------+
                    |       HIR         |  High-level IR
                    |  (type check,     |  - Type inference
                    |   borrow check)   |  - Ownership analysis
                    +-------------------+
                              |
                              v
                    +-------------------+
                    |       MIR         |  Mid-level IR
                    |  (monomorph,      |  - Generic specialization
                    |   opt passes)     |  - Constant folding
                    +-------------------+            - Dead code elimination
                              |                      - Inlining
                   +----------+----------+
                   |          |          |
                   v          v          v
           +-----------+ +-----------+ +-----------+
           |  LLVM IR  | | Cranelift | |  LLVM IR  |
           |           | |           | |           |
           +-----------+ +-----------+ +-----------+
                   |          |            |
                   v          v            v
           +-----------+ +-----------+ +-----------+
           |  Native   | |  Native   | |  wasm32   |
           |  (x86_64, | |  (fast    | |  (WASM +  |
           |   ARM64,  | |   dev     | |   WASI)   |
           |   RISC-V) | |   build)  | |           |
           +-----------+ +-----------+ +-----------+
                                          |
                                          v
                                  +--------------+
                                  |  wasm-opt    |
                                  |  (binaryen)  |
                                  +--------------+
                                          |
                                          v
                                  +--------------+
                                  | JS Bridge    |
                                  | (auto-gen    |
                                  |  bindings)   |
                                  +--------------+
```

---

## JS Interop (WASM Bridge)

### Calling JS from Turbo

```
// Declare external JS functions
extern "js" {
  fn document_getElementById(id: str) -> JsElement
  fn console_log(msg: str)
  fn fetch(url: str) -> Promise<JsResponse>
}

// Or use the bridge module
import { Document, Element } from "web/dom"
import { Request, Response } from "web/fetch"

fn main() {
  let el = Document.get_element_by_id("app")
  el.set_inner_html("<h1>Hello from WASM!</h1>")
}
```

### Exposing Turbo Functions to JS

```
@wasm_export
pub fn process_data(input: str) -> str {
  // This function is callable from JavaScript
  let result = parse_and_transform(input)
  result.to_json()
}
```

The `@wasm_export` attribute marks a function for inclusion in the generated WASM module's export table. The JS bridge layer automatically handles type marshalling between WASM linear memory and JavaScript values.

### Auto-Generated Bindings

- Like wasm-bindgen, auto-generate a JS glue layer
- TypeScript `.d.ts` files generated from Turbo types
- npm-compatible package output: `turbolang build --target wasm --npm`

Example output structure for `--npm`:

```
dist/
  package.json          # npm-ready manifest
  index.js              # JS glue code (ESM)
  index.d.ts            # TypeScript type definitions
  module.wasm           # compiled WASM binary
```

### Type Mapping

| Turbo Type | JS/TS Type | Transfer Method |
|----------|------------|-----------------|
| `i32`, `i64`, `f32`, `f64` | `number` / `bigint` | Direct (WASM primitives) |
| `str` | `string` | Copy through linear memory |
| `bool` | `boolean` | Direct (i32) |
| `[T]` | `T[]` / `TypedArray` | Copy or shared buffer |
| `T?` | `T \| null` | Tagged representation |
| `T ! E` | Throws or returns | Configurable |
| Structs | Plain objects / classes | Serialization or handle |

---

## Incremental Compilation

- File-level dependency tracking
- Only recompile changed files + dependents
- Persistent compilation cache across builds
- Parallel compilation of independent modules

```
                    +------------------+
                    |  Source change   |
                    |  detected       |
                    +------------------+
                            |
                            v
                    +------------------+
                    |  Dependency      |
                    |  graph lookup    |
                    +------------------+
                            |
                            v
                    +------------------+
                    |  Identify dirty  |
                    |  modules         |
                    +------------------+
                            |
                            v
                    +------------------+
                    |  Parallel        |
                    |  recompilation   |
                    |  (only dirty +   |
                    |   dependents)    |
                    +------------------+
                            |
                            v
                    +------------------+
                    |  Incremental     |
                    |  link            |
                    +------------------+
```

The compilation cache is stored in `.turbo-cache/` at the project root and persists across builds. Cache invalidation is based on content hashing (not timestamps) for reproducibility.

---

## Cross-Compilation

```
# Build for different targets
turbolang build --target x86_64-linux-gnu
turbolang build --target aarch64-apple-darwin
turbolang build --target wasm32-wasi
turbolang build --target thumbv7em-none-eabihf

# Zig-inspired: ship the cross-compilation toolchain
# No need to install separate cross-compilers
```

The compiler ships with all necessary target support built in. No external cross-compilation toolchains are required. This follows the approach pioneered by Zig, where the compiler itself bundles libc headers and target-specific runtime support for all supported platforms.

### Supported Target Triples

```
# Desktop / Server
x86_64-linux-gnu          # Linux x86_64
x86_64-linux-musl         # Linux x86_64 (static, musl)
aarch64-linux-gnu         # Linux ARM64
x86_64-apple-darwin       # macOS x86_64
aarch64-apple-darwin      # macOS Apple Silicon
x86_64-windows-msvc       # Windows x86_64

# WebAssembly
wasm32-wasi               # WASI (server-side, CLI)
wasm32-unknown-unknown    # Freestanding WASM (browser)

# Embedded
thumbv7em-none-eabihf     # ARM Cortex-M4/M7 (hard float)
thumbv6m-none-eabi        # ARM Cortex-M0/M0+
riscv32imc-unknown-none-elf  # RISC-V 32-bit embedded
```

---

## Performance Targets

| Metric | Target | Reference |
|--------|--------|-----------|
| Dev build (incremental) | <1s | Go-like |
| Release build (medium project) | 10-30s | Faster than Rust |
| Runtime perf (native) | Within 5% of C | Rust-level |
| Runtime perf (WASM) | Within 2x of native | Standard WASM overhead |
| Binary size (hello world) | <100KB | Zig-like |
| Binary size (WASM, hello world) | <50KB | Optimized |
| Startup time | <5ms | Go-like |

---

## Debug & Diagnostic Modes

Debug and diagnostic builds add runtime instrumentation to catch bugs that static analysis alone cannot find. All diagnostic modes are opt-in and produce clear, actionable reports.

### Debug Builds (`turbolang build --debug`)

- Full debug symbols for source-level debugging (DWARF on native, source maps on WASM)
- Bounds checking on all array/slice accesses (panics with index and length on out-of-bounds)
- Integer overflow checking (panics instead of wrapping silently)
- Initialized-memory checking (catches use of uninitialized values)
- No optimizations — code maps directly to source for predictable stepping
- Compatible with GDB, LLDB, and browser DevTools (for WASM targets)

```
turbolang build --debug                 # full debug instrumentation
turbolang build --debug --sanitize=address   # debug + address sanitizer
```

### Address Sanitizer (ASan)

Detect memory safety violations at runtime with detailed diagnostics.

- Use-after-free detection
- Heap buffer overflow / underflow
- Stack buffer overflow
- Global buffer overflow
- Memory leak detection (on exit)
- Reports include full stack traces with source locations

```
turbolang build --sanitize=address
turbolang run --sanitize=address ./my-app
```

Typical output on error:

```
ERROR: AddressSanitizer: heap-use-after-free
  READ of size 8 at 0x614000000044
    #0 process_item() src/engine.tb:42
    #1 main()         src/main.tb:15
  freed by:
    #0 drop()         src/engine.tb:38
```

- ~2x slowdown, ~2x memory overhead — suitable for testing, not production
- Inspired by: Clang/GCC ASan, Valgrind

### Thread Sanitizer (TSan)

> **Planned — not yet implemented.** There is no `--sanitize=thread` flag, and there is no async runtime or actor system for it to instrument. `spawn`/`await` are OS threads; race-checking those is a future goal.

Detect data races in concurrent code at runtime.

- Reports unsynchronized reads/writes to shared memory
- Works with Turbo's async runtime, threads, and actor messages
- Catches races even when they don't cause visible bugs (yet)

```
turbolang build --sanitize=thread
turbolang test --sanitize=thread
```

Typical output on error:

```
WARNING: ThreadSanitizer: data race
  Write of size 4 at 0x7f8b4c000000:
    #0 update_counter() src/stats.tb:27
  Previous read of size 4 at 0x7f8b4c000000:
    #0 read_counter()   src/stats.tb:19
```

- ~5-10x slowdown — use in CI or dedicated test runs
- Inspired by: Clang TSan, Go's race detector

### Leak Sanitizer (LSan)

Detect memory leaks — allocations that are never freed.

- Runs at program exit, reports all still-reachable unreferenced allocations
- Particularly useful for long-running services and tests
- Can be combined with ASan (enabled by default when ASan is active)

```
turbolang build --sanitize=leak
turbolang test --sanitize=leak
```

- Minimal overhead — suitable for regular test runs
- Inspired by: Clang LSan, Valgrind memcheck

### Coverage Mode

Generate code coverage reports to identify untested paths.

- Statement, branch, and function coverage
- `turbolang build --coverage` instruments the binary for coverage collection
- `turbolang test --coverage-report` runs tests and generates the report
- Output formats: terminal summary, HTML (with source annotation), LCOV, Cobertura
- Integrates with CI coverage services (Codecov, Coveralls)

```
turbolang build --coverage
turbolang test --coverage-report              # terminal summary + HTML
turbolang test --coverage-report=lcov         # LCOV format for CI upload
turbolang test --coverage-report --open       # generate HTML and open in browser
```

Terminal output example:

```
Coverage Report:
  src/parser.tb      92.3%  (240/260 lines)
  src/codegen.tb     87.1%  (189/217 lines)
  src/optimizer.tb   64.5%  (120/186 lines)  <-- below threshold
  ─────────────────────────────────────────
  Total              81.8%  (549/663 lines)
```

- Configure minimum coverage thresholds in `turbo.toml`:

```toml
[test]
coverage-threshold = 80   # fail if total coverage drops below 80%
```

### Compile-Time Diagnostics (`turbolang build --timings`)

Understand where compilation time is spent and identify bottlenecks.

- `turbolang build --timings` produces a per-module timing breakdown
- Shows time spent in each compiler phase: parsing, type checking, borrow checking, codegen
- Identifies the slowest modules and the critical path through the dependency graph
- HTML output with a waterfall chart showing parallel compilation

```
turbolang build --release --timings
turbolang build --timings --output html --open    # visual waterfall chart
```

Terminal output example:

```
Compilation Timings:
  Phase           Time      % of total
  ─────────────────────────────────────
  Parsing         0.12s       3.1%
  Type checking   0.84s      21.5%
  Borrow check    0.45s      11.5%
  MIR lowering    0.31s       7.9%
  LLVM codegen    1.92s      49.2%
  Linking         0.26s       6.7%
  ─────────────────────────────────────
  Total           3.90s

  Slowest modules:
    src/parser.tb      1.02s  (codegen: 0.78s)
    src/runtime.tb     0.87s  (type check: 0.52s)
    src/optimizer.tb   0.64s  (codegen: 0.41s)
```

- Helps decide where to split large modules or add `@inline` annotations
- Inspired by: Cargo `--timings`, Clang `-ftime-report`, TypeScript `--diagnostics`

---

## Comparison

| Feature | Turbo | Rust | Go | Zig | Carbon |
|---------|------|------|-----|-----|--------|
| Dev build speed | <1s | 5-30s | <1s | <1s | N/A |
| LLVM backend | No (removed 0.9.1; Cranelift only) | Yes | No (custom) | Yes | Yes |
| WASM target | Yes | Yes | Experimental | Yes | No |
| Cross-compilation | Built-in | Via rustup | Built-in | Built-in | N/A |
| Incremental compilation | Yes | Yes | Yes | Partial | N/A |
| PGO support | Yes | Yes | Yes | No | N/A |
| Fast dev backend | Cranelift | No (cranelift WIP) | Custom backend | Self-hosted | N/A |
| Sanitizers (ASan/TSan/LSan) | Built-in | Via LLVM flags | Race detector only | No | N/A |
| Code coverage | Built-in | Via grcov/tarpaulin | Built-in | No | N/A |
| Build timings | Built-in | `--timings` | No | No | N/A |

### Key Differentiators

- **Cranelift backend** *(dual-backend strategy is Planned)*: Today Cranelift powers both JIT (`run`) and AOT (`build`) for sub-second iteration. The "full LLVM for release" half of this differentiator was removed in 0.9.1 and is a future goal — an optimizing release backend would be reintroduced only if it reaches parity and shows a measured win.
- **WASM as a first-class target**: Not an afterthought. The JS bridge layer is auto-generated and type-safe, with TypeScript definitions included.
- **Built-in cross-compilation**: No external toolchain setup. Every installation can build for every supported target out of the box.
- **Embedded support without compromise**: No-std, no-alloc builds are supported from day one, with the same language and the same toolchain.
