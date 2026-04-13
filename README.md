<div align="center">

# Turbo

**TypeScript's ease. Rust's speed. No GC, no borrow checker.**

A compiled, type-safe programming language with familiar syntax and native performance. Compiles to machine code via Cranelift -- no VM, no garbage collector, no overhead.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-437%20passing-brightgreen.svg)](#testing)
[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg)](#installation)

[Website](https://turbolang.dev) &middot; [Documentation](https://turbolang.dev/docs) &middot; [Examples](#examples) &middot; [Contributing](CONTRIBUTING.md)

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
cargo build --release -p turbo-cli
export PATH="$PWD/target/release:$PATH"
```

### Hello, World

```turbo
fn main() {
    let name = "Turbo"
    print("Hello, {name}!")
}
```

```bash
turbolang run hello.tb        # JIT — compile and run instantly
turbolang build hello.tb      # AOT — produce a native binary
./hello
```

### Known Limitations (v0.7.x)

> **Warning — runtime memory leak:** The runtime does not yet perform reference counting. `rt_release` is currently a no-op, so long-running servers and hot loops that allocate repeatedly will leak memory (~2.5 KB/request on the example HTTP server). Real ARC is planned for v0.6. For short-running CLI programs this is not a problem.
>
> **Warning — HTTP server is experimental:** The built-in HTTP server binds to `127.0.0.1` by default and is not hardened for direct exposure to untrusted networks. Put it behind a reverse proxy (nginx, Caddy) in production. See [`SECURITY.md`](SECURITY.md) for the full threat model.

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

## Features

### Native Compilation

Turbo compiles directly to machine code. Programs start instantly and run at native speed.

- **JIT execution** via `turbolang run` for rapid development (Cranelift)
- **AOT compilation** via `turbolang build` for production binaries (Cranelift)
- **Optimized AOT** via `turbolang build --llvm` for maximum performance (LLVM 18)
- **WASM** via `turbolang build --target wasm` for WebAssembly output
- **Cross-compilation** via `turbolang build --target linux-arm64` from macOS

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
fn make_adder(n: i64) -> fn(i64) -> i64 {
    |x: i64| -> i64 { x + n }
}

fn main() {
    let add5 = make_adder(5)
    let nums = [1, 2, 3, 4, 5]
    let doubled = nums.map(|x: i64| -> i64 { x * 2 })
    let big = nums.filter(|x: i64| -> bool { x > 3 })
    let sum = reduce(nums, 0, |acc: i64, x: i64| -> i64 { acc + x })
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
        respond(200, "hello")
    })
    route(app, "POST", "/api/echo", |req: str| -> str {
        let body = request_body(req)
        respond(200, body)
    })
    http_listen(app)
}
```

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

64 built-in functions with no imports required. Method syntax works via UFCS -- `s.trim()` is equivalent to `trim(s)`.

| Category | Highlights |
|----------|------------|
| **I/O** | `print(value)`, `read_file(path)`, `write_file(path, data)` |
| **Strings** | `s.trim()`, `s.upper()`, `s.split(",")`, `s.contains("x")`, `s.replace("a", "b")` |
| **Arrays** | `arr.len()`, `arr.push(elem)`, `arr.map(fn)`, `arr.filter(fn)` |
| **Math** | `abs(n)`, `min(a, b)`, `max(a, b)`, `pow(base, exp)`, `sqrt(x)` |
| **HashMap** | `hashmap()`, `hashmap_set(m, k, v)`, `hashmap_get(m, k)`, `hashmap_keys(m)` |
| **JSON** | `json_get(json, key)`, `to_json(struct)`, `to_json_array(arr)` |
| **HTTP** | `http_get(url)`, `http_post(url, body)`, `http_server(port)`, `route(...)` |
| **Concurrency** | `channel()`, `send(ch, v)`, `recv(ch)`, `mutex(val)`, `sleep(ms)`, `clone(s)` |
| **Testing** | `assert(cond)`, `assert_eq(a, b)`, `assert_ne(a, b)`, `panic(msg)` |

Full reference with examples: [`docs/stdlib.md`](docs/stdlib.md)

## Examples

Three runnable example projects demonstrate real-world Turbo code.

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

### Interactive Web Dashboard

A full benchmark dashboard with a styled HTML UI served on port 3000. Run benchmarks from the browser and see results in real time.

```bash
turbolang run examples/web-dashboard/main.tb
# open http://localhost:3000
```

See [`examples/web-dashboard/main.tb`](examples/web-dashboard/main.tb)

## CLI Commands

| Command | Description |
|---------|-------------|
| `turbolang run <file.tb>` | Compile and run via JIT |
| `turbolang build <file.tb>` | Compile to native binary (Cranelift) |
| `turbolang build --llvm <file.tb>` | Compile with LLVM optimizations |
| `turbolang build --target wasm <file.tb>` | Compile to WebAssembly |
| `turbolang build --target linux-arm64 <file.tb>` | Cross-compile for Linux ARM64 |
| `turbolang build --target linux-x86 <file.tb>` | Cross-compile for Linux x86_64 |
| `turbolang test <file.tb>` | Run `@test` functions |
| `turbolang bench <file.tb>` | Benchmark with timing |
| `turbolang check <file.tb>` | Type-check without compiling |
| `turbolang install` | Install `path` and `github` dependencies from `turbo.toml` |
| `turbolang update` | Update pinned GitHub dependencies and refresh `turbo.lock` |
| `turbolang playground` | Launch browser-based playground |
| `turbolang fmt <file.tb>` | Format source code |
| `turbolang init <name>` | Create a new project |
| `turbolang doc <file.tb>` | Generate documentation |
| `turbolang repl` | Interactive REPL |
| `turbolang lsp` | Start Language Server |
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

Every compiler diagnostic has a unique, searchable error code (E0001--E0521). Look up any code from the command line:

```bash
turbolang explain E0100
```

Full reference: [`docs/errors.md`](docs/errors.md)

## Performance

Benchmarked on Apple Silicon (fib(40), recursive):

| Language | Time | Binary Size |
|----------|------|-------------|
| Rust (rustc -O) | 180ms | 441 KB |
| **Turbo (Cranelift)** | **250ms** | **55 KB** |
| C (cc -O2) | 290ms | 33 KB |
| **Turbo (LLVM)** | **290ms** | **55 KB** |
| Node.js | 580ms | N/A |
| Python | 13.1s | N/A |

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
cargo test --workspace --exclude turbo-codegen-llvm --manifest-path turbo/Cargo.toml

# Integration tests (requires release build)
cargo build --release -p turbo-cli --manifest-path turbo/Cargo.toml
cd turbo && ./tests/run_tests.sh

# Run a single file
turbolang run turbo/tests/phase1/fibonacci.tb
```

437+ tests across unit and integration suites (275 unit + 162 integration).

## LLVM Backend

The LLVM 18 backend ships with the Homebrew install for maximum performance:

```bash
turbolang build --llvm myapp.tb -o myapp
```

For building from source with LLVM support:

```bash
brew install llvm@18
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 cargo build --release -p turbo-cli --features turbo-cli/llvm
```

## Ecosystem

| Tool | Install / Link |
|------|----------------|
| **VS Code Extension** | `zvndev.turbo-lang` -- syntax highlighting, 23 snippets, LSP client |
| **Tree-sitter Grammar** | [ZVN-DEV/tree-sitter-turbo](https://github.com/ZVN-DEV/tree-sitter-turbo) |
| **Homebrew** | `brew tap ZVN-DEV/turbo && brew install turbo-lang` |
| **Docker** | [`distribution/Dockerfile`](distribution/Dockerfile) |
| **LSP Server** | `turbolang lsp` -- diagnostics, hover, completions, references, document symbols, go-to-definition |
| **Install Script** | `curl -fsSL https://raw.githubusercontent.com/ZVN-DEV/Turbo-Language/master/distribution/install.sh \| sh` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on building, testing, and submitting pull requests.

## License

MIT License. See [LICENSE](LICENSE) for details.
