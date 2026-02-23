# Innovations in Programming Language Design

A comprehensive survey of breakthroughs, paradigms, and design decisions across modern programming languages. This document serves as a research foundation for new language design by examining what works, what doesn't, and why.

---

## 1. Type System Innovations

### 1.1 Dependent Types (Idris, Agda)

Dependent types allow types to depend on *values*, not just other types. This collapses the boundary between the type level and the term level, enabling proofs to be embedded directly in programs.

**Agda** is a dependently typed language and proof assistant based on Martin-Lof's type theory. It is primarily used in academic research for formal verification. Every function in Agda must be total (terminate on all inputs), which makes it suitable as a logical framework.

**Idris** (particularly Idris 2) takes dependent types into the practical domain. Unlike Agda, Idris is designed as a general-purpose programming language. Key features:

- **Quantitative Type Theory (QTT):** Idris 2 integrates linear types directly into its core type theory. Each variable binding carries a *quantity* annotation (0, 1, or unrestricted), enabling the compiler to reason about resource usage at the type level.
- **Totality Checking:** The compiler can verify that functions are total (always produce a result) or partial (may not terminate), providing stronger safety guarantees.
- **Elaborator Reflection:** Idris exposes its type checker's internal elaboration process as a library, allowing metaprogramming within the type system itself.

**Canonical example -- length-indexed vectors:**

```idris
data Vect : Nat -> Type -> Type where
  Nil  : Vect 0 a
  (::) : a -> Vect n a -> Vect (S n) a

-- The type GUARANTEES this function only accepts non-empty vectors
head : Vect (S n) a -> a
head (x :: _) = x
```

Here, calling `head` on an empty vector is not a runtime error -- it is a *compile-time type error*. The length is encoded in the type.

| Language | Primary Use | Totality | Linear Types | Practical Focus |
|----------|-------------|----------|--------------|-----------------|
| Agda     | Proof assistant / research | Required | No | Low |
| Idris 2  | General-purpose | Optional (checked) | Yes (QTT) | High |
| Coq      | Proof assistant | Required | No | Low |
| Lean 4   | Proof assistant + programming | Required | No | Medium |
| F*       | Verification | Optional | No | Medium |

### 1.2 Refinement Types (Liquid Haskell)

Refinement types augment base types with logical predicates. Rather than proving properties in a full dependent type system, refinement types use SMT solvers (like Z3) to automatically discharge proof obligations.

**Liquid Haskell** annotates Haskell types with refinement predicates:

```haskell
{-@ type Pos = {v:Int | v > 0} @-}
{-@ type NonEmpty a = {v:[a] | len v > 0} @-}

{-@ head :: NonEmpty a -> a @-}
head (x:_) = x
-- No need to handle the empty list case: the type system proves it impossible
```

Key advantages over full dependent types:
- **Automatic verification:** SMT solvers handle the proof work; programmers just write predicates.
- **Incremental adoption:** Refinement annotations can be added to existing Haskell code gradually.
- **Abstract Refinement Types:** Allow parameterizing refinements over predicates, enabling reusable, verified abstractions.

Recent research (POPL 2024) has explored *Answer Refinement Modification (ARM)*, which extends refinement type systems to precisely track algebraic effects during execution -- bridging the gap between refinement types and effect systems.

### 1.3 Effect Systems (Koka, Unison)

Effect systems make side effects explicit in function signatures. Rather than treating all functions as potentially effectful (as in most imperative languages) or wrapping effects in monads (as in Haskell), effect systems track effects as first-class parts of the type system.

**Koka** (Microsoft Research) uses row-polymorphic effect types:

```koka
fun divide(x: int, y: int): exn int
  if y == 0 then throw("division by zero")
  else x / y

fun greet(): console ()
  println("Hello, world!")

// Effects compose automatically:
fun main(): <console, exn> ()
  val result = divide(10, 2)
  greet()
```

Key innovation: both the type *and* effect of expressions are inferred automatically using Hindley-Milner-style inference extended with row polymorphism. The programmer rarely needs to write effect annotations explicitly.

**Unison** takes a radically different approach with its "abilities" system:

- Functions declare which abilities (effects) they require in their type signature.
- Handlers provide concrete implementations for those abilities, making them swappable.
- Since abilities are tracked by the type system, it is impossible for a function to use an ability it has not declared.

```unison
structural ability Store where
  get : Nat
  put : Nat -> ()

increment : '{Store} ()
increment = do
  current = Store.get
  Store.put (current + 1)
```

Unison's deeper innovation is **content-addressed code**: functions are identified by a hash of their AST, not by name. This eliminates builds, dependency conflicts, and makes renaming trivial. Combined with its effect system, this enables seamless distributed programming -- computations can be moved between machines with missing dependencies deployed on the fly.

### 1.4 Algebraic Effects and Handlers

Algebraic effects generalize exceptions, async/await, generators, and coroutines into a single unified mechanism. An algebraic effect declares a set of operations; a handler defines how those operations are interpreted.

**Key languages:**

| Language | Effect Mechanism | Handler Style | Status |
|----------|-----------------|---------------|--------|
| Koka     | Row-polymorphic effects | First-class handlers | Production-ready |
| Eff      | Algebraic effects | First-class handlers | Research |
| Unison   | Abilities | Swappable handlers | Production-ready |
| OCaml 5  | Effect handlers | Shallow handlers | Stable (since 5.0) |
| Haskell  | Libraries (polysemy, effectful, etc.) | Type-class based | Ecosystem |

The power of algebraic effects comes from their composability: unlike monads, effects compose without requiring monad transformers. A function can declare multiple effects, and each can be handled independently.

### 1.5 Session Types

Session types encode communication protocols directly in the type system. They guarantee that:
- Messages are sent and received in the correct order.
- All branches of a protocol are handled.
- Sessions are properly completed (no dangling connections).

Originally from pi-calculus research, session types have appeared in languages like Links, and research extensions to Haskell and Rust. They are particularly relevant for distributed systems and microservice architectures.

### 1.6 Flow Typing (TypeScript)

Flow typing (also called type narrowing or occurrence typing) refines a variable's type based on control flow analysis:

```typescript
function process(value: string | number) {
  if (typeof value === "string") {
    // TypeScript knows `value` is `string` here
    console.log(value.toUpperCase());
  } else {
    // TypeScript knows `value` is `number` here
    console.log(value.toFixed(2));
  }
}
```

