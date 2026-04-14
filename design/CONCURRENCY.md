# Concurrency Model

## Philosophy
Hybrid model combining the best approaches from Go, Rust, Elixir, Kotlin, and Swift. Not one concurrency model — a layered system where developers use the right tool for the right job.

## Feels Like JavaScript

If you've written async JavaScript, you already know how to write async Turbo. The mental model is the same: `async` functions, `await` calls, and streaming — no new concepts to learn.

**What's familiar:**
- `async/await` works exactly like JavaScript — no surprises
- `Future<T>` is Turbo's version of a JavaScript `Promise` — same idea, same ergonomics
- `all()` and `race()` work like `Promise.all()` and `Promise.race()`
- Top-level `await` works out of the box — just like modern JavaScript modules
- `for await...in` for async iteration — same syntax as JS async iterators

**What's better than JavaScript:**
- No colored function problem — the runtime handles sync/async boundaries smoothly
- Compile-time data race prevention — no subtle concurrency bugs at runtime
- Lightweight tasks (not OS threads) — spawn millions of concurrent operations
- Structured concurrency — no orphan tasks or forgotten promises

```
// If you can read this JavaScript...
// const data = await fetch("https://api.example.com/data")
// const json = await data.json()

// ...you can read this Turbo
let data = await fetch("https://api.example.com/data")?
let json = data.json<MyType>()?
```

### The Colored Function Solution

The "colored function" problem (coined by Bob Nystrom) plagues JavaScript, Rust, and Python: once a function is `async`, every caller must also become `async`, and you can't call an async function from sync code without ceremony. This splits your codebase into two incompatible worlds — "red" (async) and "blue" (sync) — and refactoring between them is painful.

Turbo solves this with a practical middle ground inspired by Go:

**1. You always know what's async.** The `await` keyword is required when calling an async function. Signatures are clear. There's no hidden suspension — you can read any Turbo function and know exactly where it might yield.

**2. But sync code CAN call async functions.** If you call an async function without `await`, the runtime transparently blocks the current task until the result is ready — exactly like Go's goroutine model. The compiler emits a warning (not an error), nudging you toward the better pattern.

**3. Making a function async never breaks callers.** You can add `async` to any function and existing call sites continue to work. Callers that don't `await` will block (with a warning), but they won't fail to compile. This means you can gradually adopt async across a codebase.

```
// Sync function calling async — the compiler warns but it works
fn load_config() -> Config ! Error {
  // This blocks the current task until the future resolves (like Go)
  // Compiler: "warning: blocking call to async function `fetch` in sync context.
  //            Consider adding `async` to `load_config`."
  let data = fetch("https://config.example.com/app.toml")  // no await needed
  parse(data)?
}

// Better: mark it async (zero-cost if awaited properly)
async fn load_config() -> Config ! Error {
  let data = await fetch("https://config.example.com/app.toml")?
  parse(data)?
}

// Callers of load_config work either way:
fn main() {
  let config = load_config()    // blocks (warning), but compiles and runs
}

async fn main() {
  let config = await load_config()?  // proper async — no warning, no blocking
}
```

**Why this works:**

- **Go proved it.** Go has no async/await at all — every function can call any other function, and the runtime handles the scheduling. Turbo adopts this philosophy while keeping `async`/`await` for explicitness and zero-cost performance.
- **Warnings, not errors.** The compiler tells you when you're blocking unnecessarily, so you can fix it when it matters (hot paths, servers) and ignore it when it doesn't (scripts, CLIs, tests).
- **No viral rewriting.** Adding `async` to a leaf function doesn't force you to rewrite every function in the call chain. You refactor upward at your own pace.
- **Tasks are cheap.** Because Turbo's tasks are lightweight (~2KB, M:N scheduled), blocking a task is not blocking an OS thread. A blocked task simply parks, and the scheduler runs other tasks on the same thread. This is why the Go-style bridging is practical rather than dangerous.

### Promise-Like Futures

`Future<T>` in Turbo works like `Promise<T>` in JavaScript. If you know Promises, you know Futures.

