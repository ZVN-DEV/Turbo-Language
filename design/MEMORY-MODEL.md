# Memory Model

> **Status: Planned (design vision).** This document describes a target memory model — Auto-Clone with Compile-Time Reference Counting (CTRC) and a four-level escape-hatch ladder — that is **not** what Turbo ships today. None of the following exist in the compiler: CTRC / compile-time refcount elision, auto-clone dataflow analysis, `let ref` borrows, `region` blocks, `Shared<T>` / `WeakRef<T>`, `@no_clone` / `@manual`, the ownership/borrow checker, and the `--memory-report` / `turbolang profile` tooling.
>
> **What actually ships today:** a **runtime ARC** model. Heap values (arrays, structs, enums) are reference-counted at runtime via `rt_rc_alloc` / `rt_release`, with **copy-on-write** for arrays/structs/enums (see the COW builtins). The built-in HTTP server adds **per-request bump arenas** that reset between requests. Strings currently **malloc-and-leak until process exit** — runtime string ARC is **in development** in a parallel branch. There is no compile-time elision: reference counting happens at runtime.
>
> Read the rest of this file as the design north star, not a description of current behavior. Unbuilt sections are marked **Planned** inline.

## Philosophy

Memory management is the hardest unsolved problem in language design. Every mainstream approach carries real costs that their advocates downplay:

- **Rust's borrow checker** is the number one complaint from developers learning or using the language. Lifetime annotations, fighting the borrow checker on legitimate patterns (graphs, self-referential structs, observer patterns), and the steep learning curve all contribute to lost productivity and developer frustration. Rust is right about safety, but the ergonomic cost is too high.
- **V's autofree** claims to automatically free memory at compile time without a GC or borrow checker. In practice, it remains vaporware: the implementation leaks memory in non-trivial programs, the formal guarantees are absent, and no independent benchmarks confirm its claims.
- **Swift's ARC** imposes runtime overhead on every reference copy and destruction. Retain/release traffic pollutes the instruction cache, atomic reference counting on shared references adds synchronization cost, and retain cycles remain a real footgun that developers must manually break with `weak` and `unowned`.
- **Garbage collection** kills real-time applications. No amount of incremental or concurrent GC engineering eliminates worst-case pauses entirely, and GC fundamentally removes the developer's ability to reason about when memory is freed.

**Turbo's chosen approach: Auto-Clone with Compile-Time Reference Counting (CTRC).** After evaluating multiple strategies, we have settled on a default memory model that combines the developer experience of garbage-collected languages with the performance of compile-time memory management. Auto-clone semantics with aggressive CTRC elision is the default for all Turbo code. Developers write code that looks and feels like JavaScript, and the compiler manages memory transparently through static analysis, implicit cloning, and compile-time reference count elision.

The goal: memory safety without garbage collection, with less cognitive overhead than Rust, backed by reproducible benchmarks.

---

## The JavaScript Promise

Turbo's north star for memory management is a single, non-negotiable principle: **you should never need to think about memory unless you want to.**

JavaScript developers allocate objects, use them, and forget about them. The runtime handles the rest. That experience is what we are replicating -- but instead of a garbage collector doing the work at runtime, the Turbo compiler does it at compile time. The result is identical developer ergonomics with zero runtime cost.

**The 90/10 rule.** In any Turbo codebase, 90% of the code should read and feel like JavaScript. You create values, pass them around, return them from functions, store them in collections. The compiler silently infers ownership, determines lifetimes, and inserts the necessary cleanup code. You never see a lifetime annotation, a borrow marker, or a memory-related compiler error. It just works.

The remaining 10% -- performance-critical inner loops, FFI boundaries, real-time audio/game code -- can opt into explicit memory control. This is not a compromise; it is the entire point. Most code does not need manual memory management, and forcing it on every line (as Rust does) is an unnecessary tax on developer productivity.

**Smart defaults that match JavaScript intuitions:**

- **Values auto-clone when shared.** If you use a value after passing it to a function, the compiler automatically clones it. In JavaScript, objects are reference-shared; in Turbo, the compiler achieves the same effect through implicit cloning. You never see a "value moved here" error.
- **Collections own their contents.** Pushing a value into a `[T]` collection or storing it in a `Map` works exactly like JavaScript -- you put it in, it stays there, you can still use the original if you want (auto-clone handles it).
- **Functions receive values, not puzzles.** Calling `greet(name)` does not invalidate `name`. The compiler decides whether to move or clone based on whether `name` is used again afterward. The developer does not care.
- **Return values just work.** Returning a locally-created value from a function is always valid. No lifetime annotations, no boxing, no tricks.

**The compiler is your memory manager.** Think of the Turbo compiler as a very smart garbage collector that runs at compile time. It sees your entire program, determines exactly when every value is last used, and inserts deallocation at precisely that point. The difference from a GC: zero runtime overhead, zero pauses, zero memory bloat. The difference from Rust: zero developer overhead.

```
// This is valid Turbo. No annotations. No memory thinking.
// A JavaScript developer can write this on day one.

fn fetch_user_posts(user_id: str) -> [Post] {
    let user = await db.find_user(user_id)      // allocated, used, compiler manages it
    let raw_posts = await db.query_posts(user.id) // same -- no manual cleanup
    let posts = raw_posts
        .iter()
        .filter((p) => p.is_published)
        .map((p) => Post.from(p, user.display_name)) // user is auto-cloned into each Post
        .collect()

    posts  // returned to caller, compiler transfers ownership seamlessly
}
// user, raw_posts, and all temporaries are freed here.
// The developer did not think about any of this.
```

**You never need to go deeper than your use case requires.** A web developer building CRUD APIs may never encounter a single memory annotation in their entire Turbo career. A game engine developer can drop into explicit regions and manual control for their hot paths. Both are first-class citizens of the same language.

### Auto-Clone Semantics

> **Planned — not yet implemented.** There is no compile-time move/clone dataflow analysis. Today, arrays/structs/enums use runtime ARC with copy-on-write; there is no `@no_clone` and no clone-cost warnings.

This is the single most important design decision that separates Turbo from Rust. In Rust, using a value after it has been moved is a compile error. In JavaScript, values are freely shared. Turbo sides with JavaScript: **if you use a value after it has been "moved," the compiler silently clones it for you.**

The mental model is simple: values behave like JavaScript objects. You can pass them around, store them in multiple places, and use them as many times as you want. Behind the scenes, the compiler inserts clones only where necessary -- and optimizes most of them away entirely.

**How it works:**

1. The compiler performs dataflow analysis on every function.
2. If a value is used after being passed to another function or stored in a collection, the compiler inserts an implicit clone at the point of first transfer.
3. If a value is never used after being transferred, no clone is inserted (it becomes a move, exactly like Rust).
4. The optimization pass then eliminates clones that are provably unnecessary (e.g., the clone is immediately consumed).

```
// In Rust, this is a compile error: "value moved here"
// In Turbo, this just works.

fn main() {
    let name = "Alice"
    greet(name)            // compiler auto-clones 'name' because it is used below
    print("Goodbye, {name}!")  // works fine -- 'name' is still valid

    let config = load_config()
    start_server(config)   // no clone needed -- 'config' is never used again (move)
}
```

```
// Collections work like JavaScript arrays and objects.
// No "cannot move out of index" errors.

fn build_team() -> [str] {
    let names = ["Alice", "Bob", "Charlie"]
    let first = names[0]   // auto-clones the element out of the list
    print("Team lead: {first}")
    names  // names is still valid and returned with all elements intact
}
```

**Opting out for performance:**

For performance-critical code where implicit cloning is unacceptable, annotate the type or function with `@no_clone`:

```
@no_clone
struct LargeBuffer {
    data: [u8]  // 10GB buffer -- cloning this would be catastrophic
}

fn process(buf: LargeBuffer) {
    transform(buf)
    // print(buf.data.len())  // COMPILE ERROR: LargeBuffer is @no_clone, value was moved
    // This is the Rust-like experience, but only for types that explicitly opt in.
}
```