TypeScript's type system is intentionally *unsound* -- it allows certain operations that cannot be verified at compile time. This is a deliberate design choice: TypeScript prioritizes practical developer experience over formal correctness, accommodating JavaScript's dynamic nature. The seven known sources of unsoundness include type assertions, property access on `any`, variance in function types, and more.

**Contrast with Dart:** Dart also uses flow analysis for type promotion but chose *sound* null safety -- the compiler guarantees at the type level that a non-nullable variable can never hold null. Dart 3 completed the transition to a fully sound, null-safe language.

### 1.7 Sound Null Safety (Dart, Kotlin)

Null reference errors -- Tony Hoare's "billion-dollar mistake" -- are addressed differently across languages:

| Language | Approach | Soundness | Retrofit Status |
|----------|----------|-----------|----------------|
| Dart 3   | Sound null safety with flow analysis | Sound | Complete |
| Kotlin   | Nullable types (`T?` vs `T`) | Sound | From inception |
| Swift    | Optionals (`Optional<T>`) | Sound | From inception |
| TypeScript | Strict null checks (optional) | Unsound | Opt-in |
| C#       | Nullable reference types | Unsound (warnings only) | Opt-in |
| Rust     | `Option<T>` (no null exists) | Sound | From inception |

Dart's journey is instructive: retrofitting null safety onto an existing language required a multi-year migration with mixed-mode execution (sound and unsound code coexisting during transition).

---

## 2. Performance Breakthroughs

### 2.1 Zero-Cost Abstractions (Rust, C++)

The principle that high-level abstractions should compile down to code as efficient as hand-written low-level equivalents.

**Rust** achieves this through:
- **Monomorphization:** Generic code is compiled into specialized versions for each concrete type, eliminating virtual dispatch overhead.
- **Iterators:** Rust's iterator chains (`.map()`, `.filter()`, `.fold()`) compile to the same machine code as hand-written loops.
- **Ownership system:** Memory safety guarantees are enforced entirely at compile time, with zero runtime cost -- no reference counting, no garbage collection, no safety checks at runtime.
- **Traits with static dispatch:** Trait method calls are resolved at compile time (unless `dyn Trait` is explicitly requested).

**C++** pioneered the concept:
- Templates enable compile-time code generation.
- `constexpr` and `consteval` move computation to compile time.
- RAII ensures deterministic cleanup without runtime overhead.

**Caveat:** "Zero-cost" refers to runtime cost, not compile-time cost. Rust's extensive compile-time analysis (ownership checks, borrow checking, monomorphization) leads to significantly longer build times. In debug mode, Rust can be up to 10x slower than release mode because many "zero-cost" abstractions (especially iterators) are expensive without aggressive optimization passes.

### 2.2 Compile-Time Execution

Moving computation from runtime to compile time eliminates overhead entirely.

| Language | Mechanism | Capabilities | Limitations |
|----------|-----------|--------------|-------------|
| Zig | `comptime` keyword | Any expression, type reflection, generic programming | Increases compile time; no I/O |
| C++ | `constexpr` / `consteval` | Arithmetic, string processing, containers (C++20+) | Limited to "constant expressions"; complex rules |
| D | CTFE (Compile-Time Function Evaluation) | Nearly arbitrary D code | Memory limits; no pointers to runtime data |
| Rust | `const fn`, `const` generics | Growing subset of Rust; const generics stable | Many features still unstable |
| Nim | `static` blocks, compile-time procs | Arbitrary Nim code | Separate compile-time VM |

**Zig's `comptime`** is the most radical approach. Any expression can be prefixed with `comptime`, forcing compile-time evaluation. This replaces generics, templates, macros, and conditional compilation with a single unified mechanism:

```zig
fn Matrix(comptime T: type, comptime rows: usize, comptime cols: usize) type {
    return struct {
        data: [rows][cols]T,
        // Methods defined here are specialized per (T, rows, cols)
    };
}

// Usage: fully resolved at compile time
const Mat4x4 = Matrix(f32, 4, 4);
```

Practical benchmarks have shown `comptime` cutting runtime overhead by approximately 40% in certain workloads by shifting computation to the build phase. However, this is not free -- it merely moves costs from runtime to compile time, and complex `comptime` code can significantly increase build times.

### 2.3 Profile-Guided Optimization (PGO)

PGO uses runtime profiling data to inform compiler optimization decisions, effectively combining static analysis with actual execution behavior.

**How it works:**
1. **Instrumentation build:** The compiler inserts profiling counters into the binary.
2. **Training run:** The instrumented binary runs representative workloads, collecting data on branch frequencies, hot paths, and call patterns.
3. **Optimized build:** The compiler uses the profile data to make better decisions about inlining, branch prediction hints, code layout, and register allocation.

**Hardware-based PGO (HWPGO):** Intel's hardware sampling approach dramatically reduces profiling overhead compared to instrumentation, enabling PGO collection on production workloads with production binaries.

**Language-level support:**

| Language | PGO Support | Typical Improvement |
|----------|------------|-------------------|
| Go       | Built-in (since Go 1.20) | 2-14% (Go 1.22 benchmarks) |
| Rust     | Via LLVM flags | 5-20% |
| C/C++    | Compiler-specific (`-fprofile-generate` / `-fprofile-use`) | 10-30% |
| Java     | JIT-based (automatic, always-on) | N/A (inherent to JIT) |

### 2.4 Auto-Vectorization

Modern compilers can automatically transform scalar loops into SIMD (Single Instruction, Multiple Data) instructions. PGO enhances auto-vectorization because the compiler can better determine loop trip counts and data alignment.

Key compiler technologies:
- **LLVM's Loop Vectorizer:** Transforms loops to process multiple elements per instruction.
- **GCC's tree vectorizer:** Similar capabilities with different heuristics.
- **Polyhedral optimization:** Advanced loop transformation (used in MLIR/Mojo for GPU targeting).

Languages like Mojo explicitly target auto-vectorization through MLIR, which provides higher-level optimization passes than LLVM alone, enabling vectorization across heterogeneous hardware (CPUs, GPUs, TPUs).

### 2.5 Cache-Oblivious Algorithms in Language Design

Cache-oblivious algorithms perform well across all levels of the memory hierarchy without knowing cache sizes. While traditionally an algorithm design concern, some languages incorporate cache-awareness into their data structure design:

- **Zig:** Exposes memory layout control, allowing programmers to choose between arrays of structs (AoS) and structs of arrays (SoA) to optimize cache performance.
- **Rust:** `#[repr(C)]` and field ordering give precise control over memory layout.
- **Data-Oriented Design (DOD):** Languages like Zig and Odin embrace DOD principles, structuring data for cache efficiency rather than object-oriented encapsulation.
- **Java's Project Valhalla:** Introduces value types (inline classes) to eliminate pointer indirection and improve cache locality for small objects.

---

## 3. Developer Experience Innovations

### 3.1 Amazing Error Messages (Elm, Rust)

**Elm** pioneered the "compiler as assistant" philosophy. Its error messages:
- Use plain English, not jargon.
- Show the exact location of the error with context.
- Suggest specific fixes.
- Hide internal compiler representations entirely.
- Act as a teaching tool for the language.

```
-- TYPE MISMATCH -------------------------------------------------- Main.elm

The 2nd argument to `map` is not what I expect:

8|   List.map String.toUpper [1, 2, 3]
                              ^^^^^^^
This argument is a list of numbers, but `String.toUpper` needs:

    String

Hint: Try using String.fromInt to convert the numbers to strings first.
```

**Rust** adopted Elm's approach and extended it for a systems language context. Key innovations:
- **Error codes:** Every error has a unique code (e.g., `E0382`) with detailed online documentation.
- **Multi-span errors:** Errors can highlight multiple related locations in the code.
- **Suggestion applicability:** The compiler distinguishes between machine-applicable suggestions (safe to auto-apply) and human-guidance suggestions.
- **`cargo fix`:** Can automatically apply compiler suggestions.

```
error[E0382]: borrow of moved value: `s`
 --> src/main.rs:4:20
  |
2 |     let s = String::from("hello");
  |         - move occurs because `s` has type `String`
3 |     let s2 = s;
  |              - value moved here
4 |     println!("{}", s);
  |                    ^ value borrowed here after move
  |
  = note: this error originates because `String` does not implement
    the `Copy` trait
help: consider cloning the value
  |
3 |     let s2 = s.clone();
  |               ++++++++
```

### 3.2 Language Server Protocol (LSP)

LSP, created by Microsoft for VS Code, decouples language intelligence from editor implementations. Before LSP, every editor needed its own language plugin for each language (M editors x N languages = M*N implementations). LSP reduces this to M + N.

**Capabilities provided by LSP:**
- Auto-completion, go-to-definition, find-all-references
- Inline diagnostics, code actions, refactoring
- Hover information, signature help
- Semantic syntax highlighting

**Impact:** LSP has become the de facto standard. Nearly every modern language ships with an official language server. This has dramatically leveled the playing field -- smaller languages can provide IDE-quality tooling by implementing a single LSP server.

| Language | Language Server | Notable Features |
|----------|----------------|------------------|
| Rust     | rust-analyzer  | Inline type hints, macro expansion, lifetime visualization |
| Go       | gopls          | Integrated with go toolchain |
| TypeScript | tsserver     | Foundational LSP implementation |
| Python   | Pylance / pyright | Type inference, stub file support |
| Zig      | zls            | Comptime evaluation in editor |
| Haskell  | HLS            | Type-level information display |

### 3.3 Hot Reloading

Hot reloading allows developers to modify running programs without restarting them, preserving application state.

**Approaches by language:**
- **Clojure (ClojureScript + Figwheel):** Hot code reloading in the browser with state preservation. The REPL can evaluate code within a running application.
- **Erlang/Elixir:** Hot code swapping in production -- a running telecom system can be upgraded without downtime. This is built into the BEAM VM at a fundamental level.
- **React (JavaScript):** Fast Refresh preserves component state during edits, enabled by the functional component model.
- **Swift Playgrounds / Xcode Previews:** Immediate visual feedback for UI code.
- **Flutter (Dart):** Sub-second hot reload of widget trees, preserving navigation state and scroll positions.

### 3.4 Integrated Toolchains (Cargo, Go)

Modern languages ship with unified toolchains that replace the fragmented tool ecosystems of older languages.

**Rust's Cargo** provides:
- Package management (crates.io registry)
- Build system (dependency resolution, feature flags, build scripts)
- Test runner (`cargo test`)
- Documentation generator (`cargo doc`)
- Benchmarking (`cargo bench`)
- Linting integration (`cargo clippy`)
- Formatting (`cargo fmt`)
- Publishing (`cargo publish`)
- Workspace support for monorepos

**Go's toolchain** provides:
- `go build`, `go run`, `go test` -- unified build/test
- `go mod` -- dependency management (no separate package manager)
- `go fmt` -- canonical formatting (eliminates style debates)
- `go doc` -- documentation from source
- `go vet` -- static analysis
- Cross-compilation via `GOOS` and `GOARCH` environment variables
- Built-in profiling (`go tool pprof`)
- Built-in race detector (`go run -race`)

**Contrast with C/C++:** The C/C++ ecosystem requires separately choosing and configuring a compiler (GCC, Clang, MSVC), build system (Make, CMake, Meson, Bazel), package manager (Conan, vcpkg, none), formatter (clang-format), linter (clang-tidy), and testing framework (Google Test, Catch2, etc.).

### 3.5 REPL-Driven Development (Clojure, Julia)

REPL-driven development treats the Read-Eval-Print Loop as the central development interface, not just a debugging tool.

**Clojure** is the gold standard:
- The REPL connects directly to a running application (via nREPL protocol).
- Developers evaluate expressions in the context of the live system.
- Libraries can be hot-loaded into a running REPL without restart.
- Editor integrations (CIDER for Emacs, Cursive for IntelliJ, Calva for VS Code) send code from the editor directly to the REPL.
- The workflow is: write a function -> evaluate it -> test it interactively -> iterate -> commit.

**Julia** provides a similar experience:
- Revise.jl enables automatic reloading of modified source files.
- The REPL supports help mode (`?`), shell mode (`;`), and package mode (`]`).
- Designed for scientific computing where interactive exploration is essential.

### 3.6 Playground Environments

Browser-based playgrounds lower the barrier to trying a language:

| Language | Playground | Notable Features |
|----------|-----------|------------------|
| Rust     | play.rust-lang.org | Multiple editions, Clippy, Miri, assembly output |
| Go       | go.dev/play | Shareable links, deterministic output |
| TypeScript | typescriptlang.org/play | Side-by-side JS output, type visualization |
| Kotlin   | play.kotlinlang.org | Koans (interactive tutorials) |
| Swift    | Swift Playgrounds (app) | Visual output, educational focus |
| Zig      | zig.godbolt.org | Compiler Explorer integration |

