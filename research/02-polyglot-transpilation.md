# Polyglot Transpilation & Language Interop: State of the Art

> Research compiled February 2026. Covers transpilers, multi-target compilers,
> FFI mechanisms, code conversion tools, universal IR approaches, and the
> feasibility of WASM + native dual-target compilation.

---

## Table of Contents

1. [Existing Transpilers](#1-existing-transpilers)
2. [Multi-Target Compilation](#2-multi-target-compilation)
3. [Language Interop & FFI](#3-language-interop--ffi)
4. [Code Conversion Tools](#4-code-conversion-tools)
5. [Universal IR Approaches](#5-universal-ir-approaches)
6. [The Polyglot Dream Assessment](#6-the-polyglot-dream-assessment)
7. [WASM + Native Dual-Target Feasibility](#7-wasm--native-dual-target-feasibility)

---

## 1. Existing Transpilers

### Overview Table

| Transpiler       | Source         | Target(s)                        | Maturity    | JS Output Quality | Notes |
|------------------|---------------|----------------------------------|-------------|-------------------|-------|
| TypeScript       | TypeScript     | JavaScript                       | Production  | Excellent (1:1)   | Industry standard; type erasure model |
| CoffeeScript     | CoffeeScript   | JavaScript                       | Legacy      | Very Good (1:1)   | Declining usage; inspired ES6 features |
| Kotlin/JS        | Kotlin         | JavaScript                       | Stable      | Good              | Part of Kotlin Multiplatform |
| Scala.js         | Scala          | JavaScript                       | Stable      | Good              | Full Scala language support in browser |
| ClojureScript    | Clojure        | JavaScript                       | Stable      | Good              | Google Closure Compiler integration |
| Haxe             | Haxe           | JS/C++/C#/Java/Python/Lua/PHP    | Production  | Varies by target  | Most targets of any single transpiler |
| Nim              | Nim            | C/C++/ObjC/JavaScript            | Stable      | Good              | C backend is primary; JS is secondary |
| Cython           | Python+Cython  | C                                | Production  | N/A (C output)    | Up to 277x speedup over CPython |
| Fable            | F#             | JavaScript                       | Stable      | Good              | F# ecosystem for web development |
| Dart             | Dart           | JavaScript / WASM                | Production  | Good              | Flutter Web uses WASM by default now |
| ReScript         | ReScript       | JavaScript                       | Stable      | Excellent          | Fast compilation, readable JS output |
| PureScript       | PureScript     | JavaScript                       | Stable      | Good              | Haskell-inspired, strict evaluation |

### Detailed Assessments

**TypeScript -> JavaScript**
The dominant transpiler in industry. TypeScript's approach is notable for being a *superset* strategy -- valid JavaScript is valid TypeScript. The compiler performs type erasure, stripping type annotations to produce clean JavaScript. This design choice means zero runtime overhead and near-perfect 1:1 correspondence between source and output. TypeScript has effectively proven that the "typed superset" transpilation model works at massive scale.

**CoffeeScript -> JavaScript**
The original "compile-to-JS" language that launched an era. CoffeeScript demonstrated that developers would accept a compilation step for syntactic improvements. Output is human-readable and mostly 1:1 with input. However, CoffeeScript's influence was absorbed by ES6/ES2015 (arrow functions, destructuring, classes), and usage has declined dramatically. It remains a cautionary tale: *a transpiler whose best features get adopted by the target language becomes obsolete*.

**Kotlin -> JS/JVM/Native**
Kotlin's JS target reached Stable status. However, JetBrains is shifting web focus toward Kotlin/Wasm, which graduated to Beta in September 2025. The JS target remains supported but Wasm is the strategic direction. Kotlin 2.3 (2025) includes support for Java 25 on JVM, and direct Swift export on Native (eliminating Objective-C bridging).

**Scala.js**
Mature and battle-tested. Compiles the full Scala language to JavaScript with good dead-code elimination via Google Closure Compiler. Used in production by companies like Airbnb (for parts of their stack). Community is smaller than Kotlin/JS but deeply committed.

**ClojureScript**
One of the most successful compile-to-JS languages. Leverages Google Closure Compiler for advanced optimizations (dead code elimination, minification, module splitting). The immutable-data-by-default approach maps well to React-style UIs. Ranked higher than Scala.js in community recommendations.

**Haxe -> Multiple Targets**
Haxe is the most ambitious multi-target transpiler, compiling to: JavaScript, C++, C#, Java, JVM bytecode, Python, Lua, PHP, Flash/SWF, Neko, and HashLink. Haxe 5.0 preview was released in July 2025. The gaming industry (particularly cross-platform game engines like Heaps) is its strongest niche. The breadth of targets is impressive but comes at the cost of "lowest common denominator" API design -- you cannot easily use target-specific features.

**Nim -> C/C++/ObjC/JavaScript**
Nim's primary compilation path is through C, generating `.c` files that are then compiled with the platform's C compiler. This gives Nim native performance while maintaining a Python-like syntax. The JavaScript backend is secondary but functional, with BigInt support for 64-bit integers added in Nim 2.0. Nim also supports C++ and Objective-C output for interop scenarios.

**Cython -> C**
Not a general-purpose transpiler but a critical performance tool. Cython compiles Python-like code (with optional type annotations) into C extension modules. With 70+ million monthly PyPI downloads, it is the most widely used Python-to-C compiler. Performance gains are dramatic when types are declared (`cdef`), but naive Cython can actually be *slower* than pure Python due to function call and data conversion overhead.

**Fable (F# -> JavaScript)**
Brings the ML-family type system to the browser. Fable trades some type safety for easier JavaScript interop compared to PureScript. Used primarily in the .NET ecosystem for web frontends with Elmish architecture.

**Dart -> JS/WASM**
Flutter Web compiles Dart to both JavaScript and WebAssembly. With the `--wasm` flag, Flutter still generates JS as a fallback -- if WasmGC support is not detected at runtime, the JS output is used. WASM compilation yields 2x faster frame rendering and 3x improvement in worst-case performance. Browser support for WasmGC: Chrome 119+, Safari (supported but buggy with Flutter), Firefox 120 (supported but has compatibility issues).

**ReScript -> JavaScript**
Formerly BuckleScript/ReasonML. Produces extremely readable JavaScript output with fast compilation times. The generated JS is often indistinguishable from hand-written code. Smaller community but devoted users in the React ecosystem.

**PureScript -> JavaScript**
Haskell-inspired language with strict evaluation targeting JavaScript. Excellent type system with type classes, higher-kinded types, and row polymorphism. Output is readable CommonJS modules. Smaller ecosystem than TypeScript but used where correctness guarantees matter.

---

## 2. Multi-Target Compilation

### Platform Matrix

| Platform/Tool     | JVM | JS  | Native | WASM | iOS | Android | Desktop | Other Targets |
|-------------------|-----|-----|--------|------|-----|---------|---------|---------------|
| Kotlin Multiplatform | Yes | Yes | Yes | Beta | Yes | Yes | Yes | tvOS, watchOS |
| Haxe              | Yes | Yes | Yes (C++) | No | Via C++ | Via C++ | Yes | Flash, PHP, Lua, Python, C# |
| .NET/Blazor       | N/A | Via Blazor | AOT | Yes | MAUI | MAUI | Yes | Linux via Mono |
| GraalVM           | Yes | Yes (via Truffle) | Yes (native-image) | Yes (GraalWasm) | No | No | Yes | LLVM bitcode (Sulong) |
| Dart/Flutter      | No  | Yes | Yes | Yes | Yes | Yes | Yes | Embedded (experimental) |
| Scala             | Yes | Yes (Scala.js) | Yes (Scala Native) | No | No | Via JVM | Via JVM | -- |

### Kotlin Multiplatform (KMP)

Kotlin Multiplatform is production-ready as of 2026, targeting four compiler backends:

- **Kotlin/JVM**: Stable. Supports Java 25. The most mature backend.
- **Kotlin/JS**: Stable. Generates JavaScript for browser and Node.js. JetBrains is deprioritizing JS Canvas in favor of Wasm.
- **Kotlin/Native**: Stable for iOS/macOS. Swift export reached Beta in Kotlin 2.2.20 (direct, without Objective-C bridging). JetBrains targets stable Swift interop in 2026.
- **Kotlin/Wasm**: Beta since September 2025. Safari adopted WasmGC in December 2024, enabling 100% modern browser coverage. Multithreading support for the Wasm target is being prototyped.

Google officially endorsed KMP at Google I/O and KotlinConf 2025 for Android development, including shared business logic with iOS.

**What works well**: Shared business logic, data models, networking, serialization.
**What is hard**: UI code (each platform wants its own), platform-specific APIs require `expect`/`actual` declarations.

### Haxe Multi-Target

Haxe compiles to source code in 10+ languages:

| Target | Output | Use Case |
|--------|--------|----------|
| JavaScript | Source | Web |
| C++ | Source | Games, native apps (via HXCPP) |
| C# | Source | Unity game engine |
| Java | Source | Android, server |
| Python | Source | Scripting, ML |
| Lua | Source | Game scripting (Defold) |
| PHP | Source | Web backends |
| Flash/SWF | Bytecode | Legacy (deprecated target) |
| HashLink | Bytecode | Game VM (Haxe-specific) |
| Neko | Bytecode | Legacy VM |

Haxe's primary niche is cross-platform game development. The gaming studio Motion Twin (Dead Cells) is the most prominent user. The challenge is that each target has different performance characteristics and limitations, requiring target-specific testing.

### .NET / Mono / Blazor

The .NET ecosystem provides multiple compilation strategies:

- **Native AOT**: Compiles .NET code directly to native machine code. .NET 10 (2025) achieved 30-40% runtime improvements for server workloads and startup times cut nearly in half.
- **Blazor WebAssembly**: Runs .NET in the browser via WebAssembly. AOT compilation available since .NET 7. AOT-compiled Blazor apps are about 2x the size of IL-interpreted versions but significantly faster at runtime.
- **.NET MAUI**: Cross-platform UI framework targeting iOS, Android, macOS, Windows from a single codebase.

### GraalVM Polyglot VM

GraalVM (current: JDK 24.0.2, July 2025) provides a polyglot runtime via the Truffle framework:

- **Truffle languages**: JavaScript (ES2023), Ruby (TruffleRuby), Python (GraalPy), R (FastR), LLVM bitcode (Sulong), WebAssembly (GraalWasm), Java (Espresso)
- **Polyglot interop**: Languages running on Truffle can call each other with near-zero overhead. A Ruby program can call a JavaScript function can call a Python function, all in the same process.
- **Native Image**: Ahead-of-time compilation of JVM applications to standalone native binaries. Eliminates JVM startup overhead.
- **Sulong**: Executes LLVM bitcode (C, C++, Rust, Fortran) on the Truffle framework, enabling polyglot interop with those languages.

**Assessment**: GraalVM is technically impressive but adoption has been limited outside the JVM ecosystem. The polyglot promise is real but the practical benefit is narrow -- most organizations standardize on one or two languages rather than mixing five.

### Dart (Native/JS/WASM)

Flutter compiles Dart to multiple targets:
- **Native**: AOT compilation for iOS and Android (ARM), desktop (x86/ARM)
- **JavaScript**: dart2js for web, with tree-shaking
- **WASM**: Via dart2wasm using WasmGC. 2-3x performance improvement over JS for rendering

The WASM target requires new interop APIs (`dart:js_interop` instead of `dart:html`), which means existing Dart web code must be migrated.

---

## 3. Language Interop & FFI

### The C ABI as Universal Interface

The C calling convention (C ABI) is the de facto universal interface for language interop. Nearly every language provides some mechanism to call C functions and be called from C. This is not because C is superior, but because:

1. C's ABI is simple and stable (no name mangling, no exceptions, no generics)
2. Operating systems expose their APIs through C interfaces
3. Hardware vendors provide C libraries (drivers, SDKs)
4. The C ABI is the "lingua franca" of systems programming

### FFI Comparison Table

| Language | FFI Mechanism | Direction | Overhead | Safety | Ease of Use |
|----------|--------------|-----------|----------|--------|-------------|
| **Rust** | `extern "C"` / bindgen / cbindgen | Bidirectional | Near-zero | Unsafe blocks required | Moderate |
| **Python** | ctypes / cffi / C extensions | Call C from Python | Moderate | Low (manual memory) | ctypes: Easy; C ext: Hard |
| **Node.js** | N-API (NAPI) | Bidirectional | Moderate | Medium | Moderate |
| **Java** | JNI / JNA / Panama (FFM) | Bidirectional | High (JNI) / Medium (Panama) | Low (JNI) | JNI: Hard; Panama: Better |
| **C#/.NET** | P/Invoke / NativeAOT | Call C from .NET | Low-Medium | Medium | Moderate |
| **Zig** | `@cImport` / C ABI native | Bidirectional | Zero | Zig safety model | Excellent |
| **Go** | cgo | Bidirectional | ~40ns/call | Low | Moderate |
| **Swift** | C/ObjC bridging / C++ interop | Bidirectional | Low | High (with Swift 6.2) | Good |

### Detailed FFI Mechanisms

**Rust FFI (bindgen, cbindgen, UniFFI)**

- **bindgen**: Generates Rust FFI bindings from C/C++ header files. Takes `.h` files as input and produces Rust `extern` declarations. Widely used for wrapping system libraries.
- **cbindgen**: The reverse -- generates C/C++ headers from Rust code. Enables C programs to call Rust functions.
- **UniFFI** (Mozilla): A multi-language bindings generator. Write a Rust library once, generate bindings for Kotlin, Swift, Python, Ruby, and C#. Used by Firefox for cross-platform mobile components.

Rust's FFI is notable for requiring `unsafe` blocks at call sites, making FFI boundaries explicit in code review. The `extern "C"` ABI is stable and predictable.

**Python C Extensions / ctypes / cffi**

- **ctypes**: Standard library module. Loads shared libraries (`.so`, `.dll`) and calls C functions directly. No compilation step needed. Good for quick prototyping but manual type marshaling is error-prone.
- **cffi**: Third-party library, faster and easier than ctypes. Better PyPy support. However, runtime overhead prevents it from matching Cython or pybind11 performance.
- **C Extensions**: Writing CPython extension modules in C. Maximum performance but high development cost. The CPython Limited API and Stable ABI provide forward compatibility.
- **Cython**: Compiles Python-like code to C extensions. 70M+ monthly downloads. The practical sweet spot for most Python performance needs.
- **pybind11**: C++ header-only library for creating Python bindings of C++ code. Cleaner than raw C extensions for C++ interop.
- **PyO3**: Rust bindings for Python. Write Python extensions in Rust with a nice API. Growing rapidly.

**Node.js N-API (NAPI)**

N-API is the stable C API for building native addons. It is ABI-stable across Node.js versions, meaning a native addon compiled for one Node.js version works on newer versions without recompilation. The `napi-rs` project allows writing Node.js native addons in Rust with excellent ergonomics.

**Java JNI / Panama**

- **JNI (Java Native Interface)**: The original mechanism. Notoriously difficult: requires C header generation, manual type marshaling, careful reference management. Performance overhead is significant due to JNI call boundaries.
- **Project Panama (Foreign Function & Memory API)**: The modern replacement, stabilized in JDK 22. Provides direct access to native memory and functions without JNI boilerplate. Uses `jextract` to generate Java bindings from C headers. Dramatically better ergonomics and performance than JNI.

**P/Invoke (.NET)**

Platform Invocation Services allow .NET code to call native C functions. Declare the function signature with `[DllImport]` attributes and the runtime handles marshaling. Relatively straightforward for simple C APIs. NativeAOT compilation can eliminate the managed/native boundary entirely for .NET code calling C libraries.

**Zig's C Interop**

Zig has arguably the best C interop of any modern language:

- `@cImport` directly imports C header files -- no binding generation step needed
- Zig bundles a C/C++ compiler toolchain (based on Clang), so it can compile C dependencies as part of the build
- Zig can link against any C library as if it were native
- Bidirectional: C code can call Zig functions via the C ABI
- Zig natively supports C ABIs for the target platform
- Zig can translate C code to Zig source (experimental but useful for incremental migration)

This makes Zig an excellent "incremental modernization" tool for C codebases -- you can add Zig files to an existing C project without any wrapper layer.

**Go cgo**

Go's cgo allows calling C code from Go. The overhead is approximately 40ns per call (single-threaded), which is fast enough for most uses but problematic for tight loops. Key limitations:

- The scheduler must be notified before a C call (in case it blocks), adding overhead
- Cross-compilation with cgo is painful (requires a C cross-compiler for each target)
- Debugging across the Go/C boundary is difficult
- A 2025 empirical study identified 19 types of cgo-related issues, including risks of runtime crashes

The Go community generally advises avoiding cgo when possible and using pure Go alternatives.

**Swift/ObjC/C++ Bridging**

- **Objective-C**: Seamless bidirectional interop via the Swift/ObjC bridge. This is mature and production-hardened (it's how all iOS apps work).
- **C**: Swift can call C functions directly via bridging headers.
- **C++ Interop**: Available since Swift 5.9 (Xcode 15). Before this, C++ required an Objective-C bridging layer. Swift 6.2 (WWDC 2025) introduced safe interoperability using `Span` and `MutableSpan`, preventing access-after-free and out-of-bounds memory issues.

---

## 4. Code Conversion Tools

### Overview Table

| Tool | Domain | Approach | Success Rate | Status |
|------|--------|----------|-------------|--------|
| Amazon Q Code Transformation | Java upgrades (8->17) | AI + OpenRewrite | ~40% effort reduction | Production (AWS) |
| OpenRewrite | Java refactoring | Deterministic AST rewriting | High for supported recipes | Open source, active |
| js_of_ocaml | OCaml -> JavaScript | Bytecode compilation | High (mature) | Active, adding WASM |
| AI-assisted migration | General | LLM-based transpilation | ~50% of programs fixed | Research/early |

### Amazon Q Code Transformation

Amazon Q Developer's Transform capability automates Java version upgrades, .NET modernization, mainframe migration, and VMware workload transformation. Under the hood, it uses:

1. **OpenRewrite recipes**: Deterministic AST transformations for known patterns (e.g., deprecated API replacements)
2. **AI debugging**: An AI model that takes build errors into account and discovers solutions
3. **Combined approach**: OpenRewrite handles mechanical changes, AI handles the ambiguous cases

Results: Customers report 40%+ acceleration of migration effort. AWS plans to contribute OpenRewrite recipes developed for Amazon Q back to open source.

### OpenRewrite

OpenRewrite is an Apache2-licensed automated code refactoring ecosystem. It operates on a lossless semantic tree (LST) representation of source code, preserving formatting, comments, and whitespace. Key capabilities:

- **Language upgrades**: Java 8 -> 11 -> 17 -> 21, Spring Boot 2 -> 3
- **Framework migrations**: JUnit 4 -> 5, Log4j -> SLF4J, javax -> jakarta
- **Security patching**: Automated vulnerability fixes across dependency trees
- **Custom recipes**: Composable transformation units that can be combined

OpenRewrite succeeds because it targets a constrained domain (Java ecosystem) with deterministic, testable transformations. This is fundamentally different from general-purpose transpilation.

### js_of_ocaml

Compiles OCaml bytecode to JavaScript. Active development in 2025 includes:
- Preparation for `wasm_of_ocaml` (OCaml -> WASM)
- Effects support with dynamic switching between CPS and direct style
- Compatibility with Node.js 16+ and ECMAScript 6+ browsers

js_of_ocaml is notable as one of the most mature and correct bytecode-to-JS compilers, used in production for tools like the Coq proof assistant's web interface (jsCoq).

### AI-Assisted Code Migration

LLM-based transpilation tools (GitHub Copilot, GPT-4, Amazon Q, etc.) are being used for:
- Language migration (e.g., Python 2 -> 3, Java -> Kotlin, Ruby -> Go)
- Framework migration (e.g., AngularJS -> React)
- API modernization (e.g., callback -> async/await)

Research findings (2025): LLM transpilers can fix about 50% of buggy programs in some benchmarks, and 22% in harder datasets. The tool BatFix specifically targets "repairing" LLM-generated transpilation output. The fundamental limitation is that LLMs make plausible-looking but semantically incorrect translations, especially for edge cases involving:
- Concurrency semantics
- Error handling idioms
- Memory management models
- Type system differences

---

## 5. Universal IR Approaches

### LLVM IR

LLVM IR is a low-level, typed, static single assignment (SSA) intermediate representation. It serves as the compilation target for ~20 production languages:

| Language | LLVM Frontend | Notes |
|----------|--------------|-------|
| C/C++ | Clang | Reference frontend |
| Rust | rustc | Primary backend |
| Swift | swiftc | Apple's language |
| Zig | Self-hosted + LLVM | Transitioning to self-hosted |
| Julia | Julia compiler | JIT via LLVM |
| Kotlin/Native | Kotlin compiler | Via LLVM |
| Crystal | Crystal compiler | Via LLVM |
| Haskell | GHC (optional) | Via LLVM backend |
| Fortran | Flang | New LLVM-native frontend |
| D | LDC | Via LLVM |

**Strengths**: Excellent optimization passes, mature code generation for dozens of hardware targets, huge community and investment.

**Limitations**: LLVM IR is *too low-level* for cross-language interop. By the time code reaches LLVM IR, high-level semantics (garbage collection strategies, exception models, object layouts) have been lowered away. You cannot reconstruct Python semantics from LLVM IR.

### MLIR (Multi-Level Intermediate Representation)

MLIR, developed by Chris Lattner at Google (2018) and incorporated into LLVM, addresses LLVM IR's limitations by supporting multiple abstraction levels:

- **Dialects**: Custom operations, types, and attributes for specific domains
- **Progressive lowering**: High-level constructs are gradually lowered through multiple dialect levels to LLVM IR
- **Key users**: TensorFlow (XLA, TF Runtime), Mojo language (Modular Inc.), CIRCT (hardware design)

MLIR is not a universal IR for general-purpose languages but rather a *framework for building IRs*. It excels at domain-specific compilation (ML models, hardware synthesis) where multiple abstraction levels must coexist.

### WebAssembly as Universal Target

WebAssembly is increasingly treated as a universal compilation target beyond the browser:

| Aspect | Status (2026) |
|--------|--------------|
| Browser support | Universal (all major browsers) |
| Server-side runtimes | Wasmtime, Wasmer, WasmEdge, WAMR |
| WASI (system interface) | 0.2 stable; 0.3 targeting Feb 2026 |
| Component Model | Active development, gaining adoption |
| GC support (WasmGC) | Chrome 119+, Firefox 120+, Safari (recent) |
| Threads | Supported in Chrome, Firefox; limited elsewhere |
| SIMD | 128-bit SIMD supported; no 256-bit AVX equivalent |

**WASI (WebAssembly System Interface)**

WASI provides a capability-based system interface for WebAssembly modules:

- **WASI 0.2** (Preview 2): Stabilized late 2024. Incorporates the Component Model. Expanded APIs for filesystem, sockets, HTTP, clocks, random.
- **WASI 0.3**: Targeting February 2026. Adds native async I/O via `stream<T>` and `future<T>` types. Composable concurrency model.
- **WASI 1.0**: Expected after 0.3 stabilizes. Will be the first "stable" WASI release.

**WASM Component Model**

The Component Model is the most ambitious part of the WebAssembly ecosystem. It defines:

1. **WIT (WebAssembly Interface Types)**: A language-neutral IDL for defining component interfaces with high-level types (strings, records, variants, lists, etc.)
2. **Canonical ABI**: A standard binary encoding for WIT types across the WASM boundary
3. **Composability**: Components can be linked together regardless of source language
4. **Isolation**: Each component has its own linear memory, preventing cross-component memory corruption

The promise: write a component in Rust, another in Python, another in Go, and compose them into a single application with type-safe interfaces and no shared memory. This is fundamentally different from shared-library linking.

**Current language support for Component Model:**
- Rust: `cargo-component` tooling (mature)
- Go: TinyGo with component support
- Python: `componentize-py`
- JavaScript: `jco` (JavaScript Component Tools)
- C/C++: Via wit-bindgen

### GraalVM Truffle / Sulong

GraalVM's Truffle framework takes a different approach to universal execution:

- **Truffle**: A framework for writing AST interpreters that are automatically JIT-compiled by the Graal compiler. Languages implemented in Truffle get JIT compilation, garbage collection, and debugging "for free."
- **Sulong**: Executes LLVM bitcode on Truffle, enabling C, C++, Rust, and Fortran code to run in the GraalVM polyglot environment.
- **Polyglot protocol**: A shared object model that allows values to cross language boundaries without serialization.

**Assessment**: Technically elegant but practically limited. The overhead of running C via LLVM bitcode interpretation (even JIT-compiled) is significant compared to native execution. Adoption is strongest in the Java ecosystem (native-image for serverless, GraalPy as CPython alternative).

---

## 6. The Polyglot Dream Assessment

### What Has Actually Worked

| Approach | Example | Why It Worked |
|----------|---------|---------------|
| Typed superset transpilation | TypeScript -> JS | Zero runtime cost, gradual adoption, shared ecosystem |
| Single-language multi-target | Kotlin Multiplatform | Shared business logic, platform-specific UI |
| C ABI as lingua franca | Every language's FFI | Lowest common denominator that is actually universal |
| WASM for compute isolation | Cloudflare Workers, Fastly | Security + portability more valuable than raw performance |
| Domain-specific transpilation | OpenRewrite for Java | Constrained scope, deterministic, testable |
| Shared VM + JIT | GraalVM Truffle | Works when languages share a runtime model (JVM languages) |

### What Has Not Worked

| Approach | Example | Why It Failed |
|----------|---------|---------------|
| Universal transpiler | Various X-to-Y tools | Semantic gaps make output unidiomatic or incorrect |
| Write once, run everywhere UI | Java Swing/AWT, early Flutter Web | Platform UX expectations diverge too much |
| Full bidirectional transpilation | General case | Information loss in both directions |
| Polyglot VM for all workloads | GraalVM (outside JVM niche) | Overhead too high for systems programming |
| Language-agnostic standard library | Haxe std lib | Lowest common denominator disappoints everyone |

### Why Full Bidirectional Transpilation Is Impractical

Bidirectional transpilation between two languages A and B requires that:

1. Every concept in A has an equivalent in B, and vice versa
2. The mapping preserves semantics (not just syntax)
3. The output is idiomatic in the target language
4. Round-tripping (A -> B -> A) produces equivalent code

This fails in practice because of **semantic gaps** -- fundamental differences in how languages model computation:

**Memory management**:
- Rust: ownership + borrowing (compile-time)
- Go: tracing garbage collector
- Swift: reference counting (ARC)
- C: manual allocation/deallocation

Translating Rust ownership semantics to Go garbage collection loses the zero-overhead guarantee. Translating Go GC code to Rust requires inventing ownership annotations that did not exist in the source.

**Error handling**:
- Rust: `Result<T, E>` (algebraic types, no exceptions)
- Java: checked + unchecked exceptions
- Go: multiple return values `(value, error)`
- Python: exceptions (duck-typed)

A Java method that `throws IOException` cannot be mechanically translated to idiomatic Rust or Go without making design decisions that may be wrong.

**Concurrency models**:
- Go: goroutines + channels (CSP)
- Rust: `async`/`await` + ownership (no data races by construction)
- Java: threads + locks + virtual threads (Project Loom)
- JavaScript: single-threaded event loop + promises
- Erlang: actors + message passing

These are not syntactic differences -- they reflect fundamentally different approaches to concurrent computation. A goroutine-based Go program cannot be mechanically translated to idiomatic Rust async code.

**Type system differences**:
- Structural typing (TypeScript, Go interfaces) vs nominal typing (Java, Rust)
- Higher-kinded types (Haskell, Scala) vs no HKT (Go, Rust)
- Union types (TypeScript) vs tagged unions (Rust enums) vs exception types (Java)
- Null safety (Kotlin, Rust `Option`) vs nullable-everything (Java, Python)

### The "Lowest Common Denominator" Problem

When targeting multiple platforms/languages, the shared abstraction must be expressible in all targets. This creates a ceiling:

```
Expressive power of shared code <= MIN(expressive power of all targets)
```

Concrete examples:
- **Haxe**: Cannot use C++ templates, Java generics with reification, or Python dynamic features in shared code
- **Kotlin Multiplatform**: `expect`/`actual` declarations bridge the gap but push platform-specific code outside shared modules
- **WASM Component Model**: High-level types (strings, lists, records) are supported, but no generics, no inheritance, no closures in WIT interfaces

The practical consequence is that the "shared" layer becomes a thin interface layer, and platform-specific implementations handle the interesting parts. This is useful (it eliminates boilerplate) but falls short of the "write once" dream.

### What Actually Works in Practice

The most successful approaches accept semantic gaps and work *with* them:

1. **Shared data models + serialization** (Protocol Buffers, JSON Schema, GraphQL): Define the *interface*, not the implementation. Each language implements idiomatically.
2. **Shared business logic + platform UI** (Kotlin Multiplatform, React Native): Accept that UI must be platform-specific. Share the parts that are genuinely platform-independent.
3. **C ABI boundary** (every FFI): Accept the lowest common denominator *at the boundary*, write idiomatic code on both sides.
4. **WASM components** (emerging): Type-safe interfaces between isolated components, each written in the best language for the job.

---

## 7. WASM + Native Dual-Target Feasibility

### Languages Successfully Doing This

| Language | Native Target | WASM Target | Shared Codebase | Maturity |
|----------|--------------|-------------|-----------------|----------|
| **Rust** | All LLVM targets | wasm32-unknown-unknown, wasm32-wasip1/p2 | High (with `cfg` gates) | Production |
| **Zig** | All LLVM targets + custom | wasm32-freestanding, wasm32-wasi | High | Production |
| **C/C++** | All (native compilers) | Via Emscripten | Medium (requires porting) | Production |
| **Go** | All Go targets | GOOS=js GOARCH=wasm | Medium (large runtime) | Stable |
| **Kotlin** | Kotlin/Native (LLVM) | Kotlin/Wasm (WasmGC) | High (via KMP) | Beta (WASM) |
| **Dart** | AOT (ARM, x86) | dart2wasm (WasmGC) | High (Flutter) | Production |
| **.NET** | Native AOT | Blazor WASM | Medium | Production |

### Rust: The Gold Standard for Dual-Target

Rust is the most mature language for native + WASM dual targeting:

```
# Native compilation
cargo build --release --target x86_64-unknown-linux-gnu

# WASM compilation
cargo build --release --target wasm32-unknown-unknown

# WASI compilation
cargo build --release --target wasm32-wasip2
```

**What works well**:
- The same `core` and `std`-compatible code compiles to both targets
- `wasm-bindgen` provides ergonomic JS interop for browser WASM
- `wasm-pack` automates the build/package workflow for npm
- `cfg(target_arch = "wasm32")` gates enable platform-specific code paths

**Challenges**:
- `wasm32-unknown-unknown` has ABI incompatibility with C compiled to WASM, limiting interop to pure Rust
- Threading: WASM threads are limited compared to native (shared memory model, no fork/exec)
- SIMD: WASM SIMD is limited to 128-bit registers vs 256-bit AVX2 on native, resulting in 4x slowdown for SIMD-heavy workloads
- File I/O: requires WASI or custom JS glue in browser environments
- The WASM ecosystem is fragmented across three targets (`wasm32-unknown-unknown`, `wasm32-wasip1`, `wasm32-wasip2`)

### wasm-bindgen Approach

wasm-bindgen facilitates high-level interactions between Rust WASM modules and JavaScript:

- **Import JS into Rust**: DOM manipulation, console logging, Web APIs
- **Export Rust to JS**: Functions, structs (as JS classes), enums
- **Lightweight**: Only generates bindings for JS imports you actually use
- **Type-safe**: Statically checked bindings, with the promise of eliminating dynamic type checks for DOM access (potentially faster-than-JS DOM manipulation)
- **web-sys**: Auto-generated bindings for all Web APIs (DOM, Canvas, WebGL, Fetch, etc.)
- **js-sys**: Bindings for JavaScript built-ins (Array, Object, Promise, etc.)

### C/C++ via Emscripten

Emscripten compiles C/C++ to WASM via the LLVM backend:

**Strengths**:
- Massive existing C/C++ codebase compatibility
- SDL, OpenGL (via WebGL), POSIX emulation layers
- Successful ports: SQLite, FFmpeg, Doom, Qt

**Challenges (identified in 2025 research)**:
- 11 new compiler bugs confirmed by Emscripten developers in a cross-compilation study
- Silent miscompilation risks for legacy code
- Platform-specific C code (POSIX, Win32) requires significant porting
- Not all C++ features work (especially code relying on longjmp, signals, or platform-specific ABIs)
- Build system integration is complex (requires Emscripten's modified Clang, custom sysroot)

### Component Model Promises

The WASM Component Model aims to solve the polyglot interop problem at the WASM level:

```
┌─────────────────────────────────────────────────┐
│              Application                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐      │
│  │ Component │  │ Component │  │ Component │      │
│  │  (Rust)   │  │ (Python)  │  │  (Go)     │      │
│  │           │  │           │  │           │      │
│  │ WIT iface │  │ WIT iface │  │ WIT iface │      │
│  └─────┬─────┘  └─────┬─────┘  └─────┬─────┘      │
│        │              │              │              │
│        └──────────────┼──────────────┘              │
│                       │                             │
│              Canonical ABI                          │
│                       │                             │
│              WASM Runtime                           │
└─────────────────────────────────────────────────┘
```

**Current state (early 2026)**:
- WIT interface definitions are stable
- `cargo-component` for Rust is the most mature tooling
- `jco` enables JavaScript components
- `componentize-py` brings Python into the model
- WASI 0.3 (targeting Feb 2026) adds async I/O, which is critical for real-world usage

**Remaining gaps**:
- No streaming/incremental compilation of components
- Component composition tooling is still early
- Performance overhead of canonical ABI encoding/decoding for hot paths
- Limited debugging support across component boundaries

### Performance: Native vs WASM

| Metric | Native | WASM (Browser) | WASM (Server/Wasmtime) | Notes |
|--------|--------|----------------|----------------------|-------|
| Throughput (CPU-bound) | Baseline | 45-55% slower (avg) | 10-30% slower | Varies hugely by workload |
| Startup time | Fast (AOT) | Slow (download + compile) | Fast (AOT via Wasmtime) | Browser must download + compile |
| Memory overhead | Baseline | ~1.5-2x (linear memory) | ~1.2x | WASM linear memory model |
| SIMD | Full (AVX2/512) | 128-bit only | 128-bit only | 4x gap for SIMD-heavy work |
| Threads | Full OS threads | SharedArrayBuffer + Workers | WASI threads (experimental) | WASM threading is limited |
| File I/O | Direct syscalls | None (requires JS bridge) | Via WASI | Browser has no filesystem |
| DOM access | N/A | Via wasm-bindgen/JS glue | N/A | Still requires JS bridge |
| Binary size | Larger (includes stdlib) | Smaller (tree-shaken) | Smaller | WASM binaries are compact |

Research benchmarks (USENIX ATC):
- WASM applications run 45% (Firefox) to 55% (Chrome) slower than native on average
- Peak slowdowns reach 2.08x (Firefox) and 2.5x (Chrome)
- Rust-to-WASM has the smallest performance gap among languages tested
- Native Node.js modules are 1.75-2.5x faster than WASM equivalents

### Challenges for Dual-Target Architecture

**DOM Access**: WASM cannot directly access the DOM. All DOM manipulation goes through JavaScript glue code. wasm-bindgen aims to make this near-zero overhead, and there is a long-term goal of eliminating the JS shim entirely, but as of 2026 the JS bridge is still required.

**System APIs**: Native code has direct access to the operating system (files, network, processes, signals). WASM code must go through WASI, which provides a capability-based subset of system functionality. Not all POSIX APIs are available, and some (like `fork`, `exec`, signals) may never be.

**Conditional compilation pattern** (Rust example):
```rust
// Shared code
pub fn compute(data: &[u8]) -> Result<Vec<u8>, Error> {
    // Pure computation works identically on both targets
}

// Platform-specific
#[cfg(target_arch = "wasm32")]
pub fn load_data() -> Vec<u8> {
    // Fetch from JS / WASI filesystem
}

#[cfg(not(target_arch = "wasm32"))]
pub fn load_data() -> Vec<u8> {
    // Read from native filesystem
    std::fs::read("data.bin").unwrap()
}
```

**Recommendation for new language design**: Structure the standard library into layers:
1. **Core** (pure computation): Works everywhere, no I/O, no allocation policy
2. **Platform abstraction** (I/O, networking, filesystem): Compile-time selected backend (WASI vs POSIX vs Win32)
3. **Target-specific** (DOM bindings, native GUI, system calls): Explicitly platform-gated

This mirrors what Rust, Zig, and Kotlin Multiplatform have converged on independently.

---

## Summary of Key Findings

### For a New Language Targeting WASM + Native

1. **Transpilation is not the answer** for general-purpose cross-language interop. It works for constrained cases (TypeScript->JS, Cython->C) but fails for bidirectional or multi-target scenarios due to semantic gaps.

2. **The C ABI remains the universal FFI**, but the WASM Component Model is the most promising candidate to eventually supplement or replace it for cross-language composition.

3. **WASM + native dual-target is proven feasible** by Rust and Zig. The key architectural decision is layering the standard library so that pure computation is target-independent and I/O is abstracted behind a platform layer.

4. **Performance gap is real but narrowing**: WASM is 1.5-2.5x slower than native for most workloads. For many applications (business logic, web services), this is acceptable. For SIMD-heavy or latency-critical workloads, native remains necessary.

5. **The Component Model is the most exciting development**: Language-agnostic, type-safe, memory-isolated composition of WASM modules. WASI 0.3 (early 2026) with async I/O will be a major milestone. Design new languages with Component Model interop in mind.

6. **Kotlin Multiplatform and Dart/Flutter prove** that "shared logic + platform UI" is the practical sweet spot for multi-target development. Accept platform divergence for UI; share the rest.

7. **Design the FFI boundary deliberately**: Zig's `@cImport` approach (zero-cost C interop with no binding generation) is the gold standard for native interop. For WASM, design for WIT interfaces from day one.