**Clone cost warnings:**

The compiler tracks the cost of auto-clones and emits warnings when they exceed a configurable threshold:

```
// turbo.toml
[compiler]
auto_clone_warn_threshold = "1MB"   // warn if a single auto-clone copies more than 1MB
auto_clone_warn_count = 100         // warn if a function triggers more than 100 auto-clones
```

```
warning: auto-clone of 'large_dataset' copies approximately 50MB
  --> src/analysis.tb:42:5
   |
42 |     summarize(large_dataset)
   |               ^^^^^^^^^^^^^ auto-cloned here because it is used on line 45
   |
   = help: consider using `let ref summary = large_dataset` for zero-copy access
   = help: or annotate with @no_clone to enforce move semantics
```

This design is the KEY differentiator versus Rust. A JavaScript developer writing Turbo for the first time will never encounter a "value moved here" error. They write code naturally, and the compiler makes it work. Only when they are ready to optimize -- or when the compiler warns them about expensive clones -- do they need to think about memory at all.

### Escape Hatches Ladder

> **Planned — not yet implemented.** None of the ladder levels exist. There is no `let ref` (Level 1), no `region` block (Level 2), and no `@manual` mode (Level 3). Today there is exactly one memory model: runtime ARC + COW, plus per-request arenas inside the HTTP server.

Turbo's memory model is organized as a ladder of progressive complexity. Every developer starts at Level 0 and only climbs as high as their use case demands. Most developers will never go past Level 1.

**Level 0: Just Write Code (default)**

Auto-clone, auto-manage. No memory annotations anywhere. This is where JavaScript, Python, and TypeScript developers start. The compiler handles everything.

```
// Level 0: Pure JavaScript-style Turbo.
// No annotations, no memory thinking, no special syntax.

fn handle_request(req: Request) -> Response {
    let user = await auth.verify(req.token)
    let data = await db.query("SELECT * FROM posts WHERE author = ?", user.id)
    let html = template.render("posts.html", { user: user, posts: data })
    Response.ok(html)
}
// Everything is allocated, used, and freed automatically.
```

**Level 1: Explicit Borrowing (`let ref`)**

When you want zero-copy access -- reading a large value without cloning it -- use `let ref` to create an explicit borrow. This is the first opt-in step for developers who care about performance.

```
// Level 1: Explicit borrowing for zero-copy access.
// Use this when you know a value is large and you only need to read it.

fn analyze(dataset: [Record]) -> Summary {
    let ref first = dataset[0]      // borrow, no clone -- zero-copy access
    let ref last = dataset[dataset.len() - 1]

    // first and last are references into dataset.
    // They are valid as long as dataset is alive.
    print("Range: {first.date} to {last.date}")

    compute_summary(dataset)  // dataset moves here -- first and last are no longer valid
}
```

**Level 2: Region Blocks**

When you have a batch of allocations that should live and die together -- request processing, frame rendering, compilation passes -- use a `region` block. All allocations inside the region are freed in one shot when the block ends.

```
// Level 2: Region-based allocation for batch patterns.
// Use this for request-scoped work, per-frame game logic, etc.

fn render_frame(world: &World) -> FrameBuffer {
    region frame {
        let visible = cull(world.entities, world.camera)   // allocated in 'frame'
        let sorted = depth_sort(visible)                   // allocated in 'frame'
        let commands = generate_draw_calls(sorted)         // allocated in 'frame'
        let buffer = execute_commands(commands)             // escapes to caller
        buffer
    }
    // visible, sorted, commands: all freed in one pointer reset. Zero overhead.
}
```

**Level 3: Full Manual Control (`@manual`)**

For game engines, embedded systems, OS kernels, and other code where every allocation must be explicit and controlled. This is Rust-level (or C-level) control. You manage memory yourself.

```
// Level 3: Manual memory management.
// Use this for real-time audio, game engines, embedded, kernel code.

@manual
fn process_audio_buffer(input: &[f32], output: &mut [f32]) {
    // In a @manual block, there is no auto-clone, no implicit allocation.
    // Every allocation is explicit. Every deallocation is explicit.
    // The compiler enforces ownership and borrowing strictly (like Rust).

    let scratch = alloc<f32>(input.len())     // explicit heap allocation
    defer free(scratch)                        // explicit deallocation at scope end

    dsp.filter(input, scratch)
    dsp.normalize(scratch, output)
    // scratch is freed by the defer statement.
    // No allocator touched except where we explicitly asked.
}
```

**The ladder is a spectrum, not a wall.** You can mix levels freely within the same program. A web application might be 95% Level 0, with a few Level 1 borrows in a hot JSON serializer, and a Level 2 region for request processing. A game engine might be 60% Level 2 regions and 30% Level 3 manual, with Level 0 for its asset loading pipeline. Each level is opt-in per function, per block, or per type.

| Level | Who Uses It | Mental Model | Memory Errors Possible? |
|-------|-------------|-------------|------------------------|
| 0 | Web devs, scripting, prototyping | "Like JavaScript" | No. Compiler handles everything. |
| 1 | Performance-aware application devs | "I'll borrow instead of clone here" | No. Compiler checks borrow validity. |
| 2 | Server devs, game devs, batch processing | "These allocations live together" | No. Compiler enforces region safety. |
| 3 | Engine devs, embedded, kernel, FFI | "I control every byte" | Yes, within `@manual` blocks. Compiler warns. |

### Memory Profiling Built-In

> **Planned — not yet implemented.** `turbolang build --memory-report`, `turbolang profile`, `--profile=memory`, and the allocation flame graphs do not exist. There is no built-in memory profiler today.

Turbo's philosophy of "write it like JavaScript, then optimize" requires world-class tooling to show developers where their implicit memory decisions have performance costs. Memory profiling is not a third-party add-on -- it is built into the compiler and runtime from day one.

**`turbolang build --memory-report`: Compile-Time Memory Analysis**

After any build, pass `--memory-report` to get a detailed breakdown of how the compiler managed memory in your code:

```
$ turbolang build --memory-report

=== Memory Report: my_project ===

Auto-Clones:
  src/handlers.tb:42   user.clone()          ~256 bytes   (used after pass to template.render)
  src/handlers.tb:87   config.clone()        ~1.2KB       (used after pass to spawn_worker)
  src/models.tb:15     record.clone()        ~128 bytes   (used after push to [T]) x1000 iterations
  Total: 14 auto-clones, estimated ~180KB cumulative per request

Moves (zero-cost):
  src/handlers.tb:51   response -> return    (moved, no clone)
  src/db.tb:23         query -> execute      (moved, no clone)
  Total: 847 moves (zero overhead)

Regions:
  src/handlers.tb:38   region 'request'      ~4KB avg, freed at line 55
  Total: 1 region, O(1) deallocation

Suggestions:
  ! src/handlers.tb:87  config is cloned 10,000x/sec at load. Consider `let ref cfg = config`.
  i src/models.tb:15    record clone in loop. Consider pre-allocating with [T].with_capacity().
```

**`turbolang profile`: Runtime Memory Profiler**

A built-in runtime profiler modeled after Chrome DevTools' Memory tab. Attach to any running Turbo process:

```
$ turbolang profile --pid 12345
$ turbolang profile --attach my_server

# Or run with profiling enabled from the start:
$ turbolang run --profile=memory src/main.tb
```

The profiler provides:

- **Live allocation timeline.** See allocations and deallocations in real time, grouped by function and type. Identify allocation spikes correlated with incoming requests or game frames.
- **Heap snapshot diffing.** Take two snapshots, compare them, and see exactly what was allocated between the two points. Find leaks by comparing snapshots 10 minutes apart.
- **Allocation flame graphs.** Visualize which call stacks are responsible for the most allocations. Built into the toolchain -- no external tools needed.