```
// Just like JavaScript Promises
let data = await fetch("https://api.example.com/data")?

// Promise.all equivalent — run tasks concurrently, wait for all
let [users, posts, comments] = await all([
  fetch_users(),
  fetch_posts(),
  fetch_comments()
])

// Promise.race equivalent — first one to finish wins
let fastest = await race([
  fetch_from_primary(),
  fetch_from_fallback()
])

// Timeout (like AbortController, but simpler)
let result = await timeout(fetch_data(), 5000)  // 5 second timeout

// Promise.allSettled equivalent — never fails, collects all results
let results = await all_settled([
  fetch_users(),
  fetch_posts(),
  fetch_comments()
])
for result in results {
  match result {
    ok(data) => process(data)
    err(e) => log("Failed: {e}")
  }
}
```

### Top-Level Await

Like modern JavaScript modules, `await` works at the top level in Turbo. No need for an `async main` wrapper — just write your code.

```
// main.tb — no async main needed, just start writing
let config = await load_config("config.toml")?
let db = await Database.connect(config.db_url)?
let server = Server.new(db)

print("Listening on port 3000...")
await server.listen(3000)
```

```
// script.tb — great for quick scripts too
let response = await fetch("https://api.example.com/users")?
let users = response.json<[User]>()?

for user in users {
  print("{user.name}: {user.email}")
}
```

## The Seven Layers

### 1. Lightweight Tasks (like Go goroutines)
- Millions of concurrent tasks, M:N scheduled onto OS threads
- Cheap to create (~2KB initial stack, growable)
- Work-stealing scheduler distributes across CPU cores
- No OS thread per task — multiplexed onto a thread pool

> **JS equivalent:** Like calling an `async` function that runs in the background. In JS you might do `const promise = fetchData(url)` and `await` it later. In Turbo, `spawn` is explicit about creating a separate concurrent task -- think of it as `new Worker()` but lightweight and cheap, or like `setTimeout(async () => ..., 0)` but actually concurrent.

```
// JS: const promise = fetchData(url)   -- runs concurrently when awaited
// Turbo: spawn explicitly creates a concurrent task
let handle = spawn async {
  await fetch_data(url)
}
let result = await handle  // like: const result = await promise
```

### 2. Structured Concurrency (like Kotlin/Swift)
- Parent-child task relationships
- Automatic cancellation propagation (cancel parent → cancels all children)
- No orphan tasks — every task has a scope
- Inspired by Kotlin coroutines structured concurrency and Swift's TaskGroup

> **JS equivalent:** JavaScript has no built-in structured concurrency. The closest is `Promise.all()`, but `Promise.all` doesn't cancel other promises when one fails. Turbo's `scope` is like `Promise.all` with automatic cleanup -- if any task fails, all sibling tasks are cancelled. No orphan promises, no forgotten `.catch()` handlers.

```
// JS:   const results = await Promise.all(urls.map(url => fetch(url)))
// Turbo: scope guarantees all tasks complete (or cancel) before continuing
async fn fetch_all(urls: [str]) -> [Data] ! Error {
  scope (s) => {
    let tasks = urls.map((url) => s.spawn(async { await fetch(url)? }))
    await tasks.collect()?
  }
  // All child tasks guaranteed done here, even on error
}
```

### 3. Channels (like Go)
- Typed, buffered/unbuffered
- `select` for multiplexing across channels
- Used for communication between tasks

> **JS equivalent:** Channels are like a typed, backpressure-aware version of `EventTarget` or `EventEmitter`. Think of `tx` (transmitter) as `emitter.emit()` and `rx` (receiver) as `emitter.on()` -- but with built-in buffering, async iteration, and type safety. The `select` statement is like `Promise.race()` but for channels -- whichever channel has data first wins.

```
let (tx, rx) = channel<Message>(buffer: 32)

// Sending -- like emitter.emit('message', data) but typed and buffered
spawn async {
  for i in 0..100 {
    await tx.send(Message { id: i, data: compute(i) })
  }
}

// Receiving -- like `for await (const event of eventTarget)` in JS
for await msg in rx {
  process(msg)
}

// Select across multiple channels -- like Promise.race() but for channels
// First channel with data ready wins
select {
  msg = rx1.recv() => handle_a(msg)
  msg = rx2.recv() => handle_b(msg)
  _ = timeout(5.seconds()) => handle_timeout()
}
```

