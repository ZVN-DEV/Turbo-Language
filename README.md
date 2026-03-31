# Turbo

A compiled, type-safe programming language with JavaScript's developer experience, Rust's performance, and first-class AI agent primitives. Compiles to native code via Cranelift and targets WebAssembly.

**North star:** JavaScript's soul. Rust's speed. Built for the AI age.

## Quick Start

### Installation

```bash
# Clone and build from source
git clone https://github.com/ZVN-DEV/Turbo-Language.git
cd Turbo-Language/turbo
cargo build --release

# Add to your PATH
export PATH="$PWD/target/release:$PATH"

# Verify installation
turbo --version
```

### Hello, World

Create `hello.tb`:

```turbo
fn main() {
    print("Hello, world!")
}
```

Run it:

```bash
turbo run hello.tb
```

Compile to a native binary:

```bash
turbo build hello.tb
./hello
```

### A Taste of Turbo

```turbo
/// Compute the nth Fibonacci number.
fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}

fn main() {
    let mut i = 0
    while i <= 15 {
        print(fib(i))
        i += 1
    }
}
```

```turbo
/// Absolute value.
fn abs(x: i64) -> i64 {
    if x < 0 { 0 - x } else { x }
}

/// Clamp a value to a range.
fn clamp(val: i64, lo: i64, hi: i64) -> i64 {
    if val < lo { lo }
    else { if val > hi { hi } else { val } }
}

fn main() {
    assert(abs(-42) == 42)
    assert(clamp(150, 0, 100) == 100)
    print("All checks passed!")
}
```

## Features

### Compiled to Native Code

Turbo compiles directly to machine code using Cranelift. No interpreter, no VM, no garbage collector. Programs start instantly and run at native speed.

- **JIT execution** via `turbo run` for rapid development (Cranelift)
- **AOT compilation** via `turbo build` for production binaries (Cranelift)
- **Optimized AOT** via `turbo build --llvm` for maximum performance (LLVM 18)
- Beats C and Rust on recursive benchmarks (fib(40): 160ms vs C 170ms, Rust 180ms)

### Type System

Strong static typing with type inference, generics, traits, and algebraic data types:

```turbo
struct Point<T> {
    x: T,
    y: T,
}

type Option<T> {
    Some(T)
    None
}

trait Display {
    fn to_string(self) -> str
}

fn identity<T>(x: T) -> T { x }
```

Supported types: `i32`, `i64`, `u32`, `u64`, `f32`, `f64`, `bool`, `str`, `()`, arrays `[T]`, optionals `T?`, results `T ! E`, futures `Future<T>`.

### Expressions Everywhere

`if`, `while`, `match`, and blocks are expressions that return values:

```turbo
fn classify(n: i64) -> str {
    if n > 0 { "positive" }
    else { if n < 0 { "negative" } else { "zero" } }
}
```

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
```

### Async Runtime

Real async/await with thread-based concurrency:

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

### AI Agent Primitives

First-class `agent` and `tool fn` keywords for building AI-powered applications:

```turbo
agent Assistant {
    model: "claude-sonnet"
    system: "You help with tasks."
}

fn main() {
    let a = Assistant {}
    print(a.model)
}
```

### Closures and Higher-Order Functions

```turbo
fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let double = |x: i64| -> i64 { x * 2 }
    print(apply(double, 21))
}
```

### Module System

```turbo
import { Point, distance } from "geometry"

fn main() {
    let p = Point { x: 3, y: 4 }
    print(distance(p))
}
```

### Testing

Built-in test framework with `@test` and `assert_eq`:

```turbo
fn add(a: i64, b: i64) -> i64 { a + b }

@test fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(-1, 1), 0)
}
```

```bash
turbo test myfile.tb
#   PASS  test_add
# 1 passed, 0 failed
```

### Derive Attributes

Auto-generate trait implementations:

```turbo
@derive(Eq, Clone, Display)
struct Point { x: i64, y: i64 }

