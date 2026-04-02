# TurboExchange: Real-Time Order Matching Engine

## What This Is

TurboExchange is a real-time order matching engine built entirely in Turbo. It demonstrates the language's systems programming capabilities -- deterministic memory, sub-microsecond latency, actor isolation, and zero-allocation hot paths -- in a domain where these properties are not optional but existential. Financial exchanges have the strictest latency and correctness requirements of any software system. If Turbo can build one, it can build anything.

## Why Turbo Excels for Real-Time Systems

### The Core Problem

Real-time systems -- trading engines, audio processors, game engines, robotics controllers -- have a fundamental constraint: **every operation must complete within a hard deadline**. A trading engine that pauses for 50ms loses money. An audio processor that pauses for 2ms produces audible glitches. A game engine that pauses for 16ms drops a frame.

This deadline constraint disqualifies any language with non-deterministic memory management. Garbage collectors, by definition, introduce unpredictable pauses. The question is not whether GC pauses happen, but when and for how long -- and in real-time systems, "unpredictable" is unacceptable.

### Turbo's Answer: Deterministic Memory Without Friction

Turbo's memory model is a **ladder of progressive control**. TurboExchange uses three levels simultaneously:

**Level 0 (Auto-Clone) -- Configuration, API handlers, metrics dashboard**

The REST API in `api.tb`, the configuration loader in `main.tb`, and the metrics dashboard in `metrics.tb` all use Level 0 -- pure JavaScript-style code with no memory annotations. The compiler manages everything transparently. This is 70% of the codebase, and it reads like TypeScript.

```
// Level 0: Just write code. No memory thinking.
router.get("/book/:symbol", async (req) => {
  let symbol = req.params.symbol
  let depth = req.query.get("depth")?.parse<usize>() ?? 10
  match exchange.read().get_book_snapshot(symbol, depth: depth) {
    ok(snapshot) => Response.json(snapshot)
    err(e) => Response.json({ error: e.message() }, status: 404)
  }
})
```

**Level 2 (Regions) -- The matching hot path**

The order matching engine in `order_book.tb` and `matching.tb` uses `region {}` blocks for the critical matching loop. All scratch allocations during matching -- temporary arrays of filled order indices, empty price level lists, intermediate data -- are allocated inside a region and freed in a single pointer reset when the block ends. Zero individual frees. Zero allocator calls. O(1) deallocation.

```
// Level 2: Region-based allocation for the hot path.
// Everything inside is freed at once -- one pointer reset.
region match_scratch {
  let mut empty_prices: [Decimal] = []
  let mut filled_indices: [usize] = []

  for (price, ref mut queue) in opposite_side.iter_mut() {
    // ... matching logic, all scratch in match_scratch ...
  }

  for price in empty_prices {
    opposite_side.remove(price)
  }
}
// match_scratch freed here: ALL temporaries gone in O(1)
```

**`@inline` on the hot path**

The `execute_fill` function -- the single most performance-critical function in the system -- is annotated with `@inline`. Combined with the release profile's LTO (link-time optimization), this ensures the fill logic is inlined directly into the matching loop, eliminating function call overhead entirely.

```
@inline
fn execute_fill(
  mut self,
  aggressor: &mut Order,
  resting: &mut Order,
  price: Decimal,
  quantity: u64,
) -> Trade {
  aggressor.fill(quantity)
  resting.fill(quantity)
  // ... trade construction ...
}
```

**Lock-free metrics with Atomic<T>**

The metrics system in `metrics.tb` uses `Atomic<u64>` for all counters and gauges. Recording an order or a trade is a single atomic fetch-and-add -- no locks, no contention, no blocking. The latency histogram uses a pre-allocated circular buffer with atomic head pointer, so recording a latency sample is O(1) with zero allocation.

### Actor-Based Architecture for Isolation

TurboExchange uses Turbo's actor system for fault tolerance and isolation:

- **MatchingEngine** is an actor. Its state (order books, trade IDs, metrics) is never shared. All access is through message passing via channels. If the engine encounters an error, the supervision tree restarts it automatically with exponential backoff.

- **ClientActor** -- each WebSocket client connection is an isolated actor. A slow client, a crashed connection, or a malicious client cannot affect other clients or the matching engine. The supervisor cleans up dead actors automatically.

- **Backpressure** -- each client actor has a bounded message queue. If a client falls behind (slow network, overwhelmed consumer), messages are dropped at the client level. The matching engine and other clients are never blocked.

This is the Erlang/Elixir supervision model, built into the language as a first-class primitive.

### Fixed-Point Arithmetic for Financial Precision