### 4. Actors (like Elixir)
- Isolated stateful processes with message passing — a pure concurrency primitive with no AI/LLM involvement
- Each actor has its own state — no shared mutable state
- Supervision trees for fault tolerance (restart strategies)
- Actors are general-purpose concurrency constructs; the planned `turbo-agent` sidecar library can build supervision on top of them, but actors themselves are not tied to any agent keyword (the core language has none)
- Inspired by Erlang/OTP and Elixir GenServer

> **JS equivalent:** Think of actors as a `Worker` (or `ServiceWorker`) with its own private state that communicates only via messages. In JS, Web Workers can't share memory directly -- they use `postMessage()`. Actors work the same way, but with type-safe messages, automatic restart on failure, and no serialization overhead. The supervision tree is like a process manager (e.g., PM2) built into the language -- if an actor crashes, it restarts automatically.

```
// Like a Web Worker with private state and typed messages
actor Counter {
  state: u64 = 0

  fn increment(self) {
    self.state += 1
  }

  fn get(self) -> u64 {
    self.state
  }

  // Built-in crash recovery -- like PM2 auto-restart but in the language
  fn on_error(self, error: Error) -> ActorAction {
    .restart(backoff: exponential(base: 100.ms))
  }
}

// Supervision tree -- like a built-in process manager
let supervisor = Supervisor.new(strategy: .one_for_one)
  .child(Counter.new())
  .child(Logger.new())
  .child(CacheActor.new())
```

### 5. Async/Await Syntax
- Familiar from JS/C#/Rust
- Backed by the lightweight task runtime (not OS threads)
- Zero-cost when not used

```
async fn fetch_user(id: u64) -> User ! Error {
  let resp = await http.get("/users/{id}")?
  let user = resp.json<User>()?
  ok(user)
}
```

### 6. Fearless Concurrency (like Rust)
- Ownership system prevents data races at compile time
- `Send` trait: type can be transferred across task boundaries
- `Sync` trait: type can be shared (immutable reference) across tasks
- Mutable access requires exclusive ownership or synchronization primitives

> **JS equivalent:** JavaScript avoids data races by being single-threaded. Turbo gives you real multi-threading but the **compiler** prevents data races at compile time -- you get the safety of single-threaded JS with the performance of multi-threaded code. `Mutex` is like a lock that ensures only one task can modify shared data at a time -- think of it as `Atomics.wait()` but automatic and type-safe.

```
// Like SharedArrayBuffer + Atomics, but type-safe and automatic
let shared = Shared.new(Mutex.new({}))

spawn async {
  let mut map = await shared.lock()  // only one task can access at a time
  map.insert("key", "value")
}
// Compiler ensures no unsynchronized access -- data races caught at compile time
```

### 7. Async Streams
- First-class `Stream<T>` type for streaming data
- Critical for LLM token streaming, real-time data, SSE
- Composable with standard iterator operations

> **JS equivalent:** Streams are like `ReadableStream` or `AsyncIterator` in JavaScript. If you have used `for await (const chunk of response.body)` or Server-Sent Events (`EventSource`), you already understand the concept. Turbo's `Stream<T>` is typed, composable, and works with `for await...in` -- same syntax as JS async iteration.

```
// Like an async generator function in JS:
// async function* tokenStream(prompt) { for await (const chunk of ...) yield chunk }
async fn token_stream(prompt: str) -> Stream<Token> {
  let response = await llm.complete(prompt)
  for await chunk in response.stream() {
    yield Token.parse(chunk)
  }
}

// Consume a stream -- same as JS: for await (const token of tokenStream("Hello"))
for await token in token_stream("Hello") {
  print(token.text)
}

// Transform streams (arrow syntax preferred) -- like .pipeThrough() in JS Streams API
let uppercase_stream = token_stream("Hello")
  |> map_stream((t) => t.text.to_upper())
  |> filter_stream((t) => !t.is_empty())
```

## Synchronization Primitives

