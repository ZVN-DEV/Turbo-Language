# Toolchain — Ship Everything on Day One

## Philosophy
Lessons from research: Cargo, Go's toolchain, and Deno are the gold standard. Ship the full toolchain. No third-party tools needed to be productive on day one. One binary, all tools.

## The Complete Toolchain

### Package Manager: `turbo add`, `turbo remove`, `turbo publish`
- Centralized registry: `packages.turbo.dev` (like crates.io, npm)
- Lockfile for reproducible builds (`turbo.lock`)
- Workspace support (monorepo with multiple packages)
- Semantic versioning enforced
- Dependency resolution with conflict detection
- `turbo add serde` — add a dependency
- `turbo remove serde` — remove
- `turbo publish` — publish to registry
- `turbo update` — update dependencies
- `turbo audit` — check for known vulnerabilities
- Inspired by: Cargo, Go modules, Deno

### Build System: `turbo build`, `turbo run`, `turbo watch`
- Zero-config for simple projects (convention over configuration)
- `turbo.toml` config file (like Cargo.toml)
- `turbo build` — build the project
- `turbo build --release` — optimized build
- `turbo build --target wasm32-wasi` — cross-compile
- `turbo run` — build and run
- `turbo run --watch` — watch for changes, auto-rebuild
- `turbo clean` — clean build artifacts
- Build scripts for code generation / FFI binding generation
- Inspired by: Cargo, Go, Zig build system

### Testing Framework: `turbo test`

A full native app testing suite — not just unit tests. Built-in performance testing, memory analysis, CPU profiling, regression detection, stress testing, and native app diagnostics. No external dependencies. One command: `turbo test`.

#### CLI Overview

- `turbo test` — run all tests
- `turbo test module_name` — run tests in a module
- `turbo test --filter "pattern"` — filter tests by name
- `turbo test --parallel` — parallel execution (default)
- `turbo test --sequential` — sequential execution (useful for stateful tests)
- `turbo test --coverage` — code coverage report
- `turbo test --watch` — re-run on file changes
- `turbo test --perf` — run only performance-annotated tests
- `turbo test --stress` — run only stress tests
- `turbo test --update-snapshots` — accept snapshot changes
- Inspired by: Cargo test, Go test, Jest, Criterion.rs

---

#### Unit Testing

The foundation. Mark any function with `@test` and it becomes a test. Assertions, snapshots, mocks, parameterized tests, and property-based testing are all built in.

```
import { mock } from "turbo/test"

// Basic test
@test
fn test_addition() {
  assert_eq(add(2, 3), 5)
}

// Assertions
@test
fn test_assertions() {
  assert(user.is_active())
  assert_eq(result, expected)
  assert_ne(a, b)
  assert_matches(shape, Shape.Circle(_))

  // Test that a function throws a specific error
  assert_throws<ParseError>(() => {
    parse_int("not a number")
  })
}

// Snapshot testing — auto-generates and compares against saved output
@test
fn test_serialization() {
  let user = User { name: "Alice", age: 30, role: .admin }
  assert_snapshot(json.stringify(user))
  // First run: saves snapshot to __snapshots__/
  // Subsequent runs: compares against saved snapshot
  // turbo test --update-snapshots to accept changes
}

// Mocking via traits
@test
fn test_with_mock_db() {
  let db = mock(Database, {
    find_user: (id) => User { name: "Test", age: 25 },
    save_user: (user) => ok(()),
  })

  let service = UserService.new(db: db)
  let user = service.get_user(42)
  assert_eq(user.name, "Test")
  assert_eq(db.call_count("find_user"), 1)
}

// Property-based testing — generate random inputs, find edge cases automatically
@property
fn prop_sort_preserves_length(items: [i32]) {
  let sorted = items.sorted()
  assert_eq(sorted.len(), items.len())
}

@property
fn prop_reverse_twice_is_identity(s: str) {
  assert_eq(s.reverse().reverse(), s)
}

// Parameterized tests — run the same test with different inputs
@test_case(1, 1, 2)
@test_case(0, 0, 0)
@test_case(-1, 1, 0)
fn test_add(a: i32, b: i32, expected: i32) {
  assert_eq(add(a, b), expected)
}
```

---

#### Performance Testing

