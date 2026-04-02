# The Variant Strategy

> **Status: Historical Reference.** The memory model decision is finalized — CTRC + auto-clone is the default. This document is preserved for reference on alternative performance profiles. For the active specification, see MEMORY-MODEL.md.

## Philosophy: Don't Guess. Build, Measure, Decide.

All variants share a **common core**: syntax, type system, agentic keywords (`agent`, `tool`), pipe operator, pattern matching, ADTs, `T ! E`, generics, traits. The variants diverge on the hard tradeoff axes.

## The Shared Core

Every variant includes these features identically:
- Syntax (as defined in SYNTAX.md)
- Type system (as defined in TYPE-SYSTEM.md)
- `agent` and `tool` keywords
- Pipe operator `|>`
- Pattern matching with `match`
- Algebraic data types (sum types)
- `T ! E` and `T?`
- `?` error propagation
- Generics with trait bounds
- Traits (interfaces)
- String interpolation: `"Hello, {name}"`
- Comprehensions: `[x for x in items]`
- Async/await + Stream<T>
- Module system and imports
- Derive macros
- Compile-time execution (`const fn`)
- Attributes/decorators
- Closures/lambdas

## Axis 1: Memory Model Variants

| Variant | Model | Target Domain | Expected Tradeoff |
|---------|-------|---------------|-------------------|
| **A: Ownership-Lite** | Rust-style ownership, simplified lifetimes, `Shared<T>` escape hatch | Systems, servers, CLI | High performance, moderate learning curve |
| **B: Region-Based** | Values in named regions, bulk deallocation | Games, real-time, embedded | Predictable latency, simpler mental model |
| **C: Hybrid Own+Region** | Ownership default, `region {}` blocks for complex graphs | General purpose | Best of both, more compiler complexity |
| **D: CTRC (Compile-Time RC)** | ARC with aggressive static elision | App dev, agents, web | Simplest to use, slight perf overhead |

### Variant A: Ownership-Lite — Detailed

```
// Standard ownership — value moves
let a = [1, 2, 3]
let b = a  // a is moved to b, a is no longer valid

// Borrowing — references
fn sum(items: &[i32]) -> i32 {
  items.sum()
}
let total = sum(&b)  // b is borrowed, not moved

// Mutable borrow
fn push_one(items: &mut [i32]) {
  items.push(1)
}

// The simplification: no explicit lifetimes inside function bodies
// The compiler infers them automatically
fn first_word(s: &str) -> &str {
  // No 'a lifetime annotations needed — compiler infers
  s.split(' ').next() ?? ""
}

// Shared<T> escape hatch for shared ownership / cycles
let shared = Shared(Node { value: 1, children: [] })
let child = Shared(Node { value: 2, children: [] })
shared.children.push(child.clone())

// Public API boundaries MAY need lifetime annotations
pub fn longest<'a>(a: &'a str, b: &'a str) -> &'a str {
  if a.len() > b.len() { a } else { b }
}
```

**Strengths**: Highest performance ceiling, proven model (Rust), no runtime overhead
**Weaknesses**: Steepest learning curve of the four, still requires understanding ownership

### Variant B: Region-Based — Detailed

```
// Values allocated in named regions
region request_region {
  let user = User.parse(body)?       // allocated in request_region
  let response = process(user)        // also in request_region
  send(response)
}
// ALL memory in request_region freed here — one bulk deallocation

// Nested regions
region outer {
  let config = load_config()

  region inner {
    let temp = compute(config)  // can reference outer region
    use(temp)
  }
  // inner freed here

  use(config)  // config still alive
}
// outer freed here

// Frame-based allocation (games)
loop {
  region frame {
    let entities = update_entities()
    let draw_list = prepare_draw(entities)
    render(draw_list)
  }
  // Everything allocated this frame is freed here — zero fragmentation
}
```

**Strengths**: Simplest mental model, bulk deallocation is cache-friendly, great for frame-based apps
**Weaknesses**: Less flexible than ownership, regions must be planned, can waste memory if regions live too long

### Variant C: Hybrid Ownership + Regions — Detailed

```
// Simple values use ownership (automatic)
let name = "Alice"     // owned string, freed when out of scope
let nums = [1,2,3] // owned list, freed when out of scope

// Complex graphs use regions
region graph_region {
  let mut graph = Graph.new()
  for edge in edges {
    graph.add_edge(edge.from, edge.to)  // cyclic OK in a region
  }
  let result = shortest_path(graph, start, end)
}
// Entire graph freed here

// Compiler chooses: simple value? ownership. Complex graph? region hint.
// Developer can override with explicit region blocks

// Arena pattern for batch processing
fn process_batch(items: [RawData]) -> [Output ! Error] {
  region batch {
    items.map((item) => {
      let parsed = parse(item)      // in batch region
      let validated = validate(parsed)
      transform(validated)
    })
  }
  // batch region cleaned up, but results are moved out
}
```

**Strengths**: Best of both worlds, natural escalation path, compiler helps decide
**Weaknesses**: Most complex compiler implementation, two mental models to learn

### Variant D: CTRC (Compile-Time Reference Counting) — Detailed