> **JS equivalent:** JavaScript is single-threaded, so you rarely need synchronization primitives. In Turbo, these are the building blocks for safe multi-threaded code. Think of `Mutex` as a lock around shared state, `Semaphore` as a rate limiter (like limiting concurrent `fetch()` calls), and `Once` as a lazy singleton pattern.

| Primitive | Use Case | JS Mental Model |
|-----------|----------|-----------------|
| `Mutex<T>` | Exclusive mutable access to shared data | Like a lock around `SharedArrayBuffer` |
| `RwLock<T>` | Multiple readers OR single writer | Like a read-write lock -- many can read, one can write |
| `Atomic<T>` | Lock-free atomic operations for simple types | Like `Atomics` on `SharedArrayBuffer` |
| `Barrier` | Wait for N tasks to reach a point | Like `Promise.all()` but for synchronization points |
| `Semaphore` | Limit concurrent access to a resource | Like a concurrency limiter (e.g., `p-limit` npm package) |
| `Once<T>` | Lazy initialization (thread-safe) | Like a lazy singleton -- initialize once, use everywhere |

## Runtime Variants

### Tokio-style (default)
- Work-stealing thread pool
- M:N cooperative scheduling
- Best for: Servers, APIs, application workloads, general-purpose async

### Actor-based
- Elixir-style isolated processes
- Preemptive per-actor (can't starve other actors)
- Best for: Fault-tolerant distributed systems

### Minimal
- Single-threaded event loop
- Cooperative scheduling
- Best for: Embedded, WASM, simple CLI tools, constrained environments

```
// Select runtime at build time
// turbolang build --runtime=default    (work-stealing)
// turbolang build --runtime=actor      (Erlang-style)
// turbolang build --runtime=minimal    (single-threaded)
```

## Cancellation

> **JS equivalent:** Cancellation in Turbo works like `AbortController` in JavaScript. Create a cancellation token, pass it to the task, and call `cancel()` to stop it -- same pattern as `AbortController.abort()`. Timeouts work like `Promise.race([fetch(url), timeout(5000)])` but built into the language.

### Cooperative Cancellation
```
// Like AbortController in JS:
// const controller = new AbortController()
// fetch(url, { signal: controller.signal })
// controller.abort()

async fn long_task(cancel: CancelToken) -> Data ! Cancelled {
  for chunk in data.chunks(1000) {
    cancel.check()?  // Like checking signal.aborted in JS
    process(chunk)
  }
}

let (handle, cancel) = spawn_cancellable(async { long_task() })
// Later...
cancel.cancel()  // Like controller.abort() -- gracefully stops the task
```

### Timeout
```
// Like: Promise.race([slowOperation(), new Promise((_, reject) => setTimeout(reject, 5000))])
// But cleaner:
let result = timeout(5.seconds()) {
  await slow_operation()
}
match result {
  ok(data) => use(data)
  err(Timeout) => fallback()
}
```

## Data Race Prevention
- Compiler enforces Send/Sync boundaries at compile time
- Immutable data is freely shareable (Sync by default)
- Mutable data requires explicit synchronization
- Actor state is never shared — message passing only
- Channel communication is always safe by design

## Comparison With Other Languages

| Feature | Turbo | Go | Rust | Elixir | Kotlin | Swift |
|---------|------|-----|------|--------|--------|-------|
| Lightweight tasks | Yes | Goroutines | Tokio tasks | Processes | Coroutines | Tasks |
| Structured concurrency | Yes | No | No (library) | Supervised | Yes | Yes |
| Channels | Yes | Yes | Yes (library) | Mailboxes | Yes | No |
| Actors | Yes | No | No (library) | Yes (core) | No | Yes |
| Data race prevention | Compile-time | Runtime (race detector) | Compile-time | By design (immutable) | Partial | Partial (Sendable) |
| Async streams | Yes | No | Yes (library) | Yes (GenStage) | Yes (Flow) | Yes (AsyncSequence) |
| Supervision trees | Yes | No | No | Yes (OTP) | No | No |

## Performance Targets
- Task spawn overhead: <1us (comparable to Go goroutines)
- Channel send/recv: <100ns for unbuffered
- Context switch: <200ns between tasks
- Scale to 1M+ concurrent tasks on modern hardware