Annotate any test with `@perf(...)` to set hard limits on memory, time, and CPU usage. The test **fails** if any limit is exceeded. This turns performance from a "we'll check it later" concern into a first-class CI gate.

```
@test
@perf(max_memory: 50.mb(), max_time: 100.ms(), max_cpu: 80.percent())
fn test_data_processing_performance() {
  let data = generate_test_data(100_000)
  let result = process(data)
  assert_eq(result.len(), 100_000)
}

// The test FAILS if:
// - Peak memory exceeds 50 MB
// - Execution time exceeds 100ms
// - CPU usage exceeds 80%
```

All three constraints are optional — use only the ones you care about:

```
@test
@perf(max_time: 200.ms())
fn test_fast_enough() {
  let result = expensive_computation()
  assert(result.is_valid())
}

@test
@perf(max_memory: 10.mb(), max_time: 50.ms())
fn test_memory_bounded_parse() {
  let config = parse_config(large_input)?
  assert_eq(config.sections.len(), 42)
}
```

---

#### Memory Testing

Test for memory leaks, allocation efficiency, and auto-clone behavior. The `memory` module provides `snapshot()` for before/after comparisons and `track()` for wrapping a closure with full allocation tracking.

```
@test
fn test_no_memory_leaks() {
  let snapshot_before = memory.snapshot()

  for i in 0..1000 {
    let obj = HeavyObject.new(i)
    process(obj)
  }
  // All HeavyObjects should be freed by now

  let snapshot_after = memory.snapshot()
  assert(snapshot_after.allocated - snapshot_before.allocated < 1.kb(),
    "Memory leak detected: {snapshot_after.allocated - snapshot_before.allocated} bytes retained"
  )
}
```

For more detailed analysis, use `memory.track()` to capture peak usage, total allocations, and auto-clone counts:

```
@test
fn test_memory_efficiency() {
  let stats = memory.track(() => {
    let users = load_users(10_000)
    process_all(users)
  })

  assert(stats.peak_memory < 100.mb(), "Peak memory too high: {stats.peak_memory}")
  assert(stats.total_allocations < 50_000, "Too many allocations: {stats.total_allocations}")
  assert(stats.auto_clones < 100, "Too many auto-clones: {stats.auto_clones}")
}
```

The `MemoryStats` struct returned by `memory.track()`:

```
struct MemoryStats {
  peak_memory: usize        // high-water mark during tracked block
  total_allocations: u64    // number of heap allocations
  total_deallocations: u64  // number of heap deallocations
  bytes_allocated: usize    // total bytes allocated
  bytes_freed: usize        // total bytes freed
  auto_clones: u64          // number of compiler-inserted clones
  leaked: usize             // bytes still allocated at end of block
}
```

---

#### CPU Profiling in Tests

Wrap async or synchronous code with `cpu.track()` to capture user CPU time, context switches, and cache performance. Useful for catching regressions in hot paths and validating concurrency behavior.

```
@test
fn test_cpu_efficiency() {
  let profile = cpu.track(async () => {
    await handle_requests(generate_load(1000))
  })

  assert(profile.user_time < 500.ms(), "CPU time too high")
  assert(profile.context_switches < 100, "Too many context switches")
  assert(profile.cache_misses < 1_000_000, "Poor cache locality")
}
```

The `CpuProfile` struct returned by `cpu.track()`:

```
struct CpuProfile {
  user_time: Duration           // CPU time in user space
  system_time: Duration         // CPU time in kernel space
  wall_time: Duration           // wall-clock time
  context_switches: u64         // voluntary + involuntary switches
  voluntary_switches: u64       // yielded CPU voluntarily (e.g., await)
  involuntary_switches: u64     // preempted by scheduler
  cache_misses: u64             // L1/L2/L3 cache misses
  cache_references: u64         // total cache references
  instructions: u64             // total instructions retired
  peak_threads: u32             // max concurrent threads observed
}
```

---

#### Regression Testing

Compare test performance against a baseline branch. The `@regression` decorator records timing via `bench.measure()` and automatically compares against the saved baseline. Tests fail if performance regresses beyond the threshold.