---

## 4. Concurrency Models

### 4.1 Communicating Sequential Processes -- CSP (Go)

Go's concurrency model is based on Tony Hoare's CSP (1978). Key primitives:

- **Goroutines:** Lightweight green threads (initial stack ~2-8 KB, growing as needed). Creating millions of goroutines is routine.
- **Channels:** Typed, synchronized communication pipes between goroutines. Can be buffered or unbuffered.
- **`select` statement:** Multiplexes across multiple channel operations, similar to Unix's `select()` for file descriptors.

```go
func producer(ch chan<- int) {
    for i := 0; i < 100; i++ {
        ch <- i
    }
    close(ch)
}

func consumer(ch <-chan int) {
    for val := range ch {
        fmt.Println(val)
    }
}
```

**Key distinction from actors:** In CSP, processes are anonymous and communicate through named channels. In the actor model, actors have identities and receive messages at their mailbox. CSP emphasizes the *medium* of communication; actors emphasize the *endpoints*.

**Criticism:** Go's goroutines allow shared mutable state. The `go` keyword provides no structured lifetime guarantees -- goroutines can leak. This is one motivation for structured concurrency proposals.

### 4.2 Actor Model (Erlang/Elixir)

Erlang's actor model is the foundation of the BEAM virtual machine, designed at Ericsson in the 1980s for telecommunications systems requiring extreme reliability.

**Core principles:**
- **Process isolation:** Each actor (Erlang process) has its own heap. No shared memory. Communication is exclusively through message passing.
- **Lightweight processes:** A single BEAM VM can run millions of processes, each with ~300 bytes initial memory.
- **"Let it crash" philosophy:** Processes are expected to fail. Supervisors monitor child processes and restart them according to configurable strategies (one-for-one, one-for-all, rest-for-one).
- **Hot code swapping:** Running systems can be upgraded without downtime -- critical for telecom systems requiring 99.999% uptime.
- **Location transparency:** Sending a message to a process on another machine uses the same syntax as local messaging.

**Elixir** builds on the BEAM VM with modern syntax, metaprogramming (macros), and the Phoenix web framework. It adds:
- `Task` and `GenServer` abstractions over raw processes.
- `Broadway` for data pipeline processing.
- LiveView for real-time web UIs powered by server-side processes.

### 4.3 Async/Await (JavaScript, C#, Rust, Python)

Async/await is syntactic sugar over futures/promises that makes asynchronous code read like synchronous code.

| Language | Runtime | Colored Functions? | Zero-Cost? | Cancellation |
|----------|---------|-------------------|-----------|--------------|
| JavaScript | Single-threaded event loop | Yes | No (heap-allocated promises) | AbortController |
| C#       | Thread pool (SynchronizationContext) | Yes | No (Task allocation, though ValueTask exists) | CancellationToken |
| Rust     | User-chosen (tokio, async-std) | Yes | Yes (state machine, no heap alloc) | Drop-based |
| Python   | asyncio event loop | Yes | No | Task.cancel() |
| Kotlin   | Coroutines (structured) | No (suspend functions) | Partially | Structured (Job) |
| Swift    | Swift Concurrency runtime | No (actor isolation) | Partially | Task cancellation |

**Rust's approach is unique:** Async functions compile to state machines with no heap allocation. Futures are lazy -- they do nothing until polled. The runtime (tokio, async-std) is a library, not built into the language. This gives maximum flexibility but creates ecosystem fragmentation.

**The "colored function" problem:** In most async/await implementations, async functions can only be called from other async functions, creating a division between "sync world" and "async world." Zig, Go, and Erlang avoid this problem entirely by making all I/O non-blocking at the runtime level.

### 4.4 Structured Concurrency (Kotlin, Swift, Java)

Structured concurrency ensures that concurrent operations are scoped to a lexical block, preventing resource leaks and simplifying error handling. The key insight: concurrent tasks should follow the same structured lifetime rules as local variables.

**Kotlin Coroutines:**
```kotlin
coroutineScope {  // Scope: all children must complete before this returns
    val deferred1 = async { fetchUser() }
    val deferred2 = async { fetchOrders() }
    // If either fails, the other is automatically cancelled
    val user = deferred1.await()
    val orders = deferred2.await()
}
// Here, both tasks are guaranteed to be finished
```

**Swift Concurrency (Swift 5.5+):**
```swift
await withTaskGroup(of: Data.self) { group in
    for url in urls {
        group.addTask { await fetchData(from: url) }
    }
    for await data in group {
        process(data)
    }
}
// All tasks complete before exiting the group
```

**Java (Project Loom, JDK 21+):**
- Virtual threads: lightweight threads managed by the JVM (similar to goroutines).
- `StructuredTaskScope`: Ensures child tasks are bounded to the parent scope.

**Benefits over unstructured concurrency:**
- No orphaned tasks (goroutine/thread leaks).
- Automatic cancellation propagation.
- Exception handling follows lexical scoping.
- Easier to reason about concurrent lifetimes.

### 4.5 Software Transactional Memory (Haskell, Clojure)

STM treats memory operations like database transactions: operations are atomic, consistent, and isolated.

**Haskell's STM** provides the strongest guarantees:
- The `STM` monad is separate from the `IO` monad. You *cannot* perform arbitrary I/O inside a transaction (enforced at compile time).
- Transactions are composed with `orElse` (try A, if it retries, try B) and `retry` (block until relevant variables change).
- Uses Optimistic Concurrency Control: transactions run in parallel, and if a conflict is detected, one is automatically retried.

```haskell
transfer :: TVar Int -> TVar Int -> Int -> STM ()
transfer from to amount = do
    fromBal <- readTVar from
    if fromBal < amount
        then retry  -- Blocks until `from` changes
        else do
            writeTVar from (fromBal - amount)
            modifyTVar to (+ amount)

-- Execute atomically:
atomically $ transfer accountA accountB 100
```

**Clojure's STM** uses Multiversion Concurrency Control (MVCC):
- Refs hold a history of values (doubly linked list).
- Transactions see a consistent snapshot.
- Commutative operations (`commute`) can avoid retries.
- Side effects are handled with `agent` (asynchronous) actions that fire after commit.