TurboExchange uses a custom `Decimal` type backed by a scaled `i64` (8 decimal places of precision). No floating point is used anywhere in price calculations. This eliminates the entire class of floating-point rounding errors that plague financial systems.

```
// No floating point anywhere near money.
pub struct Decimal {
  raw: i64   // value * 10^8
}
// 150.25 is stored as 15_025_000_000
// Addition, subtraction, comparison: all exact integer arithmetic
```

## The Memory Ladder in Action

This codebase demonstrates all four levels of Turbo's memory model working together:

| Component | Memory Level | Why |
|-----------|-------------|-----|
| `main.tb` -- config, startup | Level 0 (auto-clone) | Config is loaded once. No performance sensitivity. |
| `api.tb` -- REST handlers | Level 0 (auto-clone) | Request handlers run at HTTP timescales (milliseconds). Auto-clone is invisible. |
| `feed.tb` -- WebSocket server | Level 0 (auto-clone) | Network I/O dominates. Memory management is not the bottleneck. |
| `metrics.tb` -- counters | Lock-free atomics | Recording metrics must not block the matching engine. |
| `metrics.tb` -- percentiles | Level 2 (region) | Sorting a copy of the latency buffer for percentiles. Freed immediately. |
| `order_book.tb` -- matching | Level 2 (region) | The hot path. All scratch data freed in O(1) at block end. |
| `order_book.tb` -- execute_fill | `@inline` + zero-alloc | The innermost loop. No allocations. Inlined for zero call overhead. |

This is the Turbo promise: **the same language, the same project, seamlessly spanning from "write it like JavaScript" to "control every allocation."** A developer building the REST API never touches a region. A developer optimizing the matching engine uses regions and `@inline` without rewriting the API layer.

## Comparison: What This Looks Like in Other Languages

### C++ -- The Current Industry Standard

Most production trading engines are written in C++. Here is what that entails:

- **Manual memory management everywhere.** Every `new` requires a `delete`. Custom allocators for the hot path. Memory pools for order objects. Arena allocators hand-rolled per project.
- **Undefined behavior is a constant risk.** Use-after-free, buffer overflows, dangling pointers -- all of these produce silent data corruption, not compile errors. In a trading engine, silent corruption means wrong trades.
- **No actor model.** Concurrency is raw threads + mutexes + condition variables. Deadlocks are debugged at 3am during market hours.
- **Build system complexity.** CMake, Bazel, or proprietary build systems. No standard package manager. Dependency management is manual.

Turbo gives you the same performance ceiling (regions compile to arena allocators, `@inline` compiles to forced inlining) without the undefined behavior, without manual memory management, and with actors and channels built in.

### Rust -- Safety Without Ergonomics

Rust would be the natural alternative. Here is what you'd face:

- **Borrow checker friction in the order book.** The matching loop mutates the order book while iterating over price levels and reading from order queues. In Rust, this requires splitting borrows, unsafe blocks, or restructuring the algorithm to satisfy the borrow checker. In Turbo, `ref mut` and regions handle this naturally.
- **No actor system built in.** You'd pull in `actix` or `tokio` actors, each with different APIs and guarantees. Turbo's `actor` keyword is a first-class language primitive with supervision built in.
- **Lifetime annotations at API boundaries.** The `OrderBook` holding references to `Order` objects, which reference `Decimal` values... in Rust, this cascade of lifetimes would require explicit annotations. In Turbo, the compiler infers them.
- **Verbose error handling.** Rust's `Result<T, E>` requires explicit `.unwrap()` or `?` everywhere. Turbo's `T ! E` is the same idea with cleaner syntax.

Rust can build this system. It would take longer, require more expertise, and produce more verbose code -- but it would be equally safe and performant.

### Java/Go -- GC Pauses Kill Latency

Both Java and Go use garbage collectors. Here is why that is disqualifying:

- **Java's G1/ZGC.** Even with ZGC's sub-millisecond pause targets, worst-case pauses of 1-5ms occur under allocation pressure. A matching engine processing 100k orders/second allocates heavily. GC pauses during matching mean missed price levels, stale fills, and latency spikes.
- **Go's concurrent collector.** Go's GC is optimized for low-latency servers, but it still introduces stop-the-world phases. Under sustained allocation pressure (which a matching engine produces), Go's GC can pause for 1-10ms. The standard library's map and slice operations allocate, making zero-allocation hot paths nearly impossible without `unsafe`.
- **Neither has regions.** Java's `MemorySegment` (Panama) is an afterthought. Go has no equivalent. Both languages force you into the GC for all allocations.

### JavaScript/Python -- Not Even Close

