<div align="center">

# Turbo

**Familiar code. Native execution. A path to deeper control.**

Turbo is an early-stage compiled, type-safe programming language for developers
who like the feel of TypeScript/JavaScript but want native binaries, explicit
static types, and an increasingly direct path toward Rust-class performance and
memory control. It compiles through Cranelift and runs without a VM or garbage
collector; performance claims are measured and scoped, not assumed.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-passing-brightgreen.svg)](#testing)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg)](#installation)

[Getting Started](docs/GETTING-STARTED.md) &middot; [Documentation](docs/stdlib.md) &middot; [Roadmap](docs/ROADMAP.md) &middot; [Performance](docs/PERFORMANCE.md) &middot; [Examples](#examples) &middot; [Safety](docs/SAFETY.md) &middot; [Security](SECURITY.md) &middot; [Contributing](CONTRIBUTING.md)

</div>

---

## Quick Start

### Installation

```bash
# Homebrew (recommended)
brew tap ZVN-DEV/turbo && brew install turbo-lang

# Or build from source
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd Turbo-Language/turbo
cargo build --release -p turbo-cli -p turbo-lsp
export PATH="$PWD/target/release:$PATH"

# Verify both toolchain binaries are available
turbolang --version
command -v turbo-lsp
```

> **Prerequisite:** `turbolang build` (AOT) links the C runtime, so it needs a C compiler (`cc`) on your `PATH` — Xcode Command Line Tools on macOS, `gcc`/`clang` on Linux. `turbolang run` (JIT) has no such requirement.

### Hello, World

```turbo
fn main() {
    let name = "Turbo"
    print("Hello, {name}!")
}
```

```bash
turbolang run hello.tb        # JIT — compile and run in one step
turbolang build hello.tb      # AOT — produce a native binary
./hello
```

### Known Limitations

> **Note — runtime string allocation:** Strings, arrays, structs, results, and optionals use the runtime ARC header and are released at scope exit, reassignment, and typed container drops. HTTP servers still use per-request arenas for request-scoped allocations, so handler temporaries are reclaimed in bulk at the end of each request while server state held in hashmaps persists correctly across requests.
>
> **Note — HTTP server is designed for behind-proxy deployment:** The built-in HTTP server binds to `127.0.0.1` by default, enforces body/header/connection caps and read/write/idle timeouts, does graceful shutdown on `SIGTERM`/`SIGINT`, and exposes tunables via `http_config`. It provides no TLS/HTTP2 — put it behind a reverse proxy (nginx, Caddy) for public exposure. See [`docs/production-server.md`](docs/production-server.md) for deployment and [`SECURITY.md`](SECURITY.md) for the threat model.

> **Roadmap note — agent/tool features live in a sidecar, not the compiler.** Earlier design sketches explored `agent` and `tool fn` keywords. Those are no longer planned as core-language features — they belong in a future `turbo-agent` library that builds on Turbo's async, HTTP, and typed-serialization primitives. The compiler itself stays focused on being a fast, small, general-purpose systems/application language. The current public capability set is native compilation, WASM output, thread-per-`spawn` concurrency, a behind-proxy HTTP server, built-in SQLite, a typed generic `HashMap<K,V>`, first-class function values, a package registry, REPL/playground, formatter, and LSP.

### Security Model

Turbo compiles code to native binaries or runs it via JIT -- both execute with full OS permissions. **Treat `.tb` files like executables.** Do not run untrusted code. For the full security model (JIT sandboxing, HTTP server limits, FFI, shell execution), see [`SECURITY.md`](SECURITY.md). For compile-time and runtime safety guarantees, see [`docs/SAFETY.md`](docs/SAFETY.md).

### A Taste of Turbo

```turbo
struct Counter { value: i64 }

impl Counter {
    fn get(self) -> i64 { self.value }
}

fn fib(n: i64) -> i64 {
    if n <= 1 { n }
    else { fib(n - 1) + fib(n - 2) }
}

async fn delayed_value(ms: i64, val: i64) -> i64 {
    await sleep(ms)
    val
}

async fn main() {
    let c = Counter { value: 42 }
    print("counter: {c.get()}")
    print("fib(10): {fib(10)}")

    let a = spawn delayed_value(10, 100)
    let b = spawn delayed_value(10, 200)
    print("async sum: {await a + await b}")
}
```

## What Turbo is for

Turbo's best near-term wedge is native application infrastructure for
TypeScript/JavaScript-shaped teams: CLIs, automation tools, small services,
single-binary utilities, local data processing, and compute-heavy worker code
where static types and native deployment matter.

Strong fits today:

- **CLI tools and developer utilities.** Turbo produces small native binaries,
  has a familiar expression syntax, and avoids a separate runtime install.
- **Local tools and system-adjacent apps.** File I/O, environment access,
  process execution, SQLite, JSON, HashMaps, and the LSP/formatter/test runner
  make it a good fit for package managers, indexers, migration tools, data
  converters, and internal automation.
- **Small HTTP + SQLite + JSON services.** The built-in server has caps,
  timeouts, graceful shutdown, and configuration knobs. Use it behind a reverse
  proxy for public exposure; see [`docs/production-server.md`](docs/production-server.md).
- **CPU-oriented worker kernels that fit today's runtime.** Recursive compute,
  tree traversal, string processing, and allocation-heavy managed workloads are
  now measured by committed benchmark fixtures. Some are close enough to Rust to
  be promising; others expose the next optimization targets.

Promising, but still needs roadmap work:

- **Native desktop apps.** Turbo can support native-app backends, local
  services, and tooling today. A first-class native GUI story still requires
  platform bindings, packaging/signing smoke tests, accessibility checks, and
  macOS/Linux/Windows conformance.
- **Durable task servers.** The shape is attractive: typed jobs, native
  deployment, SQLite-backed state, bounded resources. Production qualification
  still needs a durable queue contract, cancellation, worker recovery, and
  24-hour soak evidence.
- **Game development.** Turbo is most credible first for game tools, asset
  pipelines, procedural generation, simulation kernels, and simple 2D runtime
  experiments. Engines, frame-critical gameplay, hard real-time audio, and GPU
  paths require explicit no-allocation controls, tighter layouts, graphics
  bindings, and long-session frame-budget tests.
- **System and freestanding software.** Turbo is native and type-safe, but it is
  not yet a kernel, driver, embedded, or `no_std` language. That requires an
  explicit freestanding profile with no heap/RC/OS dependencies and hardware or
  emulator proof.

Current caveats:

- Concurrency is **thread-per-`spawn`** on real OS threads, plus channels and a
  mutex. There is no bounded async event-loop runtime yet.
- The HTTP server provides **no TLS or HTTP/2**. Run it behind nginx, Caddy, or
  another reverse proxy for public exposure.
- **Windows** and **WASM** are not yet promoted to the same support level as the
  macOS/Linux native path. See [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md).
- Rust-class speed and deep memory-control are goals with concrete gates in
  [docs/PERFORMANCE.md](docs/PERFORMANCE.md), not a blanket claim about every
  current Turbo program.

## Features

### Native Compilation

Turbo compiles directly to machine code through Cranelift. The goal is fast
startup, predictable native deployment, and a clear optimization path toward
Rust-class execution where the language has enough ownership and layout
information to compete honestly.

- **JIT execution** via `turbolang run` for rapid development (Cranelift)
- **AOT compilation** via `turbolang build` for production binaries (Cranelift)
- **WASM** via `turbolang build --target wasm` for WebAssembly output
- **Cross-compilation** via `turbolang build --target linux-x86` from macOS (a `linux-arm64` target emits a valid ARM64 ELF but is not yet runtime-validated or shipped as a release artifact — see below)

### Type System

Strong static typing with inference, generics, traits, and algebraic data types.

```turbo
struct Point<T> { x: T, y: T }

type Result<T> {
    ok(T)
    err(str)
}

trait Printable {
    fn to_string(self) -> str
}

fn identity<T>(x: T) -> T { x }
```

Types: `int`, `float`, `bool`, `str`, `()`, `[T]`, `T?`, `T ! E`, `Future<T>`. Also: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `f32`, `f64`, `usize` for low-level control.

### Pattern Matching

```turbo
type Shape {
    Circle(f64)
    Rectangle(f64, f64)
}

fn describe(s: Shape) -> str {
    match s {
        Circle(r) => "circle"
        Rectangle(w, h) => "rectangle"
    }
}

let s = Shape.Circle(3.14)

fn classify(n: i64) -> str {
    match n {
        0 => "zero"
        n if n > 0 => "positive"
        _ => "negative"
    }
}
```

### Async/Await & Concurrency

```turbo
async fn fetch_data() -> i64 {
    sleep(100)
    42
}

fn main() {
    let handle = spawn fetch_data()
    let result = await handle
    print(result)
}
```

### Closures & Higher-Order Functions

```turbo
// Returned closures use the explicit form so their parameter types are known.
fn make_adder(n: i64) -> fn(i64) -> i64 {
    |x: i64| -> i64 { x + n }
}

fn main() {
    let add5 = make_adder(5)
    let nums = [1, 2, 3, 4, 5]

    // In map/filter/reduce, parameter types are inferred — use the short arrow form.
    let doubled = nums.map((x) => x * 2)
    let big = nums.filter((x) => x > 3)
    let sum = reduce(nums, 0, (acc, x) => acc + x)
    print("sum: {sum}")
}
```

### Pipes, Strings & Collections

```turbo
fn main() {
    let text = "  Hello, Turbo World!  "
    let cleaned = text |> trim |> lower
    print("cleaned: {cleaned}")

    let m = hashmap()
    hashmap_set(m, "name", "Turbo")
    print(hashmap_get(m, "name"))
}
```

### HTTP Server

```turbo
fn main() {
    let app = http_server(8080)
    route(app, "GET", "/", |req: str| -> str {
        respond_text(200, "hello")
    })
    route(app, "POST", "/api/echo", |req: str| -> str {
        let body = request_body(req)
        respond_text(200, body)
    })
    http_listen(app)
}
```

The server is thread-per-connection and meant to run behind a reverse proxy
(nginx/Caddy) for TLS, HTTP/2, and public exposure. It supports graceful
shutdown (`SIGTERM`/`SIGINT`) and tunable limits (body/header size, connection
cap, timeouts, keep-alive) via `http_config(key, value)`. See
[`docs/production-server.md`](docs/production-server.md) for deployment.

### C FFI

Call C library functions directly from Turbo.

```turbo
@unsafe
extern "C" {
    fn floor(x: f64) -> f64
    fn ceil(x: f64) -> f64
    fn puts(s: str) -> i32
}

fn main() {
    print(floor(3.7))
    puts("Hello from C!")
}
```

```bash
turbolang build --link m app.tb    # link additional libraries
```

### Derive Attributes & Testing

```turbo
@derive(Eq, Clone, Display)
struct Point { x: i64, y: i64 }

fn add(a: i64, b: i64) -> i64 { a + b }

@test fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(-1, 1), 0)
}
```

```bash
turbolang test myfile.tb
#   PASS  test_add
# 1 passed, 0 failed
```

### Copy-on-Write Memory

Safe value semantics without a garbage collector.

```turbo
fn main() {
    let a = [1, 2, 3]
    let mut b = a        // shared (cheap)
    b[0] = 99            // copy-on-write (safe)
    print(a[0])          // 1 — original unchanged
    print(b[0])          // 99 — independent copy
}
```

### Standard Library

100+ built-in functions with no imports required. Method syntax works via UFCS -- `s.trim()` is equivalent to `trim(s)`.

| Category | Highlights |
|----------|------------|
| **I/O** | `print(value)`, `read_file(path)`, `write_file(path, data)`, `try_read_file(path)`, `try_write_file(path, data)` |
| **Strings** | `s.trim()`, `s.upper()`, `s.split(",")`, `s.contains("x")`, `s.replace("a", "b")` |
| **Arrays** | `arr.len()`, `arr.push(elem)`, `arr.map(fn)`, `arr.filter(fn)` |
| **Math** | `abs(n)`, `min(a, b)`, `max(a, b)`, `pow(base, exp)` (integer base/exponent), `sqrt(x)` |
| **HashMap** | typed `HashMap<K,V>` (int/`str` keys, any value incl. functions), plus `hashmap()`, `hashmap_set(m, k, v)`, `hashmap_get(m, k)`, `hashmap_has(m, k)`, `hashmap_keys(m)`, `hashmap_remove(m, k)` |
| **JSON** | `json_get(json, key)`, `to_json(struct)`, `to_json_array(arr)` |
| **Database** | built-in SQLite: `sqlite_open(path)`, `sqlite_exec(db, sql)`, `sqlite_prepare(db, sql)`, `sqlite_step(stmt)`, `sqlite_column_int/str/float(...)`, `sqlite_bind_int/str/float(...)` |
| **HTTP** | `http_get(url)`, `http_post(url, body)`, `http_server(port)`, `http_config(key, value)`, `route(...)` |
| **System** | `exec(cmd)`, `env_get(key)` |
| **Concurrency** | `channel()`, `send(ch, v)`, `recv(ch)`, `mutex(val)`, `sleep(ms)`, `clone(s)` |
| **Testing** | `assert(cond)`, `assert_eq(a, b)`, `assert_ne(a, b)`, `panic(msg)` |

Full reference with examples: [`docs/stdlib.md`](docs/stdlib.md)

## Examples

Selected runnable examples demonstrate real-world Turbo code today. More runnable projects live in [`examples/README.md`](examples/README.md), and [`examples/roadmap/`](examples/roadmap/) contains planned examples that are intentionally not runnable yet.

### Flagship Demo: Interactive Web Dashboard

If you want the fastest proof that Turbo can ship a browser-facing experience today, start here. `web-dashboard` serves a styled HTML app and five JSON benchmark endpoints from a single Turbo file.

```bash
turbolang run examples/web-dashboard/main.tb
# then open http://localhost:3000
```

What to try in the browser:

- Click **Run All Benchmarks** to exercise every endpoint
- Open `http://localhost:3000/api/info` in another tab to inspect a raw JSON route
- Keep the terminal open — the dashboard stays live until you press `Ctrl+C`

See [`examples/web-dashboard/main.tb`](examples/web-dashboard/main.tb) and [`examples/web-dashboard/README.md`](examples/web-dashboard/README.md)

### Text Statistics Analyzer

Word frequency analysis with pipes, HashMaps, and string interpolation.

```bash
turbolang run examples/simple-script/main.tb
```

See [`examples/simple-script/main.tb`](examples/simple-script/main.tb)

### REST API Benchmark Server

An HTTP server on port 8080 with endpoints for fibonacci, prime counting, and sorting benchmarks. Returns JSON responses.

```bash
turbolang run examples/speed-server/main.tb
# curl http://localhost:8080/api/fib
```

See [`examples/speed-server/main.tb`](examples/speed-server/main.tb)

## CLI Commands

| Command | Description |
|---------|-------------|
| `turbolang run <file.tb>` | Compile and run via JIT |
| `turbolang build <file.tb>` | Compile to native binary (Cranelift) |
| `turbolang build --target wasm <file.tb>` | Compile to WebAssembly |
| `turbolang build --target linux-x86 <file.tb>` | Cross-compile for Linux x86_64 |
| `turbolang build --target linux-arm64 <file.tb>` | Cross-compile for Linux ARM64 (emits a valid ARM64 ELF, but not yet runtime-validated or shipped as a release artifact) |
| `turbolang test <file.tb>` | Run `@test` functions |
| `turbolang bench <file.tb>` | Benchmark with timing |
| `turbolang check <file.tb>` | Type-check without compiling |
| `turbolang search <query>` | Search the package registry ([turbolang.dev/packages](https://turbolang.dev/packages)) |
| `turbolang install` | Install `path` and `github` dependencies from `turbo.toml` |
| `turbolang update` | Update pinned GitHub dependencies and refresh `turbo.lock` |
| `turbolang playground` | Launch browser-based playground |
| `turbolang fmt <file.tb>` | Format source code |
| `turbolang init <name>` | Create a new project |
| `turbolang doc <file.tb>` | Generate documentation |
| `turbolang repl` | Interactive REPL |
| `turbo-lsp` | Start Language Server |
| `turbolang explain <code>` | Explain an error code (e.g. `turbolang explain E0100`) |

### Dependency Installation

`turbolang install` currently supports two installable dependency shapes:

```toml
[registries]
turbo-db = "ZVN-DEV/turbo-db"

[dependencies]
mathlib = { path = "../mathlib" }
turbo-db = "0.1"
http-utils = { github = "owner/http-utils", rev = "0123456789abcdef" }
http-utils-next = { github = "owner/http-utils", version = "1.2" }
```

GitHub installs are pinned into `turbo.lock` so repeat installs use the same
commit. Versioned dependencies resolve through `[registries]` or, for packages
named `turbo-*`, the default `ZVN-DEV/<package>` GitHub convention. The
installer resolves the requested version to a matching git tag and locks the
resulting commit in `turbo.lock`.

## Error Codes

Every compiler diagnostic has a unique, searchable error code. Look up any code from the command line:

```bash
turbolang explain E0100
```

Full reference: [`docs/errors.md`](docs/errors.md)

## Performance

Turbo is not yet allowed to claim blanket Rust parity. The current committed
benchmark evidence is diagnostic but useful:

| Evidence set | What it says | Status |
|---|---|---|
| `g2-initial-20260906` | `fib(40)` median paired elapsed ratio **1.444× Rust**; existing word-count **3.871× Rust** with different implementation shape | Reproducible diagnostic subset; qualification incomplete |
| `g2-tree-diagnostic-20260906` | recursive tree workload one-pair timing **1.317× Rust** plus balanced tracked ARC allocations/frees | One-pair diagnostic, not statistical proof |
| `g2-particle-allocation-20260906` | 10,000-particle managed update reports **5,130,018 allocations and frees**, zero tracked live allocations at return | Allocation diagnostic, not timing/frame-budget proof |

The public performance contract, target gates, benchmark method, and current
evidence live in [docs/PERFORMANCE.md](docs/PERFORMANCE.md). Short version:
Turbo is native and promising, but Rust-class speed and memory control are still
engineering goals with explicit acceptance gates.

## Project Structure

```
turbo/
  crates/
    turbo-lexer/                # Tokenizer (logos-based)
    turbo-ast/                  # AST definitions + error codes
    turbo-parser/               # Recursive descent parser
    turbo-sema/                 # Semantic analysis and type checking
    turbo-codegen-cranelift/    # Cranelift JIT + AOT codegen
    turbo-cli/                  # CLI frontend (run/build/test/fmt/repl)
    turbo-lsp/                  # Language Server Protocol
  tests/
    phase1/                     # Integration tests (.tb + .expected pairs)
examples/                       # Runnable example projects
design/                         # Language specification documents
```

## Language Design

Full specification lives in `design/`: [SYNTAX.md](design/SYNTAX.md), [TYPE-SYSTEM.md](design/TYPE-SYSTEM.md), [MEMORY-MODEL.md](design/MEMORY-MODEL.md), [CONCURRENCY.md](design/CONCURRENCY.md), [COMPILATION.md](design/COMPILATION.md), [TOOLCHAIN.md](design/TOOLCHAIN.md).

> **Note:** These documents describe the full language vision. Features marked as implemented are available today; others represent the roadmap.

## Testing

```bash
# Unit tests (all crates)
cargo test --workspace --manifest-path turbo/Cargo.toml

# Integration tests (requires release build)
cargo build --release -p turbo-cli --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh

# Run a single file
turbolang run turbo/tests/phase1/fibonacci.tb
```

The test suite spans Rust unit tests, integration fixtures, and parity coverage; run the commands above for the current count.

## Ecosystem

| Tool | Install / Link |
|------|----------------|
| **VS Code Extension** | `zvndev.turbo-lang` -- syntax highlighting, 25 snippets, LSP client (diagnostics, hover, go-to-definition, completions) |
| **Tree-sitter Grammar** | [ZVN-DEV/tree-sitter-turbo](https://github.com/ZVN-DEV/tree-sitter-turbo) |
| **Homebrew** | `brew tap ZVN-DEV/turbo && brew install turbo-lang` |
| **Docker** | [`distribution/Dockerfile`](distribution/Dockerfile) |
| **LSP Server** | `turbo-lsp` -- diagnostics, hover, completions, references, document symbols, go-to-definition. `turbolang lsp` remains available for older editor integrations. |
| **Install Script** | `curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh \| bash` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on building, testing, and submitting pull requests.

## License

MIT License. See [LICENSE](LICENSE) for details.