**Key difference:** Haskell's type system *prevents* side effects inside transactions at compile time. Clojure relies on programmer discipline (though the runtime will throw if you try to modify a Ref outside a `dosync`).

### 4.6 Fearless Concurrency (Rust)

Rust's ownership and type system prevent data races at compile time:
- **Ownership rules** ensure exactly one mutable reference OR multiple immutable references at any time.
- **`Send` trait:** A type is `Send` if it can be safely transferred to another thread.
- **`Sync` trait:** A type is `Sync` if it can be safely shared between threads (via `&T`).
- **`Mutex<T>` and `RwLock<T>`:** The lock guards carry ownership, so you cannot access the data without holding the lock -- enforced at compile time, not by convention.

```rust
use std::thread;
use std::sync::{Arc, Mutex};

let counter = Arc::new(Mutex::new(0));
let mut handles = vec![];

for _ in 0..10 {
    let counter = Arc::clone(&counter);
    let handle = thread::spawn(move || {
        let mut num = counter.lock().unwrap();
        *num += 1;
        // `num` (the MutexGuard) is dropped here, releasing the lock
    });
    handles.push(handle);
}
```

Many concurrency errors that are runtime bugs in other languages become compile-time errors in Rust.

### Concurrency Models Comparison

| Model | Languages | Shared State | Data Race Prevention | Cancellation | Best For |
|-------|-----------|-------------|---------------------|-------------|----------|
| CSP (channels) | Go, Crystal, Clojure core.async | Possible (discouraged) | Race detector (runtime) | Context-based | Network services, pipelines |
| Actor model | Erlang, Elixir, Swift, Akka | None (process isolation) | By design | Supervisor trees | Fault-tolerant distributed systems |
| Async/await | JS, C#, Rust, Python, Kotlin | Language-dependent | Language-dependent | Varies | I/O-bound workloads |
| Structured concurrency | Kotlin, Swift, Java 21+ | Language-dependent | Language-dependent | Automatic (scoped) | Complex async workflows |
| STM | Haskell, Clojure | Controlled (transactional) | By design | retry/orElse | Shared-state concurrency |
| Ownership-based | Rust | Compile-time controlled | Compile-time proof | Drop-based | Systems programming |

---

## 5. Memory Management Approaches

### 5.1 Manual Memory Management (C)

The programmer explicitly calls `malloc()` and `free()`. This provides maximum control but is the source of the majority of security vulnerabilities in systems software:
- Use-after-free
- Double free
- Buffer overflows
- Memory leaks
- Dangling pointers

Microsoft and Google have independently reported that approximately **70% of all security vulnerabilities** in their C/C++ codebases are memory safety issues.

### 5.2 RAII and Ownership (Rust, C++)

**RAII (Resource Acquisition Is Initialization)** ties resource lifetimes to object lifetimes. When an object goes out of scope, its destructor releases resources deterministically.

**C++ RAII:**
- `std::unique_ptr<T>` -- exclusive ownership, non-copyable
- `std::shared_ptr<T>` -- reference-counted shared ownership
- `std::weak_ptr<T>` -- non-owning reference to break cycles
- Destructors guarantee cleanup; no `finally` blocks needed

**Rust's ownership system** enforces RAII principles at the language level with compile-time verification:

| Rule | Description |
|------|-------------|
| Single owner | Each value has exactly one owner at a time |
| Move semantics | Assignment transfers ownership (no implicit copying) |
| Borrowing | References (`&T`, `&mut T`) borrow without taking ownership |
| Lifetimes | The compiler tracks how long references are valid |
| No null | `Option<T>` replaces null with a type-safe alternative |

**Learning curve:** The ownership model requires 3-6 months to internalize. Initial development is dominated by "fighting the borrow checker." However, once learned, it eliminates entire categories of bugs.

**Swift 6's ownership additions (2024):**
- `consume` operator: transfer contents without copying
- `consuming` and `borrowing` parameter modifiers: eliminate unnecessary copies and reference counting
- Noncopyable types (`~Copyable`): types that can only be moved, not copied
- Compile-time data race safety (headline feature)

### 5.3 Garbage Collection Variants

| Variant | Languages | Pause Time | Throughput | Memory Overhead |
|---------|-----------|-----------|-----------|----------------|
| **Tracing (mark-sweep)** | Go | <1ms (concurrent tricolor) | Good | Low |
| **Generational** | Java (G1), .NET, Python (partial) | Variable (ms-range) | High | Moderate |
| **Concurrent low-latency** | Java (ZGC, Shenandoah) | <1ms (sub-millisecond) | Good (2% less than G1) | 25%+ headroom needed |
| **Reference counting** | Swift (ARC), Python, Objective-C | None (deterministic) | Good | Per-object overhead |
| **Incremental** | Lua, Ruby | Bounded | Moderate | Low |

**Go's GC** is a concurrent, tricolor, mark-sweep collector:
1. All objects start as white (unreachable).
2. Root objects (globals, stack) are colored grey.
3. Grey objects are scanned: their references turn white objects grey, then the scanned object turns black.
4. Write barriers ensure the mutator cannot hide white objects from the collector during concurrent execution.
5. Stop-the-world pauses are typically microseconds to low milliseconds.

Go's GC deliberately prioritizes latency over throughput, making it ideal for network services where consistent response times matter more than maximum throughput.

**Java's ZGC (JDK 21+):**
- Generational ZGC divides the heap into young and old generations.
- GC pauses are sub-millisecond, even with multi-terabyte heaps.
- Uses colored pointers (metadata stored in pointer bits) for concurrent relocation.
- 10% throughput improvement over non-generational ZGC.
- Recommendation: provision infrastructure with at least 25% memory headroom.

### 5.4 Reference Counting (Swift ARC, Python)

**Swift's ARC (Automatic Reference Counting):**
- The compiler inserts retain/release calls automatically.
- Deterministic deallocation: objects are freed as soon as their reference count reaches zero.
- **Problem:** Reference cycles cause memory leaks. Developers must use `weak` or `unowned` references to break cycles.
- **Performance cost:** Atomic reference count updates on shared objects are expensive (cache-line contention in multithreaded code).

Swift is evolving toward Rust-like ownership to reduce ARC overhead:
- Performance benchmarks show Rust executing 2-10x faster than Swift with 2-5x lower memory usage, partly due to ARC overhead.
- Swift 6's `consume`, `borrowing`, and noncopyable types reduce unnecessary retain/release traffic.