```
// Developer writes normal-looking code — no ownership annotations
let user = User { name: "Alice", age: 30 }
let backup = user  // Copy? Move? RC? The compiler decides.

// Behind the scenes:
// 1. Compiler analyzes: is `user` used after this line?
//    - No: emit a move (zero cost)
//    - Yes: emit a reference count increment
// 2. Most code (80%+) resolves to moves via static analysis
// 3. Only genuinely shared data gets runtime refcounting

fn process(data: Data) -> Output ! Error {
  let step1 = parse(data)         // data moved in (no RC)
  let step2 = validate(step1)     // step1 moved in (no RC)
  let step3 = transform(step2)    // step2 moved in (no RC)
  ok(step3)
}
// Compiler proves single ownership throughout — ZERO ref counting

// Where RC kicks in:
let shared_config = Config.load()
spawn async { use_config(shared_config) }  // shared across tasks
spawn async { use_config(shared_config) }  // RC increment here
// Compiler can't prove single ownership — inserts RC
```

**Strengths**: Simplest developer experience, feels like GC, no annotations, no ownership learning
**Weaknesses**: Runtime RC overhead for shared data (~5-15% vs ownership), can't guarantee zero overhead

## Axis 2: Compilation Profile Variants

| Profile | Backend | Speed | Optimization | Use Case |
|---------|---------|-------|-------------|----------|
| **Dev** | Self-hosted / lightweight | <1s incremental | Minimal | Development, REPL, hot reload |
| **Release** | LLVM | 10-60s | Full LTO, PGO | Production binaries |
| **WASM** | LLVM → wasm32 | 5-30s | WASM-specific opts | Browser, edge, WASI |
| **Embedded** | LLVM, no-std | Variable | Size-optimized | Microcontrollers, IoT |

These are not exclusive — they're build flags:
```
turbolang build                          # Dev profile (fast compile)
turbolang build --release                # Release profile (LLVM, full opts)
turbolang build --target wasm32-wasi     # WASM profile
turbolang build --target thumbv7 --no-std # Embedded profile
```

## Axis 3: Concurrency Flavor Variants

All variants have async/await + channels. They diverge on the runtime:

| Variant | Runtime | Scheduling | Best For |
|---------|---------|------------|----------|
| **Tokio-style** (default) | Work-stealing thread pool | M:N cooperative | Servers, APIs, agents |
| **Actor-based** | Elixir-style isolated processes | Preemptive per-actor | Fault-tolerant distributed systems |
| **Minimal** | Single-threaded event loop | Cooperative | Embedded, WASM, simple tools |

Selection: `turbolang build --runtime=default|actor|minimal`

## The Research Loop

```
For each memory model variant (A, B, C, D):
  1. Implement the compiler support for the memory model
  2. Implement the shared core (syntax, types, agents)
  3. Write the benchmark suite:
     a. Linked list / tree manipulation (graph stress test)
     b. HTTP server under load (real-world allocation)
     c. JSON parser (allocation-heavy, string-heavy)
     d. Game loop simulation (frame-budget, deterministic)
     e. Agent orchestration loop (streaming, tool calls)
  4. Measure: throughput, P99 latency, memory usage, compile time, LOC
  5. Document results in research/benchmarks/{variant}/
  6. Compare across variants
  7. Decision: pick winner, merge ideas, or ship as profiles
```

## Expected Outcomes

### Scenario 1: Clear Winner
One variant dominates on most metrics. Ship it as the default, deprecate others.

### Scenario 2: Domain-Specific Winners
Different variants win for different domains:
- Ownership-Lite wins for systems/servers → ship as `turbolang build --memory=ownership`
- Regions wins for games/real-time → ship as `turbolang build --memory=regions`
- CTRC wins for agents/apps → ship as `turbolang build --memory=ctrc`

### Scenario 3: Hybrid Wins
The Hybrid (C) variant proves that combining ownership + regions is strictly better. Ship it as the only option.

### Scenario 4: Convergence
Insights from benchmarking lead to a novel 5th approach that combines the best of all four. Iterate and re-benchmark.

## Decision Criteria (Weighted)

| Criterion | Weight | Description |
|-----------|--------|-------------|
| Raw performance | 25% | Throughput on benchmarks vs C/Rust baseline |
| Worst-case latency | 15% | P99 latency variance — must be deterministic |
| Developer experience | 25% | LOC, annotation burden, error message quality |
| Compile time | 15% | Impact on incremental and full build times |
| Generality | 10% | How well it handles diverse workloads |
| Implementation complexity | 10% | Compiler complexity, maintenance burden |

## Timeline

| Week | Activity |
|------|----------|
| 1-2 | Implement shared core (parser, type checker, basic codegen) |
| 3-4 | Implement Variant A (Ownership-Lite) |
| 5-6 | Implement Variant D (CTRC) — simplest to implement |
| 7-8 | Implement Variant B (Regions) |
| 9-10 | Implement Variant C (Hybrid) |
| 11-12 | Write and run benchmark suite across all variants |
| 13 | Analyze results, make decision |
| 14 | Merge winning approach into main branch, document findings |