fn main() {
    let a = Point { x: 1, y: 2 }
    let b = Point { x: 1, y: 2 }
    if a == b { print("equal!") }     // @derive(Eq)
    let c = clone(a)                   // @derive(Clone)
    print(a)                           // @derive(Display) -> "Point { x: 1, y: 2 }"
}
```

### Copy-on-Write Memory

Safe value semantics without a garbage collector:

```turbo
fn main() {
    let a = [1, 2, 3]
    let b = a           // shared (cheap)
    b[0] = 99           // copy-on-write (safe)
    print(a[0])          // 1 — original unchanged
    print(b[0])          // 99 — independent copy
}
```

### Match Guards

```turbo
fn classify(n: i64) -> str {
    match n {
        0 => "zero"
        n if n > 0 => "positive"
        _ => "negative"
    }
}
```

### Collections

```turbo
fn main() {
    let m = hashmap()
    hashmap_set(m, "name", "Turbo")
    print(hashmap_get(m, "name"))
    print(hashmap_keys(m).len())
}
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `turbo run <file.tb>` | Compile and run via JIT |
| `turbo build <file.tb>` | Compile to native binary (Cranelift) |
| `turbo build --llvm <file.tb>` | Compile with LLVM optimizations |
| `turbo test <file.tb>` | Run `@test` functions |
| `turbo bench <file.tb>` | Benchmark with timing |
| `turbo init <name>` | Create a new project |
| `turbo install` | Install dependencies from turbo.toml |
| `turbo update` | Update GitHub dependencies |
| `turbo fmt <file.tb>` | Format source code |
| `turbo doc <file.tb>` | Generate documentation |
| `turbo repl` | Interactive REPL |
| `turbo lsp` | Start Language Server |

## Project Structure

```
turbo/
  crates/
    turbo-lexer/                # Tokenizer (logos-based)
    turbo-ast/                  # Abstract syntax tree definitions
    turbo-parser/               # Recursive descent parser
    turbo-sema/                 # Semantic analysis and type checking
    turbo-codegen-cranelift/    # Code generation (Cranelift JIT + AOT)
    turbo-codegen-llvm/         # Optimized code generation (LLVM 18)
    turbo-cli/                  # CLI frontend (run/build/doc/fmt/repl)
    turbo-lsp/                  # Language Server Protocol implementation
  tests/
    phase1/                     # End-to-end test programs
design/                         # Language specification documents
examples/                       # Full application examples
```

## Language Design

The complete language specification spans design documents in `design/`:

- **SYNTAX.md** -- Full syntax reference
- **TYPE-SYSTEM.md** -- Types, generics, error handling (`T ! E`), algebraic data types
- **MEMORY-MODEL.md** -- CTRC ownership, auto-clone, regions, arenas
- **CONCURRENCY.md** -- Async/await, actors, channels, structured concurrency
- **AGENTIC.md** -- AI agent primitives: `tool fn`, `agent` keyword, streaming
- **COMPILATION.md** -- Cranelift backend, WASM pipeline
- **TOOLCHAIN.md** -- Testing framework, package manager, formatter, profiler

## What You Can Build

Turbo is designed for:

- **CLI tools** — fast startup, tiny binaries, cross-platform
- **AI agents** — `tool fn` and `agent` keywords for LLM-powered apps
- **Web APIs** — HTTP server framework (coming soon)
- **Data processing** — `map`/`filter`/`reduce` with native speed
- **Systems programming** — `@unsafe` for FFI and low-level code

## Status

**~90% feature complete.** 17,490 lines of compiler, 259 tests, dual backends (Cranelift + LLVM). The language is usable for real programs. See the [showcase](turbo/showcase/full_demo.tb) for a 400-line program exercising all features.

## LLVM Backend (Optional)

For maximum performance, install LLVM 18 and rebuild:

```bash
# macOS
brew install llvm@18

# Build with LLVM support
LLVM_SYS_180_PREFIX=/opt/homebrew/opt/llvm@18 cargo build --release

# Compile with LLVM optimizations
turbo build --llvm myapp.tb -o myapp
```

## Performance

Benchmarked on Apple Silicon (fib(40), recursive):

| Language | Time | Binary Size |
|----------|------|-------------|
| **Turbo (LLVM)** | **160ms** | **35 KB** |
| C (cc -O2) | 170ms | 33 KB |
| Rust (rustc -O) | 180ms | 441 KB |
| Turbo (Cranelift) | 220ms | 35 KB |
| Node.js | 580ms | N/A |
| Python | 13.1s | N/A |

## License

MIT License. See [LICENSE](LICENSE) for details.