**Python's approach:**
- Primary: reference counting (immediate cleanup of most objects).
- Secondary: generational garbage collector for cycle detection.
- The GIL (Global Interpreter Lock) simplifies reference counting (no atomic operations needed) but limits true parallelism.

### 5.5 Region-Based Memory Management (Cyclone, MLKit)

Region-based allocation assigns each object to a region. When the region is deallocated, all objects within it are freed simultaneously -- O(1) regardless of the number of objects.

**Cyclone** (safe C dialect):
- Supports explicit regions with region subtyping.
- Integrates stack allocation, region allocation, and a global GC arena.
- The type system ensures references never outlive their region.
- Research showed that tracing GC is more effective for certain data patterns, leading to hybrid region/GC systems.

**MLKit** (ML compiler):
- Region inference: the compiler automatically determines which region each allocation belongs to, based on its lifetime.
- Hybrid approach: combines region inference with tracing GC for the global region.
- Demonstrated that region-based management can replace GC for many programs entirely.

**Arena allocation** (general technique):
- Used extensively in game engines, compilers, and request-handling servers.
- Allocate into a contiguous memory block; free everything at once.
- Extremely fast allocation (bump pointer) and deallocation (reset pointer).
- No individual `free` calls needed.
- Zig and Rust both support arena allocation through standard libraries / allocator interfaces.

### 5.6 Mojo's Ownership+GC Hybrid

Mojo combines multiple memory management strategies:
- **Ownership and borrow checking** (influenced by Rust) for performance-critical code.
- **Automatic memory management** with ASAP (As Soon As Possible) destruction -- the compiler tracks lifetimes and destroys values as soon as they are no longer referenced.
- **No traditional garbage collector** -- instead, the compiler's static analysis determines deallocation points.
- Built on **MLIR** (Multi-Level Intermediate Representation), which tracks types, shapes, sizes, lifetimes, and ownership at the IR level.
- Supports both Python-like ergonomics (dynamic types, reference semantics) and Rust-like performance (value types, ownership, zero-cost abstractions).

### Memory Management Comparison

| Approach | Speed | Safety | Deterministic | Effort | Languages |
|----------|-------|--------|--------------|--------|-----------|
| Manual | Fastest (no overhead) | Unsafe | Yes | High | C |
| RAII/Ownership | Very fast | Safe (compile-time) | Yes | Medium-High (learning curve) | Rust, C++ |
| ARC | Fast | Mostly safe (cycles possible) | Yes | Low | Swift, ObjC |
| Tracing GC | Good | Safe | No (pauses) | Very low | Go, Java, .NET, JS |
| Region-based | Very fast | Safe (with type system) | Yes (per-region) | Medium | Cyclone, MLKit |
| Arena | Very fast | Manual within arena | Yes (per-arena) | Low-Medium | Zig, Rust, C |
| Hybrid (Mojo) | Very fast | Safe | Yes (ASAP) | Low | Mojo |

---

## 6. Cross-Platform Compilation

### 6.1 LLVM Ecosystem

LLVM is the dominant compiler infrastructure, used as a backend by Rust, Swift, Clang (C/C++/ObjC), Julia, Zig, Mojo, Kotlin/Native, and many others.

**Architecture:**
```
Source Code -> Frontend (language-specific) -> LLVM IR -> Optimization Passes -> Backend (target-specific) -> Machine Code
```

**Key advantages:**
- **Shared optimization passes:** All LLVM-based languages benefit from the same optimization infrastructure.
- **Target coverage:** x86, ARM, RISC-V, WebAssembly, AMDGPU, NVPTX, and many more.
- **Modular design:** Languages can use LLVM as a library, choosing which passes to run.
- **Mature ecosystem:** Decades of optimization work, robust debugging support (DWARF, CodeView).

**Languages using LLVM for WebAssembly:** C, C++, Rust, Zig, Swift, Haskell, and others can all compile to WebAssembly through LLVM's `wasm32` target.

### 6.2 GraalVM

GraalVM is a polyglot virtual machine from Oracle that supports multiple languages through the Truffle framework.

**Key capabilities:**
- **Native Image:** Ahead-of-time (AOT) compilation of JVM applications to standalone executables. Dramatically reduces startup time and memory footprint.
- **Truffle Framework:** A Java-based framework for implementing language interpreters that automatically get JIT compilation. Languages implemented on Truffle include JavaScript, Ruby, Python, R, and LLVM-based languages.
- **Polyglot interop:** Programs can seamlessly mix code from multiple Truffle languages with zero-overhead interop.
- **Context preinitialization:** Language contexts can be initialized at build time, significantly improving first-context startup.

**Most recent release:** GraalVM for JDK 24 (July 2025).

| Feature | JVM Mode | Native Image |
|---------|----------|-------------|
| Startup time | Slow (JVM warmup) | Fast (milliseconds) |
| Peak performance | High (JIT optimized) | Good (AOT limited) |
| Memory footprint | High | Low |
| Reflection support | Full | Requires configuration |
| Dynamic class loading | Yes | No |

### 6.3 WebAssembly (WASM) + WASI

**WebAssembly** is a portable binary instruction format designed as a compilation target for high-level languages.

**WebAssembly 3.0 (September 2025)** introduced:
- 64-bit address space (Memory64)
- Multiple address spaces
- Exception handling
- Garbage-collected struct and array types
- Tail calls

**WASI (WebAssembly System Interface):** A standardized system interface for running WebAssembly outside the browser:
- File system access
- Network sockets
- Clocks and random number generation
- Environment variables

**Languages compiling to WASM:**

| Language | WASM Support | WASI Support | Use Case |
|----------|-------------|-------------|----------|
| Rust     | Excellent (wasm-pack, wasm-bindgen) | Full | Web apps, serverless, plugins |
| C/C++    | Good (Emscripten) | Full | Porting existing code |
| Go       | Good (TinyGo preferred for size) | Partial | Serverless functions |
| Zig      | Excellent (via LLVM wasm32-wasi) | Full | Systems/embedded |
| AssemblyScript | Native (TypeScript-like syntax) | Full | Web-first applications |
| Kotlin   | Kotlin/Wasm (alpha) | Experimental | Kotlin Multiplatform |

### 6.4 Zig's Cross-Compilation

Zig provides the most seamless cross-compilation experience of any systems language:

- **No external toolchains required:** `zig build -Dtarget=aarch64-linux-gnu` just works. No need to install a separate cross-compiler, sysroot, or SDK.
- **Ships with libc headers:** Zig bundles glibc, musl, and other libc headers for all supported targets.
- **Drop-in C/C++ cross-compiler:** `zig cc` can replace GCC/Clang as a cross-compiler for C/C++ projects (used by some Rust projects for cross-compilation).
- **Supported targets:** All LLVM-supported targets, including 64-bit ARM Linux, Windows, macOS, WebAssembly, and many embedded platforms.
- **Consistent behavior:** The same Zig toolchain produces identical results on every host platform.

### 6.5 Go's Static Linking

Go compiles to statically linked binaries by default:
- **Single binary deployment:** No shared library dependencies (unless using cgo).
- **Cross-compilation:** `GOOS=linux GOARCH=arm64 go build` produces a Linux ARM64 binary from any host.
- **Minimal runtime requirements:** Go binaries include the runtime and garbage collector. No JVM, interpreter, or framework required.
- **Container-friendly:** Go binaries can run in `FROM scratch` Docker containers (no OS needed).
- **Trade-off:** Binary sizes are larger (typically 5-15 MB for simple programs) because the runtime is included.

### Cross-Platform Compilation Comparison

| Tool/Language | AOT | JIT | WASM | Cross-compile ease | Binary size |
|--------------|-----|-----|------|-------------------|-------------|
| LLVM (Rust, C, Zig) | Yes | No | Yes | Medium (Rust), Excellent (Zig) | Small-Medium |
| GraalVM | Yes (Native Image) | Yes | Limited | Medium | Medium |
| Go | Yes | No | Yes (TinyGo) | Excellent | Medium-Large |
| .NET (NativeAOT) | Yes | Yes | Yes (Blazor) | Good | Medium |
| JVM (Java, Kotlin) | GraalVM | Yes | Experimental | Medium | Large |

---

## 7. New Languages and Why They Emerged

### 7.1 Rust -- Memory Safety Without Garbage Collection

| | |
|---|---|
| **Created** | 2006 (Graydon Hoare), Mozilla sponsorship 2009, stable 1.0 in May 2015 |
| **Creator** | Graydon Hoare at Mozilla |
| **Core problem** | ~70% of security vulnerabilities in C/C++ codebases are memory safety issues |
| **Key insight** | Ownership and borrowing can enforce memory safety at compile time without runtime cost |

**Origin story:** In 2006, Graydon Hoare returned to his Vancouver apartment to find the elevator broken -- its software had crashed. He reflected that memory bugs in C/C++ caused most software crashes and began designing a language to eliminate them.

**Mozilla's motivation:** Build a browser engine (Servo) that could safely exploit parallelism. Browsers are notoriously complex, with massive C++ codebases full of memory-safety bugs.

**Why not just use GC?** Systems programming requires deterministic resource management, predictable latency, and minimal runtime overhead -- all incompatible with garbage collection.

**Impact:** Rust has been adopted by Microsoft (Windows kernel), Google (Android, Chrome), Amazon (Firecracker, S3), Meta (Buck2), the Linux kernel, and many others. The U.S. White House and NSA have recommended memory-safe languages, naming Rust specifically.

### 7.2 Go -- Simplicity + Concurrency at Scale

| | |
|---|---|
| **Created** | 2007 (design), public 2009, 1.0 in March 2012 |
| **Creators** | Robert Griesemer, Rob Pike, Ken Thompson at Google |
| **Core problem** | C++ builds were too slow, too complex, and poorly suited to concurrent network services |
| **Key insight** | A simple language with fast compilation, built-in concurrency, and a comprehensive standard library can be more productive than a complex one |

**Origin story:** In 2007, Google engineers were waiting 45 minutes for C++ builds. Rob Pike, Ken Thompson, and Robert Griesemer -- all with decades of systems programming experience -- decided to create a language that could handle Google-scale software engineering.

**Design philosophy:**
- Intentionally leave out features (no generics until 1.18, no exceptions, no inheritance) to maintain simplicity.
- One way to do things: `gofmt` enforces a single formatting style.
- Fast compilation: Go compiles most projects in under a second.
- Goroutines + channels as first-class concurrency primitives.

**Sweet spot:** Cloud infrastructure, network services, CLI tools, DevOps tooling. Docker, Kubernetes, Terraform, Hugo, and the majority of the CNCF ecosystem are written in Go.

### 7.3 Zig -- A Better C

| | |
|---|---|
| **Created** | 2015 (Andrew Kelley) |
| **Creator** | Andrew Kelley |
| **Core problem** | C is the lingua franca of systems programming but has too many footguns, undefined behavior, and lacks modern tooling |
| **Key insight** | You can have C's simplicity and performance model with better safety, a unified compile-time mechanism, and excellent cross-compilation |

**Key design decisions:**
- **No hidden control flow:** No operator overloading, no exceptions, no hidden allocations. What you see is what the machine does.
- **`comptime` replaces macros, generics, and conditional compilation** with a single mechanism.
- **Optional safety:** Bounds checking, integer overflow detection, and null checking are on by default but can be disabled for performance-critical sections.
- **Explicit allocators:** Every allocation site takes an allocator parameter, making memory management visible and controllable.
- **Interop:** Zig can directly import C headers and call C functions with no FFI bindings. Zig can also be used as a C/C++ cross-compiler.

**Use cases:** Zig is used in production at Uber (bazel-zig-cc), and the Bun JavaScript runtime is written in Zig.

### 7.4 Carbon -- A Successor to C++

| | |
|---|---|
| **Announced** | July 2022 (CppNorth keynote) |
| **Creator** | Chandler Carruth at Google |
| **Core problem** | C++ has accumulated decades of complexity and cannot evolve fast enough to address modern needs (memory safety, simpler generics, better tooling) |
| **Key insight** | A successor language with full C++ interop can enable incremental migration of billion-line codebases |
| **Status** | Experimental (as of 2025) |

**Why not just use Rust?** Carbon's FAQ addresses this directly: Rust is excellent for new projects, but migrating a large C++ codebase (like Chromium) to Rust requires rewriting. Carbon aims to be what Kotlin is to Java or Swift is to Objective-C -- a successor language that interoperates seamlessly with the predecessor.

**Design goals:**
- Bidirectional C++ interop (call C++ from Carbon and vice versa, without wrappers).
- Modern generics (checked generics, not templates).
- Memory safety (planned, not yet implemented).
- Simpler syntax and semantics.
- Fast compilation.