```
@test
@regression(baseline: "main", threshold: 5.percent())
fn test_sort_performance() {
  let data = generate_random_vec(100_000)
  bench.measure(() => {
    data.clone().sort()
  })
  // Automatically compares against baseline branch
  // Fails if performance regresses more than 5%
}
```

Use `turbo test --save-baseline` to record a new baseline, and `turbo test --compare <branch>` to compare against any branch:

```
@test
@regression(baseline: "release-v2", threshold: 10.percent())
fn test_json_parse_regression() {
  let input = load_fixture("large-payload.json")
  bench.measure(() => {
    json.parse<[Record]>(input)
  })
}
```

Regression results include statistical detail — mean, median, P95, standard deviation — so a single noisy run does not cause a false failure. The default sample size is 100 iterations with outlier removal.

---

#### Stress Testing

Test your application under sustained concurrent load. The `@stress` decorator configures duration and concurrency; the framework spawns the specified number of concurrent clients and runs them for the configured duration, then produces a full latency/throughput/error report.

```
@test
@stress(duration: 30.seconds(), concurrency: 100)
fn test_server_under_load() {
  let server = TestServer.start(app)

  // Framework spawns 100 concurrent clients for 30 seconds
  stress.run(async () => {
    let resp = await http.get(server.url("/api/users"))
    assert_eq(resp.status, 200)
  })

  let report = stress.report()
  assert(report.p99_latency < 50.ms(), "P99 too high: {report.p99_latency}")
  assert(report.error_rate < 0.01, "Error rate too high: {report.error_rate}")
  assert(report.throughput > 1000.rps(), "Throughput too low: {report.throughput}")
}
```

The `StressReport` struct:

```
struct StressReport {
  total_requests: u64
  successful: u64
  failed: u64
  error_rate: f64             // failed / total_requests
  throughput: f64             // requests per second (use .rps() for comparison)
  mean_latency: Duration
  median_latency: Duration
  p90_latency: Duration
  p95_latency: Duration
  p99_latency: Duration
  max_latency: Duration
  errors_by_type: {str: u64} // error message -> count
}
```

Stress tests are excluded from normal `turbo test` runs. Use `turbo test --stress` to run them explicitly, or `turbo test --all` to include everything.

---

#### Test Output Format

`turbo test` produces clear, information-dense output. Performance tests show resource usage inline. Failures show exactly which constraint was violated.

```
$ turbo test

Running 24 tests...

  ✓ test_user_creation (2ms)
  ✓ test_authentication (15ms)
  ✓ test_data_processing_performance (45ms, peak: 32MB, CPU: 45%)
  ✓ test_no_memory_leaks (120ms, leaked: 0 bytes)
  ✓ test_sort_performance (89ms, baseline: 85ms, delta: +4.7%)
  ✗ test_server_under_load FAILED
    → P99 latency: 67ms (limit: 50ms)
    → Error rate: 0.003 (limit: 0.01) ✓
    → Throughput: 1,245 rps (limit: 1,000) ✓

  23 passed, 1 failed (2.3s)

  Memory: peak 128MB, 0 leaks detected
  CPU: avg 62%, peak 89%
  Auto-clones: 47 (0 in hot paths)
```

For CI pipelines, use `turbo test --output json` for machine-readable output, or `turbo test --output markdown` for PR comments.

---

#### Native App Test Utilities

Real applications need more than assertion libraries. Turbo ships test utilities for file system sandboxing, network mocking, process spawning, and environment isolation — all built in, no external packages.

##### File System Sandboxing

`TestFS.sandbox()` creates an isolated temporary directory that is automatically cleaned up when the test ends. Tests never touch the real file system.

```
@test
fn test_file_operations() {
  let sandbox = TestFS.sandbox()  // temp directory, auto-cleaned

  sandbox.write("config.toml", "[app]\nname = \"test\"")
  let config = load_config(sandbox.path("config.toml"))
  assert_eq(config.name, "test")
}
// sandbox auto-deleted after test
```

##### Network Mocking

`MockServer` spins up a local HTTP server with programmable routes. No real network calls, deterministic responses, full request recording.

