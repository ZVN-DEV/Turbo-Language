# Deep-Dive Analysis: Scala, Haskell, Lua, PHP, V

> Research compiled February 2026. Five languages spanning the spectrum from academic purity
> (Haskell) to pragmatic ubiquity (PHP), from JVM powerhouse (Scala) to embeddable minimalism
> (Lua) to controversial newcomer (V).

---

## Table of Contents

1. [Scala](#1-scala)
2. [Haskell](#2-haskell)
3. [Lua](#3-lua)
4. [PHP](#4-php)
5. [V (Vlang)](#5-v-vlang)
6. [Cross-Language Comparison Matrix](#6-cross-language-comparison-matrix)
7. [Steal vs Avoid Summary](#7-steal-vs-avoid-summary)

---

## 1. Scala

### 1.1 Basics

| Property | Value |
|---|---|
| Year Created | 2004 |
| Creator | Martin Odersky (EPFL) |
| Paradigm | Multi-paradigm: functional + object-oriented |
| Typing Discipline | Static, strong, inferred, structural |
| Compilation Model | Compiles to JVM bytecode; also Scala.js (JavaScript) and Scala Native (LLVM) |
| Current Version | Scala 3.8.1 (Jan 2026); LTS 3.3.7 (Oct 2025); Scala 2.13.18 still maintained |
| License | Apache 2.0 |

Scala was designed as a "better Java" that unifies object-oriented and functional programming under
a sound type system. It runs on the JVM, giving it access to the entire Java ecosystem while
offering dramatically more expressive syntax and type-level features.

### 1.2 Best Use Cases

- **Distributed data processing**: Apache Spark, Apache Kafka, Apache Flink all built in Scala
- **Concurrent/reactive systems**: Akka actor systems for high-throughput message processing
- **Financial systems**: Type safety + JVM performance makes it popular in fintech and trading
- **Microservices**: Play Framework, http4s, ZIO HTTP for high-performance APIs
- **Streaming**: Akka Streams, FS2, ZIO Streams for backpressure-aware stream processing
- **Compiler/language tooling**: Scala's type system makes it excellent for building DSLs and compilers

### 1.3 Loved Features

- **Unified OOP + FP**: Case classes, pattern matching, higher-order functions, and traits together
- **Type inference**: Hindley-Milner-style inference eliminates most type annotations
- **Immutability by default**: `val` over `var`, immutable collections in the standard library
- **Pattern matching**: Exhaustive, nested, with extractors and guards
- **For-comprehensions**: Monadic composition that reads like imperative code
- **Implicits/Givens**: Powerful dependency injection and type class derivation at compile time
- **Macros and metaprogramming**: Compile-time code generation (redesigned in Scala 3)
- **Interop with Java**: Seamless bidirectional Java interop means access to millions of libraries
- **Algebraic data types**: Sealed traits + case classes = proper sum types
- **REPL and scripting**: Ammonite REPL, Scala CLI for quick scripting

### 1.4 Hated Features / Pain Points

- **Compile times**: Historically slow. Scala 3 improved this but large projects still suffer.
  IntelliJ users report waiting a minute or more for code completion hints with Scala 3.
- **Complexity ceiling**: The language is enormous -- implicits, macros, higher-kinded types,
  path-dependent types, structural types, etc. Teams can write code that other team members
  cannot read.
- **Scala 2 vs Scala 3 fragmentation**: The community is split. As of 2024, only ~49% of Scala
  developers used Scala 3 at all. Apache Spark still does not support Scala 3.
- **IDE experience**: IntelliJ Scala 3 support has been described as "terrible" by developers.
  Metals (VS Code) has stability issues stemming from its multi-process architecture.
- **Build tool complexity**: sbt is notoriously difficult to learn. Its "four-dimensional task
  matrix" and cryptic operator syntax (`%`, `%%`, `%%%`) confuse newcomers.
- **Implicit resolution**: When implicits go wrong, error messages are incomprehensible. Scala 3's
  `given`/`using` helps, but the underlying complexity remains.
- **Ecosystem churn**: Major library authors sometimes abandon projects or make breaking changes.
  The Typelevel and ZIO ecosystems overlap but are not always compatible.
- **Popularity decline**: Stack Overflow surveys show Scala falling from 4.4% (2018) to 2.6% (2025),
  making hiring increasingly difficult.
- **Binary compatibility**: Every Scala minor version breaks binary compatibility (2.12 vs 2.13),
  forcing library authors to cross-publish. Scala 3 mitigates this with forward compatibility
  from 2.13.

### 1.5 Common Bugs

- **Implicit ambiguity**: Multiple implicits in scope with the same type cause cryptic compile
  errors or, worse, silently select the wrong one.
- **Variance issues**: Covariant/contravariant type parameters interacting with mutable state cause
  unsoundness if not careful.
- **Null from Java interop**: Despite Scala's `Option`, calling Java libraries returns nullable
  values. Forgetting to wrap them is a top source of NPEs.
- **Future composition footguns**: `Future` starts executing eagerly on creation, leading to
  uncontrolled parallelism and race conditions when developers expect lazy semantics.
- **For-comprehension desugaring surprises**: Mixing types (e.g., `Option` and `List`) in a single
  for-comprehension produces confusing errors.
- **Collection performance**: Using the wrong collection (e.g., `List` for random access, `Vector`
  for prepending) causes hidden O(n) operations.
- **Macro divergence**: In Scala 2, implicit macro expansion can loop infinitely. Scala 3's inline
  macros are better but still have edge cases.
- **Serialization of case classes**: Adding or removing fields breaks binary-serialized data without
  explicit schema evolution.

### 1.6 Concurrency Model

Scala offers multiple concurrency paradigms, which is both a strength and a source of fragmentation:

**Akka Actors (Classic and Typed)**
- Message-passing concurrency based on the Actor Model
- Each actor encapsulates state and processes messages sequentially
- Location-transparent: actors can run on different machines in a cluster
- Akka Typed (since Akka 2.6) adds compile-time safety to actor protocols
- Akka 22.10 reverted to Apache 2.0 license after its 3-year BSL term expired
- Supervision hierarchies for fault tolerance ("let it crash")

**ZIO**
- Pure functional effect system with `ZIO[R, E, A]` (environment, error, success)
- Typed errors: the error channel `E` is not fixed to `Throwable`
- Fibers: lightweight green threads with structured concurrency
- Semantic blocking, cooperative yielding, and automatic interruption
- ZIO 2.x has its own runtime with work-stealing thread pool

**Cats Effect**
- Tagless Final approach: polymorphic in the effect type `F[_]`
- `IO` monad similar to ZIO but with `Throwable`-only error channel
- Cats Effect 3.x adopted a runtime similar to ZIO 2 (global queue + local queues + work stealing)
- Integrates with FS2 for streaming, http4s for HTTP, Doobie for database access

**Scala Futures**
- Built into the standard library
- Eager evaluation -- starts running when created
- Requires an implicit `ExecutionContext`
- Fine for simple cases but lacks structured concurrency, cancellation, and resource safety

**Scala Native 0.5**
- Introduced multithreading based on platform threads
- Integrated into Cats Effect 3.7.0 with full multithreading on LLVM

### 1.7 Type System

Scala's type system is among the most powerful in production use:

**DOT Calculus (Scala 3 Foundation)**
- Scala 3 is formally based on the Dependent Object Types (DOT) calculus
- DOT's distinguishing feature is abstract type members: fields in objects that hold types
- Type soundness has been proven for DOT, though only for restricted subsets initially
- Key insight: subtyping transitivity only needs to be invertible in code paths executed at runtime

**Path-Dependent Types**
- Types can depend on object paths: `x.A`, where `x` is a specific object
- Enables the "cake pattern" for dependency injection at the type level
- pDOT extends this to arbitrary-length paths

**Key Type System Features**
- Union types (`A | B`) and intersection types (`A & B`) in Scala 3
- Match types: type-level pattern matching
- Opaque types: zero-cost type aliases with encapsulation
- Higher-kinded types: types parameterized by type constructors (`F[_]`)
- Type classes via `given`/`using` (Scala 3) or implicits (Scala 2)
- GADTs: Generalized algebraic data types with exhaustive pattern matching
- Dependent function types: return type depends on argument value
- Type lambdas: anonymous type-level functions
- Singleton types and literal types

**Null Handling**
- Scala 3 has experimental `-Yexplicit-nulls` flag that makes all reference types non-nullable
- Without it, `null` exists for Java interop, making the type system unsound in practice
- Idiomatic Scala uses `Option[A]` to represent nullable values

**Generics**
- Full generics with variance annotations (`+A` covariant, `-A` contravariant)
- Type bounds (`A <: B`, `A >: B`)
- Context bounds for type class constraints (`[A: Ordering]`)

### 1.8 Memory Management

- **JVM Garbage Collection**: Scala inherits whatever GC the JVM uses (G1, ZGC, Shenandoah, etc.)
- **GraalVM Native Image**: Ahead-of-time compilation eliminates JVM startup; uses SubstrateVM GC
  with much lower memory footprint (55x less than JVM in some benchmarks)
- **Scala Native**: Uses Immix GC by default; memory footprint around 50MB peak, 5x less than
  JVM or Native Image
- **Value classes**: `AnyVal` subclasses compile to primitives, avoiding boxing/allocation
- **Escape analysis**: JVM can stack-allocate short-lived objects
- **Off-heap**: Libraries like Chronicle Map or Unsafe access for latency-critical paths

### 1.9 Performance Characteristics

| Metric | Assessment |
|---|---|
| Raw throughput | Excellent -- within 5-15% of Java on JVM thanks to JIT compilation |
| Startup time | Poor on JVM (1-3 seconds). Excellent with GraalVM Native Image or Scala Native |
| Memory usage | High on JVM (150-500MB baseline). Low with Native Image (~10-50MB) |
| Compilation speed | Slow (Scala 3 faster than 2 but still 3-10x slower than Java/Kotlin) |
| GraalVM optimization | 10-50% speedup over C2 JIT for Scala-specific patterns |
| Scala Native throughput | ~24% more iterations/sec than GraalVM Native Image, ~40% less than JVM |

### 1.10 Tooling & Ecosystem

| Tool | Status |
|---|---|
| **Build: sbt** | De facto standard. Powerful but steep learning curve. Cryptic DSL. |
| **Build: Mill** | Rising alternative. 5-10x faster than sbt. Standard Scala objects/defs. IDE-navigable. |
| **Build: Gradle** | Works but less idiomatic. Better for mixed Java/Scala projects. |
| **IDE: IntelliJ** | Best overall support. Scala 3 support improving but still has pain points. |
| **IDE: Metals (VS Code)** | Growing fast. Gained MCP support for AI agents in 2025. Stability improving. |
| **Package Registry** | Maven Central (shared with Java). Massive ecosystem. |
| **Testing** | ScalaTest, MUnit, ZIO Test, Specs2. Well-covered. |
| **Formatting** | Scalafmt (standard). Scalafix for automated refactoring/linting. |
| **REPL** | Ammonite (feature-rich), Scala CLI (modern, fast). |
| **Documentation** | Scaladoc. Good but not as polished as Javadoc/Rustdoc. |
| **Ecosystem Health** | Stable but niche. Typelevel + ZIO ecosystems are vibrant. Spark keeps Scala 2 alive. |

### 1.11 Agentic AI Usability

- **Strong type system** provides fast feedback for AI code generation -- if it compiles, it likely works
- **Metals MCP support** (2025) allows AI agents to compile, run tests, and inspect symbols directly
- **JVM ecosystem** gives access to all Java ML/AI libraries (DL4J, Tribuo, etc.)
- **Spark integration** makes Scala natural for data pipeline stages in AI systems
- **Effect systems** (ZIO, Cats Effect) enable safe concurrent agent orchestration
- **Drawbacks**: Slow compilation slows the edit-compile-test loop for agents. Complex type errors
  are difficult for current LLMs to diagnose. Relatively small training corpus compared to Python/JS.

### 1.12 Scala 2 vs Scala 3: Key Differences

| Feature | Scala 2 | Scala 3 |
|---|---|---|
| Syntax | Braces required | Optional indentation-based (significant whitespace) |
| Implicits | `implicit val/def/class` | `given`/`using`/`extension` |
| Enums | Sealed trait + case objects (verbose) | `enum` keyword (concise) |
| Type system | No union/intersection types | Union (`A \| B`), intersection (`A & B`), match types |
| Macros | `scala.reflect` (powerful but brittle) | `inline` + quotes API (type-safe, redesigned) |
| Trait params | Not supported | Traits can have parameters |
| Opaque types | Not available | Zero-cost type aliases with encapsulation |
| Exports | Not available | `export` for selective re-exports |
| Metaprogramming | Complex, whitebox/blackbox macros | Simpler: `inline`, `derives`, mirrors |
| Binary compat | Breaks every minor version | Forward-compatible from 2.13 |
| Adoption (2024) | ~51% of Scala developers | ~49% of Scala developers |

### 1.13 Ecosystem Fragmentation: sbt vs Mill

**sbt** (Simple Build Tool -- ironic name):
- Created in 2008, mature and widely adopted
- Plugin ecosystem is massive (sbt-assembly, sbt-native-packager, etc.)
- Configuration uses a custom DSL with operators like `%`, `%%`, `:::`, `:=`, `++=`
- "Jump to definition" in IDE lands on a `taskKey` declaration, not implementation
- Dozens of subprojects cause noticeable slowdown
- Interactive shell with incremental compilation

**Mill**:
- Created by Li Haoyi (author of Ammonite, uPickle, os-lib)
- Build definitions are plain Scala objects and methods
- IDE navigation works -- "jump to definition" goes to actual implementation
- 5-10x faster than sbt for common workflows
- Handles hundreds of modules without degradation
- Used by Coursier, Scala-CLI, and the com-lihaoyi ecosystem
- Growing adoption but smaller plugin ecosystem than sbt

### 1.14 Effect Systems: ZIO vs Cats Effect

| Aspect | ZIO 2.x | Cats Effect 3.x |
|---|---|---|
| Core type | `ZIO[R, E, A]` (environment, typed error, value) | `IO[A]` (Throwable errors only) |
| Error model | Typed errors -- compiler enforces error handling | Untyped -- all errors are `Throwable` |
| Philosophy | "Batteries included" -- framework approach | Minimal core -- compose with Tagless Final |
| Dependency injection | Built-in `ZLayer` for dependency graphs | Manual or via libraries (e.g., Kyo, Weaver) |
| Streaming | ZIO Streams | FS2 |
| HTTP | ZIO HTTP | http4s |
| Testing | ZIO Test (built-in) | MUnit, Weaver |
| Runtime | Custom fiber runtime with work-stealing | Similar runtime (adopted ZIO's approach in CE3) |
| Learning curve | Steeper but more self-contained | Requires understanding Tagless Final + type classes |
| Community size | Large, opinionated | Large, more academic-leaning |

### 1.15 What to STEAL from Scala

- **Union and intersection types**: Simple, powerful, no wrapper types needed. `String | Int` is
  immediately useful and avoids the verbosity of tagged unions in many languages.
- **For-comprehensions / monadic do-notation**: Makes sequential effectful code read naturally.
- **Case classes + pattern matching + sealed hierarchies**: The gold standard for algebraic data
  types in a practical language.
- **Extension methods** (Scala 3): Clean syntax for adding methods to existing types without
  inheritance or wrapper types.
- **Opaque types**: Zero-cost newtype wrappers. No runtime overhead, full type safety.
- **Given/Using for type class derivation**: More explicit than Haskell's orphan instances,
  clearer intent than Scala 2's implicits.
- **Companion objects**: Eliminate the need for static methods while keeping factory patterns clean.
- **`val` by default immutability**: Making immutability the path of least resistance.
- **Effect system patterns**: The idea of `IO[E, A]` with typed errors is worth studying even if
  not adopted wholesale.

### 1.16 What to AVOID from Scala

- **Complexity sprawl**: Scala proves that having every feature is worse than having the right
  features. The language has become so large that no single developer knows all of it.
- **Implicit/given abuse**: Powerful but creates invisible code paths. Debugging "where did this
  value come from?" is a real pain.
- **Slow compilation**: Do not accept multi-second compile times as normal. Design compilation
  model for speed.
- **Binary incompatibility between minor versions**: This is a disaster for ecosystem health.
  Ensure ABI stability.
- **Build tool complexity (sbt)**: Build configuration should be simple and navigable. Never
  create a custom DSL for build files that requires its own learning curve.
- **Community fragmentation**: Having two competing effect ecosystems (ZIO vs Cats Effect) splits
  the community and library ecosystem. Provide one blessed approach.
- **Java null leaking through interop**: If interoperating with a nullable language, enforce null
  checks at the boundary automatically.
- **Eager Futures**: Futures that start executing on creation are a footgun. Effects should be
  lazy/deferred by default.

---

## 2. Haskell

### 2.1 Basics

| Property | Value |
|---|---|
| Year Created | 1990 (Haskell 1.0); 2010 (Haskell 2010 standard) |
| Creator | Haskell Committee (Simon Peyton Jones, Philip Wadler, et al.) |
| Paradigm | Purely functional |
| Typing Discipline | Static, strong, inferred (Hindley-Milner + extensions) |
| Compilation Model | Compiled to native code via GHC; interpreted via GHCi |
| Current Version | GHC 9.12.3 (Dec 2025); GHC 9.14 LTS (Aug 2025) |
| License | BSD-3-Clause |

Haskell is the most influential purely functional programming language. Its ideas -- type classes,
monads, lazy evaluation, STM -- have been adopted (in diluted form) by nearly every modern language.
It remains the gold standard for type system research.

### 2.2 Best Use Cases

- **Compiler construction**: GHC itself, Pandoc, PureScript compiler, Elm compiler
- **Formal verification and theorem proving**: Close to mathematical notation
- **Financial systems**: Standard Chartered, Barclays, Jane Street (OCaml but Haskell-adjacent)
- **Parsing and language tools**: Parsec, Megaparsec, Alex/Happy
- **Concurrent systems**: STM makes concurrent state management composable and correct
- **Blockchain/cryptocurrency**: Cardano (IOHK) is built in Haskell
- **Research prototyping**: Rapid prototyping of novel algorithms with mathematical precision
- **DevOps tooling**: ShellCheck (shell script linter) is written in Haskell

### 2.3 Loved Features

- **Type classes**: Ad-hoc polymorphism that is principled, composable, and extensible.
  `Functor`, `Monad`, `Foldable`, `Traversable` form a coherent hierarchy.
- **Purity**: All effects are tracked in the type system. If a function returns `Int`, it truly
  has no side effects. This makes reasoning about code vastly easier.
- **Laziness (when it works)**: Enables elegant infinite data structures, modular programming,
  and separation of data generation from consumption.
- **Pattern matching**: Exhaustive checking, guards, view patterns, pattern synonyms.
- **Higher-kinded types**: First-class support for `Functor`, `Monad`, etc.
- **GHC extensions**: The language is extensible via pragmas (`{-# LANGUAGE ... #-}`), allowing
  gradual adoption of advanced features.
- **STM**: Composable transactional memory that is arguably the best concurrent programming
  abstraction ever designed.
- **Type inference**: Write functions with no type annotations and GHC infers the most general type.
- **Equational reasoning**: Purity means you can substitute equals for equals, enabling
  algebraic reasoning about program behavior.
- **QuickCheck**: Property-based testing invented here and still best-in-class.

### 2.4 Hated Features / Pain Points

- **Lazy evaluation gotchas**: Laziness is the default, but most code should be strict. This
  mismatch causes space leaks (see below) and confusing performance characteristics.
- **Space leaks**: Unevaluated thunks accumulate in memory. The Writer monad is so prone to
  space leaks that the community advises avoiding it entirely. The State monad has both
  lazy and strict variants, and using the wrong one causes silent memory blowup.
- **Compile times**: GHC is slow. Some libraries cannot be compiled on normal hardware due to
  resource exhaustion. Template Haskell makes it worse.
- **Error messages**: GHC error messages for type errors, especially involving type families
  or GADTs, can be walls of text that are nearly impossible to parse.
- **Records**: Haskell's record system is widely considered broken. Field names are global
  functions, causing namespace pollution. No record update syntax without lens libraries.
  Multiple competing lens libraries exist (lens, optics, generic-lens).
- **String types**: `String` is `[Char]` (a linked list of characters). Real code uses `Text`
  or `ByteString`, but the Prelude still uses `String`. This causes constant conversion noise.
- **Tooling**: No dedicated IDE. HLS (Haskell Language Server) is improving but historically
  unreliable. Setup requires command-line intervention that puts newcomers off.
- **Learning curve**: Monads, functors, applicatives, monad transformers, type families, GADTs,
  kind polymorphism -- the learning curve is a cliff.
- **Orphan instances**: Type class instances defined outside both the class and type modules
  can cause coherence issues and are a source of subtle bugs.
- **Dependency hell**: Cabal and Stack have different dependency resolution strategies. The
  ecosystem has historically suffered from "dependency hell" (ameliorated by Nix-style builds
  in modern Cabal).
- **Small talent pool**: Adoption requires 1-2 months of investment per developer. Companies
  with 50+ Haskell developers need to invest in custom tooling.

### 2.5 Common Bugs

- **Space leaks from lazy accumulators**: Folding over a large list with `(+)` using `foldl`
  instead of `foldl'` builds a massive thunk chain before evaluating. This is Haskell's most
  infamous bug pattern.
- **Lazy I/O**: `readFile` returns a lazy `String`, meaning the file handle stays open until
  the string is fully consumed. Can cause "too many open files" errors.
- **Bottom values**: `undefined`, `error`, and infinite loops inhabit every type. A function
  typed `Int -> Int` can still crash at runtime.
- **Partial functions**: `head`, `tail`, `fromJust`, `read` all crash on empty/invalid input.
  The Prelude is full of these landmines.
- **Monad transformer stack ordering**: `StateT s (ExceptT e IO)` behaves differently from
  `ExceptT e (StateT s IO)` with respect to error recovery and state rollback.
- **Orphan instances causing incoherence**: Two libraries defining the same instance for the
  same type can cause unpredictable behavior depending on import order.
- **Strictness annotations forgotten**: Forgetting `!` (bang patterns) on fields in data types
  that should be strict leads to thunk accumulation.
- **IORef/modifyIORef laziness**: `modifyIORef` is lazy by default; `modifyIORef'` is strict.
  Using the wrong one causes space leaks.
- **Numeric type ambiguity**: `show (read "123")` fails because GHC cannot infer the
  intermediate numeric type.

### 2.6 Concurrency Model

Haskell has arguably the most sophisticated concurrency model of any production language:

**Green Threads (Lightweight Threads)**
- GHC runtime multiplexes thousands of green threads onto OS threads
- `forkIO` creates a green thread with negligible overhead (~1KB stack)
- The runtime handles scheduling, yielding, and preemption automatically
- Threads can be killed (`killThread`) and will clean up resources via exception handlers

**Software Transactional Memory (STM)**
- `atomically` blocks execute as transactions -- all-or-nothing semantics
- If two transactions conflict, one is automatically retried
- `retry` blocks a thread until a watched `TVar` changes
- `orElse` composes transactions: try the first, fall back to the second
- STM is composable -- you can build complex concurrent abstractions from simple ones
  without worrying about deadlocks or lock ordering
- This is widely regarded as the most elegant solution to shared-state concurrency ever designed

**Parallelism (Pure)**
- `par` and `pseq` for pure parallel evaluation
- `parMap`, `parList` strategies for data parallelism
- No race conditions possible because data is immutable

**Async Library**
- Structured concurrency via `async`/`wait`
- `race`, `concurrently`, `mapConcurrently` combinators
- Exception propagation across threads

### 2.7 Type System

Haskell's type system is the richest in any widely-used language:

**Type Classes**
- Invented in Haskell (1989, Philip Wadler and Stephen Blott)
- Ad-hoc polymorphism: define an interface, provide instances per type
- `class Functor f where fmap :: (a -> b) -> f a -> f b`
- Hierarchical: `Functor` -> `Applicative` -> `Monad`
- Coherence: only one instance per type per class (no overlapping instances by default)
- Deriving: `deriving (Show, Eq, Ord)` auto-generates instances
- GHC extensions add: multi-parameter type classes, functional dependencies,
  associated types, default signatures, deriving via

**Monads**
- Monads are not a Haskell concept per se -- they are a category theory concept that Haskell
  adopted to sequence effects in a pure language
- `class Monad m where return :: a -> m a; (>>=) :: m a -> (a -> m b) -> m b`
- Do-notation desugars to `>>=` chains, making effectful code readable
- Common monads: IO, Maybe, Either, State, Reader, Writer, STM, Parser
- Monad transformers (`StateT`, `ReaderT`, `ExceptT`) stack effects but are verbose
  and can cause performance issues
- Modern alternatives: effect systems (polysemy, effectful, cleff) use free/freer monads or
  delimited continuations to avoid transformer stacks

**Lazy Evaluation: Pros and Cons**

*Pros:*
- Enables infinite data structures: `take 10 (repeat 1)` works naturally
- Separation of concerns: generate all possible results, then filter/take what you need
- Can avoid unnecessary computation: `head [expensive1, expensive2]` only evaluates `expensive1`
- Enables certain optimization strategies (deforestation, stream fusion)
- Makes equational reasoning valid in more cases

*Cons:*
- **Space leaks**: The fundamental problem. Unevaluated thunks consume memory proportional to
  the expression size, not the value size. A simple `sum [1..1000000]` using lazy `foldl`
  builds a million-deep thunk chain before evaluating anything.
- **Unpredictable performance**: It is extremely difficult to predict when evaluation will happen,
  how much memory will be used, and what the performance characteristics will be.
- **Debugging difficulty**: Stack traces are less useful because the call stack at the time of
  evaluation differs from the call stack at the time of thunk creation.
- **The "strictness annotation tax"**: To avoid space leaks, developers must sprinkle `!`, `seq`,
  `deepseq`, `BangPatterns`, and `StrictData` throughout their code, defeating the purpose
  of lazy-by-default.
- **Writer monad is broken**: The lazy Writer monad is so prone to space leaks that the community
  says "avoid Writer entirely."

**Space Leaks: The Definitive Problem**
- Space leaks occur when a program uses more memory than necessary due to retained thunks
- Common culprits: lazy accumulators, lazy State/Writer monads, `modifyIORef` (non-strict),
  lazy pattern matching in `StateT`, `concatMap` in the List monad
- Detection: `-hT` heap profiling, `+RTS -s` runtime statistics, `weigh` library for
  measuring allocations
- Prevention: `StrictData` extension, `BangPatterns`, using strict variants of data structures,
  using `foldl'` instead of `foldl`, using `Data.Text` instead of `String`
- The fundamental tension: Haskell chose laziness as the default because it enables elegant
  programming, but >90% of production code needs strictness

**Null Handling**
- No null! `Maybe a` is the only way to represent absence of a value
- `Nothing` is a value, not a null pointer -- it cannot crash
- Pattern matching forces handling of both `Just x` and `Nothing`

**Generics**
- Full parametric polymorphism with type variables
- Higher-kinded types: `f :: (f :: * -> *) -> f a -> f b`
- Type families: type-level functions
- GADTs: constructors can refine the type of the enclosing ADT
- DataKinds: promote data types to the kind level
- TypeApplications: explicitly pass type arguments

### 2.8 Memory Management

- **GHC Runtime System (RTS)**: Custom runtime with a generational, copying garbage collector
- **Generational GC**: Two generations. Most allocation happens in the nursery (gen 0), which
  is collected frequently and cheaply. Surviving objects promoted to gen 1.
- **Parallel GC**: GHC supports parallel garbage collection, reducing pause times by ~40% on
  multi-threaded workloads
- **Memory allocation**: Extremely fast -- GHC allocates from a per-capability (per-OS-thread)
  nursery using pointer bumping. Allocation is essentially free (a pointer increment).
- **Heap sizing**: Default heap grows dynamically. Can be tuned with `+RTS -H` (suggested
  heap size) and `+RTS -M` (maximum heap). Rule of thumb: 2-3x live data.
- **Compacting GC**: Available via `+RTS -c` for reducing fragmentation at the cost of
  longer pause times.
- **Unpacked fields**: `{-# UNPACK #-}` pragma eliminates indirection for strict fields,
  putting values directly in the constructor.
- **No manual memory management**: All memory is GC-managed. FFI bindings to C can use
  `ForeignPtr` with custom finalizers.
- **Problem**: If >40% of runtime is spent in GC, the program likely has memory issues.
  Space leaks are the usual cause.

### 2.9 Performance Characteristics

| Metric | Assessment |
|---|---|
| Raw throughput | Good -- typically 50% to 4x slower than C. Can match C with expert optimization. |
| Startup time | Fast (<100ms for simple programs). GHC compiles to native code. |
| Memory usage | Variable. Lazy evaluation can cause unpredictable memory spikes. |
| Allocation speed | Extremely fast (pointer bumping). GHC allocates ~1GB/s per core. |
| Compilation speed | Slow. Some libraries cannot be compiled on normal hardware. |
| Concurrency | Excellent. Green threads + STM scale to millions of concurrent operations. |
| GC pauses | Manageable. Parallel GC helps. No pause-free GC like ZGC. |
| Optimization flags | `-O2` reduces runtime by 15-30% on compute-intensive code. |

### 2.10 Tooling & Ecosystem

| Tool | Status |
|---|---|
| **Build: Cabal** | Official build tool. Nix-style local builds (modern). Shared global cache. |
| **Build: Stack** | Curated snapshots via Stackage. Reproducible builds. Manages GHC versions. |
| **Package Registry** | Hackage (central repo). Stackage (curated compatible sets). |
| **IDE: HLS** | Haskell Language Server. Improving but historically unreliable. v2.13.0.0 (Jan 2026). |
| **IDE: IntelliJ** | Haskell plugin exists but limited compared to Metals/HLS. |
| **Editor: Neovim** | Strong Haskell support via HLS + Neovim LSP. Popular in the community. |
| **Testing** | Hspec, Tasty, HUnit for unit tests. QuickCheck/Hedgehog for property-based testing. |
| **Formatting** | Fourmolu, Ormolu (opinionated formatters). |
| **Linting** | HLint (widely used, excellent). |
| **Profiling** | Built-in profiling (`-prof`), heap profiling (`-hT`), EventLog. |
| **Documentation** | Haddock (decent but not as nice as Rustdoc). |
| **Ecosystem Health** | Niche but deep. Strong in parsing, concurrency, and PL research. |
| **GHCup** | Official installer for GHC, Cabal, Stack, HLS. Simplified setup enormously. |

### 2.11 Agentic AI Usability

- **Strong type system** means generated code that compiles is very likely correct
- **Purity** makes reasoning about code behavior easier for AI agents
- **Small ecosystem**: Limited training data for LLMs compared to Python/JS/Java
- **Steep learning curve**: AI agents struggle with monadic code, type class instances, and
  GHC extensions
- **Symbolic AI**: Haskell excels at theorem proving, probabilistic programming, and symbolic
  reasoning -- useful for certain AI agent architectures
- **Predictable behavior**: Purity and immutability mean fewer runtime surprises
- **Drawbacks**: Compile errors are hard for LLMs to fix. Lazy evaluation semantics are
  counterintuitive for agents. Small community means fewer examples to learn from.

### 2.12 What to STEAL from Haskell

- **Type classes**: The cleanest mechanism for ad-hoc polymorphism. Coherent, hierarchical,
  derivable. Every modern language should have something like this.
- **`Maybe`/`Option` instead of null**: Haskell proved this works. No null pointer exceptions, ever.
- **Pattern matching with exhaustiveness checking**: If you add a variant to a sum type,
  the compiler tells you every place you need to handle it.
- **STM**: Composable transactions for shared mutable state. Vastly superior to locks/mutexes.
  Consider adopting if designing concurrent primitives.
- **Property-based testing (QuickCheck)**: Generate random inputs and check invariants.
  Revolutionary testing approach.
- **Separation of pure and effectful code**: Even without enforcing purity, encouraging it via
  types is powerful.
- **Do-notation / for-comprehensions**: Syntactic sugar for monadic bind makes effectful code
  readable.
- **HLint-style linting**: A linter that suggests idiomatic rewrites, not just style checks.
- **Higher-kinded types**: Enable abstracting over containers/effects (`Functor`, `Monad`).
  Consider supporting `F[_]` in your type system.

### 2.13 What to AVOID from Haskell

- **Lazy evaluation by default**: This is Haskell's original sin. The theoretical elegance
  is not worth the practical cost of space leaks, unpredictable performance, and the
  "strictness annotation tax." Default to strict evaluation.
- **String as linked list of Char**: Never make the default string type a linked list. Use
  a proper UTF-8 byte buffer.
- **Broken records**: Field names as global functions is a design mistake. Records should have
  proper namespacing, update syntax, and row polymorphism from day one.
- **Prelude full of partial functions**: `head`, `tail`, `fromJust` crash on empty input.
  The standard library should not contain partial functions without clear naming
  (e.g., `unsafeHead`).
- **Monad transformer stacks**: Verbose, slow, and fragile. Effect systems (algebraic effects,
  delimited continuations) are a better approach to composing effects.
- **GHC extension explosion**: Having 100+ language extensions that can be mixed creates a
  combinatorial explosion of "which dialect of Haskell is this?" Design the language to
  include the good features by default.
- **Orphan instances**: Allowing type class instances to be defined anywhere breaks coherence.
  Require instances to be defined with either the class or the type.
- **Compile time complexity**: Do not let the type checker take exponential time on normal code.

---

## 3. Lua

### 3.1 Basics

| Property | Value |
|---|---|
| Year Created | 1993 |
| Creator | Roberto Ierusalimschy, Waldemar Celes, Luiz Henrique de Figueiredo (PUC-Rio, Brazil) |
| Paradigm | Multi-paradigm: procedural, prototype-based OOP, functional |
| Typing Discipline | Dynamic, strong (no implicit coercions except string-to-number) |
| Compilation Model | Interpreted (bytecode VM); LuaJIT provides JIT compilation |
| Current Version | Lua 5.5.0 (Dec 2025); Lua 5.4.8 (Jun 2025) |
| License | MIT |

Lua is the quintessential embeddable scripting language. It was designed to be small, fast, and
portable, with a clean C API that makes it trivial to embed in host applications. Its minimalism
is its defining characteristic.

### 3.2 Best Use Cases

- **Game scripting**: World of Warcraft (addon system), Roblox (Luau), Love2D, Corona SDK,
  Defold, CryEngine, many others use Lua for game logic
- **Embedded systems**: Tiny footprint makes it suitable for IoT devices and microcontrollers
- **Configuration**: Nginx (OpenResty), Redis, HAProxy use Lua for dynamic configuration
- **Network/security tooling**: Wireshark dissectors, Nmap scripting engine, Snort rules
- **Application scripting**: Neovim (fully scriptable in Lua), Adobe Lightroom, VLC media player
- **Rapid prototyping**: Tables-as-everything makes iteration fast
- **Web proxies/APIs**: OpenResty (Nginx + LuaJIT) handles millions of requests/sec at Cloudflare

### 3.3 Loved Features

- **Simplicity**: The entire language fits in a ~30-page reference manual. One data structure
  (table) does everything: arrays, dictionaries, objects, namespaces, modules.
- **Embeddability**: Clean C API with ~100 functions. Embed a complete scripting engine in
  any C/C++ application with minimal effort.
- **Performance**: LuaJIT is arguably the fastest dynamic language implementation ever built.
  Up to 20x faster than standard Lua, approaching C speed for numerical code.
- **Small footprint**: Core interpreter is ~250KB. Entire standard library adds ~100KB more.
- **Portability**: Written in standard ANSI C. Runs on virtually any platform with a C compiler.
- **Coroutines**: First-class coroutines (asymmetric, stackful) for cooperative multitasking.
  Simpler than threads, no race conditions.
- **Metatables**: Prototype-based OOP via metamethods. Incredibly flexible -- implement
  operator overloading, proxy objects, read-only tables, lazy evaluation.
- **First-class functions with closures**: Full lexical scoping, closures capture upvalues.
- **Garbage collector**: Incremental GC since 5.1, generational GC since 5.2. Lua 5.5 makes
  major GC collections incremental.
- **Tables as universal data structure**: Everything is a table. No need to choose between
  HashMap, ArrayList, Object, etc. This radical simplicity reduces cognitive load.

### 3.4 Hated Features / Pain Points

- **1-based indexing**: Arrays start at 1, not 0. This causes endless bugs when interfacing
  with C code, binary protocols, or any 0-based system.
- **No built-in OOP**: Object orientation is possible via metatables but there is no standard
  approach. Every project invents its own class system.
- **Global by default**: Variables are global unless declared `local`. Forgetting `local` creates
  subtle, hard-to-find bugs where functions pollute the global namespace.
  Lua 5.5 addresses this by requiring explicit global declarations.
- **Minimal standard library**: No built-in regex (only patterns), no Unicode support in the
  base string library, no JSON, no HTTP, no filesystem traversal. Everything requires
  external libraries.
- **LuaJIT stuck on 5.1**: LuaJIT is compatible with Lua 5.1 only (with some 5.2 features).
  This splits the ecosystem between "fast but old Lua" and "modern but slower Lua."
- **No static typing**: Dynamic typing with no type annotations means errors are caught only
  at runtime. Luau (Roblox's fork) adds gradual typing.
- **Table length operator `#` is unreliable**: For tables with holes (nil values in the middle),
  `#` can return any value between the first and last index. This is a notorious footgun.
- **No continue statement**: Until Lua 5.5, there was no `continue` in loops. Workarounds
  involved `goto` (added in 5.2) or restructuring code.
- **Nil in tables is deletion**: Setting `t[k] = nil` deletes the key. There is no way to
  store nil as a value in a table, which can cause logical errors.
- **Error handling via pcall/xpcall**: No structured exception handling. Errors are strings
  by convention, making typed error handling impossible without discipline.

### 3.5 Common Bugs

- **Forgetting `local`**: Variable `x = 10` in a function creates a global. This silently
  overwrites other globals or leaks state between calls.
- **Off-by-one from 1-based indexing**: Iterating from 0 instead of 1, or calculating indices
  from 0-based APIs, causes out-of-range or skipped-element bugs.
- **Table length with holes**: `t = {1, nil, 3}; print(#t)` might return 1 or 3. Code that
  relies on `#` for tables with nils is broken.
- **Modifying tables during iteration**: `for k, v in pairs(t) do t[k] = nil end` is
  undefined behavior and can skip entries or loop infinitely.
- **String immutability performance**: Concatenating strings in a loop (`s = s .. "x"`) creates
  a new string each time, causing O(n^2) performance. Use `table.concat()` instead.
- **Dead coroutine resume**: Calling `coroutine.resume()` on a dead coroutine returns an error
  that can be difficult to trace in larger systems.
- **Upvalue capture in loops**: Closures in a loop capture the loop variable by reference,
  not by value. All closures see the final value.
- **Missing metamethod falls through silently**: Calling a missing method on an object returns
  nil, which only errors when you try to call nil as a function.

### 3.6 Concurrency Model

Lua's concurrency model is deliberately minimal:

**Coroutines (Built-in)**
- Asymmetric, stackful coroutines via `coroutine.create/resume/yield`
- Cooperative: only one coroutine runs at a time per Lua state
- Cannot leverage multiple CPU cores
- No race conditions because there is no parallel execution
- Yield points are explicit, making control flow predictable
- Used for: state machines, iterators, cooperative multitasking, async I/O simulation

**Limitations**
- `setjmp`/`longjmp` implementation means you cannot yield across C call boundaries
  (Lua calls C calls Lua -- the inner Lua cannot yield)
- No built-in event loop -- must be provided by the host application or a library
- Busy-waiting is common when no coroutine has work to do, causing up to 30x more
  CPU usage than sequential solutions in naive implementations

**External Solutions**
- **Lua Lanes**: Multithreading via multiple Lua states, with message passing between them
- **OpenResty**: Event-loop-based concurrency on top of Nginx, using coroutines for async I/O
- **Luvit**: Node.js-style event loop for Lua, using libuv
- **Love2D**: Game loop provides the concurrency framework for game scripts

### 3.7 Type System

Lua has one of the simplest type systems of any language:

**Eight Types Total**
- `nil`, `boolean`, `number`, `string`, `table`, `function`, `userdata`, `thread` (coroutine)
- That is the entire type system. No classes, no interfaces, no enums.
- Lua 5.3 added integer/float subtypes of `number`

**No Static Type Checking**
- All type checking happens at runtime via `type()` function
- No type annotations in standard Lua
- Luau (Roblox's fork) adds gradual typing with syntax like `function add(x: number, y: number): number`

**Metatables as Poor Man's Types**
- `__index` metamethod enables prototype-based inheritance
- `__newindex` can enforce property types at runtime
- `__tostring`, `__eq`, `__add` etc. provide operator overloading
- No compile-time guarantees whatsoever

**Null Handling**
- `nil` is both "no value" and "delete from table"
- No distinction between "key not present" and "key present with nil value"
- Functions can return `nil` silently without error
- No `Option`/`Maybe` type -- nil is the only way to represent absence

### 3.8 Memory Management

- **Garbage Collector**: Mark-and-sweep GC, incremental since Lua 5.1, generational since 5.2
- **Lua 5.5 improvement**: Major GC collections are now incremental, reducing pause times
- **Small footprint**: Core VM uses ~250KB RAM. Full environment typically 1-5MB.
- **Weak references**: `__mode` metamethod on tables enables weak keys, weak values, or both.
  Essential for caches that should not prevent GC.
- **Finalizers**: `__gc` metamethod called when userdata is collected (tables get finalizers
  in 5.2+)
- **Manual control**: `collectgarbage()` provides manual GC control (stop, restart, step,
  full collect, set pause/step multiplier)
- **LuaJIT memory**: LuaJIT has a 2GB memory limit for GC-managed data on 64-bit systems
  (due to 32-bit GC pointer optimization). This is a hard constraint for data-heavy apps.
- **Allocation**: All allocation goes through a single allocator function that can be replaced
  by the host application (custom allocators for embedded systems)

### 3.9 Performance Characteristics

| Metric | Assessment |
|---|---|
| Standard Lua throughput | Moderate. 3-10x slower than C for numerical code. |
| LuaJIT throughput | Excellent. Within 2-3x of C, sometimes matching C. Up to 20x faster than standard Lua. |
| Startup time | Near-instant (<10ms). Tiny runtime initialization. |
| Memory usage | Excellent. 250KB core, 1-5MB typical application. |
| String operations | Good for single operations. O(n^2) for concatenation in loops. |
| Table operations | Fast. Hash tables with array optimization for integer keys. |
| Floating-point math | LuaJIT excels here -- approaches C-level performance. |
| Function calls | LuaJIT: very fast. Standard Lua: moderate overhead. |
| Coroutine switching | Fast. Much cheaper than thread context switches. |
| GC pauses | Incremental GC keeps pauses small. Generational GC in 5.2+ helps further. |

### 3.10 Tooling & Ecosystem

| Tool | Status |
|---|---|
| **Package Manager: LuaRocks** | De facto standard. Central repository. Works but not as polished as npm/cargo. |
| **Package Manager: Lux** | Modern alternative (2025). Neovim/Nix first-class support. Compatible with LuaRocks. |
| **IDE: ZeroBrane Studio** | Dedicated Lua IDE. Lightweight, good debugger. 70% faster debugging than generic editors. |
| **IDE: VS Code** | Lua extension available. Over 50% of Lua developers use VS Code. |
| **IDE: IntelliJ** | Lua plugin with LuaRocks integration. |
| **IDE: Neovim** | Lua is Neovim's first-class scripting language. Extensive plugin ecosystem. |
| **Testing** | Busted (BDD), LuaUnit (xUnit). Adequate but not as rich as larger languages. |
| **Linting** | Luacheck (widely used, good). Selene (newer, stricter). |
| **Documentation** | LuaDoc, LDoc. Basic but functional. |
| **Typing** | Luau (Roblox) adds gradual types. lua-language-server provides type annotations. |
| **Ecosystem Health** | Niche but stable. Deeply embedded in gaming, networking, and Neovim. |
| **Fragmentation** | Lua 5.1 (LuaJIT) vs 5.4/5.5 (standard) splits the ecosystem. |

### 3.11 Agentic AI Usability

- **Embedding strength**: Lua can be embedded as a scripting layer in AI agent runtimes
- **Fast iteration**: Dynamic typing + fast startup = rapid prototyping of agent behaviors
- **Coroutines**: Natural fit for agent state machines and cooperative task execution
- **Small footprint**: Can run on edge devices / IoT for local AI agents
- **Drawbacks**: No static types means more runtime errors. Minimal standard library means
  no HTTP/JSON/ML libraries without external dependencies. Small training corpus for LLMs.
  Not suitable as the primary language for complex AI systems.
- **Best role**: Scripting/configuration layer within a larger AI system, not the core language

### 3.12 What to STEAL from Lua

- **Embeddability**: A clean C API with ~100 functions that makes embedding trivial. If designing
  a language that should be embeddable, study Lua's API design.
- **Tables as universal data structure**: The radical simplicity of one data structure for
  everything reduces cognitive load. Consider whether your language needs 10 collection types
  or could get away with fewer.
- **Coroutines**: Lua's stackful, asymmetric coroutines are simple, powerful, and easy to
  understand. They are the foundation of cooperative concurrency.
- **Small footprint**: Proving a language can be useful in 250KB is remarkable. Minimalism
  in the core runtime is a virtue.
- **Metatables / metamethods**: A flexible mechanism for operator overloading and prototype-based
  extension without committing to a class hierarchy.
- **Incremental GC**: Lua 5.5's fully incremental major GC is worth studying for any language
  targeting interactive/real-time use cases.
- **Clean, small specification**: The entire language reference is ~30 pages. Simplicity of
  specification is a feature.
- **Replaceable allocator**: Letting the host application provide a custom allocator is essential
  for embedded use cases.

### 3.13 What to AVOID from Lua

- **1-based indexing**: This is universally considered a mistake in a world where every other
  language, every C API, every binary protocol, and every bit-manipulation operation is 0-based.
- **Globals by default**: Variables should be local by default. Requiring explicit `local` is
  backwards and causes countless bugs.
- **`#` operator on holey tables**: The length operator must be well-defined for all inputs.
  Undefined behavior for tables with nil gaps is unacceptable.
- **Nil as deletion**: Conflating "no value" with "delete key" creates logical errors. These
  should be separate operations.
- **No standard OOP**: If your language supports OOP, provide one standard way to do it. Don't
  make every project reinvent class systems from metatables.
- **LuaJIT version split**: Allowing the fastest implementation to be stuck on an old language
  version for a decade splits your ecosystem. Ensure your JIT/compiler tracks the language.
- **Minimal standard library**: A language in 2026 needs built-in JSON, HTTP, Unicode, file system
  operations, and proper regex. Don't outsource basics to third-party packages.
- **No structured error handling**: String-based errors with pcall/xpcall are primitive. Provide
  proper error types and try/catch or Result types.

---

## 4. PHP

### 4.1 Basics

| Property | Value |
|---|---|
| Year Created | 1995 |
| Creator | Rasmus Lerdorf |
| Paradigm | Multi-paradigm: procedural, object-oriented, functional |
| Typing Discipline | Dynamic with optional type hints (gradually typed since 7.0) |
| Compilation Model | Interpreted (Zend Engine bytecode VM); OPcache for bytecode caching |
| Current Version | PHP 8.5.2 (Jan 2026); PHP 8.6 in development |
| License | PHP License |

PHP powers ~72.2% of all websites as of February 2026. It is the most deployed server-side
language in history, driven by WordPress, Laravel, and the LAMP stack. Modern PHP (8.x) is
a dramatically better language than its reputation suggests.

### 4.2 Best Use Cases

- **Web applications**: WordPress, Drupal, Joomla -- the CMS ecosystem is almost entirely PHP
- **API development**: Laravel, Symfony for REST/GraphQL APIs
- **E-commerce**: Magento, WooCommerce, Shopify (internal tools)
- **Content management**: The dominant language for CMS platforms
- **SaaS applications**: Many successful SaaS products (Slack's early backend, Mailchimp,
  Etsy, Facebook's origins) were/are PHP
- **Rapid prototyping**: Fast development cycle with minimal tooling requirements
- **Shared hosting**: PHP runs everywhere -- even the cheapest hosting supports it

### 4.3 Loved Features (Modern PHP 8.x)

- **Laravel ecosystem**: Arguably the best web framework DX in any language. Eloquent ORM,
  Blade templating, queues, scheduling, testing -- batteries included.
- **Composer**: Clean dependency management that solved the problem early and well. PSR
  autoloading standards mean packages just work.
- **Named arguments**: `htmlspecialchars(string: $s, double_encode: false)` -- skip optional params
- **Match expressions**: `match($x) { 1 => 'one', 2 => 'two', default => 'other' }` --
  strict comparison, returns a value
- **Enums** (8.1): Proper algebraic enums with backed values and methods
- **Fibers** (8.1): Cooperative concurrency primitives in the language core
- **Union and intersection types**: `int|string`, `Countable&Iterator`
- **Readonly properties** (8.1) and readonly classes (8.2): Immutability support
- **Attributes** (8.0): Native annotations replacing docblock hacks
- **Pipe operator** (8.5): `$result = $data |> transform(...) |> validate(...)`
- **JIT compilation** (8.0): OPcache JIT for CPU-bound workloads
- **`#[\NoDiscard]`** (8.5): Warn when important return values are ignored
- **Deployment simplicity**: Upload files, they work. No compilation step, no build process.

### 4.4 Hated Features / Pain Points

- **Inconsistent standard library**: `strpos($haystack, $needle)` but `array_search($needle, $haystack)`.
  Parameter ordering is random. Naming uses both `snake_case` and `no_separation`. `str_` vs
  `str` prefixes are inconsistent.
- **No generics**: The single most requested feature. PHPStan/Psalm provide static analysis
  generics via docblocks, but runtime generics remain absent. The PHP Foundation is exploring
  compile-time generics (experimental as of mid-2025).
- **Legacy baggage**: `mysql_*` functions (removed), `register_globals`, `magic_quotes` --
  decades of deprecated features that tutorials still reference.
- **Shared-nothing per-request model**: Every request bootstraps the entire application from
  scratch. Fast enough with OPcache but wasteful compared to persistent runtimes.
- **Type coercion footguns**: `"0" == false` is true, `"0" == null` is false, `"" == null`
  is true. The `==` operator does bizarre type juggling. Always use `===`.
- **Array is everything**: PHP arrays are ordered hash maps that serve as arrays, lists, stacks,
  queues, sets, and dictionaries. This conflation causes confusion about performance
  characteristics and API design.
- **Error handling legacy**: Mix of exceptions, trigger_error(), and return-false conventions.
  Many built-in functions return `false` on error instead of throwing exceptions.
- **No multithreading in core**: PHP-FPM processes are independent. True parallelism requires
  external tools (pcntl_fork, parallel extension, Swoole, ReactPHP).
- **Variable variables**: `$$varName` is a feature that should never have existed.
- **Security footguns**: SQL injection, XSS, and CSRF are easy to introduce without frameworks.
  `eval()`, `include()` with user input, and `unserialize()` are common attack vectors.

### 4.5 Common Bugs

- **Foreach reference leak**: `foreach ($arr as &$val) { ... }` leaves `$val` as a reference
  to the last element. A subsequent `foreach ($arr as $val)` overwrites the last element on
  each iteration. This is PHP's most infamous gotcha.
- **Type coercion with ==**: `0 == "foo"` was true before PHP 8.0 (fixed). But `"0" == false`
  is still true. Always use `===`.
- **Silent null propagation**: Accessing a property on null returns a warning, not an error
  (in many contexts). Code continues executing with garbage values.
- **Array index confusion**: Arrays with string keys `"0"`, `"1"`, `"2"` behave differently
  from arrays with integer keys `0`, `1`, `2` in some operations.
- **Timezone issues**: Date functions behave differently depending on `date.timezone` in
  php.ini. Forgetting to set it causes incorrect timestamps.
- **Session handling**: Sessions use files by default, causing locking issues under concurrent
  requests to the same session.
- **Memory leaks in long-running processes**: PHP was designed for short-lived requests.
  Long-running workers (daemons, queue consumers) can leak memory over time.
- **Empty() footgun**: `empty("0")` returns true. `empty(0)` returns true. This function
  considers too many values as "empty" and should be used cautiously.
- **Uninitialized variables**: Accessing an undefined variable is a notice, not an error.
  Code continues with `null`, leading to downstream bugs.

### 4.6 Concurrency Model

PHP's concurrency model has evolved significantly:

**Traditional Shared-Nothing (PHP-FPM)**
- Each HTTP request gets its own isolated PHP process
- No shared state between requests (shared-nothing architecture)
- Process dies after the request completes, preventing memory leaks
- Simple and reliable but wasteful: full application bootstrap on every request
- Horizontal scaling is trivial: just add more FPM workers

**Fibers (PHP 8.1+)**
- Cooperative concurrency primitive in the language core
- A fiber can suspend execution and yield control back to the caller
- Enables async I/O without callback hell
- Fibers don't run in parallel -- they are cooperative within a single process
- Foundation for libraries like Revolt (event loop) and Amp v3

**Swoole / OpenSwoole**
- PHP extension that provides a persistent, async, coroutine-based server
- Keeps the application in memory (stateful), eliminating per-request bootstrap
- A single worker can handle thousands of concurrent connections
- Provides coroutines, channels, connection pooling, and timers
- Unlike PHP-FPM, Swoole reuses context between requests
- Swoole 6.2 (2025) is considered the best option for async PHP

**ReactPHP**
- Event-loop-based async programming (similar to Node.js)
- Non-blocking I/O for HTTP, DNS, filesystem, database
- Runs as a long-lived process
- Uses promises and streams

**Parallel Extension**
- True multithreading for PHP using pthreads successor
- Channel-based communication between threads
- Not widely adopted due to complexity and PHP's shared-nothing tradition

### 4.7 Type System

PHP's type system has undergone a remarkable transformation:

**PHP 5.x and Earlier**
- Essentially untyped. Type hints only for classes and arrays.

**PHP 7.x**
- Scalar type hints: `int`, `string`, `float`, `bool`
- Return type declarations
- Nullable types: `?int`
- `void` return type
- `strict_types` declaration for strict mode

**PHP 8.x**
- Union types: `int|string`
- Intersection types: `Countable&Iterator`
- `mixed` type (explicit "anything")
- `never` return type (function never returns normally)
- Enums with backed types
- Readonly properties (immutability)
- `null`, `true`, `false` as standalone types
- DNF types (Disjunctive Normal Form): `(Countable&Iterator)|null`

**What is Still Missing**
- **Generics**: The biggest gap. PHPStan and Psalm provide static analysis generics via
  `@template T` docblocks, but no runtime support. The PHP Foundation's compile-time
  generics proposal (2025) would type-check at compile time and erase at runtime.
- **Type aliases**: No way to name complex types
- **Tuple types**: Arrays serve as tuples but without typed positions
- **Literal types**: Beyond `true`/`false`, no string or int literal types at runtime

**Null Handling**
- Null is a first-class type: `?int` means `int|null`
- Nullsafe operator: `$user?->getAddress()?->getCity()`
- `??` null coalescing: `$x = $y ?? 'default'`
- `??=` null coalescing assignment
- No `Option`/`Maybe` type -- null is used directly

### 4.8 Memory Management

- **Reference Counting**: PHP primarily uses reference counting for memory management.
  When a variable's reference count drops to zero, it is immediately freed.
- **Cycle Collector**: A separate cycle collector handles circular references. Runs periodically
  to detect and free reference cycles that reference counting alone cannot handle.
- **Copy-on-Write (COW)**: Arrays and strings use COW -- copies are shared until modified.
  This makes passing large arrays to functions cheap.
- **Per-Request Memory**: In PHP-FPM, all memory is freed when the request ends. This is the
  ultimate memory safety net -- leaks cannot persist across requests.
- **Memory Limit**: `memory_limit` in php.ini caps per-script memory (default 128MB).
  Exceeding it causes a fatal error.
- **Interned Strings**: Common strings are interned in shared memory by OPcache.
- **Swoole Caveat**: Long-running Swoole processes do not benefit from per-request cleanup.
  Memory management discipline becomes important.
- **Generators**: `yield` enables lazy iteration without loading entire datasets into memory.

### 4.9 Performance Characteristics

| Metric | Assessment |
|---|---|
| Raw throughput | Moderate. PHP 8.x is 2-3x faster than PHP 5.6. JIT helps for CPU-bound work. |
| Web request handling | Good. Laravel handles 425-445 req/s on standard hardware. |
| With Octane/Swoole | Very good. 2-3x faster than standard PHP-FPM due to persistent runtime. |
| Startup time | Fast per-request (~5-20ms with OPcache). Slow cold start without OPcache. |
| Memory usage | Moderate. 10-50MB per PHP-FPM worker. Can add up with many workers. |
| OPcache impact | Dramatic. 2-5x improvement by caching compiled bytecode. |
| JIT impact | Modest for web (5-15%). More significant for CPU-bound tasks (20-50%). |
| PHP 8.2-8.5 comparison | Minimal differences: 730 req/s (8.2), 713 req/s (8.3), ~710 req/s (8.4-8.5) |
| Database-heavy workloads | Bottleneck is usually I/O, not PHP. Proper indexing matters more. |

### 4.10 Tooling & Ecosystem

| Tool | Status |
|---|---|
| **Package Manager: Composer** | Excellent. PSR autoloading. Packagist registry. Solved dependency management well. |
| **Framework: Laravel** | Dominant. Best DX of any web framework. Massive ecosystem. |
| **Framework: Symfony** | Enterprise-grade. Components used by Laravel and many others. |
| **IDE: PhpStorm** | Gold standard PHP IDE. Excellent completion, refactoring, debugging. |
| **IDE: VS Code** | Good with Intelephense extension. Free alternative to PhpStorm. |
| **Static Analysis** | PHPStan, Psalm -- excellent. Provide generics, type inference, and bug detection. |
| **Testing** | PHPUnit (standard), Pest (modern, expressive). Good testing culture. |
| **Formatting** | PHP-CS-Fixer, Laravel Pint. |
| **Debugging** | Xdebug (standard, powerful), Blackfire (profiling). |
| **Deployment** | Laravel Forge, Envoyer, Vapor (serverless). Extremely easy to deploy. |
| **Ecosystem Health** | Massive. WordPress alone ensures PHP's relevance for decades. |
| **CMS** | WordPress, Drupal, Joomla, Craft CMS -- the CMS market is PHP-dominated. |

### 4.11 Agentic AI Usability

- **Web integration**: PHP's strength is web applications. AI agents that need to interact with
  web services, process HTTP requests, or manage content can leverage PHP's ecosystem.
- **Laravel ecosystem**: Queue workers, scheduled tasks, and event-driven architecture provide
  infrastructure for background AI processing.
- **Poor for core AI**: PHP lacks ML/AI libraries. No equivalent of Python's scikit-learn,
  PyTorch, or TensorFlow.
- **API consumption**: PHP excels at calling external AI APIs (OpenAI, Claude, etc.) and
  integrating responses into web applications.
- **LLM training data**: Massive PHP codebase on the internet means LLMs have extensive
  training data for PHP code generation.
- **Type safety gap**: Lack of generics and full static typing makes AI-generated code less
  reliable compared to statically-typed alternatives.
- **Best role**: Web layer / API gateway for AI systems, not the AI computation itself.

### 4.12 What to STEAL from PHP

- **Composer / PSR autoloading**: A package manager that just works, with standardized
  autoloading. Packages install and are immediately usable.
- **Deployment simplicity**: Upload and run. No compilation step, no build process, no server
  configuration. This dramatically lowers the barrier to entry.
- **Gradual typing evolution**: PHP's migration from untyped to gradually typed (7.0 onward)
  is a masterclass in evolving a type system without breaking the world. Each version
  added more type features while maintaining backward compatibility.
- **Shared-nothing safety net**: Per-request isolation means bugs cannot leak state between
  users. Consider this model for web-facing workloads.
- **Null coalescing operator `??`**: Simple, readable, and solves 80% of null handling needs.
  `$x = $y ?? $z ?? 'default'` chains cleanly.
- **Nullsafe operator `?->`**: Method chaining on nullable values without nested `if` checks.
- **Named arguments**: Dramatically improves readability for functions with many parameters.
- **Match expressions**: Strict-comparison switch with return values. Better than traditional
  switch/case.
- **Laravel's DX focus**: Study how Laravel prioritizes developer experience. Artisan commands,
  tinker REPL, readable error pages, comprehensive documentation.

### 4.13 What to AVOID from PHP

- **Inconsistent standard library API**: Parameter ordering, naming conventions, and function
  prefixes should be consistent from day one. This cannot be fixed later without breaking
  everything.
- **Type coercion with `==`**: Loose equality comparison is a footgun. If you have operator
  overloading or type coercion, make the default comparison strict.
- **`$` sigil for variables**: Adds visual noise without providing value. If you have sigils,
  make them meaningful (like Perl's `@` for arrays, `%` for hashes).
- **Array-as-everything**: A single data structure for arrays, maps, sets, tuples, and lists
  is convenient but makes performance characteristics opaque. At minimum, distinguish
  sequences from maps.
- **Returning false on error**: Functions that return `false` instead of throwing exceptions
  create silent failure modes. Use proper error types.
- **Variable variables `$$`**: This feature makes code impossible to analyze statically.
  Never add dynamic variable name access.
- **`eval()` and `include()` with user input**: Design the language so that code execution
  from strings is either impossible or explicitly unsafe-marked.
- **Per-request bootstrap overhead**: While shared-nothing has safety benefits, bootstrapping
  an entire application framework on every request is wasteful. Provide persistent runtime
  options from the start.
- **No generics for 30 years**: Generics should be in the language from day one. Adding them
  later is extraordinarily difficult, as PHP has discovered.

---

## 5. V (Vlang)

### 5.1 Basics

| Property | Value |
|---|---|
| Year Created | 2019 (open-sourced June 2019) |
| Creator | Alexander Medvednikov |
| Paradigm | Multi-paradigm: procedural, functional, concurrent, object-oriented |
| Typing Discipline | Static, strong, inferred |
| Compilation Model | Compiled via C backend (primary), also native x64 and LLVM backends |
| Current Version | Weekly releases (e.g., weekly.2025.09). No 1.0 release yet. |
| License | MIT |

V is a systems programming language inspired by Go, Rust, Oberon, and Swift. It promises
C-level performance, fast compilation, and memory safety -- but has been the subject of
significant controversy regarding unfulfilled claims.

### 5.2 Best Use Cases

- **Small tools and utilities**: Quick compilation makes it good for CLI tools
- **Web development**: Vweb module for simple web servers
- **Game development**: Basic game frameworks available
- **Scripting replacement**: Positioned as a safer alternative to Bash/Python for scripting

**Claimed but unproven use cases:**
- Systems programming (autofree not ready)
- High-performance services (runtime performance not yet competitive)
- C/C++ replacement (translation tool not functional for real projects)

### 5.3 Loved Features

- **Simple syntax**: Deliberately minimal, Go-like syntax that is easy to learn
- **Fast compilation**: Compiles significantly faster than Rust/C++ (though slower than claimed)
- **No header files**: Single-pass compilation without forward declarations
- **Built-in testing**: `assert` and test blocks are first-class
- **No null**: `Option` type instead of null (`?Type`)
- **No undefined behavior**: Claims to eliminate UB (though this is disputed for edge cases)
- **No global state**: Global variables are restricted by default
- **Immutable by default**: Variables are immutable unless declared with `mut`
- **Concurrency primitives**: Go-style goroutines and channels
- **Cross-compilation**: Can compile for multiple targets from a single machine
- **No semicolons or braces**: Clean, minimal syntax

### 5.4 Hated Features / Pain Points -- The Controversy

V is the most controversial programming language of the 2020s. The criticism centers on a
pattern of extravagant claims that do not match reality:

**Autofree: The Central Broken Promise**
- V promised automatic memory management without a garbage collector -- "autofree" would
  insert `free()` calls at compile time, eliminating the need for GC or manual management
- Reality: Autofree is still experimental and not production-ready as of 2026
- Version 0.2 promised autofree would be "enabled by default in 0.3." It was not.
- Version 0.3 pushed the promise to 0.5. It was not delivered.
- The ROADMAP now targets 1.0 for autofree completion (no timeline)
- Boehm GC was added as the actual default memory management -- the thing autofree was
  supposed to eliminate
- Ved (V's own editor) crashes immediately when compiled with autofree
- Simple programs leak memory even with autofree enabled
- Independent reviewers conclude autofree is "a very crude technology"

**Compile Speed Claims vs Reality**
- Claimed: "1 million lines/second" with tcc backend
- V's own benchmark site shows ~207,972 V lines/sec with tcc
- Even accounting for hardware variance (2-3x factor), this is ~500,000-600,000 lines/sec --
  roughly half the claimed amount
- Benchmarks run on burstable AWS instances with turboboosting (unstable measurement environment)
- The benchmark generates 400,000 lines of synthetic code, acknowledged to "not represent
  real code" and to be "a bit silly"

**C/C++ Translation Claims**
- V promised the ability to "translate your entire C/C++ project" to V
- Claimed translations of Doom, Doom 3, LevelDB, and SQLite were advertised
- The Doom/Doom 3 translation links were never functional
- LevelDB and SQLite translation links were placeholders
- These claims were slowly watered down and some quietly removed

**Pattern of Behavior**
- Feature claims are made boldly, then watered down, then quietly removed
- Abandoned projects: V UI, a V-based OS, and other ecosystem projects started and dropped
- Critics report being banned from V community platforms
- The language creator has been accused of prioritizing marketing over engineering

### 5.5 Common Bugs

- **Autofree memory leaks**: Programs compiled with `-autofree` leak memory even in simple cases
- **Compiler crashes**: The V compiler has historically crashed on valid code more frequently
  than mature compilers
- **C backend issues**: Since V compiles to C, C compiler errors sometimes surface with
  unhelpful error messages
- **Missing features silently ignored**: Some advertised features are stubs that compile but
  do not function correctly
- **String handling edge cases**: Unicode handling has had bugs in various versions
- **Generic instantiation bugs**: Generics, while supported, have had codegen bugs for
  certain type combinations

### 5.6 Concurrency Model

V's concurrency model is modeled after Go:

**Goroutine-style Lightweight Threads**
- `go fn()` spawns a lightweight concurrent task (renamed from `go` to `spawn` to
  differentiate from Go)
- Built on top of OS threads with a runtime scheduler
- Conceptually similar to goroutines but with less mature scheduling

**Channels**
- Go-style channels for message passing between concurrent tasks
- Typed channels: `ch := chan int{}`
- Buffered and unbuffered channels
- Select statement for multiplexing channel operations

**Shared State**
- `shared` keyword for shared mutable state between threads
- `rlock` and `lock` blocks for read/write synchronization
- Atomic operations available

**Limitations**
- The concurrency implementation is less battle-tested than Go's
- No work-stealing scheduler (as of current versions)
- Thread pool management is less sophisticated than Go's runtime
- Real-world performance under high concurrency is not well-benchmarked

### 5.7 Type System

V's type system draws from several languages:

- **Static typing with inference**: Types inferred from initialization, explicit annotations optional
- **No null**: Uses `?Type` (Option type) instead of null
- **Sum types**: `type JsonValue = int | float | string | bool | []JsonValue`
- **Generics**: Supported. `fn max[T](a T, b T) T { ... }` -- code generated per type
- **Result type**: `!Type` for functions that can fail (similar to Rust's `Result`)
- **Structs, not classes**: No inheritance. Composition via embedded structs.
- **Interfaces**: Structural (duck) typing -- any struct implementing the interface's methods
  automatically satisfies it
- **Compile-time reflection**: `$for` loop for iterating over struct fields at compile time

**Soundness Concerns**
- The type system is less rigorously specified than Rust's or Haskell's
- Edge cases around sum types and generics have produced compiler bugs
- Autofree (if ever completed) would need to integrate with the type system for
  ownership tracking, which is not yet designed

### 5.8 Memory Management

V offers multiple memory management strategies:

| Strategy | Flag | Status |
|---|---|---|
| **Boehm GC** | Default | Working. Conservative GC. Standard for most V programs. |
| **Autofree** | `-autofree` | Experimental. Compiler inserts free() calls. Leaks in practice. |
| **Manual** | `-gc none` | Working. Developer calls `free()` manually. |
| **Arena allocation** | `Arena` type | Basic support. Pool-based allocation. |

**The Autofree Reality**
- Marketed as V's killer feature: compile-time memory management without GC overhead
- The compiler claims to handle "~90-100%" of objects automatically
- In practice, even V's own editor (Ved) crashes when compiled with autofree
- Simple test programs produce memory leaks with autofree enabled
- The ~90-100% claim is unverifiable and disputed by independent reviewers
- Boehm GC remains the practical default, making V's memory management story essentially
  "conservative GC" -- same as many other languages

### 5.9 Performance Characteristics

| Metric | Assessment |
|---|---|
| Compilation speed | Fast for a compiled language, but slower than claimed. ~500K lines/sec realistic. |
| Runtime performance | Moderate. Compiles to C but C codegen is not optimized like hand-written C. |
| vs C | Slower. Compiling to C does not automatically give C-level performance. |
| vs Go | Roughly comparable for simple programs. Go's runtime is more mature for concurrency. |
| vs Rust | Significantly slower. Rust's LLVM optimizations are more sophisticated. |
| Startup time | Fast (native binary, no VM). |
| Memory usage | Depends on GC mode. Boehm GC adds overhead. Autofree would be better if it worked. |
| Self-compilation | V compiles itself in ~1 second (a frequently cited metric). |

### 5.10 Tooling & Ecosystem

| Tool | Status |
|---|---|
| **Package Manager: VPM** | Basic. Packages installed to ~/.vmodules. Small ecosystem. |
| **Build Tool** | Built into the compiler. `v build`, `v run`. No separate build tool needed. |
| **IDE: VS Code** | V extension available. Basic syntax highlighting and completion. |
| **IDE: Vim/Neovim** | Plugin available. |
| **Testing** | Built-in test runner. `v test .` runs all test files. |
| **Formatting** | `v fmt` built-in formatter. |
| **Documentation** | `v doc` generates documentation from code. |
| **Ecosystem Size** | Small. Hundreds of packages on VPM vs hundreds of thousands for mature languages. |
| **Ecosystem Health** | Uncertain. Many packages are abandoned or incomplete. |
| **Web Framework** | Vweb (built-in, basic). Not production-grade. |
| **GUI** | V UI (largely abandoned). |

### 5.11 Agentic AI Usability

- **Not suitable**: V's immature ecosystem, experimental features, and unproven reliability
  make it a poor choice for AI agent development
- **No ML/AI libraries**: No equivalent of any mainstream ML framework
- **Small training corpus**: LLMs have very little V code to learn from
- **Compiler instability**: AI-generated code may hit compiler bugs
- **Potential future**: If V matures and autofree becomes reliable, its simple syntax could
  be good for AI code generation. But that future is uncertain.

### 5.12 What to STEAL from V

Despite the controversy, V has some good ideas worth studying:

- **Compilation speed as a priority**: Making compilation fast enough to feel interactive
  (<1 second for the compiler itself) is a legitimate goal worth pursuing.
- **Simple syntax**: V's Go-inspired syntax is genuinely easy to learn. Low syntactic noise
  is a virtue.
- **Option/Result types built-in**: `?Type` for optional, `!Type` for results. Having these
  in the language from day one is correct.
- **Immutable by default**: Variables are immutable unless `mut` is specified. This is the
  right default.
- **Built-in testing and formatting**: No external tools needed for testing or formatting.
  These should be part of the language toolchain.
- **No null**: Eliminating null and using Option types is unambiguously good.
- **Cross-compilation simplicity**: Making cross-compilation easy and accessible from the
  start is valuable.

### 5.13 What to AVOID from V

- **Making claims before delivering**: V's reputation has been severely damaged by promising
  features years before they were implemented. Never announce features that do not exist.
  Ship, then talk.
- **Autofree without formal ownership model**: Trying to automatically manage memory without
  a rigorous ownership/lifetime system (like Rust's borrow checker) produces an unreliable
  half-solution. Either commit to GC, commit to manual management, or commit to a provably
  correct ownership model.
- **Marketing-driven development**: Features should be driven by engineering rigor, not
  marketing claims. V's pattern of bold claims followed by quiet retractions destroys trust.
- **Abandoning ecosystem projects**: Starting V UI, an OS, and other projects then abandoning
  them signals lack of focus. Focus on making the core language solid before expanding.
- **Banning critics**: Silencing critical voices in the community creates an echo chamber and
  prevents real quality improvement.
- **Synthetic benchmarks**: Benchmarks should use real-world code, run on stable hardware,
  and be independently reproducible. Generating 400K lines of synthetic code on burstable
  AWS instances is not credible benchmarking.
- **Compiling to C as a performance claim**: "Compiles to C" does not mean "C-level
  performance." The quality of the generated C code matters enormously. Do not use your
  compilation target as a proxy for runtime performance claims.
- **AST-less compiler for speed**: Skipping the AST to make compilation faster sacrifices
  the ability to do proper analysis, optimization, and error reporting. The AST exists
  for a reason.

---

## 6. Cross-Language Comparison Matrix

| Dimension | Scala | Haskell | Lua | PHP | V |
|---|---|---|---|---|---|
| **Type Safety** | Very High | Highest | None (dynamic) | Medium (gradual) | High |
| **Performance** | High (JVM JIT) | Good (native) | Moderate/High (LuaJIT) | Moderate | Moderate |
| **Compilation Speed** | Slow | Slow | N/A (interpreted) | N/A (interpreted) | Fast |
| **Startup Time** | Poor (JVM) | Good | Excellent | Good | Good |
| **Memory Safety** | GC (JVM) | GC (GHC) | GC (inc/gen) | Refcount + GC | GC (Boehm) |
| **Concurrency** | Rich (Akka/ZIO/CE) | Excellent (STM) | Coroutines only | Shared-nothing + Fibers | Go-style |
| **Ecosystem Size** | Large (JVM) | Medium | Small | Massive (web) | Tiny |
| **Learning Curve** | Steep | Very Steep | Gentle | Gentle | Gentle |
| **Industry Adoption** | Niche (declining) | Niche (stable) | Niche (embedded) | Massive (web) | Minimal |
| **AI/ML Libraries** | Good (JVM) | Limited | None | None | None |
| **Null Safety** | Partial (Option) | Complete (Maybe) | None | Partial (?->) | Good (?Type) |
| **Generics** | Full + HKT | Full + HKT | None | None (planned) | Basic |
| **Maturity** | High | Very High | Very High | Very High | Low |

---

## 7. Steal vs Avoid Summary

### Universal Lessons Across All Five Languages

**STEAL: Ideas That Worked**
1. **Type classes / traits with coherence** (Haskell/Scala) -- the best ad-hoc polymorphism mechanism
2. **Option/Maybe instead of null** (Haskell/Scala/V) -- eliminates the billion-dollar mistake
3. **Pattern matching with exhaustiveness** (Haskell/Scala) -- makes sum types safe and practical
4. **Package manager that just works** (PHP Composer) -- solve dependency management early
5. **Immutability by default** (Scala/V/Haskell) -- mutable state should require explicit opt-in
6. **Coroutines / green threads** (Lua/Haskell) -- lightweight concurrency without OS thread overhead
7. **STM for shared state** (Haskell) -- composable transactions beat locks
8. **Built-in testing and formatting** (V/Go-style) -- these are not optional tools
9. **Gradual type system evolution** (PHP) -- add types incrementally without breaking existing code
10. **Embeddable design** (Lua) -- a clean FFI/embedding API extends a language's reach enormously

**AVOID: Mistakes That Hurt**
1. **Lazy evaluation by default** (Haskell) -- causes space leaks and unpredictable performance
2. **Language complexity sprawl** (Scala) -- having every feature is worse than having the right ones
3. **Inconsistent standard library** (PHP) -- parameter ordering and naming must be consistent
4. **Marketing-driven feature claims** (V) -- never announce what you have not built
5. **1-based indexing** (Lua) -- the world is 0-based; do not swim against the current
6. **Globals by default** (Lua pre-5.5) -- variables should be local/lexical by default
7. **Binary incompatibility between versions** (Scala) -- destroys ecosystem stability
8. **No generics at launch** (PHP) -- adding them later is extraordinarily painful
9. **Implicit resolution magic** (Scala) -- powerful but creates invisible, un-debuggable code paths
10. **Slow compilation** (Scala/Haskell) -- compilation speed is a developer experience feature

### Per-Language Summary

| Language | Top Steal | Top Avoid |
|---|---|---|
| **Scala** | Union/intersection types, for-comprehensions, opaque types, companion objects | Complexity sprawl, slow compilation, build tool complexity, implicit abuse |
| **Haskell** | Type classes, Maybe, STM, QuickCheck, purity tracking | Lazy-by-default, space leaks, broken records, String as [Char], partial functions in Prelude |
| **Lua** | Embeddability, coroutines, small footprint, metatables, incremental GC | 1-based indexing, globals by default, `#` on holey tables, nil-as-deletion, no standard OOP |
| **PHP** | Composer, deployment simplicity, gradual typing, `??` operator, named arguments | Inconsistent stdlib, no generics, `==` coercion, variable variables, returning false on error |
| **V** | Compilation speed focus, immutable-by-default, built-in tooling, `?`/`!` types, simple syntax | Unfulfilled promises, autofree without ownership model, marketing over engineering, banning critics |

---

*This research is intended to inform the design of a new programming language by learning from
the successes and failures of Scala, Haskell, Lua, PHP, and V. Each language offers valuable
lessons -- both in what to emulate and what to avoid.*