### 7.5 Mojo -- Python Performance for AI

| | |
|---|---|
| **Announced** | May 2023 |
| **Creator** | Chris Lattner (creator of LLVM, Swift, and MLIR) at Modular |
| **Core problem** | Python dominates AI/ML but is ~35,000x slower than optimized code for compute-intensive workloads |
| **Key insight** | Build on MLIR to get Python's usability with systems-language performance, targeting heterogeneous hardware (CPU + GPU + accelerators) |

**Technical approach:**
- Superset of Python: valid Python is valid Mojo (goal, not yet fully achieved).
- `fn` keyword for strict, typed functions (vs. `def` for Python-compatible dynamic functions).
- `struct` for value types with ownership semantics.
- Built on MLIR, not LLVM directly, enabling higher-level optimization passes and multi-target compilation (CPU, GPU, TPU, ASICs).
- ASAP destruction: the compiler determines deallocation points statically.

**Performance claims:**
- Launch benchmarks showed up to 35,000x speedup over CPython for kernel-dense workloads.
- 2025 benchmarks show competitive performance with CUDA/HIP for memory-bound kernels.
- Gaps remain for certain GPU workloads (atomic operations on AMD, fast-math compute-bound kernels).

**Controversy:** The language was initially closed-source, which drew criticism. It has since been open-sourced.

### 7.6 V -- Simplicity Claims

| | |
|---|---|
| **Announced** | 2019 |
| **Creator** | Alexander Medvednikov |
| **Core problem** | Modern languages are too complex; aims to combine Go's simplicity, Rust's safety, and C's performance |
| **Status** | Pre-1.0, controversial |

**Claims vs. reality:** V has been heavily criticized for a gap between marketing claims and actual capabilities:

| Claim | Reality |
|-------|---------|
| Compiles 1M+ lines/second | Independent testing shows ~500K-600K lines/second |
| Translates C/C++ projects (Doom, Doom 3) | Links to demos were dead or incomplete |
| No undefined behavior | Implementation has numerous known issues |
| Zero dependencies | Partially true for the compiler itself |
| Memory safety | Not formally verified; relies on conventions |

The core criticism is not that V is a bad language, but that its marketing has consistently overpromised relative to its implementation state. The 0.x version status is often cited as a defense, but the claims were made as if the features were production-ready.

**Legitimate design goals:** V does aim for genuine simplicity -- a small language that can be learned in an afternoon. Whether it can deliver on its more ambitious claims remains to be seen.

### 7.7 Gleam -- Typed Erlang VM

| | |
|---|---|
| **Created** | 2016, 1.0 released March 2024 |
| **Creator** | Louis Pilfold |
| **Core problem** | Erlang and Elixir are dynamically typed; the BEAM VM's fault-tolerance is excellent but type errors are caught only at runtime |
| **Key insight** | An ML-inspired type system can bring compile-time safety to the BEAM ecosystem while preserving its concurrency and fault-tolerance strengths |

**Design decisions:**
- **Runs on BEAM and JavaScript:** Gleam compiles to both Erlang (for BEAM) and JavaScript, enabling full-stack development.
- **No exceptions:** Gleam uses `Result` types for error handling (like Rust's `Result<T, E>`).
- **No null:** Uses `Option` type instead.
- **No macros:** Simplicity over metaprogramming power.
- **No auto-currying:** Unlike Haskell/ML, prioritizing readability over brevity.
- **Full Erlang/Elixir interop:** Can call any Erlang or Elixir library directly.
- **Friendly error messages:** Inspired by Elm's approach.

**Sweet spot:** Teams that want BEAM's legendary reliability and concurrency model but also want the safety net of a static type system. Gleam aims to be approachable for developers coming from TypeScript, Rust, or Go.

### New Languages Summary

| Language | Year | Predecessor | Key Innovation | Maturity |
|----------|------|-------------|---------------|----------|
| Rust     | 2015 | C/C++ | Ownership-based memory safety | Mature (production) |
| Go       | 2012 | C/C++/Python | Simplicity + goroutines | Mature (production) |
| Zig      | 2015 | C | `comptime` + explicit allocators | Approaching 1.0 |
| Carbon   | 2022 | C++ | C++ interop + modern design | Experimental |
| Mojo     | 2023 | Python | MLIR-based heterogeneous compute | Early production |
| V        | 2019 | Go/Rust/C | Claimed simplicity | Pre-1.0, controversial |
| Gleam    | 2024 | Erlang/Elixir | Static types on BEAM | 1.0 (production) |

---

## Key Takeaways for Language Design

1. **Type systems are the primary innovation frontier.** The spectrum from TypeScript's pragmatic unsoundness to Idris's full dependent types represents a fundamental trade-off between adoption ease and safety guarantees. The sweet spot for a new language likely lies in refinement types or effect systems -- more safety than TypeScript, less complexity than Idris.

2. **Compile-time computation is converging.** Zig's `comptime`, Rust's `const fn`, and C++'s `constexpr` all demonstrate that moving work to compile time is a major performance lever. Zig's unified approach (one mechanism for generics, metaprogramming, and conditional compilation) is the most elegant.

3. **Developer experience is a first-class language feature.** Elm and Rust proved that error messages are a design surface. LSP proved that tooling can be decoupled from editors. Cargo and Go proved that integrated toolchains eliminate friction. Any new language must ship with excellent error messages, an LSP server, and a unified toolchain from day one.

4. **Structured concurrency is the future.** Unstructured goroutines, raw threads, and callback-based async are being superseded by structured concurrency (Kotlin, Swift, Java). The actor model (Erlang) remains the gold standard for fault-tolerant distributed systems.

5. **Memory management is a spectrum, not a binary choice.** The trend is toward hybrid approaches: Mojo combines ownership with automatic management, Swift is adding Rust-like ownership to complement ARC, and MLKit demonstrated region/GC hybrids decades ago. A new language can choose its position on this spectrum based on its target domain.

6. **Cross-compilation and WASM are table stakes.** LLVM provides the backend. WASM+WASI provides the universal target. Zig has shown that cross-compilation can be made trivially easy. A new language should target WASM from the start.

7. **Every successful new language solves a specific, painful problem.** Rust exists because memory bugs kill people. Go exists because C++ builds took 45 minutes at Google. Mojo exists because Python is too slow for AI. A new language needs a clear, compelling answer to "why does this need to exist?"