```
$ turbolang profile --flamegraph --duration 30s --output allocs.svg
# Generates an SVG flame graph of allocation hotspots over a 30-second sample.
```

**The optimization migration path:**

The workflow Turbo enables -- and encourages -- is:

1. **Write it like JavaScript.** Use Level 0. Do not think about memory. Ship it.
2. **Profile under real load.** Use `turbolang profile` to see where memory is actually spent.
3. **Read the memory report.** Run `turbolang build --memory-report` to see auto-clone costs.
4. **Optimize the hot paths.** The report tells you exactly which lines to address. Add `let ref` borrows (Level 1) or `region` blocks (Level 2) only where the data shows it matters.
5. **Repeat.** Most codebases stabilize at step 2 with zero manual intervention. Only performance-critical systems need step 4.

```
// Before optimization (Level 0 -- pure JS-style):
fn process_all(records: [Record]) -> [Result] {
    records.iter().map((r) => {
        let enriched = enrich(r)       // auto-clone of r
        let validated = validate(enriched)
        transform(validated)
    }).collect()
}
// turbolang build --memory-report says: "record.clone() ~2KB x 100,000 iterations = ~200MB"

// After optimization (Level 1 -- one annotation added):
fn process_all(records: [Record]) -> [Result] {
    records.iter().map((ref r) => {    // 'ref r' borrows instead of cloning
        let enriched = enrich(r)       // zero-copy access
        let validated = validate(enriched)
        transform(validated)
    }).collect()
}
// Memory cost: ~0 bytes for the record access. Same result, 200MB saved.
```

This is the Turbo promise in action: you never need to think about memory until the profiler shows you a reason to. And when it does, the fix is a single keyword.

---

## Why Not GC