```
@test
fn test_api_client() {
  let mock_server = MockServer.start()
  mock_server.on("GET", "/users", Response.json([
    { id: 1, name: "Alice" }
  ]))

  let client = ApiClient.new(base_url: mock_server.url())
  let users = await client.get_users()
  assert_eq(users.len(), 1)
  assert_eq(users[0].name, "Alice")

  // Verify the request was made
  assert_eq(mock_server.requests().len(), 1)
  assert_eq(mock_server.requests()[0].path, "/users")
}
```

##### Process Testing (for CLI apps)

`TestProcess.run()` spawns a child process and captures its exit code, stdout, and stderr. Test your CLI tools end-to-end.

```
@test
fn test_cli_output() {
  let result = TestProcess.run("turbo", ["run", "my-app.tb", "--verbose"])
  assert_eq(result.exit_code, 0)
  assert(result.stdout.contains("Success"))
  assert(result.stderr.is_empty())
}

@test
fn test_cli_error_handling() {
  let result = TestProcess.run("turbo", ["run", "nonexistent.tb"])
  assert_ne(result.exit_code, 0)
  assert(result.stderr.contains("file not found"))
}
```

##### Environment Variable Isolation

`TestEnv.with()` sets environment variables for the duration of a closure, then restores the original environment. Tests never leak state to each other.

```
@test
fn test_env_config() {
  TestEnv.with({ "DATABASE_URL": "sqlite://test.db", "LOG_LEVEL": "debug" }, () => {
    let config = Config.from_env()
    assert_eq(config.db_url, "sqlite://test.db")
    assert_eq(config.log_level, .debug)
  })
  // Original environment restored here
}
```

##### Time Mocking

`TestClock` lets you control time in tests — freeze it, advance it manually, or set it to a specific instant. Deterministic tests for time-dependent logic.

```
@test
fn test_cache_expiry() {
  let clock = TestClock.frozen(DateTime.parse("2026-01-01T00:00:00Z")?)

  let cache = Cache.new(ttl: 5.minutes(), clock: clock)
  cache.set("key", "value")

  assert_eq(cache.get("key"), some("value"))

  clock.advance(6.minutes())
  assert_eq(cache.get("key"), none)  // expired
}
```

---

#### Summary of Test Decorators

| Decorator | Purpose | Example |
|-----------|---------|---------|
| `@test` | Mark a function as a test | `@test fn test_foo() { ... }` |
| `@test_case(...)` | Parameterized test inputs | `@test_case(1, 2, 3)` |
| `@property` | Property-based (random input) testing | `@property fn prop_foo(x: i32) { ... }` |
| `@perf(...)` | Performance limits (memory, time, CPU) | `@perf(max_time: 100.ms())` |
| `@regression(...)` | Compare against baseline branch | `@regression(baseline: "main", threshold: 5.percent())` |
| `@stress(...)` | Sustained concurrent load testing | `@stress(duration: 30.seconds(), concurrency: 100)` |

### Formatter: `turbo fmt`
- One style, no configuration (like gofmt)
- Deterministic output — same code always formats the same way
- Fast enough to run on save
- `turbo fmt` — format all files
- `turbo fmt --check` — check without modifying (for CI)
- Editor integration via LSP
- Inspired by: gofmt, rustfmt, Prettier

### Linter: `turbo check`
- Built-in linting rules for common mistakes, performance, style
- `turbo check` — run all lints
- `turbo check --fix` — auto-fix what can be fixed
- Configurable: enable/disable rules in `turbo.toml`
- Custom lint rules via compiler plugins (stretch goal)
- Categories: correctness, performance, style, complexity, security
- Inspired by: Clippy, ESLint, golangci-lint

### Doc Generator: `turbo doc`
- Generate documentation from doc comments (`///`)
- HTML output with search (like docs.rs, ExDoc)
- `turbo doc` — generate docs
- `turbo doc --open` — generate and open in browser
- Doc tests: code examples in docs are compiled and tested
- Cross-references between modules/types/functions
- Inspired by: Cargo doc, ExDoc, Go doc

### REPL: `turbo repl`
- Interactive REPL for exploration and prototyping
- Full language support (not a subset)
- Tab completion, syntax highlighting
- History, multi-line editing
- Import packages in the REPL
- Inspired by: Julia, Python, Clojure REPL