- **JavaScript** has V8's GC with major collection pauses of 10-50ms. The single-threaded event loop means a GC pause blocks all order processing. There is no way to write a zero-allocation hot path in JavaScript.
- **Python** has the GIL (Global Interpreter Lock), which serializes all CPU-bound work to a single core. A matching engine needs to process orders as fast as possible on dedicated cores. Python's GIL makes this impossible. Additionally, Python is 50-100x slower than native code for compute-bound work.

## Key Turbo Features Demonstrated

| Feature | Where Used | What It Does |
|---------|-----------|--------------|
| `region {}` | `order_book.tb`, `matching.tb`, `metrics.tb` | Zero-allocation matching. All scratch data freed in O(1). |
| `@inline` | `order_book.tb` (`execute_fill`, `best_bid`, etc.) | Eliminates function call overhead on the hot path. |
| `actor` | `matching.tb` (MatchingEngine), `feed.tb` (ClientActor) | Isolated stateful processes with supervision. No shared mutable state. |
| `channel<T>` | `matching.tb`, `feed.tb` | Typed, buffered communication between actors. Backpressure-aware. |
| `select {}` | `feed.tb` (ClientActor) | Multiplexing across WebSocket messages, feed data, and heartbeat timers. |
| `Atomic<T>` | `metrics.tb` | Lock-free counters and gauges. Zero contention. |
| `Shared<T>` | `main.tb`, `api.tb` | Thread-safe shared state with read/write locks. |
| `@no_clone` | `order_book.tb` (OrderBook) | Prevents accidental copying of the entire order book. |
| `@perf` | `latency_test.tb` | Performance tests with max_time and max_memory constraints. |
| `@regression` | `latency_test.tb` | Latency regression detection against baseline measurements. |
| `@stress` | `stress_test.tb` | Sustained concurrent load testing with configurable duration and parallelism. |
| `memory.snapshot()` | `latency_test.tb` | Heap snapshot for memory leak detection. |
| `Decimal` (fixed-point) | `models.tb` | No floating-point for money. Exact integer arithmetic. |
| `type` (algebraic data types) | `models.tb` | `OrderType`, `Side`, `OrderStatus`, `EngineCommand`, `EngineEvent`. |
| `defer` | `main.tb` | Graceful shutdown: print final metrics, stop engine, stop server. |
| `for await` | `feed.tb`, `main.tb` | Async iteration over channels and event streams. |
| `guard let` | `matching.tb`, `api.tb`, `order_book.tb` | Early return on validation failure. Clean control flow. |

## Performance Targets

| Metric | Target | How Verified |
|--------|--------|-------------|
| Simple cross latency (P50) | < 10us | `latency_test.tb::test_simple_cross_latency` |
| Simple cross latency (P99) | < 50us | `latency_test.tb::test_simple_cross_latency` |
| Throughput | > 100k orders/sec | `latency_test.tb::test_100k_orders_per_second_throughput` |
| Memory under sustained load | Flat (no growth) | `latency_test.tb::test_memory_flat_under_sustained_load` |
| 1M order correctness | All invariants hold | `stress_test.tb::test_one_million_orders_correctness` |
| Concurrent P99 | < 1ms | `stress_test.tb::test_concurrent_order_submission` |

## Running

```bash
# Build optimized release binary
turbo build --release

# Run the exchange
turbo run src/main.tb

# Run all tests
turbo test

# Run performance tests only
turbo test --filter @perf

# Run stress tests only
turbo test --filter @stress

# Build with memory report to see allocation analysis
turbo build --release --memory-report
```

## Architecture

```
                    REST API (:8080)
                         |
                    +-----------+
                    |  api.tb   |
                    +-----------+
                         |
                  SubmitOrder / Cancel
                         |
                    +-----------+
                    | matching  |  <-- Actor: isolated state
                    |   .tb     |      processes commands sequentially
                    +-----------+      uses region {} for zero-alloc matching
                         |
                   EngineEvent channel
                         |
              +----------+----------+
              |                     |
        +-----------+         +-----------+
        |  feed.tb  |         | metrics   |
        | WebSocket |         |   .tb     |
        |  server   |         | Atomics   |
        +-----------+         +-----------+
              |                     |
     ClientActor per conn     OTLP export
     (backpressure, isolation)
```

Each component runs independently. The matching engine is the only stateful hot path. Everything else (API, feed, metrics) operates at I/O timescales where Level 0 auto-clone is invisible. The matching engine uses Level 2 regions for its inner loop. This is the memory ladder in practice: use the simplest level that meets your performance requirements, and never pay for complexity you don't need.