Garbage collection is deliberately excluded from our candidate set. This is not a casual decision. GC is the dominant memory management strategy across the most popular languages (Java, Go, Python, JavaScript, C#). It works. But it fails our requirements:

1. **GC pauses kill real-time applications.** Games targeting 16ms frame budgets cannot tolerate 5-50ms GC pauses. Audio processing at 44.1kHz requires deterministic buffer fills every ~1.5ms. High-frequency trading systems measure latency in microseconds; a GC pause is a missed trade. No production GC — not G1, not ZGC, not Go's concurrent collector — can guarantee zero pauses under all workloads.

2. **GC removes developer control over allocation and deallocation timing.** When you allocate in a GC language, you know when the allocation happens. When the object is freed is entirely opaque. You cannot force a collection at a specific point. You cannot guarantee that a destructor runs at the end of a scope. This makes resource management (file handles, network connections, GPU buffers) a second-class concern that requires separate patterns (try-with-resources, using blocks, defer).

3. **Rust proved you don't need GC for safety.** Before Rust, the conventional wisdom was that manual memory management meant use-after-free, double-free, and buffer overflows. Rust demonstrated that compile-time ownership tracking can prevent these classes of bugs entirely, without runtime cost. The existence proof is in: safety without GC is achievable.

4. **GC increases baseline memory usage.** Generational collectors require headroom — typically 2-3x the live set — to amortize collection costs. This is wasteful on memory-constrained devices and in containerized deployments where memory limits are hard.

5. **GC complicates FFI.** Calling into C libraries from a GC language requires pinning objects so the collector does not relocate them. Passing GC-managed objects across FFI boundaries requires prevent-collection guards. This friction makes systems programming and native interop harder than it needs to be.

**Our goal: make the non-GC path less painful than Rust, with data to back the choice.**

### Turbo vs. JavaScript's GC: Same Feel, No Cost

The question developers will ask is: "If Turbo feels like JavaScript, why not just use a GC like JavaScript does?" Here is the honest comparison:

| Dimension | JavaScript (V8 GC) | Turbo (Compiler-Managed) |
|-----------|-------------------|--------------------------|
| **Developer experience** | Excellent. Allocate and forget. | Identical. Allocate and forget. |
| **When memory is freed** | Whenever the GC decides to run. Unpredictable. Could be milliseconds later, could be seconds. | At the exact point the compiler determines the value is no longer reachable. Deterministic to the instruction. |
| **Pause behavior** | GC pauses range from sub-millisecond (minor GC) to 10-50ms (major GC). V8's concurrent collector reduces but does not eliminate pauses. | Zero pauses. Deallocation is interleaved with normal code execution at compiler-chosen points. No stop-the-world, ever. |
| **Memory overhead** | Generational GC requires 2-3x the live data set as headroom. A program using 100MB of live data may consume 250-300MB of RSS. | Minimal overhead. Memory is freed as soon as it is no longer needed. A program using 100MB of live data uses ~100MB of RSS plus allocator metadata. |
| **Resource cleanup** | Non-deterministic. Calling `close()` on a file handle requires explicit `finally` blocks or the developer remembers. Finalizers run at GC time, which may be never. | Deterministic. Destructors run at scope exit. File handles, sockets, and GPU buffers are cleaned up at the exact point they go out of scope. No `finally` needed. |
| **FFI cost** | High. Objects must be pinned to prevent GC relocation. Crossing the JS-to-C boundary requires marshaling. ArrayBuffers exist specifically to work around GC limitations. | Low. Values have stable addresses. Passing a pointer to C is straightforward. No pinning, no marshaling for compatible types. |
| **Suitability for real-time** | Poor. Game loops, audio processing, and HFT cannot tolerate GC pauses. Workarounds (object pooling, avoiding allocation) defeat the purpose of GC. | Excellent. Deterministic deallocation means predictable frame times, glitch-free audio, and consistent latency. |

**The bottom line: all the convenience of GC, none of the runtime cost.** Turbo achieves this by moving the "when to free" decision from runtime (GC) to compile time (static analysis). The developer experience is the same -- you write code, the system handles memory -- but the implementation has fundamentally better performance characteristics.

---

## Default Memory Model: Auto-Clone + CTRC

> **Planned — not yet implemented.** Despite the "Turbo ships with…" wording below, Turbo does **not** ship auto-clone or CTRC. The shipping default is runtime ARC + copy-on-write (plus per-request arenas in the HTTP server); strings currently leak until exit (string ARC in development). The ownership/region/hybrid "opt-in performance profiles" below are also unbuilt.

Turbo ships with auto-clone and compile-time reference counting (CTRC) as the default memory model for all Turbo code. This is the model described in "The JavaScript Promise" section above: values auto-clone when shared, the compiler elides reference counting operations where ownership can be statically proven, and developers never need to think about memory unless they choose to optimize.

The auto-clone + CTRC default is described in detail in "The Default: Compile-Time Reference Counting (CTRC)" section below, enhanced with the escape hatch ladder (Levels 0-3). This is not one of several competing candidates -- it is the chosen default that ships with Turbo.

The ownership and region strategies described below are available as **opt-in performance profiles** for specialized workloads. They are not competing alternatives to auto-clone; they are advanced tools that developers can reach for when profiling reveals a need:

- **Ownership profile** -- For code that needs Rust-style move semantics and zero reference counting overhead. Useful for FFI boundaries, embedded systems, and performance-critical libraries.
- **Region profile** -- For batch workloads where many allocations share a lifecycle (per-request server processing, per-frame game rendering, compilation passes). Provides O(1) bulk deallocation.
- **Hybrid profile** -- Combines ownership and regions for maximum control. Intended for engine-level code and advanced systems programming.

These profiles are selected per-module or per-function, not per-project. A web application might use the default auto-clone for 95% of its code, with a region profile for request processing and an ownership profile for a hot serialization path.

---

## Performance Profiles (Advanced, Opt-In)

> **Planned — not yet implemented.** None of the profiles below (Rust-Lite Ownership, Region-Based, Hybrid) are available. `&T` / `&mut T` borrows, `Shared<T>`, `WeakRef<T>`, `region` blocks, and lifetime annotations do not exist in the compiler. This section is design exploration.

The following strategies are available as opt-in performance profiles for developers who need finer control. They are documented here for completeness and for the benefit of contributors working on the compiler. Most Turbo developers will never need to read this section.

### Profile A — Rust-Lite Ownership

#### Core Idea

Take Rust's ownership and borrowing model — the part that actually works — and strip away the complexity that makes developers quit. The key insight is that most lifetime annotations in Rust exist to satisfy the compiler at API boundaries, not because the developer needs to think about them. If we aggressively infer lifetimes inside function bodies and only require explicit annotations at public API surfaces, we can preserve Rust's safety guarantees while dramatically reducing the annotation burden.

#### Rules

- **Single owner.** Every value has exactly one owner at any point in time. When the owner goes out of scope, the value is dropped (destructor runs, memory freed).
- **Move semantics by default.** Assigning a value to a new binding or passing it to a function transfers ownership. The original binding becomes invalid.
- **Copy for small types.** Types that are small and cheap to copy (integers, floats, booleans, small structs that opt in) are copied on assignment instead of moved.
- **Borrows: `&T` and `&mut T`.** You can lend a reference to a value without transferring ownership. `&T` is a shared immutable borrow (multiple allowed). `&mut T` is an exclusive mutable borrow (only one at a time, no concurrent `&T`).
- **Aggressive lifetime inference.** The compiler infers lifetimes inside function bodies automatically. Rust already does this for simple cases (single-input-single-output functions). We extend this to complex cases: multi-parameter functions, struct methods, closures capturing references. The compiler uses dataflow analysis to determine the minimum lifetime for each borrow.
- **Explicit lifetimes only at public API boundaries.** If you are writing a public function that returns a reference, and the compiler cannot determine which input the reference is derived from, you must annotate. Inside function bodies, inside private functions, inside closures: never.
- **`Shared<T>` as a non-unsafe escape hatch.** When you genuinely need shared ownership (cyclic data structures, observer patterns, shared caches), `Shared<T>` provides reference-counted ownership without requiring `unsafe` or fighting the borrow checker. This is a first-class citizen, not a shameful workaround.
- **Auto-boxing.** When a value would require a lifetime annotation that the compiler cannot infer (typically when storing a reference inside a struct that outlives the current scope), the compiler can automatically box the value (heap-allocate with single ownership) to avoid forcing the developer to restructure their code.

#### Hypothesis

80% of Rust's safety with 50% of the complexity. The escape hatches (`Shared<T>`, auto-boxing) prevent developers from hitting walls where they must restructure their entire program to satisfy the borrow checker.

#### Code Examples

**Basic ownership and moves:**

```
fn main() {
    let name = "Alice"                // name owns the string
    greet(name)                        // ownership moves to greet
    // print(name)                     // COMPILE ERROR: name was moved
}

fn greet(who: str) {
    print("Hello, {who}!")
}   // who is dropped here, string memory freed
```

**Borrowing without lifetime annotations:**

```
// No lifetime annotations needed. The compiler infers that the
// returned &str lives as long as the input &str.
fn first_word(s: &str) -> &str {
    let bytes = s.as_bytes()
    for (i, byte) in bytes.iter().enumerate() {
        if byte == ' ' {
            return s[0..i]
        }
    }
    s
}

// Even with multiple parameters, the compiler infers lifetimes
// when the logic is unambiguous.
fn longer(a: &str, b: &str) -> &str {
    if a.len() >= b.len() { a } else { b }
}
// The compiler sees both a and b could be returned, so the result
// lifetime is the intersection (shorter) of the two input lifetimes.
// In Rust you'd write: fn longer<'a>(a: &'a str, b: &'a str) -> &'a str
// Here, the compiler figures this out.
```

**Where explicit lifetimes ARE required (public API boundaries):**

```
// Public struct that holds a reference: lifetime required.
// This is a public API contract — callers need to know.
pub struct Parser<'src> {
    source: &'src str
    position: usize
}

// Public function returning a reference tied to the struct:
// the annotation documents the contract.
pub fn current_token<'src>(parser: &Parser<'src>) -> &'src str {
    // ... inside the body, no annotations needed
    let start = parser.position
    let end = find_token_end(parser.source, start)  // inferred
    parser.source[start..end]                        // inferred
}
```

**`Shared<T>` for shared ownership and cyclic data:**

```
// A doubly-linked list node. Ownership is inherently shared:
// each node is pointed to by its predecessor AND successor.
// In Rust, you'd need Rc<RefCell<Node>> or unsafe. Here, Shared<T>.

struct Node {
    value: i32
    next: Shared<Node>?      // optional shared reference
    prev: WeakRef<Node>?     // weak reference to break cycles
}

fn build_list() -> Shared<Node> {
    let a = Shared(Node { value: 1, next: none, prev: none })
    let b = Shared(Node { value: 2, next: none, prev: WeakRef(a) })
    a.next = Shared.clone(b)  // a and b now share ownership of b's node
    a
}
// When all strong references to a node are dropped, it is freed.
// WeakRef<T> does not prevent deallocation — accessing a dead weak ref
// returns none.
```

**Auto-boxing to avoid annotation pressure:**

```
struct Config {
    name: str
    // Without auto-boxing, storing a closure that captures references
    // would require lifetime annotations on Config.
    // The compiler auto-boxes the closure to avoid this.
    validator: fn(str) -> bool   // auto-boxed if it captures references
}

fn make_config(prefix: str) -> Config {
    Config {
        name: "my_config"
        // This closure captures `prefix` by value (moved in).
        // No boxing needed — it owns its data.
        validator: (s) => s.starts_with(prefix)
    }
}
```

---

### Profile B — Region-Based Memory

#### Core Idea

Instead of tracking ownership of individual values, group values into named regions. A region is a contiguous block of memory (an arena). All values allocated into a region live until the entire region is freed at once. There are no individual frees — deallocation is always bulk. The compiler tracks which region each value belongs to and enforces that references from an inner region to an outer region are valid, but references from an outer region to an inner region are forbidden (because the inner region may be freed first).

This is inspired by Cyclone (the first language to formalize region-based memory management), MLKit (which showed region inference can work for functional languages), and more recently Mojo's approach to scoped lifetimes.

#### Rules

- **Every value belongs to a region.** If no region is specified, values belong to the function's default region, which is freed when the function returns.
- **Regions are declared with `region name { ... }`.** All allocations inside the block belong to that region.
- **Regions nest.** An inner region can reference values in an outer region (the outer region will outlive the inner one). An outer region cannot hold references to values in an inner region (the inner region may be freed while the outer is still live).
- **No individual frees.** You never free a single value. You free the entire region at once when the block ends.
- **Bulk deallocation is fast.** Freeing a region is O(1) — reset a pointer. No destructor cascade, no traversal. If values need destructors (file handles, network connections), they are registered on a destructor list and called in reverse order when the region is freed.
- **Region inference.** For simple cases, the compiler infers which region a value should belong to based on its usage. Explicit region annotations are needed when the compiler cannot determine the correct region.

#### Hypothesis

Simpler mental model than ownership. Developers think in terms of "this batch of data lives together and dies together" rather than tracking individual value lifetimes. Bulk deallocation is cache-friendly and fast.

#### Code Examples

**Basic region usage:**

```
fn process_request(req: Request) -> Response {
    // Everything in this block is allocated in the 'scratch' region.
    // When the block ends, ALL of it is freed at once — one pointer reset.
    region scratch {
        let parsed = parse_json(req.body)      // allocated in 'scratch'
        let validated = validate(parsed)        // allocated in 'scratch'
        let query = build_query(validated)      // allocated in 'scratch'
        let result = db.execute(query)          // allocated in 'scratch'

        // The response is built from 'result' but must outlive 'scratch'.
        // It is allocated in the function's default region.
        build_response(result)  // returned value escapes to outer region
    }
    // 'scratch' is freed here. parsed, validated, query, result: all gone.
    // The returned Response survives because it was allocated outside.
}
```

**Nested regions:**

```
fn compile(source: str) -> Program {
    region ast_region {
        let tokens = tokenize(source)       // in ast_region
        let ast = parse(tokens)             // in ast_region

        region optimization_region {
            // optimization_region can reference values in ast_region
            // (ast_region outlives optimization_region)
            let ir = lower_to_ir(ast)           // in optimization_region
            let optimized = optimize(ir)        // in optimization_region
            let code = codegen(optimized)       // escapes to outer

            // The generated code escapes optimization_region.
            // ir, optimized: freed when this block ends.
            code
        }
        // optimization_region freed here.

        // ast, tokens still alive — they're in ast_region.
        let program = link(code, ast.metadata)
        program  // escapes to function's default region
    }
    // ast_region freed here. tokens, ast: gone.
}
```

**Region parameters on functions:**

```
// This function allocates its result into a caller-specified region.
// The caller controls how long the result lives.
fn parse_json(input: str, into: region) -> JsonValue in into {
    // All allocations for the JsonValue tree go into 'into'
    let root = into.alloc(JsonObject{})
    for (key, value) in tokenize_json(input) {
        root.set(key, parse_value(value, into))
    }
    root
}

fn main() {
    region long_lived {
        // The JSON tree lives in 'long_lived' — survives the parse call.
        let config = parse_json(read_file("config.json"), long_lived)

        region request_scope {
            // This JSON tree lives in 'request_scope' — freed after the block.
            let body = parse_json(request.body, request_scope)
            handle(config, body)  // config is in outer region, body in inner. Valid.
        }
        // body is freed here (request_scope gone).
        // config still alive (long_lived still active).
    }
}
```

**Compile error: dangling reference across regions:**

```
fn bad_example() {
    let outer_ref: &Node

    region temp {
        let node = Node { value: 42 }
        outer_ref = &node
        // COMPILE ERROR: 'node' belongs to region 'temp', but 'outer_ref'
        // belongs to the function's default region which outlives 'temp'.
        // Cannot store a reference to a shorter-lived region in a
        // longer-lived region.
    }
}
```

---

### Profile C — Hybrid Ownership + Regions

#### Core Idea

Combine Profile A and Profile B. Use ownership (single owner, move semantics, borrowing) as the default for most code. When the code involves complex allocation patterns — batch processing, graph construction, request handling — switch to region-based allocation with explicit `region` blocks. The compiler can auto-select: values with simple, linear lifetimes use ownership; values that are part of a batch or graph use regions.

#### Rules

- **Default: ownership.** All the rules from Profile A apply by default. Values have single owners, move on assignment, can be borrowed.
- **Opt-in: regions.** When you open a `region` block, all allocations inside use arena-style allocation. Ownership rules are relaxed inside the region — you can have multiple references to values within the same region without borrow-checking headaches, because everything in the region dies together.
- **Compiler auto-selection.** For structs and values with straightforward lifetimes (created, used, dropped in sequence), the compiler uses ownership. For values allocated inside a `region` block, the compiler uses the region allocator. The developer can override in either direction.
- **Crossing the boundary.** Values can move from ownership into a region (the region takes ownership). Values can escape a region into ownership (they are copied or moved out of the region before it is freed). References from owned code into a region are valid only while the region is alive (enforced at compile time).

#### Hypothesis

Best of both worlds. Ownership handles the 90% case (local values, function parameters, return values) with zero overhead. Regions handle the 10% case (graph construction, batch processing, request-scoped allocation) where ownership becomes painful. Developers never need to fight the borrow checker on complex data structures because they can drop into a region.

#### Code Examples

**Default ownership (same as Profile A):**

```
fn process(data: [i32]) -> i32 {
    let total = data.iter().sum()    // data owned, iterator borrows
    total
}   // data dropped here
```

**Region for complex graph construction:**

```
fn build_dependency_graph(packages: &[Package]) -> DepGraph {
    // Graph construction involves cycles, shared references, complex topology.
    // Rather than fighting the borrow checker, use a region.
    region graph_arena {
        let nodes: Map<str, &Node> = Map.new()

        for pkg in packages {
            let node = graph_arena.alloc(Node {
                name: pkg.name
                deps: []
                dependents: []
            })
            nodes.insert(pkg.name, node)
        }

        // Now wire up edges. Multiple nodes reference each other.
        // In ownership mode, this would require Shared<T> everywhere.
        // In a region, it just works — all nodes live in the same region.
        for pkg in packages {
            let node = nodes[pkg.name]
            for dep_name in pkg.dependencies {
                let dep_node = nodes[dep_name]
                node.deps.push(dep_node)         // shared reference, OK in region
                dep_node.dependents.push(node)    // back-reference, OK in region
            }
        }

        // Topological sort. result is a [str] that escapes the region.
        let order = topological_sort(nodes.values())
        DepGraph.from_order(order)  // escapes to caller's ownership
    }
    // All nodes, the map, all edges: freed in one shot.
}
```

**Compiler auto-selection:**

```
fn handle_request(req: Request) -> Response {
    // 'config' has a simple lifetime — compiler uses ownership.
    let config = load_config()

    // 'response_builder' involves many small allocations that all
    // die together. Compiler suggests a region (or developer opts in).
    region request_scope {
        let headers = parse_headers(req)
        let body = parse_body(req)
        let auth = validate_auth(headers)
        let result = execute(auth, body, config)
        // Only the final Response escapes.
        Response.from(result)
    }
}
```

**Mixing ownership and regions in a single function:**

```
fn transform_dataset(input: [Record]) -> [Record] {
    let mut output = [].with_capacity(input.len())  // owned

    for chunk in input.chunks(1000) {
        region chunk_scope {
            // Intermediate processing for this chunk lives in the region.
            let parsed = chunk.iter().map((r) => parse(r)).collect()
            let enriched = enrich_all(parsed)       // in chunk_scope
            let filtered = filter(enriched)         // in chunk_scope

            // Move surviving records out of the region into 'output'.
            for record in filtered {
                output.push(record.to_owned())  // copied out of region
            }
        }
        // chunk_scope freed. Intermediate data for this chunk: gone.
        // output retains only the records we kept.
    }

    output  // returned to caller via ownership
}
```

---

### The Default: Compile-Time Reference Counting (CTRC)

#### Core Idea

Start with the simplest possible model for the developer: every value is reference-counted. When you assign a value to a new binding, the reference count increments. When a binding goes out of scope, the reference count decrements. When the count hits zero, the value is freed. This is what Swift does with ARC (Automatic Reference Counting).

The innovation is aggressive compile-time elision. The compiler performs static analysis to determine where reference count operations are unnecessary — because ownership can be statically proven — and removes them. In the ideal case, the compiler elides so many retain/release pairs that the generated code looks like Rust's zero-cost ownership. In the fallback case, runtime reference counting handles what static analysis cannot prove.

#### Rules

- **Every heap-allocated value has a reference count.** Stack-allocated primitives (integers, floats, booleans) do not.
- **Assignment increments the count. Scope exit decrements.** This is the semantic model the developer reasons about.
- **The compiler elides redundant operations.** If the compiler can prove that a value has exactly one owner at a given point, it removes the retain/release pair. If it can prove that a function receives a value and does not store it beyond the call, it removes the retain on entry and release on exit (a "guaranteed" parameter).
- **Cycle detection.** The developer must break cycles manually using `WeakRef<T>` references (same as Swift). The compiler warns when it detects potential cycle patterns (e.g., two types that reference each other).
- **Atomic vs. non-atomic.** By default, reference counts are non-atomic (single-threaded fast path). Values shared across threads use atomic reference counts. The compiler infers which is needed based on whether the value crosses a thread boundary.

#### Hypothesis

ARC's simplicity (developers never think about ownership or lifetimes) with near-ownership performance via aggressive compile-time elision. The 80% of code where ownership is straightforward gets zero-cost elision. The 20% of code with complex sharing patterns pays a small runtime cost for reference counting, which is still deterministic (no GC pauses).

#### Code Examples

**Basic semantics — the developer's mental model:**

```
fn main() {
    let a = "hello"                // refcount = 1
    let b = a                      // refcount = 2 (conceptually)
    print(b)                       // b is used
    // b goes out of scope: refcount = 1
    // a goes out of scope: refcount = 0, string freed
}
```

**After compile-time elision — what the compiler actually generates:**

```
// The compiler sees that 'a' is never used after 'let b = a'.
// It elides the retain on 'b' and the release on 'a'.
// Effectively: 'b' takes ownership. No refcount operations at all.
fn main() {
    let a = "hello"                // allocated, no refcount overhead
    let b = a                      // move (elided retain/release)
    print(b)
    // b goes out of scope: freed directly, no decrement needed
}
```

**Where elision works (common patterns):**

```
// Pattern 1: Last use before scope end — elided to move
fn process(data: str) {
    // Compiler sees 'data' is consumed by transform and never used again.
    // Elides to a move. Zero refcount overhead.
    let result = transform(data)
    print(result)
}

// Pattern 2: Temporary values — elided entirely
fn build_greeting(name: str) -> str {
    // The intermediate string " world" is created, appended, never shared.
    // Compiler elides all refcount operations on the temporary.
    let greeting = "Hello, " + name + "!"
    greeting
}

// Pattern 3: Guaranteed parameters — elide retain/release at call boundary
fn print_length(s: str) {
    // Compiler proves 's' is only read, never stored.
    // Marks it as a "guaranteed" parameter: no retain on entry, no release on exit.
    print(s.len())
}
```

**Where elision fails — runtime RC required:**

```
// Pattern 1: Storing in a collection with unknown lifetime
fn cache_result(result: str, cache: Map<str, str>) {
    // The result is stored in 'cache', which outlives this function.
    // The compiler cannot elide: it must retain 'result' for the cache
    // and release it when the cache drops the entry.
    cache.insert("latest", result)   // retain (runtime)
}

// Pattern 2: Conditional storage — compiler can't prove statically
fn maybe_save(data: str, should_save: bool) {
    if should_save {
        global_store.push(data)   // retain (runtime) — escapes
    }
    // If should_save was false, data is dropped here (release, runtime).
    // Compiler can't remove the retain/release because the branch is dynamic.
}

// Pattern 3: Shared mutable state across closures
fn setup_handlers(state: str) {
    let shared = state  // must be retained

    on_click(|| {
        print(shared)   // closure 1 retains 'shared'
    })

    on_hover(|| {
        print(shared)   // closure 2 retains 'shared'
    })

    // 'shared' has 3 owners (local + 2 closures). Runtime RC required.
}
```

**Breaking cycles with `WeakRef<T>`:**

```
struct Employee {
    name: str
    manager: WeakRef<Employee>?   // weak to break the cycle
    reports: [Employee]     // strong ownership of reports
}

fn build_org() {
    let ceo = Employee { name: "Alice", manager: none, reports: [] }
    let vp = Employee { name: "Bob", manager: WeakRef(ceo), reports: [] }
    ceo.reports.push(vp)

    // ceo owns vp (via reports). vp references ceo weakly (via manager).
    // No cycle. When ceo is dropped, vp is dropped (as part of reports),
    // and the weak reference to ceo in vp becomes none (already being freed).
}
```

**Elision analysis summary:**

| Pattern | Elision? | Runtime Cost |
|---------|----------|--------------|
| Last use of a value (consumed or returned) | Yes, elided to move | Zero |
| Temporary values never stored | Yes, elided entirely | Zero |
| Function parameters only read, not stored | Yes, "guaranteed" calling convention | Zero |
| Values stored in collections with longer lifetimes | No, runtime RC | Retain + release |
| Conditionally stored values | No, runtime RC | Retain + release (branch-dependent) |
| Values captured by multiple closures | No, runtime RC | Retain per closure |
| Values shared across threads | No, atomic RC | Atomic retain + release |

In practice, we estimate that 70-85% of reference count operations can be elided at compile time for typical application code. Systems code with heavy sharing patterns will see lower elision rates (50-60%). The key question for benchmarking is whether the remaining runtime RC cost is material.

---

## Benchmark Criteria

| Metric | How We Measure | Why It Matters |
|--------|----------------|----------------|
| **Scalability** | Measure throughput and latency across working sets from 1KB to 1GB. Plot performance curves. Look for cliffs (points where the strategy degrades non-linearly). | A strategy that works for small programs but falls apart at scale is not viable for production use. |
| **Determinism** | Run each benchmark 1000 times. Measure variance in latency. Report p50, p99, p99.9, and max latency. Compute jitter (max - min). Compare worst-case to average. | Non-GC strategies should deliver near-zero jitter. Any strategy with high variance fails the real-time use case. |
| **Raw throughput** | Micro-benchmarks in three categories: allocation-heavy (millions of small allocations), compute-heavy (tight loops over owned data), mixed (realistic allocation + computation blend). Compare wall-clock time against Rust and C baselines compiled with equivalent optimization levels. | The memory strategy must not impose more than 5-10% overhead over the theoretical optimum (manual malloc/free). |
| **Cognitive load** | For each benchmark, implement the same algorithm in all 4 profiles. Count: lines of code, number of memory-related annotations (lifetime parameters, region declarations, RC operations), number of compiler errors during development, time to resolve each error. | Developer productivity is a first-class metric. A strategy that is 5% faster but requires 50% more code is not necessarily better. |
| **Compile time impact** | Measure compilation time for each benchmark across all 4 profiles. Measure incrementally (single-file change) and from scratch (clean build). Report overhead as a percentage relative to a hypothetical "no memory checking" baseline. | Compile times above 10-15 seconds for incremental builds break developer flow. Strategies with heavy compile-time analysis (ownership inference, region checking, CTRC elision) must not impose excessive compile-time costs. |
| **Error message quality** | For each profile, deliberately introduce 10 common memory errors (use-after-free, dangling reference, cycle creation, move-after-use, etc.). Rate each error message on: (a) does it identify the problem? (b) does it suggest a fix? (c) is it concise? Score 1-5 on each axis. | When the developer gets it wrong, the compiler must be helpful, not cryptic. Rust's early error messages were infamously bad; its later investment in diagnostics was transformative. |

---

## Benchmark Programs

### 1. Linked List / Tree Manipulation

**What it tests:** Graph-shaped data structures that stress ownership models. Linked lists require shared pointers (each node points to the next), doubly-linked lists require cyclic references (each node points to both neighbors), and trees require parent-to-child and optional child-to-parent references.

**Memory patterns stressed:**
- Shared ownership (multiple references to the same node)
- Cyclic references (doubly-linked lists, trees with parent pointers)
- Incremental mutation (inserting, deleting, rebalancing nodes)
- Traversal of pointer-heavy structures (cache pressure from chasing pointers)

**What we expect to learn:**
- Profile A (Ownership) will require `Shared<T>` and `WeakRef<T>` for doubly-linked lists, adding overhead. How much overhead?
- Profile B (Regions) should excel: allocate all nodes in one region, cross-references within the region are free, bulk deallocation on cleanup.
- Profile C (Hybrid) should use a region for the graph structure, ownership for the algorithm logic. Is the boundary clean?
- The default (CTRC) will have runtime RC on every node link operation. Is the elision rate acceptable for graph-heavy code?

### 2. HTTP Server Under Load

**What it tests:** Real-world allocation patterns in a network server. Each incoming request triggers allocations for headers, body parsing, routing, handler execution, and response construction. Connections have lifecycles (accept, read, process, write, close). Under load, thousands of connections are active concurrently.

**Memory patterns stressed:**
- Per-request allocation bursts (allocate on request arrival, free on response send)
- Connection lifecycle management (sockets, buffers, TLS state)
- Shared state (routing table, configuration, connection pool) vs. per-request state
- Concurrent access to shared data across threads/coroutines

**What we expect to learn:**
- Profile B (Regions) and Profile C (Hybrid) should excel with per-request regions that free all request state at once.
- Profile A (Ownership) will require careful lifetime management for request-scoped data that references shared state (e.g., a database connection borrowed from a pool for the duration of a request).
- The default (CTRC) will have high elision rates for per-request data (created, used, discarded) but runtime RC for shared state (connection pools, caches).
- All profiles should show deterministic latency under load (unlike a GC-based server which would show tail latency spikes).

### 3. JSON Parser

**What it tests:** Allocation-heavy, string-heavy workload. Parsing JSON requires creating many small objects (strings, numbers, arrays, objects), building a tree structure, and often converting to a target type. This stresses the allocator's performance on many small allocations and the memory strategy's overhead on short-lived intermediate values.

**Memory patterns stressed:**
- Many small allocations (string slices, number boxes, array/object containers)
- Tree construction (parent-child relationships)
- String handling (slicing, copying, escaping/unescaping)
- Temporary values (intermediate parse state that is discarded)

**What we expect to learn:**
- Raw allocation throughput comparison across all 4 profiles.
- Profile B (Regions) should show the best throughput: allocate all parse output into a region, bump-pointer allocation, zero per-object overhead, bulk free.
- The default (CTRC) should show high elision rates (parse state is mostly temporary), but the sheer number of allocations will amplify any per-allocation overhead.
- Profile A (Ownership) should perform similarly to Rust, since JSON parsing is mostly a linear pipeline (no complex sharing).
- String handling strategy matters: are strings reference-counted (shared substrings) or copied? This will differ across profiles.

### 4. Game Loop Simulation

**What it tests:** Frame-budget allocation in a game loop. Each frame has a hard deadline (16.67ms at 60fps). Within each frame, the game allocates for entity updates, physics simulation, rendering commands, UI layout, and audio mixing. Any allocation strategy that causes latency spikes beyond the frame budget is disqualifying.

**Memory patterns stressed:**
- Fixed time budget (worst-case latency matters more than average throughput)
- Per-frame temporary allocations (allocated at frame start, freed at frame end)
- Persistent game state (entity positions, game world) that lives across frames
- Double-buffering patterns (old frame state vs. new frame state)

**What we expect to learn:**
- Determinism is the primary metric. Plot per-frame allocation/deallocation time as a histogram. Any outliers beyond 1ms are concerning.
- Profile B (Regions) with a per-frame region is the natural fit. Allocate all frame temporaries into a frame region, reset at frame end.
- The default (CTRC) must show that reference counting overhead does not cause frame time spikes. Even deterministic RC has per-object cost that could accumulate.
- Profile A (Ownership) should be near-zero overhead but may require more code to manage the frame-scope vs. persistent-state boundary.
- Profile C (Hybrid) is expected to be the best fit: ownership for persistent game state, region for per-frame temporaries.

### 5. Agent Orchestration Loop

**What it tests:** A long-running AI agent that processes a stream of events, makes tool calls, maintains conversation context, and streams responses. This models the workload of an LLM-powered application (agent frameworks, chatbots, autonomous coding tools). The process runs for hours or days, making memory leaks and fragmentation critical concerns.

**Memory patterns stressed:**
- Long-running process (memory leaks accumulate, fragmentation worsens over time)
- Streaming data (chunks arrive over time, are assembled into messages, then discarded)
- Tool-call lifecycle (allocate context for a tool call, execute, process result, free context)
- Growing conversation history (append-only, occasionally compacted)
- Mixed lifetimes: short-lived (individual tokens), medium-lived (tool call contexts), long-lived (conversation history)

**What we expect to learn:**
- Memory usage over time (should be stable, not growing). Plot RSS over a 1-hour run.
- All profiles should handle the streaming pattern efficiently. Profile B (Regions) can use per-tool-call regions. Profile A (Ownership) naturally frees tool-call context when the call completes.
- The default (CTRC) must not leak due to reference cycles in the conversation graph (messages referencing tool results referencing messages).
- Fragmentation analysis: after 1 hour of operation, how fragmented is the heap? Profile B (Regions) should show minimal fragmentation (bulk allocation/deallocation). Profile A and the default (CTRC) may show more fragmentation from individual allocations and frees.
- This benchmark is the most representative of a modern application workload and may be the tiebreaker.

---

## Comparison Matrix

| Dimension | Profile A: Rust-Lite Ownership | Profile B: Region-Based Memory | Profile C: Hybrid Own+Region | Default: CTRC |
|-----------|---------------------------|---------------------------|--------------------------|-------------|
| **Learning curve** | Moderate. Developers must understand ownership, moves, and borrowing. But no explicit lifetimes in most code, and `Shared<T>` provides an escape hatch. Easier than Rust, harder than GC languages. | Low-to-moderate. "Values belong to a region, region frees everything" is a simple concept. Knowing when to create a region and how to nest them requires some learning. | Moderate. Must understand both ownership and regions, but can start with just ownership and learn regions when needed. | Low. "Everything is reference counted" is the simplest model. Developers from Swift, Python, or Objective-C will feel at home. `WeakRef<T>` for cycles is the only new concept. |
| **Performance ceiling** | High. Equivalent to Rust. Zero-cost abstractions when ownership is sufficient. `Shared<T>` adds overhead only where used. | High. Bump-pointer allocation is faster than malloc. Bulk deallocation is O(1). But values cannot be individually freed, so memory may be held longer than necessary. | Highest. Can match ownership performance for simple code and region performance for batch code. Overhead is the complexity of two systems. | Medium-high. Elided RC matches ownership. Non-elided RC adds per-operation cost. Atomic RC for shared data adds synchronization. Ceiling depends on elision rate. |
| **Worst-case latency** | Excellent. Deterministic drop at scope end. Destructor cascades are the worst case (dropping a large tree), but these are bounded and predictable. | Excellent. Region free is O(1) for memory, O(n) for destructors. Destructor-free workloads have ideal worst-case. | Excellent. Inherits the better of both: ownership drops for simple values, region resets for batch data. | Good. Deterministic but destructor cascades from RC hitting zero can cause spikes. A large reference graph can trigger a cascade of decrements and frees. |
| **Code verbosity** | Low-moderate. Less verbose than Rust due to lifetime inference. `Shared<T>` is more verbose than GC but less verbose than Rust's `Rc<RefCell<T>>`. | Low. Region declarations are concise. No per-value annotations. Slight verbosity from explicit region parameters when passing allocations across function boundaries. | Low-moderate. Simple code is as concise as Profile A. Region blocks add a small amount of syntax for complex cases. | Lowest. No ownership annotations, no lifetime parameters, no region declarations. Code looks like a GC language but without GC. |
| **Graph/cyclic data** | Requires `Shared<T>` and `WeakRef<T>`. Functional but verbose. Developer must manually identify cycle-breaking points. | Excellent. All nodes in a region can freely reference each other. Cycles within a region are not a problem — the region frees everything at once. Cross-region cycles are forbidden by the compiler. | Excellent. Use a region for the graph structure. Best approach for graph-heavy code. | Requires `WeakRef<T>` to break cycles, same as Swift. Compiler warns on potential cycles. Simpler than Rust's `Rc<RefCell<T>>` equivalent. |
| **FFI compatibility** | Good. Owned values map directly to C pointers. Borrows map to C const/mutable pointers. `Shared<T>` must be pinned for FFI. | Moderate. Region-allocated values have non-standard lifetimes. Passing region-allocated data to C requires careful handling (the region must outlive the C function's use of the data). | Good. Owned values (used at FFI boundary) work like Profile A. Region values should not be passed to FFI directly. | Good. Reference-counted values can be pinned for FFI. The RC overhead is invisible to C code. Similar to Swift's approach, which has proven FFI track record. |
| **Compile time cost** | Moderate. Ownership checking and lifetime inference add compile-time work. Less than Rust (fewer lifetime parameters to resolve) but more than a simple type checker. | Low-moderate. Region checking is simpler than full ownership tracking. The main cost is verifying that cross-region references respect nesting. | Moderate-high. Must run both ownership checking and region analysis. The two systems interact, adding complexity. | Moderate. The elision analysis (dataflow analysis to determine where RC can be removed) is similar in cost to ownership inference. More optimizations = more compile time. |
| **Error message clarity** | Good. Errors are about "this value was already moved" or "cannot borrow as mutable while borrowed as immutable." Concrete and actionable. `Shared<T>` escape hatch means fewer "impossible" errors. | Good. Errors are about "this value belongs to region X which is freed before region Y." Region-based reasoning is spatial (which container) rather than temporal (which lifetime), which may be more intuitive. | Mixed. Simple errors are clear (ownership or region, not both). Errors at the ownership-region boundary may be confusing: "should this be owned or in a region?" | Excellent. Very few compile-time errors related to memory. The main error is "potential reference cycle" which is a warning, not a hard error. Most memory bugs manifest as runtime leaks, not compile errors. This is simpler for the developer but catches fewer bugs at compile time. |

---

## Implementation Strategy

### Shared Infrastructure

All four profiles are built on the same compiler frontend and share the following components:

1. **Shared parser and AST.** All profiles use the same syntax for non-memory-related constructs. The AST includes memory-related nodes (region declarations, ownership annotations) that are present for all profiles but ignored by profiles that do not use them.

2. **Shared type system.** The core type system (structs, enums, generics, traits/interfaces) is the same across all profiles. Each profile extends the type system with its memory-related types (`Shared<T>`, `WeakRef<T>`, region parameters, etc.).

3. **Shared backend (code generation).** All profiles target the same IR and backend. The memory strategy is lowered to explicit allocation/deallocation instructions in the IR. The backend does not know which strategy produced the IR.

4. **Shared standard library interface.** The standard library exposes the same API regardless of profile. Internal implementations differ (e.g., `[T]` uses ownership in Profile A, region allocation in Profile B, RC in the default CTRC) but the developer-facing API is identical.

5. **Shared test suite.** All benchmark programs are written once and compiled under all four profiles. Where syntax differences are unavoidable (e.g., region declarations in Profile B), we use conditional compilation or thin adapter layers.

### Parallel Development Plan

- **Weeks 1-2:** Build the shared infrastructure (parser, type system, IR, backend). Define the benchmark programs. Establish measurement infrastructure (automated benchmarking, statistical analysis, regression detection).
- **Weeks 3-4:** Implement Profile A (Ownership). Ownership checking is well-understood (borrow from Rust's algorithms). Run initial benchmarks.
- **Weeks 5-6:** Implement CTRC (the default). CTRC is well-understood (borrow from Swift's approach). Run initial benchmarks.
- **Weeks 7-8:** Implement Profile B (Regions). Regions require new compiler infrastructure (region inference, region parameter passing).
- **Weeks 9-10:** Implement Profile C (Hybrid). The hybrid profile requires integrating ownership and regions, which is the most complex implementation. Run benchmarks on all four.
- **Weeks 11-12:** Write and run full benchmark suite across all four profiles.
- **Week 13:** Analyze results. Identify winners, losers, and edge cases. Make decision.
- **Week 14:** Merge winning approach into main branch. Archive alternative profiles with full documentation of their strengths and weaknesses for future reference.

### Ensuring Fair Comparison

- **Same hardware.** All benchmarks run on the same machine, same OS, same kernel configuration, with CPU frequency scaling disabled and other processes minimized.
- **Same optimization level.** All profiles use the same backend optimization passes. Memory-strategy-specific optimizations (RC elision, region coalescing) are measured separately.
- **Same source code.** Benchmark programs are semantically identical across profiles. Syntactic differences are minimized and documented.
- **Blind analysis.** Benchmark results are analyzed by team members who did not implement the profile, to avoid bias.
- **Statistical rigor.** Each benchmark is run at least 1000 times. Results are reported with confidence intervals. Outliers are investigated, not discarded.

---

## Decision Framework

> **Planned — design decision, not shipping behavior.** "CTRC with auto-clone is the chosen default" records a *design* decision. It is not implemented: today's runtime uses plain runtime ARC + COW with no compile-time elision and no auto-clone. The benchmark suite, criteria, and development plan above describe intended work, not completed work.

**Decision: CTRC with auto-clone is the chosen default.** After evaluating all four strategies against the weighted criteria below, CTRC (Compile-Time Reference Counting) with auto-clone semantics was selected as Turbo's default memory model. It scored highest on developer experience (the top-weighted criterion tied with performance) and delivered acceptable performance through aggressive compile-time elision.

The other strategies are retained as opt-in performance profiles, available for specialized workloads where benchmarking shows the default is insufficient. They are stretch goals for the compiler team, not blocking requirements for Turbo's initial release.

### Weighted Criteria (Used to Select CTRC as Default)

| Criterion | Weight | Rationale |
|-----------|--------|-----------|
| **Performance (throughput)** | 25% | Must be competitive with Rust and C. More than 10% overhead on the throughput benchmarks is a red flag. More than 20% is disqualifying. |
| **Developer experience (cognitive load + verbosity + error messages)** | 25% | Our primary differentiator vs. Rust. If a profile is as hard to use as Rust, we have not solved the problem we set out to solve. This is weighted equally with performance because developer adoption determines the language's success. |
| **Worst-case latency (determinism)** | 15% | Our primary differentiator vs. GC languages. If a profile cannot deliver deterministic latency, it fails our core value proposition regardless of other merits. |
| **Compile time impact** | 15% | Compile times above 5 seconds for incremental builds impact developer flow. Above 15 seconds is disqualifying for the inner development loop. |
| **Generality** | 10% | How well the strategy handles diverse workloads — from systems programming to application development to real-time. A strategy that only works for one domain is less valuable. |
| **Implementation complexity** | 10% | Compiler complexity and maintenance burden. Complexity is a long-term cost that grows over the language's lifetime. Simpler implementations are easier to evolve and debug. |

### Why CTRC Won

1. **Lowest cognitive load.** Developers never think about ownership, lifetimes, or regions. Code looks like a GC language. The only new concept is `WeakRef<T>` for cycle breaking, which Swift developers already understand.

2. **High elision rates.** In typical application code (web servers, CLI tools, agent pipelines), 70-85% of reference count operations are elided at compile time. The remaining runtime RC cost is deterministic and predictable.

3. **Best alignment with the "JavaScript Promise."** The auto-clone + CTRC combination delivers exactly the developer experience described in the JavaScript Promise section: write code naturally, the compiler handles memory. No other strategy achieves this without annotations.

4. **Proven at scale.** Swift's ARC model (which CTRC extends with compile-time elision) powers iOS, macOS, and server-side Swift. The approach is battle-tested, not experimental.

### Performance Profile Roadmap (Stretch Goals)

| Profile | Priority | Target Use Case | Status |
|---------|----------|-----------------|--------|
| CTRC + Auto-Clone (default) | P0 -- ships at launch | All Turbo code | Chosen default |
| Ownership (Profile A) | P1 -- post-launch | FFI, embedded, performance-critical libraries | Stretch goal |
| Regions (Profile B) | P1 -- post-launch | Per-request servers, game frames, batch processing | Stretch goal |
| Hybrid (Profile C) | P2 -- future | Engine-level systems programming | Stretch goal |

### Revisit Clause

The decision is revisited after 6 months of production development. If CTRC reveals unforeseen problems (poor ergonomics in real-world code, performance cliffs in workloads we did not benchmark, unacceptable elision rates), we reserve the right to promote a performance profile to the default before the language reaches 1.0 stability.