### LSP Server: Built Into Compiler
- Ships with the compiler, not a separate install
- Features: autocomplete, go to definition, find references, rename, inline hints, hover docs, error diagnostics, code actions
- Real-time type information and error reporting
- Agent/tool-aware: autocomplete tool names, validate agent configs
- Inspired by: rust-analyzer, gopls, TypeScript language service

### Benchmarking: `turbo bench`
- Built-in micro-benchmarking framework — see [Performance Monitoring & Observability > Benchmarking](#benchmarking-turbo-bench) for full details

### Cross-Compilation: `turbo build --target`
- Ship the cross-compilation toolchain in the standard install
- No need for separate toolchains/SDKs per target
- Common targets:
  - `x86_64-linux-gnu` — Linux x64
  - `aarch64-linux-gnu` — Linux ARM64
  - `x86_64-apple-darwin` — macOS x64
  - `aarch64-apple-darwin` — macOS ARM64 (Apple Silicon)
  - `x86_64-windows-msvc` — Windows x64
  - `wasm32-wasi` — WebAssembly
  - `thumbv7em-none-eabihf` — ARM embedded
- Inspired by: Zig, Go

### Central Registry: `packages.turbo.dev`
- Package hosting and discovery
- Documentation auto-generated for every package
- Download statistics, dependency graphs
- Security scanning for published packages
- Namespaced packages: `@org/package`
- Inspired by: crates.io, npm, pkg.go.dev

## Performance Monitoring & Observability

### Built-in Profiler (`turbo profile`)

- CPU profiling with flame graph generation (like Chrome DevTools)
- Memory allocation tracking with per-function breakdown
- Async task profiling — see where time is spent across concurrent tasks
- Zero overhead when disabled (compile-time instrumentation switches)
- `turbo profile run ./my-app` — profile an execution
- `turbo profile --web` — open an interactive web UI for exploring profiles
- `turbo profile --cpu` — CPU time only
- `turbo profile --alloc` — memory allocations only
- `turbo profile --async` — async task scheduling and wait times
- Works for both native and WASM targets
- Output formats: HTML flame graph, JSON, Chrome Trace Event format
- Inspired by: Chrome DevTools, perf, Instruments, tokio-console

### Structured Logging (`turbo/log` standard library)

- Built into the standard library, not a third-party dependency
- Structured JSON logging by default (like pino/winston in Node.js)
- Log levels: `trace`, `debug`, `info`, `warn`, `error`, `fatal`
- Zero-cost when compiled out — log macros expand to nothing in release builds without `--enable-logging`
- Contextual logging with automatic span/trace propagation
- Configurable outputs: stdout, file, network (syslog, OTLP)

```
import { log } from "turbo/log"

fn process_request(req: Request) -> Response {
  log.info("Processing request", { method: req.method, path: req.path })

  let result = trace("db_query") {
    await db.query(req.params)
  }

  log.debug("Query complete", { rows: result.len(), duration: trace.elapsed() })
  Response.ok(result)
}
```

- `trace()` blocks create structured spans that propagate through async boundaries
- Spans nest automatically — child spans inherit parent context
- Inspired by: pino, slog (Go), tracing (Rust)

### Compile-Time Performance Hints

- `turbo build --perf-hints` analyzes your code and shows optimization suggestions
- Warns about:
  - Unnecessary heap allocations where stack allocation suffices
  - Auto-clones in hot loops (suggests borrowing instead)
  - Blocking calls inside async contexts
  - Large structs passed by value instead of by reference
- Suggests:
  - Where to add `@inline` for small hot functions
  - Where to use `&` references to avoid copies
  - Where to batch allocations or use arena allocators
- Think: ESLint but for performance — actionable, not noisy
- Only runs when explicitly requested (not part of normal builds)
- Inspired by: Clippy perf lints, PGO feedback, -Wsuggest-attribute

### Runtime Metrics (`turbo/metrics`)

- Built-in metrics collection: counters, gauges, histograms
- OpenTelemetry-compatible export out of the box
- `turbo/metrics` ships in the standard library
- Near-zero overhead when no exporter is configured (atomic increments only)

```
import { counter, histogram } from "turbo/metrics"

let request_count = counter("http_requests_total")
let response_time = histogram("http_response_duration_ms")

fn handle(req: Request) -> Response {
  request_count.inc({ method: req.method, path: req.path })
  let timer = response_time.start()
  let result = process(req)
  timer.observe()
  result
}
```

- Labels are type-checked at compile time — misspelled label keys are caught early
- Built-in exposition endpoint: `turbo/metrics.serve(":9090")` for Prometheus scraping
- Inspired by: Prometheus client libraries, OpenTelemetry, micrometer

### Benchmarking (`turbo bench`)

- Built-in micro-benchmarking framework with statistical rigor
- Statistical analysis: mean, median, P95, P99, standard deviation
- Comparison mode: `turbo bench --compare main` compares against a git branch
- Regression detection for CI: `turbo bench --fail-on-regression=5%`
- Warm-up iterations, outlier detection, and configurable sample sizes
- Output formats: terminal table, JSON, markdown (for CI comments)

```
@bench
fn bench_sort(b: Bencher) {
  let data = generate_random_vec(10000)
  b.iter(|| {
    let mut copy = data.clone()
    copy.sort()
  })
}

@bench
fn bench_parse_json(b: Bencher) {
  let input: str = load_fixture("large.json")
  b.iter(|| {
    parse_json(input)
  })
}
```

- `turbo bench` — run all benchmarks
- `turbo bench --compare baseline` — compare against saved baseline
- `turbo bench --output json` — machine-readable output for CI pipelines
- Inspired by: criterion.rs, Go benchmark, Cargo bench

## Project Structure (Convention)

```
my-project/
├── turbo.toml              # Project config (like Cargo.toml)
├── turbo.lock              # Lockfile
├── src/
│   ├── main.tb          # Entry point (binary)
│   └── lib.tb           # Library root
├── tests/
│   ├── integration.tb   # Integration tests
│   ├── perf.tb          # @perf and @regression tests
│   └── stress.tb        # @stress tests
├── benches/
│   └── perf.tb          # @bench micro-benchmarks
├── examples/
│   └── basic.tb
└── docs/
```

## Configuration: `turbo.toml`

```toml
[package]
name = "my-project"
version = "0.1.0"
edition = "2026"
authors = ["Alice <alice@example.com>"]
description = "A sample project"
license = "MIT"
repository = "https://github.com/alice/my-project"

[dependencies]
http = "1.2"
serde = { version = "2.0", features = ["json"] }

[dev-dependencies]
mock = "0.5"

[build]
target = "native"          # or "wasm32-wasi"
memory = "ownership"       # or "regions", "hybrid", "ctrc"
runtime = "default"        # or "actor", "minimal"
opt-level = "release"      # or "dev", "size"

[lints]
security = "deny"
performance = "warn"
style = "allow"
```

## Comparison With Other Toolchains

| Tool | Turbo | Rust/Cargo | Go | Zig | Deno | Node.js |
|------|------|-----------|-----|-----|------|---------|
| Package manager | Built-in | Built-in | Built-in | In progress | Built-in | npm (separate) |
| Build system | Built-in | Built-in | Built-in | Built-in | Built-in | Webpack etc. |
| Test runner | Built-in | Built-in | Built-in | Built-in | Built-in | Jest etc. |
| Formatter | Built-in | rustfmt | gofmt | zig fmt | deno fmt | Prettier |
| Linter | Built-in | Clippy | golangci-lint | N/A | deno lint | ESLint |
| LSP | Built-in | rust-analyzer | gopls | ZLS | Built-in | tsserver |
| REPL | Built-in | No | No | No | Built-in | Node REPL |
| Benchmarks | Built-in | Built-in | Built-in | No | Built-in | No |
| Profiler | Built-in | External (perf) | pprof | No | No | External |
| Metrics | Built-in | External | External | No | No | External |
| Structured logging | Built-in | External (tracing) | slog | No | No | External |
| Doc gen | Built-in | Built-in | Built-in | No | No | JSDoc etc. |
| Cross-compile | Built-in | Via rustup | Built-in | Built-in | N/A | N/A |

## Standard Library (`turbo/`)

Turbo ships a rich standard library under the `turbo/` namespace. Every module is available from day one with no external dependencies. The design philosophy: **if you need it in your first week, it should be in the standard library.**

### `turbo/io` — File System and I/O

All I/O is async by default. No callback hell, no sync/async split -- just `await`.

```
import { fs } from "turbo/io"

// Read a file — returns str or IoError
let content = await fs.read("config.toml")  // str ! IoError

// Write a file
await fs.write("output.txt", "Hello, Turbo!")  // void ! IoError

// Check existence
if fs.exists("config.toml") {
  let config = await fs.read("config.toml")?
  process(config)
}

// Stream large files line-by-line — constant memory
for await line in fs.lines("huge-dataset.csv") {
  let record = parse_csv_line(line)?
  await db.insert(record)
}

// Streaming byte reads for binary files
let stream = await fs.open("video.mp4")?
for await chunk in stream.chunks(8192) {
  await socket.send(chunk)
}

// stdin / stdout / stderr
import { stdin, stdout, stderr } from "turbo/io"

let input = await stdin.read_line()  // str ! IoError
await stdout.write("Hello\n")
await stderr.write("Warning: something happened\n")
```

### `turbo/http` — HTTP Client and Server

A complete HTTP stack: client, server, WebSocket, SSE. No external framework needed.

```
import { http, Server, Router } from "turbo/http"

// --- Client ---

// Simple GET — returns parsed response
let data = await http.get("https://api.example.com/users")  // Response ! NetworkError

// POST with automatic JSON serialization
let user = await http.post("https://api.example.com/users", body: {
  name: "Alice",
  email: "alice@example.com"
})

// Full control when you need it
let resp = await http.request({
  method: .POST,
  url: "https://api.example.com/upload",
  headers: { "Authorization": "Bearer {token}" },
  body: file_bytes,
  timeout: 30.seconds()
})?

// --- Server ---

let router = Router.new()

router.get("/users/:id", async (req) => {
  let user = await db.find_user(req.params.id)?
  Response.json(user)
})

router.post("/users", async (req) => {
  let input = req.json<CreateUser>()?
  let user = await db.create_user(input)?
  Response.json(user, status: 201)
})

// Middleware
router.use(cors())
router.use(rate_limit(max: 100, window: 1.minute()))

let server = Server.new(router)
await server.listen(3000)

// --- WebSocket ---

router.ws("/chat", async (socket) => {
  for await msg in socket.messages() {
    await socket.send("Echo: {msg.text}")
  }
})

// --- Server-Sent Events ---

router.get("/events", async (req) => {
  Response.sse(async (stream) => {
    for i in 0..100 {
      await stream.send({ data: "tick {i}" })
      await sleep(1.second())
    }
  })
})
```

### `turbo/json` — JSON Parsing and Serialization

Type-safe JSON with compile-time schema validation.

```
import { json } from "turbo/json"

// Parse JSON into a typed struct
let user = json.parse<User>(raw_text)?  // User ! ParseError

// Stringify any serializable value
let text = json.stringify(user)  // str

// Parse into dynamic JSON value
let value = json.parse_value(raw_text)?  // Json ! ParseError
match value {
  Json.Object(obj) => print(obj["name"])
  Json.Array(arr) => print("Got {arr.len()} items")
  _ => print("Unexpected type")
}

// Streaming parser for large files — constant memory
for await item in json.stream_array<Record>("huge-data.json") {
  await process(item?)
}

// Compile-time schema validation via @derive
@derive(Schema)
struct ApiResponse {
  status: str
  data: [User]
  pagination: Pagination?
}

// Schema is generated at compile time — no runtime reflection cost
let response = json.parse<ApiResponse>(body)?  // validated against schema
```

### `turbo/test` — Testing Framework

A full native app testing suite. Unit tests, performance tests, memory analysis, CPU profiling, regression detection, stress testing, and native app diagnostics — all built in. See the [Testing Framework: `turbo test`](#testing-framework-turbo-test) section above for the complete reference with all code examples.

Core imports:

```
import { mock, MockServer, TestFS, TestProcess, TestEnv, TestClock } from "turbo/test"
import { memory, cpu, bench, stress } from "turbo/test"
```

Quick overview of capabilities:

```
// Unit tests — @test, assert_eq, assert_throws, snapshots, mocks, property-based
@test
fn test_addition() {
  assert_eq(add(2, 3), 5)
}

// Performance gates — fail if memory/time/CPU limits exceeded
@test
@perf(max_memory: 50.mb(), max_time: 100.ms(), max_cpu: 80.percent())
fn test_perf() {
  process(generate_test_data(100_000))
}

// Memory leak detection — snapshot before/after comparison
@test
fn test_no_leaks() {
  let before = memory.snapshot()
  for i in 0..1000 { process(HeavyObject.new(i)) }
  let after = memory.snapshot()
  assert(after.allocated - before.allocated < 1.kb())
}

// CPU profiling — user time, context switches, cache misses
@test
fn test_cpu() {
  let profile = cpu.track(async () => { await handle_requests(generate_load(1000)) })
  assert(profile.user_time < 500.ms())
}

// Regression testing — compare against baseline branch
@test
@regression(baseline: "main", threshold: 5.percent())
fn test_regression() {
  bench.measure(() => { data.clone().sort() })
}

// Stress testing — sustained concurrent load
@test
@stress(duration: 30.seconds(), concurrency: 100)
fn test_load() {
  stress.run(async () => { assert_eq((await http.get(url)).status, 200) })
  assert(stress.report().p99_latency < 50.ms())
}

// Native app utilities — sandbox FS, mock HTTP, process spawning, env isolation
@test
fn test_fs() {
  let sandbox = TestFS.sandbox()
  sandbox.write("test.txt", "hello")
  assert_eq(fs.read(sandbox.path("test.txt")), "hello")
}
```

### `turbo/time` — Time, Durations, and Scheduling

```
import { Duration, Instant, DateTime, sleep, timeout } from "turbo/time"

// Durations — ergonomic literals
let d = 5.seconds()
let d2 = 100.milliseconds()
let d3 = 2.minutes() + 30.seconds()

// Sleep
await sleep(1.second())

// Timeout — wraps any future with a deadline
let result = await timeout(fetch_data(), 5.seconds())  // T ! TimeoutError

// Measuring elapsed time
let start = Instant.now()
do_work()
let elapsed = start.elapsed()  // Duration
print("Took {elapsed.as_millis()}ms")

// DateTime
let now = DateTime.now()         // current UTC time
let formatted = now.format("YYYY-MM-DD HH:mm:ss")  // "2026-02-21 14:30:00"
let parsed = DateTime.parse("2026-01-15", "YYYY-MM-DD")?  // DateTime ! ParseError

// Arithmetic
let tomorrow = now + 1.day()
let last_week = now - 7.days()
let diff = tomorrow - now  // Duration
```

### `turbo/collections` — Data Structures

Core collections beyond arrays. Map and set literals desugar to these types.

```
import { BTreeMap, BTreeSet, Queue, Stack, LinkedList, LRU } from "turbo/collections"

// HashMap and HashSet are accessed via literal syntax
let scores: {str: i32} = { "Alice": 100, "Bob": 85 }   // HashMap<str, i32>
let ids: {u64} = { 1, 2, 3 }                            // HashSet<u64>

// Ordered collections — sorted by key
let sorted_map = BTreeMap.from({ "banana": 2, "apple": 5, "cherry": 1 })
for { key, value } in sorted_map {
  print("{key}: {value}")  // apple, banana, cherry — sorted order
}

let sorted_set = BTreeSet.from([3, 1, 4, 1, 5])
// {1, 3, 4, 5} — sorted, deduplicated

// Queue (FIFO)
let mut q = Queue.new<str>()
q.push("first")
q.push("second")
let next = q.pop()  // some("first")

// Stack (LIFO)
let mut stack = Stack.new<i32>()
stack.push(1)
stack.push(2)
let top = stack.pop()  // some(2)

// LRU cache — fixed capacity, evicts least recently used
let cache = LRU.new<str, UserProfile>(capacity: 1000)
cache.put("user:42", profile)
let cached = cache.get("user:42")  // UserProfile?
// When capacity is reached, the least recently accessed entry is evicted
```

## Install Experience
```
# One command, everything included
curl -fsSL https://turbo.dev/install | sh

# Or via package managers
brew install turbo
scoop install turbo
apt install turbo

# Verify
turbo --version
# turbo 0.1.0 (aarch64-apple-darwin)
# Includes: compiler, lsp, fmt, test, bench, doc, repl, pkg

# Create a new project
turbo new my-project
cd my-project
turbo run
# Hello, World!
```
